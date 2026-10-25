//! Tauri 命令层：UI 与后端之间的唯一入口。

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::core::config::AppConfig;
use crate::core::logging::{self, LogEntry};
use crate::core::provider::{Account, CheckinStatus, ClaimState, Provider, Quota};
use crate::core::quota_history;
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
    /// 各平台适配器的具体类型（浏览器授权导入不走通用 trait，需要单独拿到）
    pub qoder: Arc<crate::providers::qoder::QoderProvider>,
    pub minimax: Arc<crate::providers::minimax::MiniMaxProvider>,
    pub workbuddy: Arc<crate::providers::workbuddy::WorkBuddyProvider>,
}

impl AppState {
    pub fn new() -> Self {
        let minimax = Arc::new(crate::providers::minimax::MiniMaxProvider::new());
        let qoder = Arc::new(crate::providers::qoder::QoderProvider::new());
        let workbuddy = Arc::new(crate::providers::workbuddy::WorkBuddyProvider::new());
        let list: Vec<Arc<dyn Provider>> = vec![
            minimax.clone(),
            qoder.clone(),
            Arc::new(crate::providers::miaoshou::MiaoshouProvider::new()),
            workbuddy.clone(),
        ];
        let mut store = AccountStore::load().unwrap_or_default();
        // 旧版本把 MiniMax 登录态记录键的 hash 当账号 id（设备级，换号不变），
        // 会造成「导入新号覆盖旧号」。启动时把池里的旧时代条目归位一次（幂等）。
        crate::providers::minimax::migrate_device_key_ids(&mut store);
        Self {
            store: Mutex::new(store),
            config: Mutex::new(AppConfig::load().unwrap_or_default()),
            scheduler: Mutex::new(SchedulerState::load().unwrap_or_default()),
            auth_sessions: Mutex::new(load_pending_auth()),
            providers: list,
            qoder,
            minimax,
            workbuddy,
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
///
/// 超过 30 分钟的会话直接丢弃：上游的设备码 / 授权 state 本身只有十几分钟
/// 有效期，留着的会话永远等不到确认，还会让启动时出现一条无意义的
/// 「等待授权」横幅（实测残留）。
fn load_pending_auth() -> BTreeMap<String, AuthSession> {
    let path = crate::core::paths::auth_pending_file();
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return BTreeMap::new();
    };
    let Ok(map) = serde_json::from_str::<BTreeMap<String, AuthSession>>(&raw) else {
        return BTreeMap::new();
    };
    let now = chrono::Utc::now().timestamp();
    let live: BTreeMap<String, AuthSession> = map
        .clone()
        .into_iter()
        .filter(|(_, s)| now - s.created_at < 30 * 60)
        .collect();
    if live.len() != map.len() {
        // 有过期会话被清掉，顺手把落盘文件也更新
        if live.is_empty() {
            let _ = std::fs::remove_file(&path);
        } else if let Ok(text) = serde_json::to_string_pretty(&live) {
            let _ = crate::core::paths::atomic_write(&path, text.as_bytes());
        }
    }
    live
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
    /// 是否支持浏览器授权导入账号（设备码 / OAuth 页）
    /// （显式 rename：serde 的 camelCase 会把它转成 supportsOauth，不是 OAuth）
    #[serde(rename = "supportsOAuth")]
    pub supports_oauth: bool,
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
    /// 客户端当前实际登录的是不是这个账号
    pub is_client_login: bool,
    /// 客户端数据目录（项目/会话都在这里）
    pub data_dir: Option<String>,
    /// 本地项目数
    pub project_count: usize,
    /// 本地会话数
    pub session_count: usize,
    /// 积分是否低于预警阈值
    pub is_low: bool,
    /// 积分预警阈值
    pub alert_threshold: u32,
    /// 最近 7 天的积分快照（升序），用于画趋势
    pub trend: Vec<QuotaPoint>,
}

/// 趋势上的一个点。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaPoint {
    pub timestamp: String,
    pub remaining: f64,
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
    /// 该 agent 的数据自动同步间隔（分钟）
    pub refresh_interval_minutes: u32,
    /// 客户端当前实际登录的账号（None = 读不到 / 客户端没登录）
    pub client_account: Option<ClientAccountView>,
    /// 能不能靠「写凭据文件」给这个客户端换账号
    pub switchable: bool,
}

/// 客户端当前登录的账号。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientAccountView {
    pub account_id: String,
    pub label: String,
    /// 这个账号在不在我们的账号池里（不在说明是客户端自己登的号）
    pub in_pool: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub accounts: Vec<AccountView>,
    pub providers: Vec<ProviderView>,
    pub today_points: i64,
    pub claimed_count: usize,
    pub total_count: usize,
    /// 签到总闸
    pub scheduler_enabled: bool,
    /// 自动同步（积分 / 状态 / 资料）总开关
    pub sync_enabled: bool,
    /// 每个 agent 自己的调度配置（前端按 agent 渲染各自的开关）
    pub agents: BTreeMap<String, AgentScheduleView>,
    /// 每个账号今天的计划执行时刻（分钟）
    pub plans: BTreeMap<String, u32>,
    /// 应用数据目录（账号池 / 配置 / 日志都在这里）
    pub data_dir: String,
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
    /// 发起授权的 provider
    pub provider: String,
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
    // 需要改成真实昵称的账号（循环里只收集，最后统一落盘）
    let mut relabel: Vec<Account> = Vec::new();

    // 客户端当前实际登录的账号（只读检测；先算好，账号行与 agent 视图共用，
    // 避免每个账号重复解一次客户端登录态文件）。
    // ⚠️ MiniMax 走 token 实测（client_session）：客户端配置里的 sharedUser
    // 在外部切换后是陈旧的，信它会把身份判错、污染账号池（2026-10-25 踩坑）。
    let mut client_login: BTreeMap<String, Option<ClientAccountView>> = BTreeMap::new();
    for provider in state.providers.iter() {
        let detected = if provider.id() == "minimax" {
            crate::providers::minimax::client_session()
                .await
                .map(|(id, name, _)| (id, name))
        } else {
            crate::core::client_switch::detect(provider.id())
                .ok()
                .flatten()
                .map(|d| (d.account_id, d.label))
        };
        client_login.insert(
            provider.id().to_string(),
            detected.map(|(account_id, label)| ClientAccountView {
                in_pool: store.contains(provider.id(), &account_id),
                account_id,
                label,
            }),
        );
    }

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
        // 多账号状态保存：当前登录的捕获最新 token，非当前登录的临过期自动刷新
        let cred = ensure_fresh_cred(&state, &account, &cred).await;

        // 兜底 label（`Qoder 01a1348c-3fc` 这种 id 前缀）才去换真实昵称，
        // 避免每 45 秒给每个账号多打一次资料接口。换到就落盘，之后不用再查。
        let mut display_account = account.clone();
        if account.label_is_fallback() {
            if let Ok(Some(profile)) = provider.profile(&account, &cred).await {
                if let Some(name) = profile.name.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                    display_account.label = name.to_string();
                    relabel.push(display_account.clone());
                }
            }
        }

        let (status, error) = match provider.status(&display_account, &cred).await {
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
        let (quota, quota_error) = match provider.quota(&display_account, &cred).await {
            Ok(q) => {
                // 落一条积分快照（趋势图用）。失败不致命。
                let _ = quota_history::append(&quota_history::QuotaSnapshot::new(
                    &account.provider,
                    &account.id,
                    &account.label,
                    q.total_remaining,
                    q.total_granted,
                    q.total_consumed,
                ));
                (Some(q), None)
            }
            Err(e) => (None, Some(e.to_string())),
        };

        let today_log = logs
            .iter()
            .find(|l| l.account_id == account.id && l.provider == account.provider)
            .cloned();

        let is_active = config.active_of(&account.provider).as_deref() == Some(account.id.as_str());
        let is_client_login = client_login
            .get(&account.provider)
            .and_then(|c| c.as_ref())
            .map(|c| c.account_id == account.id)
            .unwrap_or(false);
        let (data_dir, project_count, session_count) = match crate::core::launcher::account_stats(&account.provider) {
            Some((p, s)) => (
                crate::core::launcher::data_dir(&account.provider)
                    .map(|d| d.to_string_lossy().to_string()),
                p,
                s,
            ),
            None => (None, 0, 0),
        };
        let threshold = config.alert_threshold_of(&account.provider);
        let trend = quota_history::recent_of(&account.provider, &account.id, 7)
            .unwrap_or_default()
            .into_iter()
            .map(|s| QuotaPoint {
                timestamp: s.timestamp,
                remaining: s.remaining,
            })
            .collect();
        // 积分是否低于预警阈值
        let is_low = quota
            .as_ref()
            .map(|q| q.total_remaining < threshold as f64)
            .unwrap_or(false);

        accounts.push(AccountView {
            provider_name: provider.display_name().to_string(),
            accent: provider.accent().to_string(),
            account: display_account,
            status,
            quota,
            quota_error,
            error,
            today_log,
            is_active,
            is_client_login,
            data_dir,
            project_count,
            session_count,
            is_low,
            alert_threshold: threshold,
            trend,
        });
    }

    // 把换到的真实昵称落盘（只写变化的，且整体一次写完）
    if !relabel.is_empty() {
        let mut guard = state.store.lock().unwrap();
        for account in &relabel {
            let _ = guard.touch(account.clone());
        }
    }

    // 平台按用户拖拽的自定义顺序排列（未配置的按注册顺序排在后面）
    let ordered_ids = config.sort_provider_ids(
        state.providers.iter().map(|p| p.id().to_string()).collect(),
    );

    let provider_views = ordered_ids
        .iter()
        .filter_map(|id| state.provider(id))
        .map(|p| {
            let exe = crate::core::launcher::client_exe(p.id());
            ProviderView {
                id: p.id().to_string(),
                name: p.display_name().to_string(),
                accent: p.accent().to_string(),
                docs_url: p.docs_url().to_string(),
                supports_discovery: matches!(p.id(), "minimax" | "miaoshou" | "workbuddy"),
                supports_oauth: matches!(p.id(), "qoder" | "minimax" | "workbuddy"),
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
                refresh_interval_minutes: config.refresh_interval_of(provider.id()),
                // 客户端当前登录的账号（只读，读不到就是 None）
                client_account: client_login.get(provider.id()).cloned().flatten(),
                switchable: crate::core::client_switch::is_switchable(provider.id()),
            },
        );
    }

    let plans = scheduler
        .accounts
        .iter()
        .map(|(k, v)| (k.clone(), v.scheduled_minute))
        .collect();

    let data_dir = crate::core::paths::data_dir().to_string_lossy().to_string();

    Ok(Overview {
        total_count: accounts.len(),
        claimed_count,
        today_points,
        accounts,
        providers: provider_views,
        scheduler_enabled: config.scheduler.enabled,
        sync_enabled: config.scheduler.sync_enabled,
        agents: agents_view,
        plans,
        data_dir,
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
    let cred = ensure_fresh_cred(&state, &account, &cred).await;

    let result = run_checkin(provider.as_ref(), &account, &cred).await;
    record_result(&app, &account, &result, true);

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
                let result = AccountResult {
                    account_id: account.id.clone(),
                    provider: account.provider.clone(),
                    label: account.label.clone(),
                    status: "failed".into(),
                    points: 0,
                    message: e.to_string(),
                };
                record_result(&app, &account, &result, true);
                out.push(result);
                continue;
            }
        };
        let cred = ensure_fresh_cred(state, &account, &cred).await;
        let result = run_checkin(provider.as_ref(), &account, &cred).await;
        record_result(&app, &account, &result, true);
        out.push(result);
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
    let quota = provider.quota(&account, &cred).await.map_err(|e| e.to_string())?;
    // 用户主动刷新看到的最新积分也应该进趋势图，否则曲线和界面显示对不上
    let _ = quota_history::append(&quota_history::QuotaSnapshot::new(
        &account.provider,
        &account.id,
        &account.label,
        quota.total_remaining,
        quota.total_granted,
        quota.total_consumed,
    ));
    Ok(quota)
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

/// 发起浏览器授权导入（Qoder / MiniMax / WorkBuddy）：返回一个需要在浏览器里确认的地址。
#[tauri::command]
pub async fn begin_device_auth(
    state: State<'_, AppState>,
    provider_id: String,
) -> Result<AuthChallenge, String> {
    let now = chrono::Utc::now().timestamp();
    let session = match provider_id.as_str() {
        "qoder" => state
            .qoder
            .begin_authorization(&provider_id)
            .map_err(|e| e.to_string())?,
        "minimax" => {
            let d = state
                .minimax
                .begin_device_authorization()
                .await
                .map_err(|e| e.to_string())?;
            AuthSession {
                id: uuid::Uuid::new_v4().to_string(),
                provider: provider_id.clone(),
                auth_url: d.auth_url,
                verifier: d.code_verifier,
                nonce: d.poll_code,
                machine_id: String::new(),
                created_at: now,
                extra: serde_json::json!({ "poll_param": d.poll_param, "interval": d.interval_secs }),
            }
        }
        "workbuddy" => {
            let (st, url) = state
                .workbuddy
                .begin_authorization()
                .await
                .map_err(|e| e.to_string())?;
            AuthSession {
                id: uuid::Uuid::new_v4().to_string(),
                provider: provider_id.clone(),
                auth_url: url,
                verifier: String::new(),
                nonce: st,
                machine_id: String::new(),
                created_at: now,
                extra: serde_json::Value::Null,
            }
        }
        other => return Err(format!("{other} 不支持浏览器授权导入")),
    };
    let challenge = AuthChallenge {
        session_id: session.id.clone(),
        auth_url: session.auth_url.clone(),
        provider: session.provider.clone(),
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
            provider: s.provider,
        })
        .collect())
}

/// 轮询浏览器授权结果；`None` 表示用户还没确认。
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

    let polled = match session.provider.as_str() {
        "qoder" => state.qoder.poll_authorization(&session).await,
        "minimax" => {
            let poll_param = session.extra["poll_param"].as_str().unwrap_or("device_code");
            state
                .minimax
                .poll_device_authorization(&session.verifier, &session.nonce, poll_param)
                .await
        }
        "workbuddy" => state.workbuddy.poll_authorization(&session.nonce).await,
        other => return Err(format!("{other} 不支持浏览器授权导入")),
    };

    match polled {
        Ok(Some((mut account, cred))) => {
            // 账号归属以发起时的 provider 为准
            account.provider = provider_id.clone();
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
    {
        let mut store = state.store.lock().unwrap();
        store
            .remove(&provider_id, &account_id)
            .map_err(|e| e.to_string())?;
    }
    // 删掉的是「当前账号」的话，同步清掉引用 —— 否则 config 里留下指向
    // 已删账号的野指针，界面的「当前账号」徽标永远匹配不上
    {
        let mut config = state.config.lock().unwrap();
        if config.active_of(&provider_id).as_deref() == Some(account_id.as_str()) {
            config.active_accounts.remove(&provider_id);
            config.save().map_err(|e| e.to_string())?;
        }
    }
    Ok(state.store.lock().unwrap().list())
}

/// 保存侧栏 agent 的拖拽顺序（校验：必须是已注册平台的排列）。
#[tauri::command]
pub async fn set_agent_order(
    state: State<'_, AppState>,
    order: Vec<String>,
) -> Result<AppConfig, String> {
    let known: Vec<&str> = state.providers.iter().map(|p| p.id()).collect();
    for id in &order {
        if !known.contains(&id.as_str()) {
            return Err(format!("未知平台：{id}"));
        }
    }
    let unique = order.len() == order.iter().collect::<std::collections::BTreeSet<_>>().len();
    if !unique {
        return Err("顺序里有重复的平台".into());
    }
    let mut guard = state.config.lock().unwrap();
    guard.agent_order = order;
    guard.save().map_err(|e| e.to_string())?;
    Ok(guard.clone())
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
        if cfg.refresh_interval_minutes == 0 {
            return Err(format!("{id} 的同步间隔不合法（至少 1 分钟）"));
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

/// 设置某个 agent 的积分预警阈值。
#[tauri::command]
pub async fn set_alert_threshold(
    state: State<'_, AppState>,
    provider_id: String,
    value: u32,
) -> Result<AppConfig, String> {
    let mut guard = state.config.lock().unwrap();
    guard.set_alert_threshold(&provider_id, value);
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

/// 切换「自动同步」总开关（积分 / 签到状态 / 账号资料）。
#[tauri::command]
pub async fn set_sync_enabled(
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<AppConfig, String> {
    let mut guard = state.config.lock().unwrap();
    guard.scheduler.sync_enabled = enabled;
    guard.save().map_err(|e| e.to_string())?;
    Ok(guard.clone())
}

/// 设置某个 agent 的数据自动同步间隔（分钟）。
#[tauri::command]
pub async fn set_refresh_interval(
    state: State<'_, AppState>,
    provider_id: String,
    minutes: u32,
) -> Result<AppConfig, String> {
    if !(1..=24 * 60).contains(&minutes) {
        return Err("同步间隔需在 1–1440 分钟之间".into());
    }
    let mut guard = state.config.lock().unwrap();
    let mut cfg = guard.scheduler.agent(&provider_id);
    cfg.refresh_interval_minutes = minutes;
    guard.scheduler.set_agent(&provider_id, cfg);
    guard.save().map_err(|e| e.to_string())?;
    Ok(guard.clone())
}

/// 立即同步全部账号（不等间隔）。返回同步过的账号数。
#[tauri::command]
pub async fn sync_now(app: AppHandle, state: State<'_, AppState>) -> Result<usize, String> {
    let accounts = state.store.lock().unwrap().list();
    let mut done = 0usize;
    for account in &accounts {
        if sync_one(&app, &state, account).await {
            done += 1;
        }
    }
    let now = chrono::Local::now().timestamp();
    {
        let mut sched = state.scheduler.lock().unwrap();
        for provider in state.providers.iter() {
            if accounts.iter().any(|a| a.provider == provider.id()) {
                sched.mark_synced(provider.id(), now);
            }
        }
        let _ = sched.save();
    }
    let _ = app.emit("accounts-synced", ());
    Ok(done)
}

/// 在文件管理器里打开应用数据目录。
#[tauri::command]
pub async fn open_data_dir() -> Result<(), String> {
    let dir = crate::core::paths::data_dir();
    crate::core::paths::ensure_dir(&dir).map_err(|e| e.to_string())?;
    open::that(dir).map_err(|e| e.to_string())
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

/// 读客户端当前实际登录的账号（只读，不改任何文件）。
#[tauri::command]
pub async fn detect_client_account(
    provider_id: String,
) -> Result<Option<crate::core::client_switch::DetectedAccount>, String> {
    crate::core::client_switch::detect(&provider_id).map_err(|e| e.to_string())
}

/// 把客户端**当前登录**的账号捕获进账号池（借鉴 Cockpit 的切换保护）。
///
/// 「切换客户端账号」切走前调用：客户端同一时刻只能登一个号，切走后它的
/// 凭据就不再更新 —— 先捕获，谁都不丢，且池里留的是客户端续期过的最新凭据。
/// MiniMax / 妙手走 `discover`（读明文登录态）；Qoder 解密 auth.v1.dat。
async fn capture_client_login(state: &AppState, provider_id: &str) {
    if provider_id == "qoder" {
        if let Some((account, cred)) = crate::core::client_switch::qoder_client_login() {
            let mut store = state.store.lock().unwrap();
            let _ = store.upsert(account, &cred);
        }
        return;
    }
    let Some(provider) = state.provider(provider_id) else {
        return;
    };
    let Ok(found) = provider.discover().await else {
        return;
    };
    let mut store = state.store.lock().unwrap();
    for (account, cred) in found {
        let _ = store.upsert(account, &cred);
    }
}

/// 把客户端切换成指定账号，然后启动客户端。
///
/// ⚠️ 会改写客户端的登录态文件（先备份）。**客户端在运行时会被拒绝**
/// （`client_switch::switch` 入口统一拦截），否则它内存里的旧登录态
/// 会覆盖我们写进去的文件。
///
/// 切换成功后顺手把这个账号设为该 agent 的「当前账号」——
/// 客户端登的是谁，工具里的当前账号就应该是谁，两边保持一致。
#[tauri::command]
pub async fn switch_client_account(
    state: State<'_, AppState>,
    provider_id: String,
    account_id: String,
) -> Result<crate::core::client_switch::SwitchOutcome, String> {
    if !crate::core::client_switch::is_switchable(&provider_id) {
        return Err(format!("{provider_id} 不支持切换客户端账号（凭据格式无法注入）"));
    }
    let store = state.store.lock().unwrap().clone();
    let account = store
        .list_of(&provider_id)
        .into_iter()
        .find(|a| a.id == account_id)
        .ok_or_else(|| format!("账号不存在：{account_id}"))?;
    let cred = store
        .credential(&provider_id, &account_id)
        .map_err(|e| e.to_string())?;

    // 切走前先把客户端当前登录的账号捕获进池 —— 谁都不丢，
    // 池里留下的还是客户端续期过的最新凭据（借鉴 Cockpit）
    capture_client_login(&state, &provider_id).await;

    let outcome =
        crate::core::client_switch::switch(&provider_id, &cred, &account_id, &account.label)
            .map_err(|e| e.to_string())?;

    // 客户端已经登录这个账号了，工具里的「当前账号」跟着同步
    {
        let mut config = state.config.lock().unwrap();
        config.set_active(&provider_id, &account_id);
        config.save().map_err(|e| e.to_string())?;
    }
    Ok(outcome)
}

/// 客户端是否正在运行（前端在「切换客户端账号」前先问一次，给个友好的拦截提示）。
#[tauri::command]
pub async fn client_running(provider_id: String) -> Result<bool, String> {
    Ok(crate::core::launcher::is_running(&provider_id))
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

/// 让账号凭据保持新鲜 —— **多账号状态保存的核心**（方案借鉴 Cockpit Tools）：
///
/// * 客户端**当前登录**的账号 → 从客户端登录态**捕获**最新 token 回写池
///   （客户端开着会自动续期；捕获不动客户端的 refresh token，现场安全）。
/// * **不在客户端里**的账号 → token 临过期时用 refresh token 走 OAuth 刷新。
///   MiniMax 的 RT 每次刷新都会轮换，新 RT 回写池 —— 所以池里的账号即使
///   长期不是当前登录，也能一直活着、正常签到。
///
/// 刷新失败（RT 被吊销等）返回原凭据，调用方按普通错误如实展示。
async fn ensure_fresh_cred(state: &AppState, account: &Account, cred: &serde_json::Value) -> serde_json::Value {
    // MiniMax 特殊路径：当前登录账号从客户端捕获（不触发 OAuth 刷新，保护客户端现场）
    if account.provider == "minimax" {
        if let Some(fresh) = crate::providers::minimax::capture_current_cred(account).await {
            let changed = fresh.get("access_token").and_then(|v| v.as_str())
                != cred.get("access_token").and_then(|v| v.as_str());
            if changed {
                let mut store = state.store.lock().unwrap();
                let _ = store.upsert(account.clone(), &fresh);
            }
            return fresh;
        }
    }

    // MiniMax 非当前登录：还有 5 分钟以上有效期就不动，临过期才刷新
    if account.provider == "minimax" {
        let now_ms = chrono::Utc::now().timestamp_millis();
        let expires = cred.get("expires_at_ms").and_then(|v| v.as_i64()).unwrap_or(0);
        if expires > now_ms + 5 * 60_000 {
            return cred.clone();
        }
    } else {
        // 其他平台：凭据里带过期时间且还没到（1 小时缓冲）就不动；
        // 没带过期时间的（如 WorkBuddy 部分凭据）交给各平台 refresh 自行判断
        let now_ms = chrono::Utc::now().timestamp_millis();
        let expires = cred.get("expires_at")
            .and_then(|v| v.as_i64())
            .or_else(|| cred.get("expires_at_ms").and_then(|v| v.as_i64()))
            .unwrap_or(0);
        if expires > now_ms + 60 * 60_000 {
            return cred.clone();
        }
    }

    let Some(provider) = state.provider(&account.provider) else {
        return cred.clone();
    };
    match provider.refresh(account, cred).await {
        Ok(Some(fresh)) => {
            let mut store = state.store.lock().unwrap();
            let _ = store.upsert(account.clone(), &fresh);
            fresh
        }
        // 刷新失败（RT 吊销等）返回原凭据，调用方按普通错误如实展示
        _ => cred.clone(),
    }
}

/// 核心签到流程：查状态 → 需要则领取。
///
/// **只返回结果，不落流水** —— 落不落流水由调用方决定（调度器对
/// 「上游还没开窗」的重试不落，否则一个时间窗内会刷出几十条没信息量的记录）。
async fn run_checkin(
    provider: &dyn Provider,
    account: &Account,
    cred: &serde_json::Value,
) -> AccountResult {
    let status = match provider.status(account, cred).await {
        Ok(s) => s,
        Err(e) => {
            return AccountResult {
                account_id: account.id.clone(),
                provider: account.provider.clone(),
                label: account.label.clone(),
                status: "failed".into(),
                points: 0,
                message: e.to_string(),
            };
        }
    };

    if status.state != ClaimState::Claimable {
        return AccountResult {
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
    }

    let outcome = match provider.claim(account, cred).await {
        Ok(o) => o,
        Err(e) => {
            return AccountResult {
                account_id: account.id.clone(),
                provider: account.provider.clone(),
                label: account.label.clone(),
                status: "failed".into(),
                points: 0,
                message: e.to_string(),
            };
        }
    };

    AccountResult {
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
    }
}

/// 落一条签到流水 + 广播进度给前端。
///
/// `record == false` 时两者都不做 —— 调度器对「上游还没开窗」的重试用它，
/// 否则一个时间窗内会刷出几十条没有信息量的记录。
fn record_result(app: &AppHandle, account: &Account, result: &AccountResult, record: bool) {
    if record {
        let _ = logging::append(&LogEntry::new(
            &account.provider,
            &account.id,
            &account.label,
            &result.status,
            result.points,
            &result.message,
        ));
    }
    let _ = app.emit("checkin-progress", result);
}

/// 同步一个账号：刷新签到状态 + 积分（落快照）+ 账号资料（补真实昵称）。
///
/// 单个账号失败不影响其他账号，返回是否成功取到过数据。
async fn sync_one(app: &AppHandle, state: &AppState, account: &Account) -> bool {
    let Some(provider) = state.provider(&account.provider) else {
        return false;
    };
    let store = state.store.lock().unwrap().clone();
    let Ok(cred) = store.credential(&account.provider, &account.id) else {
        return false;
    };
    let cred = ensure_fresh_cred(state, account, &cred).await;

    // 签到状态（失败不致命，UI 里仍以 overview 的结果为准）
    let _ = provider.status(account, &cred).await;

    // 积分 + 落一条快照（趋势图用）
    if let Ok(q) = provider.quota(account, &cred).await {
        let _ = quota_history::append(&quota_history::QuotaSnapshot::new(
            &account.provider,
            &account.id,
            &account.label,
            q.total_remaining,
            q.total_granted,
            q.total_consumed,
        ));
    }

    // 账号资料：只有兜底 label 才去换真实昵称
    if account.label_is_fallback() {
        if let Ok(Some(profile)) = provider.profile(account, &cred).await {
            if let Some(name) = profile.name.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                let mut renamed = account.clone();
                renamed.label = name.to_string();
                let mut guard = state.store.lock().unwrap();
                let _ = guard.touch(renamed);
            }
        }
    }

    let _ = app.emit("account-synced", (&account.provider, &account.id));
    true
}

/// 调度器后台循环。
///
/// 每 30 秒一轮，做三件事：
///   1. 跨天时重排计划（各 agent 用自己的时间窗）；
///   2. 到点的账号执行签到（受「签到总闸」控制）；
///   3. 按 agent 自己的间隔自动同步积分 / 签到状态 / 账号资料
///      （受「自动同步」开关控制）。**跨天时不看间隔，强制同步一次**，
///      这样「今天已签 / 今天可签」不会停在昨天的状态。
pub async fn scheduler_loop(app: AppHandle) {
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;

        let state = match app.try_state::<AppState>() {
            Some(s) => s,
            None => continue,
        };

        let config = state.config.lock().unwrap().clone();

        let today = scheduler::today();
        let now_minute = scheduler::now_minute();
        let now_ts = chrono::Local::now().timestamp();

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

        // 跨天检测：计划日期不是今天就说明过了一天
        let day_changed = {
            let sched = state.scheduler.lock().unwrap();
            sched.date != today
        };

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

        // ---- 到点签到（受「签到总闸」控制）----
        if config.scheduler.enabled {
            for account in accounts.clone() {
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
                // 时间窗内「补跑」：计划分钟已经过了但今天还没处理过，就现在做。
                // 这样晚启动（比如 10:20 才打开软件）也不会整天漏签。
                if now_minute > agent_cfg.end_minute {
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
                // 自动签到多在早上跑，非当前登录账号的 token（1 小时寿命）必然已过期 ——
                // 签到前先刷新凭据，否则永远签不上
                let cred = ensure_fresh_cred(&state, &account, &cred).await;

                let result = run_checkin(provider.as_ref(), &account, &cred).await;

                // 「skipped」= 上游今天还没开窗。既**不能标记成已处理**
                //（否则一整天都不再重试），也不值得落流水 / 广播 ——
                // 否则一个时间窗内会刷出几十条没有信息量的记录。
                let opened = result.status == "success" || result.status == "already";
                record_result(&app, &account, &result, opened);

                let mut sched = state.scheduler.lock().unwrap();
                if opened {
                    sched.mark_done(&key, &today);
                }
                let _ = sched.save();
            }
        }

        // ---- 自动同步（积分 / 状态 / 资料）----
        if config.scheduler.sync_enabled {
            // 每个 agent 距上次同步多久了
            let last_sync: BTreeMap<String, i64> = {
                let sched = state.scheduler.lock().unwrap();
                state
                    .providers
                    .iter()
                    .map(|p| {
                        (
                            p.id().to_string(),
                            sched.last_sync_of(p.id()).unwrap_or(0),
                        )
                    })
                    .collect()
            };

            let mut synced_providers: Vec<String> = Vec::new();
            for provider in state.providers.iter() {
                let list: Vec<Account> = accounts
                    .iter()
                    .filter(|a| a.provider == provider.id())
                    .cloned()
                    .collect();
                if list.is_empty() {
                    continue;
                }
                let interval_secs = config.refresh_interval_of(provider.id()) as i64 * 60;
                let elapsed = now_ts - last_sync.get(provider.id()).copied().unwrap_or(0);
                // 跨天强制同步；否则按各自间隔
                if !day_changed && elapsed < interval_secs {
                    continue;
                }
                for account in &list {
                    sync_one(&app, &state, account).await;
                }
                synced_providers.push(provider.id().to_string());
            }

            if !synced_providers.is_empty() {
                let mut sched = state.scheduler.lock().unwrap();
                for pid in &synced_providers {
                    sched.mark_synced(pid, now_ts);
                }
                let _ = sched.save();
                let _ = app.emit("accounts-synced", ());
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 命令行入口（不启 GUI）
//
// 界面里的「从本机导入」和「添加账号」需要点按钮，在无界面 / 自动化场景下
// 用不了，所以补两个等价命令：
//   CheckinBox --import-local              导入本机客户端登录态
//   CheckinBox --device-auth qoder
// ---------------------------------------------------------------------------

/// 把所有「读本机客户端登录态」的平台账号导入账号池（MiniMax / 妙手）。
pub fn cli_import_local() -> Result<(), String> {
    let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    rt.block_on(async {
        let state = AppState::new();
        let providers: Vec<Arc<dyn Provider>> = state
            .providers
            .iter()
            .filter(|p| matches!(p.id(), "minimax" | "miaoshou" | "workbuddy"))
            .cloned()
            .collect();

        for provider in providers {
            let found = match provider.discover().await {
                Ok(f) => f,
                Err(e) => {
                    println!("  {} 发现失败：{e}", provider.display_name());
                    continue;
                }
            };
            if found.is_empty() {
                println!("  {}：本机客户端没有登录态", provider.display_name());
                continue;
            }
            for (account, cred) in found {
                state
                    .store
                    .lock()
                    .unwrap()
                    .upsert(account.clone(), &cred)
                    .map_err(|e| e.to_string())?;
                println!("  已导入 {} · {}", provider.display_name(), account.label);
            }
        }
        Ok(())
    })
}

/// 命令行版：把客户端切换成指定账号（备份原登录态 → 写入 → 启动客户端）。
///
/// 用途：脚本里切账号，或者界面点不动时的兜底。
///   CheckinBox --switch-client <minimax|miaoshou> <account_id>
pub fn cli_switch_client(provider_id: &str, account_id: &str) -> Result<(), String> {
    let state = AppState::new();
    let store = state.store.lock().unwrap().clone();
    let account = store
        .list_of(provider_id)
        .into_iter()
        .find(|a| a.id == account_id)
        .ok_or_else(|| format!("账号不存在：{account_id}"))?;
    let cred = store.credential(provider_id, account_id).map_err(|e| e.to_string())?;

    let out = crate::core::client_switch::switch(provider_id, &cred, account_id, &account.label)
        .map_err(|e| e.to_string())?;

    println!("已把 {} 的客户端账号切换成 {}", provider_id, account.label);
    println!("  写入凭据: {}", out.target_file);
    if let Some(bak) = out.backup {
        println!("  原登录态备份: {bak}");
    }
    println!("  客户端: {}", out.client_exe);
    Ok(())
}

/// 命令行版的设备授权：发起会话、落盘、打印授权地址。
///
/// **不在这里轮询** —— 轮询交给应用本身：会话写进 `auth_pending.json`，
/// 应用启动时会读回来接着轮询（和界面里「等待授权」那条提示是同一条路径）。
/// 命令行进程在后台容易被收掉，让它长时间轮询不可靠。
pub fn cli_device_auth(provider_id: &str) -> Result<(), String> {
    if provider_id != "qoder" {
        return Err(format!("不支持设备授权：{provider_id}"));
    }
    let state = AppState::new();
    let session = state
        .qoder
        .begin_authorization(provider_id)
        .map_err(|e| e.to_string())?;

    let mut sessions = load_pending_auth();
    sessions.insert(session.id.clone(), session.clone());
    save_pending_auth(&sessions);

    println!("授权地址：{}", session.auth_url);
    println!(
        "已登记为待完成会话（provider={}）。启动应用后会显示「等待授权」提示并自动轮询，"
        , session.provider
    );
    println!("在浏览器里打开上面的地址，用要登录的账号点「确认授权」即可。");
    Ok(())
}
