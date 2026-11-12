//! MiniMax Code 每日签到适配器。
//!
//! 协议（已在本机实测）：
//!   * 登录态：`~/.minimax/auth/<region>/mcode-public/auth.json`，**明文 JSON**，
//!     含 `accessToken`（`mmoat_`）/ `refreshToken`（`mmort_`）/ `expiresAtMs`；
//!   * 查状态：`GET  https://agent.minimax.cn/minimax-cloud/api/v1/signin/status`
//!   * 领取：  `POST https://agent.minimax.cn/minimax-cloud/api/v1/signin/claim`
//!   * 两个接口都必须带 `?timezone_id=<IANA>`，否则报 `1406010011 invalid timezone_id`；
//!   * 响应包一层 `base_resp.status_code`（0 为成功）与 `data`。
//!
//! 面板语义：`days[].status` 1=Upcoming 2=Claimable 3=Claimed 4=Disabled；
//! 领取结果 `claim_result` 1=Claimed 2=AlreadyClaimed。

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Mutex;

use crate::core::provider::{
    Account, CheckinStatus, ClaimOutcome, ClaimState, CreditItem, Provider, Quota,
};
use crate::core::launcher::ClientProcessState;
use crate::core::tz;

const BASE: &str = "https://agent.minimax.cn";
const STATUS_PATH: &str = "/minimax-cloud/api/v1/signin/status";
const CLAIM_PATH: &str = "/minimax-cloud/api/v1/signin/claim";
/// OAuth token 端点（刷新 access token 用）。协议来自客户端逆向（app.asar
/// 的 oauth-client.js / endpoints.js）+ 本机实测，详见 `refresh` 的注释。
const TOKEN_ENDPOINT: &str = "https://account.minimax.cn/oauth2/token";
/// OAuth 设备授权端点（浏览器授权导入账号用），协议同样来自 app.asar + 实测。
const DEVICE_AUTH_ENDPOINT: &str = "https://account.minimax.cn/oauth2/device/code";
const USER_AGENT: &str = "MiniMax Code";

const STATUS_UPCOMING: i64 = 1;
const STATUS_CLAIMABLE: i64 = 2;
const STATUS_CLAIMED: i64 = 3;

pub struct MiniMaxProvider {
    http: reqwest::Client,
}

impl MiniMaxProvider {
    pub fn new() -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .user_agent(USER_AGENT)
            .build()
            .unwrap_or_default();
        Self { http }
    }
}

/// 递归收集本机 MiniMax Code 的登录态文件。
///
/// 实际布局是 `~/.minimax/auth/<buildEnv>/<region>/<clientId>/auth.json`
/// （例如 `auth/prod/cn/mcode-public/auth.json`），层级随版本会变，
/// 所以按目录深度递归找，而不是写死层数。
fn auth_files() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Some(home) = dirs::home_dir() else {
        return out;
    };
    let root = home.join(".minimax").join("auth");
    collect_auth_files(&root, &mut out, 0);
    // 排序确定化：read_dir 顺序未定义，机器上有多套命名空间时
    // 「取第一个」会选到不同的文件 —— 官方默认布局（prod/cn）优先，
    // 其余按路径字典序，保证每次都选同一个。
    out.sort_by(|a, b| {
        let pa = a.to_string_lossy().to_lowercase();
        let pb = b.to_string_lossy().to_lowercase();
        let da = pa.contains("prod") && pa.contains("cn");
        let db = pb.contains("prod") && pb.contains("cn");
        db.cmp(&da).then_with(|| pa.cmp(&pb))
    });
    out
}

fn collect_auth_files(dir: &PathBuf, out: &mut Vec<PathBuf>, depth: usize) {
    if depth > 4 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_auth_files(&path, out, depth + 1);
        } else if path.file_name().and_then(|n| n.to_str()) == Some("auth.json") {
            out.push(path);
        }
    }
}

/// 读本机客户端的用户信息，用于给账号起个看得懂的名字。
fn local_profile() -> Option<Value> {
    let appdata = std::env::var("APPDATA").ok()?;
    let path = PathBuf::from(appdata)
        .join("MiniMax")
        .join("minimax-agent-cn-config.json");
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

/// 读本机客户端的用户信息，用于给账号起个看得懂的名字。
pub fn local_label() -> String {
    local_profile()
        .and_then(|p| {
            let shared = p.get("sharedUser")?;
            shared
                .get("subUserName")
                .or_else(|| shared.get("userName"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| "本机账号".to_string())
}

/// 本机客户端**当前登录账号**的真实账号 id（客户端配置里的 `sharedUser.userID`）。
///
/// ⚠️ 不能拿登录态文件记录键的 hash 当账号 id —— 实测那是**设备级**的：
/// 在客户端里换号登录后记录键不变（只有 token 换成新号的）。拿它当 id，
/// 「从本机导入」新号时会跟旧号共用同一个键，直接覆盖掉池里的旧号条目。
pub fn local_user_id() -> Option<String> {
    local_profile().and_then(|p| {
        p.get("sharedUser")?
            .get("userID")?
            .as_str()
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
    })
}

/// 本机客户端登录态文件的路径（**只取路径，不读内容**）。
///
/// 设备授权导入的账号没有登录态文件归属，用它把凭据锚定到正确的命名空间，
/// 避免换号时全盘搜索选中别的环境。文件里的记录属于客户端当前登录的那个
/// 账号，与本次授权的账号无关 —— **绝不能把它的凭据一起取出来**。
fn local_auth_path() -> Option<PathBuf> {
    auth_files().into_iter().next()
}

/// 本机登录态文件（含路径）。
fn local_auth_with_path() -> Option<(PathBuf, Value)> {
    let path = auth_files().into_iter().next()?;
    let raw = std::fs::read_to_string(&path).ok()?;
    let parsed: Value = serde_json::from_str(&raw).ok()?;
    Some((path, parsed))
}

/// 登录态文件里第一条非空 token（属于客户端**当前登录**的那个账号）。
fn file_current_token() -> Option<String> {
    let (_, parsed) = local_auth_with_path()?;
    parsed["records"].as_object()?.values().find_map(|rec| {
        rec["accessToken"]
            .as_str()
            .filter(|t| !t.is_empty())
            .map(String::from)
    })
}

/// 客户端当前登录的是不是这个账号。
///
/// 以** token 实测身份**为准（见 `client_session` 的注释）：缓存里有身份且
/// **当前文件里的 token 前缀和缓存时一致**才算 —— token 轮换/被外部切换后
/// 缓存自动失效（返回 false，走快照路径），绝不会拿别的账号的 token 用。
fn client_login_is(account: &Account) -> bool {
    let guard = IDENTITY_CACHE.lock().unwrap();
    let Some((cached_prefix, user_id, _)) = guard.as_ref() else {
        return false;
    };
    if file_token_prefix().as_deref() != Some(cached_prefix.as_str()) {
        return false;
    }
    user_id.as_str() == account.id
}

/// 登录态文件里当前 token 的前缀（用于判断身份缓存是否仍然有效）。
fn file_token_prefix() -> Option<String> {
    let token = file_current_token()?;
    Some(token.chars().take(32).collect())
}

// ---------------------------------------------------------------------------
// 客户端会话身份（token 实测，不信配置缓存）
// ---------------------------------------------------------------------------

/// 身份缓存：`(token 前缀, userID, 昵称)`。token 轮换后前缀变化，自动失效重探。
static IDENTITY_CACHE: Mutex<Option<(String, String, String)>> = Mutex::new(None);

// ---------------------------------------------------------------------------
// 凭据落盘钩子（commands 层注入，避免 providers → commands 循环依赖）
// ---------------------------------------------------------------------------

/// 401 抢救刷新后的回写/读取钩子。账号池是唯一权威：刷新必须成对落盘，
/// 否则轮换掉的 RT 就永久丢了（2026-10-26 那次「1 小时必死」的根因）。
static PERSIST_HOOK: Mutex<Option<fn(&str, &str, &Value)>> = Mutex::new(None);
static FETCH_HOOK: Mutex<Option<fn(&str, &str) -> Option<Value>>> = Mutex::new(None);

/// 应用启动时注入一次。
pub fn set_cred_hooks(persist: fn(&str, &str, &Value), fetch: fn(&str, &str) -> Option<Value>) {
    *PERSIST_HOOK.lock().unwrap() = Some(persist);
    *FETCH_HOOK.lock().unwrap() = Some(fetch);
}

fn persist_cred(provider_id: &str, account_id: &str, cred: &Value) {
    if let Some(f) = PERSIST_HOOK.lock().unwrap().as_ref() {
        f(provider_id, account_id, cred);
    }
}

fn fetch_cred(provider_id: &str, account_id: &str) -> Option<Value> {
    let guard = FETCH_HOOK.lock().unwrap();
    let f = guard.as_ref()?;
    f(provider_id, account_id)
}

/// 用 access token 调客户端自用的用户信息接口，拿**真实账号身份**。
///
/// 协议来自 app.asar 的 `fetchIdentity`（本机实测）：
/// `GET {BASE}/v1/api/user/info?device_platform=mcode&biz_id=3&app_id=3001…`
/// → `data.userInfo.{userID, subUserName}`。
async fn probe_identity(http: &reqwest::Client, token: &str) -> Result<(String, String)> {
    let url = format!(
        "{BASE}/v1/api/user/info?device_platform=mcode&biz_id=3&app_id=3001&version_code=22201&unix={}&timezone_offset=-480",
        chrono::Utc::now().timestamp()
    );
    let resp: Value = http
        .get(&url)
        .bearer_auth(token)
        .header("accept", "application/json")
        .header("accept-language", "zh-CN,zh;q=0.9")
        .send()
        .await?
        .json()
        .await?;
    if resp["statusInfo"]["code"].as_i64().unwrap_or(-1) != 0 {
        return Err(anyhow!(
            "身份探测失败：{}",
            resp["statusInfo"]["message"]
                .as_str()
                .unwrap_or("上游返回异常")
        ));
    }
    let user = &resp["data"]["userInfo"];
    let uid = user["userID"].as_str().unwrap_or("");
    if uid.is_empty() {
        return Err(anyhow!("身份探测响应里没有 userID（上游可能已变更）"));
    }
    let name = user["subUserName"]
        .as_str()
        .or_else(|| user["name"].as_str())
        .unwrap_or("");
    Ok((uid.to_string(), name.to_string()))
}

/// 客户端**当前会话**：`(真实 userID, 昵称, 最新凭据)`。
///
/// ⚠️ 身份必须用 token 调接口实测 —— 客户端配置里的 `sharedUser` 只在客户端
/// 自己走登录流程时更新，「外部切换」后它一直是陈旧的。用它判定身份会把
/// A 账号的 token 捕获进 B 账号的池条目（2026-10-25 实测踩坑：两个池条目
/// 被污染成同一个会话）。探测结果按 token 前缀缓存，token 轮换才重新探测；
/// 探测失败返回 None —— 宁可不捕获，也不错绑到别的账号。
pub async fn client_session() -> Option<(String, String, Value)> {
    let (path, parsed) = local_auth_with_path()?;
    let rec = parsed["records"].as_object()?.values().find(|r| {
        r["accessToken"]
            .as_str()
            .map(|t| !t.is_empty())
            .unwrap_or(false)
    })?;
    let token = rec["accessToken"].as_str()?;
    let prefix: String = token.chars().take(32).collect();

    {
        let guard = IDENTITY_CACHE.lock().unwrap();
        if let Some((cached_prefix, id, name)) = guard.as_ref() {
            if *cached_prefix == prefix {
                return Some((
                    id.clone(),
                    name.clone(),
                    json!({
                        "access_token": token,
                        "refresh_token": rec["refreshToken"].as_str().unwrap_or(""),
                        "expires_at_ms": rec["expiresAtMs"].as_i64().unwrap_or(0),
                        "login_epoch": rec["loginEpoch"].as_str().unwrap_or(""),
                        "auth_file": path.to_string_lossy().to_string(),
                    }),
                ));
            }
        }
    }

    let provider = MiniMaxProvider::new();
    let (id, name) = probe_identity(&provider.http, token).await.ok()?;
    *IDENTITY_CACHE.lock().unwrap() = Some((prefix, id.clone(), name.clone()));
    Some((
        id,
        name,
        json!({
            "access_token": token,
            "refresh_token": rec["refreshToken"].as_str().unwrap_or(""),
            "expires_at_ms": rec["expiresAtMs"].as_i64().unwrap_or(0),
            "login_epoch": rec["loginEpoch"].as_str().unwrap_or(""),
            "auth_file": path.to_string_lossy().to_string(),
        }),
    ))
}

/// 客户端当前登录态与本账号的关系。
///
/// `Unknown` 很关键：探测失败时不能假设「不是当前账号」——MiniMax 的 RT
/// 用一次就轮换，刷错账号会直接把客户端顶下线（2026-11-02 实测）。
pub enum ClientLoginState {
    /// 客户端当前登录的就是这个账号（附带客户端最新凭据）
    Same(Value),
    /// 客户端当前登录的是别的账号（可以安全刷新本账号）
    Other,
    /// 无法确认（探测失败 / 客户端未登录）
    Unknown,
}

impl ClientLoginState {
    /// 是否允许我们自己刷新这个账号的 RT。
    ///
    /// 只有「确认客户端登录的是别的账号」，或「**确认**客户端没在运行」才允许。
    ///
    /// ⚠️ 进程状态未知（枚举失败 / 平台不支持）时一律不允许 ——
    /// 宁可不刷新，也不能轮换掉客户端手里的 RT：那会让客户端下次续期直接
    /// `invalid_grant` 掉登录（2026-11-11 上游 cockpit-tools 实测掉线）。
    pub fn allow_self_refresh(&self, process: ClientProcessState) -> bool {
        match self {
            Self::Same(_) => false,
            Self::Other => true,
            Self::Unknown => process == ClientProcessState::Absent,
        }
    }
}

/// 探测客户端当前登录账号与本账号的关系（失败绝不猜）。
pub async fn client_login_state(account: &Account) -> ClientLoginState {
    match client_session().await {
        Some((id, _name, cred)) if id == account.id => ClientLoginState::Same(cred),
        Some(_) => ClientLoginState::Other,
        None => ClientLoginState::Unknown,
    }
}

impl MiniMaxProvider {
    /// 带一次重试的 GET。
    ///
    /// MiniMax 的 token 约 1 小时过期，客户端续期有几分钟空窗；
    /// 空窗内请求会失败，等 2 秒重读本机登录态再试一次就能恢复。
    async fn get_json(&self, url: &str, account: &Account, cred: &Value) -> Result<Value> {
        self.request_with_retry(url, account, cred, false).await
    }

    /// 带重试的 POST（领取用）。
    async fn post_json(&self, url: &str, account: &Account, cred: &Value) -> Result<Value> {
        self.request_with_retry(url, account, cred, true).await
    }

    /// 统一的请求 + 重试。
    ///
    /// MiniMax 的 token 约 1 小时过期，客户端会在到期前后续期，中间有几十秒空窗。
    /// 空窗内请求会拿到 401，所以 **401/403 时多重试几次**（间隔递增），
    /// 其他错误只重试一次。
    ///
    /// 401 还有第二种来头（2026-10-27 实测）：**上游提前吊销** —— 客户端切号/
    /// 重新登录会把旧会话作废，池里的 token 明明 `expires_at_ms` 未到却已被拒。
    /// 这种等是等不来的，所以**非当前登录**的账号 401 时立即强制走一次 RT 刷新
    /// （并落盘）再用新 token 重试；刷新也救不回来才如实报错。
    async fn request_with_retry(
        &self,
        url: &str,
        account: &Account,
        cred: &Value,
        is_post: bool,
    ) -> Result<Value> {
        // 鉴权类错误的退避：2s / 5s / 10s；其他错误只试两次
        let backoff = [2000u64, 5000, 10000];
        let mut last_err = anyhow!("请求失败");
        let mut attempt = 0usize;
        // 「为什么没自动刷新」的说明，最后并进错误信息
        let mut blocked: Option<String> = None;
        // 可变副本：401 抢救刷新后换成新凭据继续本次请求
        let mut cred = cred.clone();
        // 每次请求链最多抢救一次，避免刷新风暴
        let mut rescued = false;

        loop {
            let token = current_token(account, &cred)?;
            let result = if is_post {
                self.http
                    .post(url)
                    .bearer_auth(&token)
                    .header("accept", "application/json")
                    .header("content-type", "application/json")
                    .json(&json!({}))
                    .send()
                    .await
            } else {
                self.http
                    .get(url)
                    .bearer_auth(&token)
                    .header("accept", "application/json")
                    .header("accept-language", "zh-CN,zh;q=0.9")
                    .send()
                    .await
            };

            match result {
                Ok(resp) if resp.status().is_success() => {
                    return resp.json().await.map_err(|e| anyhow!("解析响应失败：{e}"));
                }
                Ok(resp) => {
                    let code = resp.status().as_u16();
                    if code == 401 || code == 403 {
                        // 抢救刷新前先确认「刷这个账号不会把客户端顶下线」。
                        // 客户端在跑且身份未知时绝不刷（RT 轮换即作废）。
                        let login_state = client_login_state(account).await;
                        // 进程状态是三态：枚举失败 = 「不知道」，不等于「没在跑」。
                        let process = crate::core::launcher::client_process_state("minimax");
                        if login_state.allow_self_refresh(process) {
                            if rescued {
                                return Err(anyhow!(
                                    "凭据已失效（HTTP {code}）：自动刷新后仍被拒绝。\
                                     请在客户端登录此账号后重新导入，或用「切换客户端账号」切过去"
                                ));
                            }
                            rescued = true;
                            match self.force_refresh_cred(account, &cred).await {
                                Some(fresh) => {
                                    cred = fresh;
                                    attempt += 1;
                                    continue; // 立刻用新 token 重试，不退避
                                }
                                None => {
                                    return Err(anyhow!(
                                        "凭据已失效（HTTP {code}）：自动刷新也没能恢复 —— \
                                         refresh token 可能已被吊销（此账号曾在客户端登录后又切走）。\
                                         请在客户端登录此账号后重新导入，或用「切换客户端账号」切过去"
                                    ));
                                }
                            }
                        }
                        // 客户端根本没在运行 → 没人续期 token，退避重试只是白等。
                        // 快速失败，把「该怎么办」说清楚（这也是首屏加载慢的常见原因）。
                        if process == ClientProcessState::Absent {
                            return Err(anyhow!(
                                "凭据已过期（HTTP {code}）：MiniMax Code 客户端未在运行，\
                                 token 不会被续期。打开一次客户端即可恢复"
                            ));
                        }
                        // 不能自己刷时，把原因记下来，最后一并说清楚 ——
                        // 别让用户只看到一个干巴巴的 HTTP 401。
                        if blocked.is_none() {
                            blocked = Some(match (&login_state, process) {
                                (ClientLoginState::Same(_), _) => {
                                    "客户端当前登录的就是这个账号，刷新会轮换掉它手里的 refresh token \
                                     （会导致客户端掉线），所以没有自动刷新；\
                                     请在客户端里重新登录该账号，或退出客户端后重试"
                                        .to_string()
                                }
                                (_, ClientProcessState::Absent) => {
                                    "MiniMax Code 客户端未在运行，token 不会被续期；\
                                     打开一次客户端即可恢复"
                                        .to_string()
                                }
                                _ => {
                                    "无法确认 MiniMax Code 客户端是否在运行，为避免顶掉客户端登录没有自动刷新；\
                                     请退出 MiniMax Code 客户端后重试"
                                        .to_string()
                                }
                            });
                        }
                    }
                    last_err = anyhow!("HTTP {code}");
                    // 鉴权失败才继续重试；其它错误重试一次就够
                    let auth_error = code == 401 || code == 403;
                    if !auth_error && attempt >= 1 {
                        break;
                    }
                    if auth_error && attempt >= backoff.len() {
                        break;
                    }
                }
                Err(e) => {
                    last_err = anyhow!("{e}");
                    if attempt >= 1 {
                        break;
                    }
                }
            }

            let delay = backoff.get(attempt).copied().unwrap_or(10000);
            attempt += 1;
            tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
        }

        // 有「没自动刷新」的原因时并进错误信息，别只留一个干巴巴的 HTTP 401
        if let Some(reason) = blocked {
            return Err(anyhow!("{last_err}：{reason}"));
        }
        Err(last_err)
    }

    /// 401 时的抢救：先看账号池里有没有别的任务刚刷新出的新凭据（避免重复
    /// 轮换 RT），没有才强制走一次 OAuth 刷新并**立即落盘**。
    /// 返回 None = RT 也死了，只能重新导入。
    async fn force_refresh_cred(&self, account: &Account, cred: &Value) -> Option<Value> {
        let my_token = cred
            .get("access_token")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if let Some(pool) = fetch_cred(&account.provider, &account.id) {
            let pool_token = pool
                .get("access_token")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if !pool_token.is_empty() && pool_token != my_token {
                return Some(pool); // 别人刚刷新过：直接采纳，不再轮换
            }
        }
        match self.refresh(account, cred).await {
            Ok(Some(fresh)) => {
                persist_cred(&account.provider, &account.id, &fresh);
                Some(fresh)
            }
            _ => None,
        }
    }
}

/// 取当前可用的 access token。
///
/// MiniMax 的 token 生命周期很短（实测约 1 小时），本机客户端会自动续期并把新
/// token 写回 `auth.json`。客户端是**单账号**的：文件里的 token 永远属于
/// **当前登录**的那个账号 —— 只有当这个账号就是当前登录账号时才能用文件 token，
/// 否则会拿别人的 token 发请求，把别的账号的积分/签到状态当成自己的展示（数据串号）。
/// 此时只能回退到导入时存的 token 快照（客户端不再续期它，多半已过期，
/// 401 会如实报错 —— 宁可报错也不显示错数据）。
fn current_token(account: &Account, cred: &Value) -> Result<String> {
    if client_login_is(account) {
        if let Some(token) = file_current_token() {
            return Ok(token);
        }
    }
    cred["access_token"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("缺少 access token"))
}

// （记录键 hash 当账号 id 的做法已删 —— 它是设备级的，正是多账号覆盖事故的根因）

/// 极简 percent-encode（只有时区名需要处理 `/`）。
fn urlenc(value: &str) -> String {
    value
        .chars()
        .map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
            _ => format!("%{:02X}", c as u32),
        })
        .collect()
}

/// 校验 `base_resp` 包装层。
fn check_base_resp(body: &Value) -> Result<()> {
    let Some(code) = body["base_resp"]["status_code"].as_i64() else {
        return Ok(());
    };
    if code == 0 {
        return Ok(());
    }
    let msg = body["base_resp"]["status_msg"]
        .as_str()
        .unwrap_or("未知错误");
    Err(anyhow!("上游返回错误 {code}：{msg}"))
}

/// 从面板算连续签到天数。
fn compute_streak(days: &[Value]) -> i32 {
    let mut sorted: Vec<&Value> = days.iter().collect();
    sorted.sort_by_key(|d| d["day_no"].as_i64().unwrap_or(0));

    let Some(today_index) = sorted
        .iter()
        .position(|d| d["is_today"].as_bool().unwrap_or(false))
    else {
        return 0;
    };
    let today_status = sorted[today_index]["status"].as_i64().unwrap_or(0);

    let mut index = match today_status {
        STATUS_CLAIMED => today_index as i64,
        STATUS_CLAIMABLE => today_index as i64 - 1,
        _ => -1,
    };
    let mut streak = 0i32;
    while index >= 0 {
        if sorted[index as usize]["status"].as_i64().unwrap_or(0) != STATUS_CLAIMED {
            break;
        }
        streak += 1;
        index -= 1;
    }
    streak
}

#[async_trait]
impl Provider for MiniMaxProvider {
    fn id(&self) -> &'static str {
        "minimax"
    }

    fn display_name(&self) -> &'static str {
        "MiniMax Code"
    }

    fn accent(&self) -> &'static str {
        // iOS 系统蓝，和妙手的橙、Qoder 的紫区分开
        "#0a84ff"
    }

    fn docs_url(&self) -> &'static str {
        "https://agent.minimax.io/docs/changelog"
    }

    async fn discover(&self) -> Result<Vec<(Account, Value)>> {
        // 身份必须 token 实测（`client_session`）：sharedUser 配置在外部切换后是
        // 陈旧的，信它会把 A 的凭据挂到 B 的账号条目上（污染账号池）。
        // 探测失败（网络/上游异常）就返回空 —— 宁可不导入，也不错绑。
        let Some((id, name, cred)) = client_session().await else {
            return Ok(Vec::new());
        };
        let label = if name.is_empty() {
            "本机账号".to_string()
        } else {
            name
        };
        Ok(vec![(
            Account {
                id,
                provider: "minimax".to_string(),
                label,
                created_at: chrono::Utc::now().timestamp(),
                last_used: None,
                origin: "本机 MiniMax Code 登录态".to_string(),
            },
            cred,
        )])
    }

    async fn status(&self, account: &Account, cred: &Value) -> Result<CheckinStatus> {
        let tz = urlenc(&tz::iana_timezone());
        let url = format!("{BASE}{STATUS_PATH}?timezone_id={tz}");

        let body: Value = self.get_json(&url, account, cred).await?;

        check_base_resp(&body)?;

        let empty = Vec::new();
        let days = body["data"]["days"].as_array().unwrap_or(&empty);
        let Some(today) = days
            .iter()
            .find(|d| d["is_today"].as_bool().unwrap_or(false))
        else {
            return Ok(CheckinStatus {
                state: ClaimState::NotAvailable,
                message: "上游没有返回今天的签到信息".to_string(),
                ..Default::default()
            });
        };

        let status = today["status"].as_i64().unwrap_or(0);
        let points = today["points"].as_i64().unwrap_or(0);
        let bonus = today["bonus_points"].as_i64().unwrap_or(0);
        let day_index = today["day_no"].as_i64().map(|v| v as i32);
        let streak = compute_streak(days);

        let (state, message) = match status {
            STATUS_CLAIMABLE => (
                ClaimState::Claimable,
                format!("可签到 +{} 积分", points + bonus),
            ),
            STATUS_CLAIMED => (ClaimState::Claimed, "今日已签到".to_string()),
            STATUS_UPCOMING => (ClaimState::NotAvailable, "今日暂不可签".to_string()),
            _ => (ClaimState::Unknown, "签到状态未知".to_string()),
        };

        Ok(CheckinStatus {
            state,
            points: points + bonus,
            streak_days: streak,
            message,
            day_index,
            bonus_points: bonus,
        })
    }

    async fn claim(&self, account: &Account, cred: &Value) -> Result<ClaimOutcome> {
        let tz = urlenc(&tz::iana_timezone());
        let url = format!("{BASE}{CLAIM_PATH}?timezone_id={tz}");

        let body: Value = self.post_json(&url, account, cred).await?;

        check_base_resp(&body)?;

        let data = &body["data"];
        let result = data["claim_result"].as_i64().unwrap_or(0);
        let points = data["points"].as_i64().unwrap_or(0);

        match result {
            1 => Ok(ClaimOutcome::claimed(
                points,
                format!("签到成功 +{points} 积分"),
            )),
            2 => Ok(ClaimOutcome::already(points, "今日已签到")),
            _ => Ok(ClaimOutcome::failed(
                data["claim_id"].as_str().unwrap_or("上游未返回领取结果"),
            )),
        }
    }

    async fn quota(&self, account: &Account, cred: &Value) -> Result<Quota> {
        let tz = urlenc(&tz::iana_timezone());
        let url = format!("{BASE}/minimax-cloud/api/v1/credit/details?timezone_id={tz}");

        let body: Value = self.get_json(&url, account, cred).await?;

        check_base_resp(&body)?;

        // credit_type: 1 = 购买, 2 = 签到赠送
        let type_name = |t: i64| -> &'static str {
            match t {
                1 => "purchased",
                _ => "free",
            }
        };
        let amount = |v: &Value| -> f64 {
            let n = v
                .as_str()
                .and_then(|s| s.replace(',', "").trim().parse::<f64>().ok())
                .or_else(|| v.as_f64())
                .unwrap_or(0.0);
            // 上游会返回 "-0.00" 这种形式，归一成 0 避免界面显示 -0
            if n == 0.0 {
                0.0
            } else {
                n
            }
        };

        let empty = Vec::new();
        let details = body["details"].as_array().unwrap_or(&empty);

        let mut items = Vec::new();
        for d in details {
            items.push(CreditItem {
                credit_type: type_name(d["credit_type"].as_i64().unwrap_or(0)).to_string(),
                granted: amount(&d["granted_amount"]),
                consumed: amount(&d["consumed_amount"]),
                remaining: amount(&d["remaining_amount"]),
                granted_at_ms: d["granted_at_ms"].as_i64().unwrap_or(0),
                expire_at_ms: d["expire_at_ms"].as_i64().unwrap_or(0),
                package_name: String::new(),
                refresh_at_ms: 0,
            });
        }

        let total_granted: f64 = items.iter().map(|i| i.granted).sum();
        let total_consumed: f64 = items.iter().map(|i| i.consumed).sum();
        let total_remaining: f64 = items.iter().map(|i| i.remaining).sum();

        let soonest = items.iter().map(|i| i.expire_at_ms).min().unwrap_or(0);
        let message = if soonest > 0 && total_remaining > 0.0 {
            let days = ((soonest - chrono::Utc::now().timestamp_millis()) / 86_400_000).max(0);
            format!("剩余 {total_remaining:.0} 积分，最近一批 {days} 天后到期")
        } else {
            "当前无剩余积分".to_string()
        };

        Ok(Quota {
            total_granted,
            total_consumed,
            total_remaining,
            items,
            message,
            refresh_at_ms: 0,
            credit_groups: Vec::new(),
        })
    }

    /// 用 refresh token 换新的 access token（让**不在客户端里**的账号也活着）。
    ///
    /// 协议来自客户端逆向（app.asar 的 `oauth-client.js` + `endpoints.js`），
    /// 已在本机实测：
    ///
    /// ```text
    /// POST https://account.minimax.cn/oauth2/token   (application/x-www-form-urlencoded)
    ///   grant_type=refresh_token  refresh_token=<mmort_…>
    ///   client_id=mcode-public  scope=agent.default  audience=agent-backend
    /// → { access_token: mmoat_…, refresh_token: mmort_…, expires_in: 3600, … }
    /// ```
    ///
    /// ⚠️ **refresh token 每次刷新都会轮换**（旧 RT 作废）。因此：
    ///   1. 刷新成功后必须把新 RT 回写账号池（调用方 `ensure_fresh_cred` 负责）；
    ///   2. **绝不能**对「客户端当前登录的账号」调用 —— 客户端内存里还握着旧 RT，
    ///      轮换会让它下次续期失败掉登录（借鉴 Cockpit 的运行中账号保护）。
    async fn refresh(&self, _account: &Account, cred: &Value) -> Result<Option<Value>> {
        let refresh_token = cred["refresh_token"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow!("凭据里没有 refresh token，无法刷新"))?;

        let resp = self
            .http
            .post(TOKEN_ENDPOINT)
            .form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token),
                ("client_id", "mcode-public"),
                ("scope", "agent.default"),
                ("audience", "agent-backend"),
            ])
            .send()
            .await?;

        let status = resp.status();
        let body: Value = resp.json().await.unwrap_or(Value::Null);
        if !status.is_success() {
            let err = body["error"]
                .as_str()
                .or_else(|| body["error_description"].as_str())
                .unwrap_or("");
            return Err(anyhow!(
                "token 刷新失败（HTTP {} {err}）：refresh token 可能已被吊销，请在客户端登录此账号后重新导入",
                status.as_u16()
            ));
        }

        let access = body["access_token"].as_str().unwrap_or("");
        if access.is_empty() {
            return Err(anyhow!("刷新响应里没有 access_token（上游可能已变更）"));
        }

        let mut fresh = cred.clone();
        fresh["access_token"] = json!(access);
        // refresh token 会轮换，新 RT 必须一起保存
        if let Some(rt) = body["refresh_token"].as_str().filter(|s| !s.is_empty()) {
            fresh["refresh_token"] = json!(rt);
        }
        let expires_in = body["expires_in"].as_i64().unwrap_or(3600);
        fresh["expires_at_ms"] = json!(chrono::Utc::now().timestamp_millis() + expires_in * 1000);
        Ok(Some(fresh))
    }
}

// ---------------------------------------------------------------------------
// OAuth 设备授权（浏览器授权导入账号，协议来自 app.asar 的 oauth-client.js）
// ---------------------------------------------------------------------------

/// 发起的设备授权会话。
pub struct DeviceAuthorization {
    /// 让用户在浏览器里打开的地址（已带 user_code）
    pub auth_url: String,
    /// PKCE code_verifier
    pub code_verifier: String,
    /// 轮询用的码（device_code 或 user_code，取决于上游支持哪种）
    pub poll_code: String,
    /// 轮询参数名："device_code" 或 "user_code"
    pub poll_param: String,
    /// 建议轮询间隔（秒）
    pub interval_secs: u64,
    /// 授权码有效期（秒）—— 上游只给 5 分钟，超时后网页会报「缺少有效的 user_code」
    pub expires_in: u64,
}

impl MiniMaxProvider {
    /// 发起设备授权：POST /oauth2/device/code（PKCE S256）。
    pub async fn begin_device_authorization(&self) -> Result<DeviceAuthorization> {
        use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64URL;
        use base64::Engine;

        let mut verifier_bytes = [0u8; 32];
        getrandom::getrandom(&mut verifier_bytes).map_err(|e| anyhow!("取随机数失败：{e}"))?;
        let code_verifier = B64URL.encode(verifier_bytes);
        use sha2::{Digest, Sha256};
        let challenge = B64URL.encode(Sha256::digest(code_verifier.as_bytes()));

        let resp: Value = self
            .http
            .post(DEVICE_AUTH_ENDPOINT)
            .header("content-type", "application/x-www-form-urlencoded")
            .form(&[
                ("client_id", "mcode-public"),
                ("scope", "agent.default"),
                ("audience", "agent-backend"),
                ("code_challenge", challenge.as_str()),
                ("code_challenge_method", "S256"),
            ])
            .send()
            .await?
            .json()
            .await?;

        let user_code = resp["user_code"].as_str().unwrap_or("");
        let device_code = resp["device_code"].as_str().unwrap_or("");
        let verification = resp["verification_uri_complete"]
            .as_str()
            .or_else(|| resp["verification_uri"].as_str())
            .ok_or_else(|| anyhow!("设备授权接口没返回验证地址（上游可能已变更）"))?;
        if user_code.is_empty() && device_code.is_empty() {
            return Err(anyhow!("设备授权接口没返回 user_code / device_code"));
        }
        // 上游给了 device_code 就用 device_code 轮询，否则用 user_code（客户端同款逻辑）
        let (poll_code, poll_param) = if !device_code.is_empty() {
            (device_code.to_string(), "device_code".to_string())
        } else {
            (user_code.to_string(), "user_code".to_string())
        };
        Ok(DeviceAuthorization {
            auth_url: verification.to_string(),
            code_verifier,
            poll_code,
            poll_param,
            interval_secs: resp["interval"].as_u64().unwrap_or(5).max(1),
            expires_in: resp["expires_in"].as_u64().unwrap_or(300).max(30),
        })
    }

    /// 轮询设备授权结果；`Ok(None)` = 用户还没在浏览器里确认。
    pub async fn poll_device_authorization(
        &self,
        code_verifier: &str,
        poll_code: &str,
        poll_param: &str,
    ) -> Result<Option<(Account, Value)>> {
        let grant = format!("urn:ietf:params:oauth:grant-type:device_code");
        let resp = self
            .http
            .post(TOKEN_ENDPOINT)
            .header("content-type", "application/x-www-form-urlencoded")
            .form(&[
                ("grant_type", grant.as_str()),
                (poll_param, poll_code),
                ("client_id", "mcode-public"),
                ("code_verifier", code_verifier),
            ])
            .send()
            .await?;
        let status = resp.status();
        let body: Value = resp.json().await.unwrap_or(Value::Null);

        let resp_status = body["status"].as_str().unwrap_or("");
        if resp_status == "pending" || resp_status == "authorization_pending" {
            return Ok(None);
        }
        if resp_status == "slow_down" {
            return Ok(None);
        }
        if resp_status == "denied" || resp_status == "access_denied" {
            return Err(anyhow!("授权被拒绝（浏览器里点了取消？）"));
        }
        if !status.is_success() {
            // 400 + pending 也会走 HTTP 错误码，这里按未确认处理
            let err = body["error"].as_str().unwrap_or("");
            if err == "authorization_pending" || err == "slow_down" {
                return Ok(None);
            }
            // 授权码过期：网页那边会显示「授权链接无效 / 缺少有效的 user_code」，
            // 根因是上游的 device code 只有 5 分钟有效期（2026-11-02 实测）
            if err == "expired_token" || err == "expired" {
                return Err(anyhow!("授权码已过期（有效期约 5 分钟）：请重新发起授权"));
            }
            return Err(anyhow!("授权失败（HTTP {} {err}）", status.as_u16()));
        }

        let access = body["access_token"].as_str().unwrap_or("");
        if access.is_empty() {
            return Ok(None);
        }
        let refresh = body["refresh_token"].as_str().unwrap_or("");
        let expires_in = body["expires_in"].as_i64().unwrap_or(3600);

        // 身份用 token 实测（和导入/捕获同一条路径）
        let (id, name) = probe_identity(&self.http, access).await.unwrap_or_default();
        if id.is_empty() {
            return Err(anyhow!(
                "授权成功但拿不到账号身份（user/info 接口异常），请稍后重试"
            ));
        }
        let account = Account {
            id,
            provider: "minimax".to_string(),
            label: if name.is_empty() {
                "MiniMax 账号".to_string()
            } else {
                name
            },
            created_at: chrono::Utc::now().timestamp(),
            last_used: None,
            origin: "浏览器授权登录".to_string(),
        };
        let mut cred = json!({
            "access_token": access,
            "refresh_token": refresh,
            "expires_at_ms": chrono::Utc::now().timestamp_millis() + expires_in * 1000,
        });
        // 锚定客户端登录态文件：换号时直接用这个路径，不再全盘搜索
        // （多套命名空间时全盘搜索会选错环境）。只记路径，不取里面的凭据。
        if let Some(path) = local_auth_path() {
            cred["auth_file"] = json!(path.to_string_lossy());
        }
        Ok(Some((account, cred)))
    }
}

// ---------------------------------------------------------------------------
// 账号池迁移
// ---------------------------------------------------------------------------

/// 一次性迁移：旧版本把登录态文件**记录键的 hash** 当账号 id（实测那是设备级的，
/// 换号登录键不变），导致「从本机导入」新号时覆盖池里的旧号条目。
///
/// 这里把设备键时代的条目归位：删掉它们，按**当前登录态**重新落一条
/// （id = 真实账号 id `sharedUser.userID`，token 取文件里的最新值 ——
/// 文件里的记录本来就属于当前登录账号）。
///
/// 幂等：归位后 id 不再等于任何记录键 hash，不会重复触发。
/// 丢掉的旧号凭据救不回来（早被覆盖），要找回旧号：在客户端登录它 → 重新导入。
pub fn migrate_device_key_ids(store: &mut crate::core::store::AccountStore) {
    use crate::core::store::AccountStore;

    let Some((path, parsed)) = local_auth_with_path() else {
        return;
    };
    let Some(records) = parsed["records"].as_object() else {
        return;
    };
    let Some(user_id) = local_user_id() else {
        return;
    };

    // 池里有没有「id = 记录键 hash」的旧时代条目
    let stale_keys: Vec<String> = records
        .keys()
        .map(|k| {
            let hash = k.rsplit('\u{0}').next().unwrap_or(k.as_str());
            AccountStore::key("minimax", hash)
        })
        .filter(|k| store.accounts.contains_key(k))
        .collect();
    if stale_keys.is_empty() {
        return;
    }

    for k in &stale_keys {
        store.accounts.remove(k);
    }

    // 文件里的记录就是当前登录账号的（取第一条非空 token），按新 id 落一条干净的
    if let Some((_key, rec)) = records.iter().find(|(_, r)| {
        r["accessToken"]
            .as_str()
            .map(|t| !t.is_empty())
            .unwrap_or(false)
    }) {
        let account = Account {
            id: user_id,
            provider: "minimax".to_string(),
            label: local_label(),
            created_at: chrono::Utc::now().timestamp(),
            last_used: None,
            origin: "本机 MiniMax Code 登录态（迁移）".to_string(),
        };
        let cred = json!({
            "access_token": rec["accessToken"].as_str().unwrap_or(""),
            "refresh_token": rec["refreshToken"].as_str().unwrap_or(""),
            "expires_at_ms": rec["expiresAtMs"].as_i64().unwrap_or(0),
            "login_epoch": rec["loginEpoch"].as_str().unwrap_or(""),
            "auth_file": path.to_string_lossy().to_string(),
        });
        if store.upsert(account, &cred).is_err() {
            // upsert 失败时也把删除落盘，避免下次重复迁移一半
            let _ = store.save();
        }
    } else {
        let _ = store.save();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 客户端登录态_刷新许可规则() {
        use crate::core::launcher::ClientProcessState;
        // 当前登录：任何情况都不自己刷（否则会把客户端 RT 轮换掉）
        assert!(!ClientLoginState::Same(Value::Null).allow_self_refresh(ClientProcessState::Running));
        assert!(!ClientLoginState::Same(Value::Null).allow_self_refresh(ClientProcessState::Absent));
        assert!(!ClientLoginState::Same(Value::Null).allow_self_refresh(ClientProcessState::Unknown));
        // 确认是别的账号：三种进程状态都安全
        assert!(ClientLoginState::Other.allow_self_refresh(ClientProcessState::Running));
        assert!(ClientLoginState::Other.allow_self_refresh(ClientProcessState::Absent));
        assert!(ClientLoginState::Other.allow_self_refresh(ClientProcessState::Unknown));
        // 身份未知：只有「确认客户端没在跑」才允许；进程状态未知（枚举失败）一律不刷
        // （2026-11-11 上游 cockpit-tools 实测：这样会把客户端刷掉线）。
        assert!(!ClientLoginState::Unknown.allow_self_refresh(ClientProcessState::Running));
        assert!(ClientLoginState::Unknown.allow_self_refresh(ClientProcessState::Absent));
        assert!(!ClientLoginState::Unknown.allow_self_refresh(ClientProcessState::Unknown));
    }
}
