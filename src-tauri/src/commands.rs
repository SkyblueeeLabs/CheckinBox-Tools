//! Tauri 命令层：UI 与后端之间的唯一入口。

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::core::config::AppConfig;
use crate::core::logging::{self, LogEntry};
use crate::core::provider::{Account, CheckinStatus, ClaimState, Provider, Quota};
use crate::core::scheduler::{self, SchedulerState};
use crate::core::store::AccountStore;
use crate::providers::qoder::AuthSession;

/// 全局应用状态。
pub struct AppState {
    pub store: Mutex<AccountStore>,
    pub config: Mutex<AppConfig>,
    pub scheduler: Mutex<SchedulerState>,
    pub auth_sessions: Mutex<BTreeMap<String, AuthSession>>,
    pub providers: Vec<Arc<dyn Provider>>,
    /// Qoder 适配器的具体类型（设备授权不走通用 trait，需要单独拿到）
    pub qoder: Arc<crate::providers::qoder::QoderProvider>,
}

impl AppState {
    pub fn new() -> Self {
        let qoder = Arc::new(crate::providers::qoder::QoderProvider::new());
        let list: Vec<Arc<dyn Provider>> = vec![
            Arc::new(crate::providers::minimax::MiniMaxProvider::new()),
            qoder.clone(),
            Arc::new(crate::providers::qwenwork::QwenWorkProvider::new()),
            Arc::new(crate::providers::miaoshou::MiaoshouProvider::new()),
        ];
        Self {
            store: Mutex::new(AccountStore::load().unwrap_or_default()),
            config: Mutex::new(AppConfig::load().unwrap_or_default()),
            scheduler: Mutex::new(SchedulerState::load().unwrap_or_default()),
            auth_sessions: Mutex::new(load_pending_auth()),
            providers: list,
            qoder,
        }
    }

    fn provider(&self, id: &str) -> Option<Arc<dyn Provider>> {
        self.providers.iter().find(|p| p.id() == id).cloned()
    }
}

// ---------------------------------------------------------------------------
// 待完成授权会话的持久化
// ---------------------------------------------------------------------------

/// 读回上次没完成的设备授权会话（应用重启后恢复轮询用）。
fn load_pending_auth() -> BTreeMap<String, AuthSession> {
    let path = crate::core::paths::auth_pending_file();
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return BTreeMap::new();
    };
    let Ok(map) = serde_json::from_str::<BTreeMap<String, AuthSession>>(&raw) else {
        return BTreeMap::new();
    };
    map
}

fn save_pending_auth(sessions: &BTreeMap<String, AuthSession>) {
    let path = crate::core::paths::auth_pending_file();
    if sessions.is_empty() {
        let _ = std::fs::remove_file(&path);
        return;
    }
    if let Ok(text) = serde_json::to_string_pretty(sessions) {
        let _ = crate::core::paths::atomic_write(&path, text.as_bytes());
    }
}

// ---------------------------------------------------------------------------
// 返回给前端的数据结构
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderView {
    pub id: String,
    pub name: String,
    pub accent: String,
    pub docs_url: String,
    /// 是否支持从本机客户端自动发现账号
    pub supports_discovery: bool,
    /// 是否已配置账号
    pub configured: bool,
    /// 本机是否装了该 agent 的客户端
    pub has_client: bool,
    /// 客户端可执行文件路径
    pub client_exe: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountView {
    pub account: Account,
    pub provider_name: String,
    pub accent: String,
    pub status: Option<CheckinStatus>,
    pub quota: Option<Quota>,
    pub quota_error: Option<String>,
    pub error: Option<String>,
    pub today_log: Option<LogEntry>,
    /// 是否为该 agent 的当前账号
    pub is_active: bool,
    /// 客户端数据目录（项目/会话都在这里）
    pub data_dir: Option<String>,
    /// 本地项目数
    pub project_count: usize,
    /// 本地会话数
    pub session_count: usize,
}

/// 单个 agent 的调度配置（前端用来渲染它自己的自动签到开关）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentScheduleView {
    pub enabled: bool,
    pub start_minute: u32,
    pub end_minute: u32,
    pub account_count: usize,
    pub today_claimed: usize,
    pub today_claimable: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub accounts: Vec<AccountView>,
    pub providers: Vec<ProviderView>,
    pub today_points: i64,
    pub claimed_count: usize,
    pub total_count: usize,
    /// 总闸
    pub scheduler_enabled: bool,
    /// 每个 agent 自己的调度配置（前端按 agent 渲染各自的开关）
    pub agents: BTreeMap<String, AgentScheduleView>,
    /// 每个账号今天的计划执行时刻（分钟）
    pub plans: BTreeMap<String, u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountResult {
    pub account_id: String,
    pub provider: String,
    pub label: String,
    /// `success` / `already` / `failed` / `skipped`
    pub status: String,
    pub points: i64,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredAccount {
    pub account: Account,
    pub provider: String,
    pub already_added: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthChallenge {
    pub session_id: String,
    pub auth_url: String,
}

// ---------------------------------------------------------------------------
// 命令
// ---------------------------------------------------------------------------

/// 汇总所有账号的今日状态。
#[tauri::command]
pub async fn overview(state: State<'_, AppState>) -> Result<Overview, String> {
    let store = state.store.lock().unwrap().clone();
    let config = state.config.lock().unwrap().clone();
    let scheduler = state.scheduler.lock().unwrap().clone();
    let logs = logging::recent(1).unwrap_or_default();

    let mut accounts = Vec::new();
    let mut today_points = 0i64;
    let mut claimed_count = 0usize;

    for account in store.list() {
        let provider = match state.provider(&account.provider) {
            Some(p) => p,
            None => continue,
        };
        let cred = match store.credential(&account.provider, &account.id) {
            Ok(c) => c,
            // 单个账号凭据损坏不应让整个界面空白：跳过它，其余照常展示
            Err(_) => continue,
        };

        let (status, error) = match provider.status(&account, &cred).await {
            Ok(s) => {
                if s.state == ClaimState::Claimed {
                    claimed_count += 1;
                }
                today_points += if s.state == ClaimState::Claimed { s.points } else { 0 };
                (Some(s), None)
            }
            Err(e) => (None, Some(e.to_string())),
        };

        // 积分与签到是两个独立请求：积分失败不影响签到状态展示
        let (quota, quota_error) = match provider.quota(&account, &cred).await {
            Ok(q) => (Some(q), None),
            Err(e) => (None, Some(e.to_string())),
        };

        let today_log = logs
            .iter()
            .find(|l| l.account_id == account.id && l.provider == account.provider)
            .cloned();

        let is_active = config.active_of(&account.provider).as_deref() == Some(account.id.as_str());
        let (data_dir, project_count, session_count) = match crate::core::launcher::account_stats(&account.provider) {
            Some((p, s)) => (
                crate::core::launcher::data_dir(&account.provider)
                    .map(|d| d.to_string_lossy().to_string()),
                p,
                s,
            ),
            None => (None, 0, 0),
        };

        accounts.push(AccountView {
            provider_name: provider.display_name().to_string(),
            accent: provider.accent().to_string(),
            account,
            status,
            quota,
            quota_error,
            error,
            today_log,
            is_active,
            data_dir,
            project_count,
            session_count,
        });
    }

    let provider_views = state
        .providers
        .iter()
        .map(|p| {
            let exe = crate::core::launcher::client_exe(p.id());
            ProviderView {
                id: p.id().to_string(),
                name: p.display_name().to_string(),
                accent: p.accent().to_string(),
                docs_url: p.docs_url().to_string(),
                supports_discovery: p.id() == "minimax",
                configured: !store.list_of(p.id()).is_empty(),
                has_client: exe.is_some(),
                client_exe: exe.map(|p| p.to_string_lossy().to_string()),
            }
        })
        .collect();

    // 每个 agent 自己的调度配置
    let mut agents_view: BTreeMap<String, AgentScheduleView> = BTreeMap::new();
    for provider in state.providers.iter() {
        let cfg = config.scheduler.agent(provider.id());
        let list = store.list_of(provider.id());
        let claimed = accounts
            .iter()
            .filter(|a| a.account.provider == provider.id() && a.status.as_ref().map(|s| s.state == ClaimState::Claimed).unwrap_or(false))
            .count();
        let claimable = accounts
            .iter()
            .filter(|a| a.account.provider == provider.id() && a.status.as_ref().map(|s| s.state == ClaimState::Claimable).unwrap_or(false))
            .count();
        agents_view.insert(
            provider.id().to_string(),
            AgentScheduleView {
                enabled: cfg.enabled,
                start_minute: cfg.start_minute,
                end_minute: cfg.end_minute,
                account_count: list.len(),
                today_claimed: claimed,
                today_claimable: claimable,
            },
        );
    }

    let plans = scheduler
        .accounts
        .iter()
        .map(|(k, v)| (k.clone(), v.scheduled_minute))
        .collect();

    Ok(Overview {
        total_count: accounts.len(),
        claimed_count,
        today_points,
        accounts,
        providers: provider_views,
        scheduler_enabled: config.scheduler.enabled,
        agents: agents_view,
        plans,
    })
}

/// 对单个账号执行签到。
#[tauri::command]
pub async fn checkin_account(
    app: AppHandle,
    state: State<'_, AppState>,
    provider_id: String,
    account_id: String,
) -> Result<AccountResult, String> {
    let Some(provider) = state.provider(&provider_id) else {
        return Err(format!("未知平台：{provider_id}"));
    };
    let store = state.store.lock().unwrap().clone();
    let Some(account) = store
        .list_of(&provider_id)
        .into_iter()
        .find(|a| a.id == account_id)
    else {
        return Err(format!("账号不存在：{account_id}"));
    };
    let cred = store
        .credential(&provider_id, &account_id)
        .map_err(|e| e.to_string())?;

    let result = run_checkin(&app, provider.as_ref(), &account, &cred).await;

    // 手动执行后清掉「今日已签」标记，交给调度器重新判定状态。
    if result.status != "failed" {
        let mut sched = state.scheduler.lock().unwrap();
        sched.clear_done(&AccountStore::key(&provider_id, &account_id));
        let _ = sched.save();
    }
    Ok(result)
}

/// 对所有账号执行签到。
#[tauri::command]
pub async fn checkin_all(app: AppHandle, state: State<'_, AppState>) -> Result<Vec<AccountResult>, String> {
    Ok(checkin_all_inner(app, &state).await)
}

/// 对所有账号执行签到（供托盘菜单等非命令入口复用）。
pub async fn checkin_all_inner(app: AppHandle, state: &AppState) -> Vec<AccountResult> {
    let store = state.store.lock().unwrap().clone();
    let mut out = Vec::new();
    for account in store.list() {
        let Some(provider) = state.provider(&account.provider) else {
            continue;
        };
        let cred = match store.credential(&account.provider, &account.id) {
            Ok(c) => c,
            Err(e) => {
                out.push(AccountResult {
                    account_id: account.id.clone(),
                    provider: account.provider.clone(),
                    label: account.label.clone(),
                    status: "failed".into(),
                    points: 0,
                    message: e.to_string(),
                });
                continue;
            }
        };
        out.push(run_checkin(&app, provider.as_ref(), &account, &cred).await);
    }
    out
}

/// 刷新单个账号的积分（不碰签到状态）。
#[tauri::command]
pub async fn refresh_quota(
    state: State<'_, AppState>,
    provider_id: String,
    account_id: String,
) -> Result<Quota, String> {
    let provider = state
        .provider(&provider_id)
        .ok_or_else(|| format!("未知平台：{provider_id}"))?;
    let store = state.store.lock().unwrap().clone();
    let cred = store
        .credential(&provider_id, &account_id)
        .map_err(|e| e.to_string())?;
    let account = store
        .list_of(&provider_id)
        .into_iter()
        .find(|a| a.id == account_id)
        .ok_or_else(|| format!("账号不存在：{account_id}"))?;
    provider.quota(&account, &cred).await.map_err(|e| e.to_string())
}

/// 刷新全部账号的积分。
#[tauri::command]
pub async fn refresh_all_quotas(state: State<'_, AppState>) -> Result<Vec<(String, String, Quota)>, String> {
    let store = state.store.lock().unwrap().clone();
    let mut out = Vec::new();
    for account in store.list() {
        let Some(provider) = state.provider(&account.provider) else {
            continue;
        };
        let Ok(cred) = store.credential(&account.provider, &account.id) else {
            continue;
        };
        if let Ok(q) = provider.quota(&account, &cred).await {
            out.push((account.provider.clone(), account.id.clone(), q));
        }
    }
    Ok(out)
}

/// 从本机客户端自动发现已登录账号（不写盘）。
#[tauri::command]
pub async fn discover_accounts(state: State<'_, AppState>) -> Result<Vec<DiscoveredAccount>, String> {
    let store = state.store.lock().unwrap().clone();
    let mut out = Vec::new();
    for provider in state.providers.iter() {
        if let Ok(found) = provider.discover().await {
            for (account, _) in found {
                let already = store.contains(&account.provider, &account.id);
                out.push(DiscoveredAccount {
                    provider: account.provider.clone(),
                    already_added: already,
                    account,
                });
            }
        }
    }
    Ok(out)
}

/// 把发现的账号加入账号池。
#[tauri::command]
pub async fn add_discovered_account(
    state: State<'_, AppState>,
    provider_id: String,
) -> Result<Vec<Account>, String> {
    let Some(provider) = state.provider(&provider_id) else {
        return Err(format!("未知平台：{provider_id}"));
    };
    let found = provider.discover().await.map_err(|e| e.to_string())?;
    let mut store = state.store.lock().unwrap();
    for (account, cred) in found {
        store.upsert(account, &cred).map_err(|e| e.to_string())?;
    }
    Ok(store.list())
}

/// 发起设备授权：返回一个需要在浏览器里确认的地址。
///
/// Qoder CN 与千问办公共用同一套设备授权 URL，只是各自账号不同，
/// 所以这里按 provider 分发到同一个流程。
#[tauri::command]
pub async fn begin_device_auth(
    state: State<'_, AppState>,
    provider_id: String,
) -> Result<AuthChallenge, String> {
    if provider_id != "qoder" && provider_id != "qwenwork" {
        return Err(format!("{provider_id} 不支持设备授权"));
    }
    let qoder = state.qoder.clone();
    let session = qoder
        .begin_authorization(&provider_id)
        .map_err(|e| e.to_string())?;
    let challenge = AuthChallenge {
        session_id: session.id.clone(),
        auth_url: session.auth_url.clone(),
    };
    {
        let mut sessions = state.auth_sessions.lock().unwrap();
        sessions.insert(session.id.clone(), session);
        save_pending_auth(&sessions);
    }
    Ok(challenge)
}

/// 取当前所有待完成的授权会话（前端启动时恢复轮询用）。
#[tauri::command]
pub async fn pending_auth(state: State<'_, AppState>) -> Result<Vec<AuthChallenge>, String> {
    let sessions = state.auth_sessions.lock().unwrap().clone();
    Ok(sessions
        .into_values()
        .map(|s| AuthChallenge {
            session_id: s.id,
            auth_url: s.auth_url,
        })
        .collect())
}

/// 轮询设备授权结果；`None` 表示用户还没确认。
#[tauri::command]
pub async fn poll_device_auth(
    state: State<'_, AppState>,
    provider_id: String,
    session_id: String,
) -> Result<Option<Account>, String> {
    let session = {
        let sessions = state.auth_sessions.lock().unwrap();
        sessions.get(&session_id).cloned()
    };
    let Some(session) = session else {
        return Err("授权会话已过期，请重新发起".into());
    };
    let qoder = state.qoder.clone();

    match qoder.poll_authorization(&session).await {
        Ok(Some((mut account, cred))) => {
            // 账号归属以发起时的 provider 为准（同一台机器可能同时授权多个平台）
            account.provider = provider_id;
            let mut store = state.store.lock().unwrap();
            store.upsert(account.clone(), &cred).map_err(|e| e.to_string())?;
            {
                let mut sessions = state.auth_sessions.lock().unwrap();
                sessions.remove(&session_id);
                save_pending_auth(&sessions);
            }
            Ok(Some(account))
        }
        Ok(None) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// 从账号池移除账号。
#[tauri::command]
pub async fn remove_account(
    state: State<'_, AppState>,
    provider_id: String,
    account_id: String,
) -> Result<Vec<Account>, String> {
    let mut store = state.store.lock().unwrap();
    store
        .remove(&provider_id, &account_id)
        .map_err(|e| e.to_string())?;
    Ok(store.list())
}

/// 读取配置。
#[tauri::command]
pub async fn get_config(state: State<'_, AppState>) -> Result<AppConfig, String> {
    Ok(state.config.lock().unwrap().clone())
}

/// 写回配置。
#[tauri::command]
pub async fn set_config(
    state: State<'_, AppState>,
    config: AppConfig,
) -> Result<AppConfig, String> {
    // 校验每个 agent 自己的时间窗
    for (id, cfg) in &config.scheduler.agents {
        if !cfg.is_valid() {
            return Err(format!("{id} 的调度时间窗不合法（起必须早于止）"));
        }
    }
    let mut guard = state.config.lock().unwrap();
    *guard = config;
    guard.save().map_err(|e| e.to_string())?;
    Ok(guard.clone())
}

/// 设置某个 agent 自己的自动签到配置（开关 + 时间窗）。
#[tauri::command]
pub async fn set_agent_schedule(
    state: State<'_, AppState>,
    provider_id: String,
    enabled: bool,
    start_minute: u32,
    end_minute: u32,
) -> Result<AppConfig, String> {
    let mut guard = state.config.lock().unwrap();
    let mut cfg = guard.scheduler.agent(&provider_id);
    cfg.enabled = enabled;
    cfg.start_minute = start_minute;
    cfg.end_minute = end_minute;
    if !cfg.is_valid() {
        return Err("时间窗不合法（起必须早于止）".into());
    }
    guard.scheduler.set_agent(&provider_id, cfg);
    guard.save().map_err(|e| e.to_string())?;
    Ok(guard.clone())
}

/// 切换某个 agent 的自动签到总闸（应急用）。
#[tauri::command]
pub async fn set_scheduler_enabled(
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<AppConfig, String> {
    let mut guard = state.config.lock().unwrap();
    guard.scheduler.enabled = enabled;
    guard.save().map_err(|e| e.to_string())?;
    Ok(guard.clone())
}

/// 把某个账号设为该 agent 的当前账号（多账号切换）。
#[tauri::command]
pub async fn set_active_account(
    state: State<'_, AppState>,
    provider_id: String,
    account_id: String,
) -> Result<AppConfig, String> {
    let store = state.store.lock().unwrap().clone();
    if !store.contains(&provider_id, &account_id) {
        return Err(format!("账号不存在：{account_id}"));
    }
    let mut guard = state.config.lock().unwrap();
    guard.set_active(&provider_id, &account_id);
    guard.save().map_err(|e| e.to_string())?;
    Ok(guard.clone())
}

/// 打开该 agent 的客户端（用当前账号的上下文）。
#[tauri::command]
pub async fn launch_client(provider_id: String) -> Result<String, String> {
    let exe = crate::core::launcher::launch(&provider_id).map_err(|e| e.to_string())?;
    Ok(exe.to_string_lossy().to_string())
}

/// 读取最近若干天的签到流水。
#[tauri::command]
pub async fn get_logs(days: Option<i64>) -> Result<Vec<LogEntry>, String> {
    logging::recent(days.unwrap_or(7)).map_err(|e| e.to_string())
}

/// 在系统默认浏览器里打开链接（Qoder 授权页等）。
#[tauri::command]
pub async fn open_external(url: String) -> Result<(), String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("只允许打开 http(s) 链接".into());
    }
    open::that(url).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// 内部
// ---------------------------------------------------------------------------

/// 核心签到流程：查状态 → 需要则领取 → 落流水 → 广播进度。
async fn run_checkin(
    app: &AppHandle,
    provider: &dyn Provider,
    account: &Account,
    cred: &serde_json::Value,
) -> AccountResult {
    let emit = |result: &AccountResult| {
        let _ = app.emit("checkin-progress", result);
    };

    let status = match provider.status(account, cred).await {
        Ok(s) => s,
        Err(e) => {
            let result = AccountResult {
                account_id: account.id.clone(),
                provider: account.provider.clone(),
                label: account.label.clone(),
                status: "failed".into(),
                points: 0,
                message: e.to_string(),
            };
            let _ = logging::append(&LogEntry::new(
                &account.provider,
                &account.id,
                &account.label,
                "failed",
                0,
                &result.message,
            ));
            emit(&result);
            return result;
        }
    };

    if status.state != ClaimState::Claimable {
        let result = AccountResult {
            account_id: account.id.clone(),
            provider: account.provider.clone(),
            label: account.label.clone(),
            status: if status.state == ClaimState::Claimed {
                "already".into()
            } else {
                "skipped".into()
            },
            points: status.points,
            message: if status.message.is_empty() {
                "今日无签到活动".into()
            } else {
                status.message.clone()
            },
        };
        let _ = logging::append(&LogEntry::new(
            &account.provider,
            &account.id,
            &account.label,
            &result.status,
            result.points,
            &result.message,
        ));
        emit(&result);
        return result;
    }

    let outcome = match provider.claim(account, cred).await {
        Ok(o) => o,
        Err(e) => {
            let result = AccountResult {
                account_id: account.id.clone(),
                provider: account.provider.clone(),
                label: account.label.clone(),
                status: "failed".into(),
                points: 0,
                message: e.to_string(),
            };
            let _ = logging::append(&LogEntry::new(
                &account.provider,
                &account.id,
                &account.label,
                "failed",
                0,
                &result.message,
            ));
            emit(&result);
            return result;
        }
    };

    let result = AccountResult {
        account_id: account.id.clone(),
        provider: account.provider.clone(),
        label: account.label.clone(),
        status: if outcome.state == ClaimState::Claimed {
            "success".into()
        } else {
            "failed".into()
        },
        points: outcome.points,
        message: outcome.message.clone(),
    };
    let _ = logging::append(&LogEntry::new(
        &account.provider,
        &account.id,
        &account.label,
        &result.status,
        result.points,
        &result.message,
    ));
    emit(&result);
    result
}

/// 调度器后台循环：每 30 秒检查一次是否有到点的账号。
pub async fn scheduler_loop(app: AppHandle) {
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;

        let state = match app.try_state::<AppState>() {
            Some(s) => s,
            None => continue,
        };

        let config = state.config.lock().unwrap().clone();
        if !config.scheduler.enabled {
            continue;
        }

        let today = scheduler::today();
        let now_minute = scheduler::now_minute();

        // 按 agent 分组：每个 agent 有自己独立的时间窗
        let accounts = state.store.lock().unwrap().list();
        let mut window_by_provider: BTreeMap<String, (u32, u32)> = BTreeMap::new();
        for account in &accounts {
            if !window_by_provider.contains_key(&account.provider) {
                let cfg = config.scheduler.agent(&account.provider);
                window_by_provider
                    .insert(account.provider.clone(), (cfg.start_minute, cfg.end_minute));
            }
        }

        let keys: Vec<String> = accounts
            .iter()
            .map(|a| AccountStore::key(&a.provider, &a.id))
            .collect();

        // 跨天重排计划（各 agent 用自己的时间窗）
        {
            let mut sched = state.scheduler.lock().unwrap();
            for (provider, (start, end)) in &window_by_provider {
                let provider_keys: Vec<String> = accounts
                    .iter()
                    .filter(|a| &a.provider == provider)
                    .map(|a| AccountStore::key(&a.provider, &a.id))
                    .collect();
                sched.roll_over(&today, &provider_keys, *start, *end);
            }
            // 统一清理：只保留仍然存在的账号
            sched.prune(&keys);
            let _ = sched.save();
        }

        for account in accounts {
            // 每个 agent 自己管自己的开关
            let agent_cfg = config.scheduler.agent(&account.provider);
            if !agent_cfg.enabled || !agent_cfg.is_valid() {
                continue;
            }

            let key = AccountStore::key(&account.provider, &account.id);
            let (done, minute) = {
                let sched = state.scheduler.lock().unwrap();
                (
                    sched.is_done(&key, &today),
                    sched.minute_of(&key).unwrap_or(u32::MAX),
                )
            };
            if done || minute == u32::MAX || now_minute < minute {
                continue;
            }
            // 只在计划分钟之后的第一个 tick 执行，避免同一分钟内重复触发
            if now_minute > minute + 1 {
                continue;
            }

            let Some(provider) = state.provider(&account.provider) else {
                continue;
            };
            let cred = match state.store.lock().unwrap().credential(&account.provider, &account.id)
            {
                Ok(c) => c,
                Err(e) => {
                    let result = AccountResult {
                        account_id: account.id.clone(),
                        provider: account.provider.clone(),
                        label: account.label.clone(),
                        status: "failed".into(),
                        points: 0,
                        message: e.to_string(),
                    };
                    let _ = logging::append(&LogEntry::new(
                        &account.provider,
                        &account.id,
                        &account.label,
                        "failed",
                        0,
                        &result.message,
                    ));
                    let _ = app.emit("checkin-progress", &result);
                    continue;
                }
            };

            let result = run_checkin(&app, provider.as_ref(), &account, &cred).await;

            let mut sched = state.scheduler.lock().unwrap();
            if result.status != "failed" {
                sched.mark_done(&key, &today);
            }
            let _ = sched.save();
        }
    }
}
