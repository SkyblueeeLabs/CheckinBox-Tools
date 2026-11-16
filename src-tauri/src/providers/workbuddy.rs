//! WorkBuddy（腾讯 CodeBuddy CN）每日签到适配器。
//!
//! 凭据存储（两代，按客户端版本选）：
//!   * 5.6+：共享 auth 文件 `%LOCALAPPDATA%\CodeBuddyExtension\Data\Public\auth\
//!     workbuddy-desktop.info`，敏感字段带官方 AES-256-GCM 信封（`$wbEncrypted`），
//!     读写见 `core/wb_session.rs`（对齐 Cockpit Tools 的做法）；
//!   * 5.6 之前：`%APPDATA%\WorkBuddy\User\globalStorage\state.vscdb`
//!     （SQLite 的 ItemTable），键 `secret://{"extensionId":"tencent-cloud.coding-copilot",
//!     "key":"planning-genie.new.accessTokencn"}`，值是 `{type:"Buffer",data:[…]}`，
//!     字节是 **Electron safeStorage**（`v10` + AES-256-GCM，密钥在 `Local State`
//!     里由 DPAPI 保护）—— 与 Qoder 的 auth.v1.dat 同一套加密，复用 `core/os_crypt.rs`。
//!     解密后的 JSON：`{account:{uid,nickname,uin,…}, token, refreshToken, expiresAt, …}`。
//!
//! 协议（接口参考 jlcodes99/cockpit-tools 的 codebuddy_cn 实现，CC BY-NC-SA，
//! 仅作协议参考独立实现；本机实测调通）：
//! ```text
//! 签到状态  POST www.codebuddy.cn/v2/billing/meter/checkin-activity-status
//! 每日签到  POST www.codebuddy.cn/v2/billing/meter/daily-checkin
//! 积分      POST www.codebuddy.cn/v2/billing/meter/get-user-resource
//! 积分汇总  POST www.codebuddy.cn/billing/meter/get-user-resource-summary
//! 包明细    POST /billing/meter/get-user-resource-{paid,free}-packages
//! 消耗明细  POST /billing/meter/get-user-request-usage（需 X-Client-Platform: workbuddy）
//! 刷新      POST www.codebuddy.cn/v2/plugin/auth/token/refresh（X-Refresh-Token 头）
//! 浏览器授权 POST /v2/plugin/auth/state?platform=ide → authUrl → GET /v2/plugin/auth/token?state=
//! ```

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use base64::Engine;
use serde_json::{json, Value};

use crate::core::os_crypt;
use crate::core::provider::{
    Account, CheckinStatus, ClaimOutcome, ClaimState, CreditGroup, CreditItem, Provider, Quota,
};

/// 默认基址（CN 版）。实际调用时跟随账号的 domain 字段（国际版是 codebuddy.ai）
const BASE_DEFAULT: &str = "https://www.codebuddy.cn";
/// 浏览器 UA（网关会校验，非浏览器 UA 直接 401）
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36";

/// 请求基址：跟随账号 domain（如 www.codebuddy.ai），没有就用 CN 默认。
pub(crate) fn base_of(cred: &Value) -> String {
    match cred["domain"].as_str().filter(|d| !d.is_empty()) {
        Some(d) => format!("https://{d}"),
        None => BASE_DEFAULT.to_string(),
    }
}
const STATUS_PATH: &str = "/v2/billing/meter/checkin-activity-status";
const STATUS_PATH_FALLBACK: &str = "/v2/billing/meter/checkin-status";
const CLAIM_PATH: &str = "/v2/billing/meter/daily-checkin";
const RESOURCE_PATH: &str = "/v2/billing/meter/get-user-resource";
const REFRESH_PATH: &str = "/v2/plugin/auth/token/refresh";
// 5.9.3 客户端的新端点：基址去掉了 /v2（本机探针实测 200）
const SUMMARY_PATH: &str = "/billing/meter/get-user-resource-summary";
const PAID_PACKAGES_PATH: &str = "/billing/meter/get-user-resource-paid-packages";
const FREE_PACKAGES_PATH: &str = "/billing/meter/get-user-resource-free-packages";
const USAGE_PATH: &str = "/billing/meter/get-user-request-usage";

/// 实时模型目录里的一条模型（与客户端对话选择器同源）。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FreeModel {
    pub id: String,
    pub name: String,
    /// 计费倍率原文（如 "x0.00" / "x0.15"）
    pub credits: String,
    pub free: bool,
}

/// 计费倍率是否为 0（"x0"/"x0.00"/"0"/"0.00"/"免费"/"free" 都算免费）。
pub fn is_free_multiplier(credits: &str) -> bool {
    let t = credits.trim();
    if t.is_empty() {
        return false;
    }
    if t.contains("免费") || t.eq_ignore_ascii_case("free") {
        return true;
    }
    let num = t.trim_start_matches(['x', 'X', '×']);
    num.parse::<f64>().map(|v| v == 0.0).unwrap_or(false)
}

#[cfg(test)]
mod free_model_tests {
    use super::is_free_multiplier;

    #[test]
    fn 零倍率与免费字样算免费() {
        for s in ["x0", "x0.00", "0", "0.00", "X0", "×0", "免费", "free", "Free"] {
            assert!(is_free_multiplier(s), "{s} 应判为免费");
        }
    }

    #[test]
    fn 非零倍率与空串不算免费() {
        for s in ["x0.15", "x1", "1", "0.5", "", "  ", "x0.001"] {
            assert!(!is_free_multiplier(s), "{s} 不应判为免费");
        }
    }
}

/// 拉取实时模型目录，供福利提示核对「当前哪些模型免费」。
///
/// 协议对齐 cockpit-tools 的 ModelsService.getTeamsModels：
/// GET {base}/console/enterprises/{personal|enterpriseId}/models，
/// 响应 body（或 body.data）含 models: [{id, name, credits, descriptionZh, …}]。
/// credits 为计费倍率，0 倍率 / 描述带「限免/免费」/ 名字带 Free 视为免费。
pub async fn fetch_free_models(cred: &Value) -> Result<Vec<FreeModel>> {
    let scope = cred["enterpriseId"]
        .as_str()
        .or_else(|| cred["enterprise_id"].as_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("personal");
    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(std::time::Duration::from_secs(15))
        .build()?;
    let resp = client
        .get(format!(
            "{}/console/enterprises/{}/models",
            base_of(cred),
            scope
        ))
        .headers(WorkBuddyProvider::auth_headers(cred))
        .send()
        .await?;
    if !resp.status().is_success() {
        return Err(anyhow!("模型目录 HTTP {}", resp.status().as_u16()));
    }
    let body: Value = resp.json().await?;
    let payload = body.get("data").filter(|d| d.is_object()).unwrap_or(&body);
    let models = payload
        .get("models")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("模型目录响应缺少 models 数组"))?;
    let mut out = Vec::new();
    for m in models {
        let id = m["id"].as_str().unwrap_or("").to_string();
        if id.is_empty() {
            continue;
        }
        let name = m["name"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or(&id)
            .to_string();
        let credits = m["credits"].as_str().unwrap_or("").to_string();
        let desc = m["descriptionZh"]
            .as_str()
            .or_else(|| m["description"].as_str())
            .unwrap_or("");
        let free = is_free_multiplier(&credits)
            || desc.contains("限免")
            || desc.contains("免费")
            || name.contains("Free");
        out.push(FreeModel {
            id,
            name,
            credits,
            free,
        });
    }
    Ok(out)
}

pub struct WorkBuddyProvider {
    http: reqwest::Client,
}

impl WorkBuddyProvider {
    pub fn new() -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .user_agent(USER_AGENT)
            .build()
            .unwrap_or_default();
        Self { http }
    }

    pub(crate) fn auth_headers(cred: &Value) -> reqwest::header::HeaderMap {
        let mut headers = reqwest::header::HeaderMap::new();
        if let Some(token) = cred["access_token"].as_str() {
            if let Ok(v) = format!("Bearer {token}").parse() {
                headers.insert(reqwest::header::AUTHORIZATION, v);
            }
        }
        headers.insert("accept", "application/json".parse().unwrap());
        // 网关要求带用户身份；少了 X-User-Id / X-Domain 会 401（实测）
        if let Some(uid) = cred["uid"].as_str().filter(|u| !u.is_empty()) {
            if let Ok(v) = uid.parse() {
                headers.insert("X-User-Id", v);
            }
        }
        if let Some(d) = cred["domain"].as_str().filter(|x| !x.is_empty()) {
            if let Ok(v) = d.parse() {
                headers.insert("X-Domain", v);
            }
        }
        headers
    }

    async fn post_json(
        &self,
        path: &str,
        cred: &Value,
        body: Value,
    ) -> Result<(reqwest::StatusCode, Value)> {
        self.post_json_with_headers(path, cred, body, &[]).await
    }

    async fn post_json_with_headers(
        &self,
        path: &str,
        cred: &Value,
        body: Value,
        extra: &[(&str, &str)],
    ) -> Result<(reqwest::StatusCode, Value)> {
        let mut req = self
            .http
            .post(format!("{}{path}", base_of(cred)))
            .headers(Self::auth_headers(cred))
            .header("content-type", "application/json");
        for (k, v) in extra {
            req = req.header(*k, *v);
        }
        let resp = req.json(&body).send().await?;
        let status = resp.status();
        let body: Value = resp.json().await.unwrap_or(Value::Null);
        Ok((status, body))
    }
}

// ---------------------------------------------------------------------------
// 本机凭据读取（state.vscdb + safeStorage）
// ---------------------------------------------------------------------------

/// WorkBuddy 客户端的 Electron userData 目录。
///
/// 实测 WorkBuddy.exe（`D:\Programs\WorkBuddy`）的数据写在 `%APPDATA%\WorkBuddy`，
/// 旧版 CodeBuddy.exe 才用 `%APPDATA%\CodeBuddy`。本工具只接管 WorkBuddy。
pub fn user_data_dir() -> Option<std::path::PathBuf> {
    let appdata = std::env::var_os("APPDATA")?;
    let dir = std::path::PathBuf::from(appdata).join("WorkBuddy");
    dir.is_dir().then_some(dir)
}

/// state.vscdb 里 safeStorage 密文的键名（换号写回时也要用）。
pub fn secret_key() -> String {
    secret_keys()[0].to_string()
}

/// 需要写入的 secret 键名。
///
/// WorkBuddy 客户端只认 `accessTokencn`（`%APPDATA%\WorkBuddy` 的库里只有这一个键）；
/// 旧版 CodeBuddy 用的 `accessToken` 是另一个客户端的事，不写。
pub fn secret_keys() -> [&'static str; 1] {
    [
        r#"secret://{"extensionId":"tencent-cloud.coding-copilot","key":"planning-genie.new.accessTokencn"}"#,
    ]
}

/// 读出并解密客户端当前登录的凭据 JSON（只读，不写任何文件）。
/// 换号（`core/client_switch.rs::switch_workbuddy`）也用它拿模板和判身份。
pub fn read_local_secret() -> Option<Value> {
    let dir = user_data_dir()?;
    let key = os_crypt::read_key(&dir.join("Local State")).ok()?;
    let conn =
        rusqlite::Connection::open(dir.join("User").join("globalStorage").join("state.vscdb"))
            .ok()?;
    // 注意：这列在库里存的是 TEXT（JSON 字符串），不是 BLOB
    let row: String = conn
        .query_row(
            "SELECT value FROM ItemTable WHERE key = ?1",
            [secret_key()],
            |r| r.get(0),
        )
        .ok()?;
    // 值形如 {"type":"Buffer","data":[118,49,48,…]}
    let wrapper: Value = serde_json::from_str(&row).ok()?;
    let bytes: Vec<u8> = wrapper["data"]
        .as_array()?
        .iter()
        .filter_map(|b| b.as_u64().map(|v| v as u8))
        .collect();
    let plain = os_crypt::decrypt(&key, &bytes).ok()?;
    serde_json::from_slice(&plain).ok()
}

/// 从 Cockpit Tools 的加密账号库导入多账号（用户明确要求的功能）。
///
/// `~/.cockpit_tools/workbuddy_accounts/<id>.json` 是 AES-256-GCM 加密的
/// 账号明细（`algorithm: "AES-256-GCM"`, `key_id: "local-secure-account-storage-v1"`），
/// 密钥是 `~/.cockpit_tools/secure-account-storage.key`（base64 的 32 字节），
/// nonce 在字段里，无 AAD —— 已实测解密。token 的 domain 有 workbuddy.cn 和
/// codebuddy.cn 两种，调用时跟随。
fn read_cockpit_accounts() -> Vec<Value> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    let dir = home.join(".cockpit_tools").join("workbuddy_accounts");
    let key_file = home
        .join(".cockpit_tools")
        .join("secure-account-storage.key");
    if !dir.is_dir() || !key_file.is_file() {
        return Vec::new();
    }
    let Ok(key_text) = std::fs::read_to_string(&key_file) else {
        return Vec::new();
    };
    let Ok(key) = base64::engine::general_purpose::STANDARD.decode(key_text.trim()) else {
        return Vec::new();
    };
    let Ok(key_arr) = <[u8; 32]>::try_from(key.as_slice()) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Ok(raw) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(envelope) = serde_json::from_str::<Value>(&raw) else {
            continue;
        };
        // 只认 Cockpit 的 AES-256-GCM 信封
        if envelope["algorithm"].as_str() != Some("AES-256-GCM")
            || envelope["ciphertext"].as_str().is_none()
            || envelope["nonce"].as_str().is_none()
        {
            continue;
        }
        let (Ok(nonce), Ok(cipher_text)) = (
            base64::engine::general_purpose::STANDARD
                .decode(envelope["nonce"].as_str().unwrap_or("")),
            base64::engine::general_purpose::STANDARD
                .decode(envelope["ciphertext"].as_str().unwrap_or("")),
        ) else {
            continue;
        };
        let Some(plain) = cockpit_cipher::decrypt(&key_arr, &nonce, &cipher_text) else {
            continue;
        };
        if let Ok(account) = serde_json::from_slice::<Value>(&plain) {
            out.push(account);
        }
    }
    out
}

/// 上面的解密辅助（避免和 os_crypt 名字冲突的小封装）。
mod cockpit_cipher {
    use aes_gcm::aead::{Aead, KeyInit, Payload};
    use aes_gcm::{Aes256Gcm, Nonce};

    pub fn decrypt(key: &[u8; 32], nonce: &[u8], cipher: &[u8]) -> Option<Vec<u8>> {
        Aes256Gcm::new(key.into())
            .decrypt(Nonce::from_slice(nonce), Payload::from(cipher))
            .ok()
    }
}

// ---------------------------------------------------------------------------
// OAuth 浏览器授权导入（借鉴 Cockpit 的 codebuddy_cn_oauth 流程）
// ---------------------------------------------------------------------------

impl WorkBuddyProvider {
    /// 发起授权：拿到 state 和浏览器授权地址。
    pub async fn begin_authorization(&self) -> Result<(String, String)> {
        let resp: Value = self
            .http
            .post(format!("{BASE_DEFAULT}/v2/plugin/auth/state?platform=ide"))
            .header("accept", "application/json")
            .send()
            .await?
            .json()
            .await?;
        let state = resp["data"]["state"]
            .as_str()
            .or_else(|| resp["state"].as_str())
            .ok_or_else(|| anyhow!("授权 state 接口没返回 state（上游可能已变更）"))?;
        let url = resp["data"]["authUrl"]
            .as_str()
            .or_else(|| resp["data"]["auth_url"].as_str())
            .or_else(|| resp["data"]["url"].as_str())
            .or_else(|| resp["authUrl"].as_str())
            .ok_or_else(|| anyhow!("授权 state 接口没返回授权地址"))?;
        Ok((state.to_string(), url.to_string()))
    }

    /// 轮询授权结果；`Ok(None)` = 用户还没在浏览器里确认。
    pub async fn poll_authorization(&self, state: &str) -> Result<Option<(Account, Value)>> {
        let resp = self
            .http
            .get(format!("{BASE_DEFAULT}/v2/plugin/auth/token?state={state}"))
            .header("accept", "application/json")
            .send()
            .await?;
        if resp.status() == 202 || resp.status() == 404 {
            return Ok(None);
        }
        let body: Value = resp.json().await.unwrap_or(Value::Null);

        let pick = |keys: &[&str]| -> Option<String> {
            for k in keys {
                for root in [&body, body.get("data").unwrap_or(&Value::Null)] {
                    if let Some(v) = root[*k].as_str().filter(|s| !s.is_empty()) {
                        return Some(v.to_string());
                    }
                }
            }
            None
        };

        let access = pick(&["accessToken", "access_token", "token"]).unwrap_or_default();
        if access.is_empty() {
            // 还没确认，或响应结构异常 —— 202/404 之外的「无 token」按未确认处理
            return Ok(None);
        }
        let refresh = pick(&["refreshToken", "refresh_token"]).unwrap_or_default();

        // 账号资料（uid / 昵称）；失败不影响入库（id 用 uid，拿不到用 token 指纹）
        let info: Value = self
            .http
            .get(format!(
                "{BASE_DEFAULT}/v2/plugin/login/account?state={state}"
            ))
            .header("accept", "application/json")
            .send()
            .await?
            .json()
            .await
            .unwrap_or(Value::Null);
        let uid = info["data"]["uid"]
            .as_str()
            .or_else(|| info["uid"].as_str())
            .unwrap_or("")
            .to_string();
        let nickname = info["data"]["nickname"]
            .as_str()
            .or_else(|| info["nickname"].as_str())
            .unwrap_or("")
            .to_string();

        let uid_for_cred = uid.clone();
        let id = if uid.is_empty() {
            // 没拿到 uid：用 access token 的 sha256 前 16 位当稳定 id
            use sha2::{Digest, Sha256};
            let hex: String = Sha256::digest(access.as_bytes())
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            hex[..16].to_string()
        } else {
            uid
        };
        let label = if nickname.is_empty() {
            format!("WorkBuddy {}", &id[..id.len().min(8)])
        } else {
            nickname
        };

        let account = Account {
            id,
            provider: "workbuddy".to_string(),
            label,
            created_at: chrono::Utc::now().timestamp(),
            last_used: None,
            origin: "浏览器授权登录".to_string(),
        };
        let expires_at = pick(&["expiresAt", "expires_at"])
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(0);
        // expiresAt 可能是秒或毫秒
        let expires_at = if expires_at > 0 && expires_at < 1_000_000_000_000 {
            expires_at * 1000
        } else {
            expires_at
        };
        let cred = json!({
            "access_token": access,
            "refresh_token": refresh,
            "expires_at": expires_at,
            "uid": uid_for_cred,
            "domain": pick(&["domain"]).unwrap_or_default(),
        });
        Ok(Some((account, cred)))
    }
}

// ---------------------------------------------------------------------------
// Provider 实现
// ---------------------------------------------------------------------------

#[async_trait]
impl Provider for WorkBuddyProvider {
    fn id(&self) -> &'static str {
        "workbuddy"
    }

    fn display_name(&self) -> &'static str {
        "WorkBuddy"
    }

    fn accent(&self) -> &'static str {
        // 腾讯蓝
        "#0052d9"
    }

    fn docs_url(&self) -> &'static str {
        "https://www.codebuddy.cn"
    }

    async fn discover(&self) -> Result<Vec<(Account, Value)>> {
        // 优先读 Cockpit Tools 的加密账号库（可能有多账号）；
        // 没有再退回本机 WorkBuddy 客户端的当前登录态（单账号）。
        let cockpit = read_cockpit_accounts();
        if !cockpit.is_empty() {
            let now = chrono::Utc::now().timestamp();
            let mut out = Vec::new();
            for a in cockpit {
                let uid = a["uid"].as_str().unwrap_or("");
                if uid.is_empty() {
                    continue;
                }
                let label = a["nickname"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .map(String::from)
                    .or_else(|| a["email"].as_str().map(String::from))
                    .unwrap_or_else(|| format!("WorkBuddy {}", &uid[..uid.len().min(8)]));
                let account = Account {
                    id: uid.to_string(),
                    provider: "workbuddy".to_string(),
                    label,
                    created_at: now,
                    last_used: None,
                    origin: "Cockpit Tools 账号库".to_string(),
                };
                let cred = json!({
                    "access_token": a["access_token"].as_str().unwrap_or(""),
                    "refresh_token": a["refresh_token"].as_str().unwrap_or(""),
                    "expires_at": a["expires_at"].as_i64().unwrap_or(0),
                    "uid": uid,
                    "domain": a["domain"].as_str().unwrap_or(""),
                });
                out.push((account, cred));
            }
            if !out.is_empty() {
                return Ok(out);
            }
        }

        // 本机客户端登录态（单账号）：新版解密共享 auth 文件、老版解密
        // state.vscdb（`workbuddy_client_login` 内部按客户端版本选择）
        if let Some((account, cred)) = crate::core::client_switch::workbuddy_client_login() {
            return Ok(vec![(account, cred)]);
        }
        Ok(Vec::new())
    }

    async fn status(&self, _account: &Account, cred: &Value) -> Result<CheckinStatus> {
        // 主接口失败（路径变更 / 请求失败）时退回旧路径
        let tried = self.post_json(STATUS_PATH, cred, json!({})).await;
        let (code, body) = match tried {
            Ok(r) if r.0.is_success() => r,
            _ => self
                .post_json(STATUS_PATH_FALLBACK, cred, json!({}))
                .await
                .map_err(|e| anyhow!("签到状态接口请求失败：{e}"))?,
        };
        if !code.is_success() {
            return Err(anyhow!("签到状态接口返回 HTTP {}", code.as_u16()));
        }

        let data = body.get("data").cloned().unwrap_or_else(|| body.clone());
        let checked = data["todayCheckedIn"]
            .as_bool()
            .or_else(|| data["today_checked_in"].as_bool())
            .or_else(|| data["todayChecked"].as_bool())
            .unwrap_or(false);
        let active = data["active"].as_bool().unwrap_or(true);
        let streak = data["streakDays"]
            .as_i64()
            .or_else(|| data["streak_days"].as_i64())
            .unwrap_or(0);
        let daily = data["dailyCredit"]
            .as_i64()
            .or_else(|| data["daily_credit"].as_i64())
            .unwrap_or(0);
        let bonus = data["todayCredit"]
            .as_i64()
            .or_else(|| data["today_credit"].as_i64())
            .map(|c| (c - daily).max(0))
            .unwrap_or(0);

        if !active {
            return Ok(CheckinStatus {
                state: ClaimState::NotAvailable,
                message: "签到活动未开启".to_string(),
                ..Default::default()
            });
        }
        Ok(if checked {
            CheckinStatus {
                state: ClaimState::Claimed,
                points: daily + bonus,
                streak_days: streak as i32,
                message: "今日已签到".to_string(),
                ..Default::default()
            }
        } else {
            CheckinStatus {
                state: ClaimState::Claimable,
                points: daily + bonus,
                streak_days: streak as i32,
                message: format!("可签到 +{} 积分", daily + bonus),
                ..Default::default()
            }
        })
    }

    async fn claim(&self, _account: &Account, cred: &Value) -> Result<ClaimOutcome> {
        let (code, body) = self.post_json(CLAIM_PATH, cred, json!({})).await?;
        if !code.is_success() {
            let msg = body["message"]
                .as_str()
                .or_else(|| body["msg"].as_str())
                .unwrap_or("");
            return Err(anyhow!("签到失败 HTTP {} {msg}", code.as_u16()));
        }
        let data = body.get("data").cloned().unwrap_or_else(|| body.clone());
        let success = data["success"].as_bool().unwrap_or(true);
        let credit = data["credit"]
            .as_i64()
            .or_else(|| data["reward"].as_i64())
            .or_else(|| data["credit"].as_f64().map(|f| f as i64))
            .unwrap_or(0);
        if success {
            Ok(ClaimOutcome::claimed(
                credit,
                format!("签到成功 +{credit} 积分"),
            ))
        } else {
            Ok(ClaimOutcome::already(
                credit,
                data["message"].as_str().unwrap_or("今日已签到"),
            ))
        }
    }

    async fn quota(&self, _account: &Account, cred: &Value) -> Result<Quota> {
        let now = chrono::Local::now();
        let now_ms = now.timestamp_millis();
        let range_begin = now.format("%Y-%m-%d %H:%M:%S").to_string();
        let range_end = (now + chrono::Duration::days(365 * 101))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        let (items_res, summary_res) = tokio::join!(
            self.post_json(
                RESOURCE_PATH,
                cred,
                json!({
                    "PageNumber": 1,
                    "PageSize": 200,
                    "ProductCode": "p_tcaca",
                    "Status": [0, 3],
                    "PackageEndTimeRangeBegin": range_begin,
                    "PackageEndTimeRangeEnd": range_end,
                }),
            ),
            self.post_json(SUMMARY_PATH, cred, json!({})),
        );
        let (code, body) = items_res?;
        if !code.is_success() {
            return Err(anyhow!("积分接口返回 HTTP {}", code.as_u16()));
        }

        // 响应结构有几种可能的嵌套路径，按 Cockpit 的经验逐个找 accounts 数组
        let candidates: Vec<Value> = [
            "data/resources",
            "data/data/Response/Data/Accounts",
            "data/Response/Data/Accounts",
            "data",
        ]
        .iter()
        .filter_map(|path| {
            let mut cur = &body;
            for seg in path.split('/') {
                cur = cur.get(seg)?;
            }
            cur.as_array().cloned()
        })
        .next()
        .unwrap_or_default();

        let mut items = Vec::new();
        let mut granted = 0.0;
        let mut used = 0.0;
        let mut remaining = 0.0;
        for acc in &candidates {
            let g = num(&acc["CycleCapacitySizePrecise"]).max(num(&acc["CycleCapacitySize"]));
            let u = num(&acc["CycleCapacityUsedPrecise"]).max(num(&acc["CycleCapacityUsed"]));
            let r = num(&acc["CycleCapacityRemainPrecise"]).max(num(&acc["CycleCapacityRemain"]));
            granted += g;
            used += u;
            remaining += r;
            items.push(CreditItem {
                credit_type: code_kind(acc["PackageCode"].as_str().unwrap_or("")).to_string(),
                granted: g,
                consumed: u,
                remaining: r,
                granted_at_ms: 0,
                expire_at_ms: package_expire_ms(acc),
                package_name: acc["PackageName"].as_str().unwrap_or("").to_string(),
                refresh_at_ms: package_refresh_ms(acc),
            });
        }

        // 总量与分组以服务端 summary 为准（口径与官网一致，本机探针核对过
        // TotalCredits == 旧接口逐项求和）；summary 挂了退回旧的逐项求和。
        let summary = summary_res
            .ok()
            .filter(|(code, _)| code.is_success())
            .and_then(|(_, body)| parse_summary(&body));
        match summary {
            Some((sg, su, sr, mut groups)) => {
                for group in &mut groups {
                    let (time_ms, uses_expiry) = group_time(&items, &group.kind, now_ms);
                    group.time_ms = time_ms;
                    group.uses_expiry = uses_expiry;
                }
                let message = format!("剩余 {} / 共 {} credits", fmt_amount(sr), fmt_amount(sg));
                Ok(Quota {
                    total_granted: sg,
                    total_consumed: su,
                    total_remaining: sr,
                    items,
                    message,
                    refresh_at_ms: 0,
                    credit_groups: groups,
                })
            }
            None => {
                if items.is_empty() {
                    return Ok(Quota::empty(
                        "积分接口返回了数据，但没找到资源列表（上游可能已变更）",
                    ));
                }
                let message = format!(
                    "剩余 {} / 共 {} credits",
                    fmt_amount(remaining),
                    fmt_amount(granted)
                );
                Ok(Quota {
                    total_granted: granted,
                    total_consumed: used,
                    total_remaining: remaining,
                    items,
                    message,
                    refresh_at_ms: 0,
                    credit_groups: Vec::new(),
                })
            }
        }
    }

    /// token 过期时用 X-Refresh-Token 换新（实测 token 有效期很长，极少触发）。
    async fn refresh(&self, _account: &Account, cred: &Value) -> Result<Option<Value>> {
        let rt = cred["refresh_token"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow!("凭据里没有 refresh token"))?;
        let resp = self
            .http
            .post(format!("{}{REFRESH_PATH}", base_of(cred)))
            .header(
                reqwest::header::AUTHORIZATION,
                format!("Bearer {}", cred["access_token"].as_str().unwrap_or("")),
            )
            .header("X-Refresh-Token", rt)
            .header("X-Auth-Refresh-Source", "ide-main")
            .header("accept", "application/json")
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(anyhow!("token 刷新失败 HTTP {}", resp.status().as_u16()));
        }
        let body: Value = resp.json().await.unwrap_or(Value::Null);
        let data = body.get("data").cloned().unwrap_or_else(|| body.clone());
        let access = data["accessToken"]
            .as_str()
            .or_else(|| data["access_token"].as_str())
            .unwrap_or("");
        if access.is_empty() {
            return Err(anyhow!("刷新响应里没有 accessToken（上游可能已变更）"));
        }
        let mut fresh = cred.clone();
        fresh["access_token"] = json!(access);
        if let Some(new_rt) = data["refreshToken"]
            .as_str()
            .or_else(|| data["refresh_token"].as_str())
            .filter(|s| !s.is_empty())
        {
            fresh["refresh_token"] = json!(new_rt);
        }
        Ok(Some(fresh))
    }
}

// ---------------------------------------------------------------------------
// 积分分组与明细（5.9.3 新端点，协议参考 cockpit-tools，独立实现）
// ---------------------------------------------------------------------------

/// 官方分组 → 包代码清单（与 summary 的 CreditGroups.Kind 对齐）。
const PLAN_BASE_CODES: &[&str] = &[
    "TCACA_code_002_AkiJS3ZHF5",
    "TCACA_code_005_maRGyrHhw1",
    "TCACA_code_003_FAnt7lcmRT",
    "TCACA_code_023_4xbGhMrE6q",
    "TCACA_code_026_BaESVICNoi",
    "TCACA_code_027_0FCGVA6vSa",
    "TCACA_code_043_6oIkxT7Z0S",
    "TCACA_code_008_cfWoLwvjU4",
    "TCACA_code_035_ArVxJcGDsm",
    "TCACA_code_039_KRcQj7wUat",
    "TCACA_code_040_mi9rCYg46x",
];
const PLAN_BONUS_CODES: &[&str] = &["TCACA_code_028_NtpWi0jzXs", "TCACA_code_037_WxOD3MpI2o"];
const BENEFIT_CODES: &[&str] = &[
    "TCACA_code_007_nzdH5h4Nl0",
    "TCACA_code_029_6wCGEWquYy",
    "TCACA_code_030_BjSt89qTvr",
];
const ADDON_CODES: &[&str] = &[
    "TCACA_code_009_0XmEQc2xOf",
    "TCACA_code_038_OhvqZtiPKr",
    "TCACA_code_036_lupO5WgNdG",
];
/// 明细接口按付费/免费分两条路由，各只认自己清单里的包代码。
const PAID_ROUTE_CODES: &[&str] = &[
    "TCACA_code_002_AkiJS3ZHF5",
    "TCACA_code_005_maRGyrHhw1",
    "TCACA_code_003_FAnt7lcmRT",
    "TCACA_code_023_4xbGhMrE6q",
    "TCACA_code_026_BaESVICNoi",
    "TCACA_code_027_0FCGVA6vSa",
    "TCACA_code_043_6oIkxT7Z0S",
    "TCACA_code_009_0XmEQc2xOf",
    "TCACA_code_038_OhvqZtiPKr",
    "TCACA_code_036_lupO5WgNdG",
];
const FREE_ROUTE_CODES: &[&str] = &[
    "TCACA_code_001_PqouKr6QWV",
    "TCACA_code_008_cfWoLwvjU4",
    "TCACA_code_035_ArVxJcGDsm",
    "TCACA_code_006_DbXS0lrypC",
    "TCACA_code_039_KRcQj7wUat",
    "TCACA_code_040_mi9rCYg46x",
    "TCACA_code_007_nzdH5h4Nl0",
    "TCACA_code_028_NtpWi0jzXs",
    "TCACA_code_037_WxOD3MpI2o",
    "TCACA_code_029_6wCGEWquYy",
    "TCACA_code_030_BjSt89qTvr",
];

/// 积分明细弹窗里的一个资源包。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreditPackage {
    pub id: String,
    /// 发放原因（AccountAttributes.grantReason），没有则用平台包名
    pub name: String,
    pub total: f64,
    pub used: f64,
    pub remaining: f64,
    pub expire_at_ms: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreditPackagePage {
    pub items: Vec<CreditPackage>,
    pub total: i64,
    pub has_more: bool,
}

/// 消耗明细的一行（近 30 天）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreditUsageRow {
    pub request_id: String,
    /// 服务端给的本地时间字符串（YYYY-MM-DD HH:mm:ss），直接展示
    pub time: String,
    pub input: String,
    pub input_trunc: String,
    pub credit: f64,
    pub model: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreditUsagePage {
    pub items: Vec<CreditUsageRow>,
    pub total: i64,
}

fn opt_num(v: &Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_i64().map(|i| i as f64))
        .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

fn num(v: &Value) -> f64 {
    opt_num(v).unwrap_or(0.0)
}

/// 消息里的金额：整数不带小数，非整数最多两位（避免 3805.50000093 这种长尾）。
fn fmt_amount(v: f64) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// 包代码 → 官方分组 kind；不在映射里的（001 免费日包 / 006 赠送等）归 "other"。
fn code_kind(code: &str) -> &'static str {
    if PLAN_BASE_CODES.contains(&code) {
        "plan_base"
    } else if PLAN_BONUS_CODES.contains(&code) {
        "plan_bonus"
    } else if BENEFIT_CODES.contains(&code) {
        "benefit"
    } else if ADDON_CODES.contains(&code) {
        "addon"
    } else {
        "other"
    }
}

/// 分组 → 组内全部包代码（明细弹窗按这个清单查询）。
fn group_codes(kind: &str) -> &'static [&'static str] {
    match kind {
        "plan_base" => PLAN_BASE_CODES,
        "plan_bonus" => PLAN_BONUS_CODES,
        "benefit" => BENEFIT_CODES,
        "addon" => ADDON_CODES,
        _ => &[],
    }
}

/// 解析 summary：返回 (总发放, 总已用, 总剩余, 非空分组)。
/// 结构异常返回 None（调用方退回旧的逐项求和口径）。
fn parse_summary(body: &Value) -> Option<(f64, f64, f64, Vec<CreditGroup>)> {
    if !matches!(body["code"].as_i64(), Some(0) | Some(200)) {
        return None;
    }
    let data = &body["data"];
    let remaining = opt_num(&data["TotalCredits"])?;
    let raw = data["CreditGroups"].as_array()?;
    let mut groups = Vec::new();
    let mut granted = 0.0;
    let mut used = 0.0;
    for g in raw {
        let kind = g["Kind"].as_str()?.to_string();
        let total = num(&g["CycleTotalCapacity"]);
        let g_used = num(&g["CycleUsedCapacity"]);
        let g_remaining = num(&g["CycleRemainCapacity"]);
        let count = g["TotalCount"].as_i64().unwrap_or(0);
        granted += total;
        used += g_used;
        // 服务端四种分组都会返回，没包的组不展示（低噪音）
        if count == 0 && total == 0.0 && g_used == 0.0 && g_remaining == 0.0 {
            continue;
        }
        groups.push(CreditGroup {
            kind,
            name: g["Name"].as_str().unwrap_or("").to_string(),
            total,
            used: g_used,
            remaining: g_remaining,
            count,
            time_ms: 0,
            uses_expiry: false,
        });
    }
    // 固定展示顺序：套餐基础 / 套餐赠送 / 平台奖励 / 购买
    groups.sort_by_key(|g| match g.kind.as_str() {
        "plan_base" => 0,
        "plan_bonus" => 1,
        "benefit" => 2,
        "addon" => 3,
        _ => 4,
    });
    Some((granted, used, remaining, groups))
}

/// 分组的时间线索：benefit/addon 看「最近到期」（只算还有剩余的批次），
/// plan_base/plan_bonus 看「下次刷新」。没有可展示的为 0。
fn group_time(items: &[CreditItem], kind: &str, now_ms: i64) -> (i64, bool) {
    let uses_expiry = matches!(kind, "benefit" | "addon");
    let time = items
        .iter()
        .filter(|i| i.credit_type == kind)
        .filter_map(|i| {
            if uses_expiry {
                (i.remaining > 0.0 && i.expire_at_ms > now_ms).then_some(i.expire_at_ms)
            } else {
                (i.refresh_at_ms > now_ms).then_some(i.refresh_at_ms)
            }
        })
        .min()
        .unwrap_or(0);
    (time, uses_expiry)
}

/// 明细弹窗的请求体：有效期内 / 已过期两种口径（与 5.9.3 客户端一致）。
fn detail_body(page: u32, expired: bool) -> Value {
    let now = chrono::Local::now().naive_local();
    let mut body = json!({
        "PageNumber": page.max(1),
        "PageSize": 200,
        "IsDisplayTotalInfo": true,
        "Status": if expired { [2, 3] } else { [0, 3] },
    });
    if expired {
        // 101 年历史窗口 + 按结束时间倒序
        let begin = now
            .checked_sub_months(chrono::Months::new(101 * 12))
            .unwrap_or(now);
        body["PackageEndTimeRangeBegin"] = json!(begin.format("%Y-%m-%d %H:%M:%S").to_string());
        body["PackageEndTimeRangeEnd"] = json!(now.format("%Y-%m-%d %H:%M:%S").to_string());
        body["OrderBy"] = json!("endTime");
        body["SortBy"] = json!("desc");
    } else {
        body["OnlyValidPeriod"] = json!(true);
    }
    body
}

/// 解析明细行：总量型包（CapacitySize>0）看 Capacity*，周期型包看 CycleCapacity*。
fn parse_credit_package(acc: &Value, expired: bool) -> CreditPackage {
    let capacity = num(&acc["CapacitySizePrecise"]);
    let (total, used_value, remain_value) = if capacity > 0.0 {
        (
            capacity,
            &acc["CapacityUsedPrecise"],
            &acc["CapacityRemainPrecise"],
        )
    } else {
        (
            num(&acc["CycleCapacitySizePrecise"]).max(0.0),
            &acc["CycleCapacityUsedPrecise"],
            &acc["CycleCapacityRemainPrecise"],
        )
    };
    let used = if expired {
        opt_num(&acc["ActualCycleCapacityUsed"])
    } else {
        None
    }
    .or_else(|| opt_num(used_value))
    .unwrap_or_else(|| total - opt_num(remain_value).unwrap_or(0.0))
    .max(0.0);
    let name = acc["AccountAttributes"]
        .as_array()
        .and_then(|attrs| {
            attrs
                .iter()
                .find(|a| a["Key"].as_str() == Some("grantReason"))
        })
        .and_then(|a| a["Value"].as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| acc["PackageName"].as_str().filter(|s| !s.is_empty()))
        .unwrap_or("")
        .to_string();
    let id = acc["ResourceId"]
        .as_str()
        .map(String::from)
        .or_else(|| acc["ResourceId"].as_u64().map(|n| n.to_string()))
        .or_else(|| acc["ResourceId"].as_i64().map(|n| n.to_string()))
        .unwrap_or_default();
    CreditPackage {
        id,
        name,
        total,
        used,
        remaining: (total - used).max(0.0),
        expire_at_ms: detail_expire_ms(acc),
    }
}

impl WorkBuddyProvider {
    /// 某官方分组下的资源包明细。kind: plan_base / plan_bonus / benefit / addon。
    /// 一个分组可能横跨付费/免费两条路由（如 plan_base），两边都查再合并。
    pub async fn credit_packages(
        &self,
        cred: &Value,
        kind: &str,
        expired: bool,
        page: u32,
    ) -> Result<CreditPackagePage> {
        let codes = group_codes(kind);
        if codes.is_empty() {
            return Err(anyhow!("未知积分分组：{kind}"));
        }
        let paid: Vec<&str> = codes
            .iter()
            .copied()
            .filter(|c| PAID_ROUTE_CODES.contains(c))
            .collect();
        let free: Vec<&str> = codes
            .iter()
            .copied()
            .filter(|c| FREE_ROUTE_CODES.contains(c))
            .collect();
        let body = detail_body(page, expired);
        let (paid_res, free_res) = tokio::join!(
            self.detail_group(PAID_PACKAGES_PATH, cred, body.clone(), &paid, expired),
            self.detail_group(FREE_PACKAGES_PATH, cred, body, &free, expired),
        );
        let (mut items, paid_total, paid_more) = paid_res?;
        let (free_items, free_total, free_more) = free_res?;
        items.extend(free_items);
        // 两条路由可能返回同一个 ResourceId，按 id 去重（保留先出现的付费路由行）
        let mut seen = std::collections::HashSet::new();
        items.retain(|item| item.id.is_empty() || seen.insert(item.id.clone()));
        Ok(CreditPackagePage {
            items,
            total: paid_total + free_total,
            has_more: paid_more || free_more,
        })
    }

    async fn detail_group(
        &self,
        path: &str,
        cred: &Value,
        mut body: Value,
        codes: &[&str],
        expired: bool,
    ) -> Result<(Vec<CreditPackage>, i64, bool)> {
        if codes.is_empty() {
            return Ok((Vec::new(), 0, false));
        }
        body["PackageCodes"] = json!(codes);
        let (code, resp) = self.post_json(path, cred, body.clone()).await?;
        if !code.is_success() {
            return Err(anyhow!("积分明细接口返回 HTTP {}", code.as_u16()));
        }
        if !matches!(resp["code"].as_i64(), Some(0) | Some(200)) {
            let msg = resp["msg"]
                .as_str()
                .or_else(|| resp["message"].as_str())
                .unwrap_or("");
            return Err(anyhow!("积分明细接口返回错误：{msg}"));
        }
        let data = &resp["data"];
        let accounts = data["Accounts"].as_array().cloned().unwrap_or_default();
        let total = data["TotalCount"]
            .as_i64()
            .or_else(|| data["TotalCount"].as_str().and_then(|s| s.parse().ok()))
            .unwrap_or(0);
        let page = body["PageNumber"].as_u64().unwrap_or(1);
        let has_more = if total > 0 {
            page * 200 < total as u64
        } else {
            accounts.len() >= 200
        };
        let items = accounts
            .iter()
            .map(|acc| parse_credit_package(acc, expired))
            .collect();
        Ok((items, total, has_more))
    }

    /// 近 30 天消耗明细（最近 5 条，与官网「用量」页同口径）。
    pub async fn credit_usage(&self, cred: &Value) -> Result<CreditUsagePage> {
        let today = chrono::Local::now().date_naive();
        let start = today - chrono::Days::new(30);
        let (code, resp) = self
            .post_json_with_headers(
                USAGE_PATH,
                cred,
                json!({
                    "startTime": format!("{} 00:00:00", start.format("%Y-%m-%d")),
                    "endTime": format!("{} 23:59:59", today.format("%Y-%m-%d")),
                    "pageNum": 1,
                    "pageSize": 5,
                }),
                &[("X-Client-Platform", "workbuddy")],
            )
            .await?;
        if !code.is_success() {
            return Err(anyhow!("消耗明细接口返回 HTTP {}", code.as_u16()));
        }
        if !matches!(resp["code"].as_i64(), Some(0) | Some(200)) {
            let msg = resp["msg"]
                .as_str()
                .or_else(|| resp["message"].as_str())
                .unwrap_or("");
            return Err(anyhow!("消耗明细接口返回错误：{msg}"));
        }
        let rows = resp["data"]["data"]
            .as_array()
            .ok_or_else(|| anyhow!("消耗明细响应里没找到记录列表（上游可能已变更）"))?;
        let total = resp["data"]["total"].as_i64().unwrap_or(0);
        let items = rows
            .iter()
            .take(5)
            .map(|r| CreditUsageRow {
                request_id: r["requestId"].as_str().unwrap_or("").to_string(),
                time: r["requestTime"].as_str().unwrap_or("").to_string(),
                input: r["input"].as_str().unwrap_or("").to_string(),
                input_trunc: r["inputTrunc"].as_str().unwrap_or("").to_string(),
                credit: num(&r["credit"]),
                model: r["model"].as_str().unwrap_or("").to_string(),
            })
            .collect();
        Ok(CreditUsagePage { items, total })
    }
}

/// 资源批次的到期时间：优先 PackageEndTime（套餐结束），退回 CycleEndTime。
///
/// 实测 `CycleEndTime` 在纯积分包上经常是 0，真正的套餐结束时间在
/// `PackageEndTime`（请求里也按这个字段做时间范围筛选）。数字兼容秒/毫秒。
fn package_expire_ms(acc: &Value) -> i64 {
    const KEYS: [&str; 7] = [
        "PackageEndTime",
        "CycleEndTime",
        "packageEndTime",
        "cycleEndTime",
        "ExpireTime",
        "ValidEndTime",
        "EndTime",
    ];
    for key in KEYS {
        let ms = parse_epoch_ms(&acc[key]);
        if ms > 0 {
            return ms;
        }
    }
    0
}

/// 批次下次刷新：CycleResetTime 优先，缺了按周期结束 +1s 推（套餐结束即刷新）。
fn package_refresh_ms(acc: &Value) -> i64 {
    let reset = parse_epoch_ms(&acc["CycleResetTime"]);
    if reset > 0 {
        return reset;
    }
    let end = package_expire_ms(acc);
    if end > 0 {
        end + 1000
    } else {
        0
    }
}

/// 明细行的到期时间：实际扣减截止 > 过期时间 > 周期结束。
/// 已用完的批次 ExpiredTime 早于 CycleEndTime，要展示真实失效时刻。
fn detail_expire_ms(acc: &Value) -> i64 {
    for key in ["DeductionEndTime", "ExpiredTime", "CycleEndTime"] {
        let ms = parse_epoch_ms(&acc[key]);
        if ms > 0 {
            return ms;
        }
    }
    0
}

/// 秒/毫秒/日期字符串统一转毫秒；无法解析为 0。
fn parse_epoch_ms(value: &Value) -> i64 {
    if let Some(n) = value
        .as_i64()
        .or_else(|| value.as_str().and_then(|s| s.trim().parse::<i64>().ok()))
    {
        if n > 0 {
            return if n < 1_000_000_000_000 { n * 1000 } else { n };
        }
    }
    if let Some(s) = value.as_str() {
        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
            return dt.timestamp_millis();
        }
        if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
            return dt
                .and_local_timezone(chrono::Local)
                .single()
                .map(|d| d.timestamp_millis())
                .unwrap_or(0);
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 资源到期优先取PackageEndTime并兼容秒与字符串() {
        let acc = json!({"PackageEndTime": 1793401339i64, "CycleEndTime": 0});
        assert_eq!(package_expire_ms(&acc), 1793401339000);

        let acc = json!({"CycleEndTime": 1793401339000i64});
        assert_eq!(package_expire_ms(&acc), 1793401339000);

        let acc = json!({"PackageEndTime": "2026-11-17 23:59:59"});
        assert!(package_expire_ms(&acc) > 0);

        let acc = json!({});
        assert_eq!(package_expire_ms(&acc), 0);
    }

    #[test]
    fn 包代码分组映射与路由清单一致() {
        assert_eq!(code_kind("TCACA_code_008_cfWoLwvjU4"), "plan_base");
        assert_eq!(code_kind("TCACA_code_028_NtpWi0jzXs"), "plan_bonus");
        assert_eq!(code_kind("TCACA_code_007_nzdH5h4Nl0"), "benefit");
        assert_eq!(code_kind("TCACA_code_030_BjSt89qTvr"), "benefit");
        assert_eq!(code_kind("TCACA_code_009_0XmEQc2xOf"), "addon");
        assert_eq!(code_kind("TCACA_code_001_PqouKr6QWV"), "other");
        assert_eq!(code_kind("TCACA_code_006_DbXS0lrypC"), "other");
        assert_eq!(code_kind("unknown"), "other");
        assert!(group_codes("nope").is_empty());

        // 每个分组代码都必须落在且只落在一条明细路由里，否则弹窗会漏包
        for kind in ["plan_base", "plan_bonus", "benefit", "addon"] {
            for code in group_codes(kind) {
                let paid = PAID_ROUTE_CODES.contains(code);
                let free = FREE_ROUTE_CODES.contains(code);
                assert!(paid ^ free, "{kind} 的 {code} 路由归属异常");
            }
        }
    }

    /// 探针实测响应（晚晚账号）：TotalCredits 与旧接口逐项求和一致。
    fn summary_fixture() -> Value {
        json!({
            "code": 0,
            "data": {
                "CreditGroups": [
                    {"Kind": "plan_base", "Name": "套餐基础积分", "CycleTotalCapacity": "500", "CycleUsedCapacity": "0", "CycleRemainCapacity": "500", "TotalCount": 1},
                    {"Kind": "plan_bonus", "Name": "套餐赠送积分", "CycleTotalCapacity": "0", "CycleUsedCapacity": "0", "CycleRemainCapacity": "0", "TotalCount": 0},
                    {"Kind": "addon", "Name": "购买积分", "CycleTotalCapacity": "0", "CycleUsedCapacity": "0", "CycleRemainCapacity": "0", "TotalCount": 0},
                    {"Kind": "benefit", "Name": "平台奖励积分", "CycleTotalCapacity": "4443", "CycleUsedCapacity": "1137.49999907", "CycleRemainCapacity": "3305.50000093", "TotalCount": 47}
                ],
                "TotalCredits": "3805.50000093"
            }
        })
    }

    #[test]
    fn summary解析_总量分组与空组过滤() {
        let (granted, used, remaining, groups) =
            parse_summary(&summary_fixture()).expect("解析失败");
        assert!((granted - 4943.0).abs() < 1e-6);
        assert!((used - 1137.49999907).abs() < 1e-6);
        assert!((remaining - 3805.50000093).abs() < 1e-6);
        // 空组（plan_bonus/addon）不展示；顺序固定 plan_base 在前
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].kind, "plan_base");
        assert_eq!(groups[0].name, "套餐基础积分");
        assert_eq!(groups[0].count, 1);
        assert_eq!(groups[1].kind, "benefit");
        assert_eq!(groups[1].count, 47);
        assert!((groups[1].remaining - 3305.50000093).abs() < 1e-6);
    }

    #[test]
    fn summary异常时返回None走旧口径() {
        assert!(parse_summary(&json!({"code": 401, "data": {}})).is_none());
        assert!(parse_summary(&json!({"code": 0, "data": {"CreditGroups": []}})).is_none());
        assert!(parse_summary(&json!({"code": 0, "data": {"TotalCredits": "10"}})).is_none());
        // 空账号：四个组全 0、TotalCredits=0 是合法的
        let body = json!({"code": 0, "data": {"CreditGroups": [
            {"Kind": "plan_base", "Name": "套餐基础积分", "CycleTotalCapacity": "0", "CycleUsedCapacity": "0", "CycleRemainCapacity": "0", "TotalCount": 0}
        ], "TotalCredits": "0"}});
        let (g, u, r, groups) = parse_summary(&body).expect("空账号也应解析成功");
        assert_eq!((g, u, r), (0.0, 0.0, 0.0));
        assert!(groups.is_empty());
    }

    #[test]
    fn 批次刷新时间_重置优先_周期结束加一秒兜底() {
        let acc =
            json!({"CycleResetTime": 1795654809000i64, "CycleEndTime": "2026-11-26 09:00:09"});
        assert_eq!(package_refresh_ms(&acc), 1795654809000);

        // 探针实测：CycleResetTime=null，CycleEndTime 与 DeductionEndTime 同一时刻
        let acc = json!({"CycleResetTime": null, "CycleEndTime": "2026-11-26 09:00:09", "DeductionEndTime": 1795654809000i64});
        assert_eq!(package_refresh_ms(&acc), 1795654809000 + 1000);

        assert_eq!(package_refresh_ms(&json!({})), 0);
    }

    fn item(kind: &str, remaining: f64, expire_at_ms: i64, refresh_at_ms: i64) -> CreditItem {
        CreditItem {
            credit_type: kind.to_string(),
            granted: 100.0,
            consumed: 100.0 - remaining,
            remaining,
            granted_at_ms: 0,
            expire_at_ms,
            package_name: String::new(),
            refresh_at_ms,
        }
    }

    #[test]
    fn 分组时间_奖励看最近到期_套餐看下次刷新() {
        let now = 1_000_000_000_000i64;
        let items = vec![
            item("benefit", 50.0, now + 2 * 86_400_000, 0),
            item("benefit", 30.0, now + 86_400_000, 0),
            item("benefit", 0.0, now + 3_600_000, 0), // 已用完的不参与到期
            item("benefit", 10.0, now - 86_400_000, 0), // 已过期的不参与
            item("plan_base", 0.0, 0, now + 5 * 86_400_000), // 用完的套餐仍会刷新
            item("plan_base", 100.0, 0, now + 3 * 86_400_000),
            item("plan_base", 100.0, 0, now - 86_400_000), // 过去的刷新不展示
        ];
        let (time, uses_expiry) = group_time(&items, "benefit", now);
        assert!(uses_expiry);
        assert_eq!(time, now + 86_400_000);
        let (time, uses_expiry) = group_time(&items, "plan_base", now);
        assert!(!uses_expiry);
        assert_eq!(time, now + 3 * 86_400_000);
        assert_eq!(group_time(&items, "addon", now), (0, true));
    }

    #[test]
    fn 明细请求体_有效与过期两种口径() {
        let valid = detail_body(1, false);
        assert_eq!(valid["Status"], json!([0, 3]));
        assert_eq!(valid["OnlyValidPeriod"], json!(true));
        assert_eq!(valid["PageSize"], json!(200));
        assert!(valid.get("OrderBy").is_none());
        assert!(valid.get("PackageEndTimeRangeBegin").is_none());

        let expired = detail_body(2, true);
        assert_eq!(expired["PageNumber"], json!(2));
        assert_eq!(expired["Status"], json!([2, 3]));
        assert_eq!(expired["OrderBy"], json!("endTime"));
        assert_eq!(expired["SortBy"], json!("desc"));
        assert!(expired.get("OnlyValidPeriod").is_none());
        let begin = expired["PackageEndTimeRangeBegin"].as_str().unwrap();
        let end = expired["PackageEndTimeRangeEnd"].as_str().unwrap();
        assert!(begin < end);
        let begin_year: i32 = begin[..4].parse().unwrap();
        let end_year: i32 = end[..4].parse().unwrap();
        assert_eq!(end_year - begin_year, 101, "101 年历史窗口");
    }

    #[test]
    fn 明细行解析_周期型总量型与过期口径() {
        // 周期型（探针里的每日签到包）：CapacitySize=100 但 CapacityType=1，
        // 上游口径是 CapacitySizePrecise>0 就用 Capacity*
        let acc = json!({
            "ResourceId": "codebuddy-abc",
            "PackageName": "CodeBuddy个人版国内运营裂变包",
            "AccountAttributes": [{"Key": "grantReason", "Value": "Buddy 加油站签到"}],
            "CapacitySizePrecise": "100", "CapacityUsedPrecise": "100", "CapacityRemainPrecise": "0",
            "CycleCapacitySizePrecise": "100", "CycleCapacityUsedPrecise": "100", "CycleCapacityRemainPrecise": "0",
            "CycleEndTime": "2026-11-10 22:39:10", "ExpiredTime": "2026-10-28 14:16:13",
            "DeductionEndTime": 1794321550000i64
        });
        let row = parse_credit_package(&acc, false);
        assert_eq!(row.id, "codebuddy-abc");
        assert_eq!(row.name, "Buddy 加油站签到");
        assert_eq!(row.total, 100.0);
        assert_eq!(row.used, 100.0);
        assert_eq!(row.remaining, 0.0);
        // 明细到期取实际扣减截止（不是周期结束）
        assert_eq!(row.expire_at_ms, 1794321550000);

        // 过期行优先 ActualCycleCapacityUsed
        let mut expired_acc = acc.clone();
        expired_acc["ActualCycleCapacityUsed"] = json!("62.5");
        let row = parse_credit_package(&expired_acc, true);
        assert_eq!(row.used, 62.5);
        assert_eq!(row.remaining, 37.5);

        // 没有 grantReason 退回包名；没有已用字段按 总量-剩余 推
        let acc = json!({
            "PackageName": "体验包",
            "CycleCapacitySizePrecise": "500", "CycleCapacityRemainPrecise": "120.5"
        });
        let row = parse_credit_package(&acc, false);
        assert_eq!(row.name, "体验包");
        assert_eq!(row.total, 500.0);
        assert_eq!(row.used, 379.5);
        assert_eq!(row.remaining, 120.5);
        assert_eq!(row.id, "");
    }

    #[test]
    fn 明细到期_扣减截止优先_过期时间兜底() {
        let acc = json!({"DeductionEndTime": 1794321550000i64, "ExpiredTime": "2026-10-28 14:16:13", "CycleEndTime": "2026-11-10 22:39:10"});
        assert_eq!(detail_expire_ms(&acc), 1794321550000);
        let acc =
            json!({"ExpiredTime": "2026-10-28 14:16:13", "CycleEndTime": "2026-11-10 22:39:10"});
        let expected =
            chrono::NaiveDateTime::parse_from_str("2026-10-28 14:16:13", "%Y-%m-%d %H:%M:%S")
                .unwrap()
                .and_local_timezone(chrono::Local)
                .single()
                .unwrap()
                .timestamp_millis();
        assert_eq!(detail_expire_ms(&acc), expected);
        assert_eq!(detail_expire_ms(&json!({})), 0);
    }

    #[test]
    fn 金额格式化_去尾零() {
        assert_eq!(fmt_amount(4943.0), "4943");
        assert_eq!(fmt_amount(3805.50000093), "3805.5");
        assert_eq!(fmt_amount(0.86), "0.86");
        assert_eq!(fmt_amount(0.0), "0");
    }

    /// 诊断用：列出 Cockpit 账号库里的 uid（只读）。
    /// 手动验证：`cargo test -- --ignored Cockpit账号库里的uid --nocapture`
    #[test]
    #[ignore = "依赖本机 Cockpit Tools 账号库，手动跑"]
    fn Cockpit账号库里的uid() {
        for a in read_cockpit_accounts() {
            eprintln!(
                "uid={} nickname={} domain={}",
                a["uid"].as_str().unwrap_or(""),
                a["nickname"].as_str().unwrap_or(""),
                a["domain"].as_str().unwrap_or("")
            );
        }
    }

    /// 迁移探针（只读）：对比旧 get-user-resource 与新 summary/明细/消耗端点。
    /// 手动跑：`cargo test --lib -- --ignored 积分端点口径探针 --nocapture`
    #[tokio::test]
    #[ignore = "依赖本机账号池与真实网络，手动跑"]
    async fn 积分端点口径探针() {
        let store = crate::core::store::AccountStore::load().expect("账号池加载失败");
        let accounts = store.list_of("workbuddy");
        assert!(!accounts.is_empty(), "账号池里没有 workbuddy 账号");
        let p = WorkBuddyProvider::new();
        let now = chrono::Local::now();
        for a in &accounts {
            let cred = store.credential("workbuddy", &a.id).expect("凭据解密失败");
            eprintln!("== {} ({})", a.label, a.id);

            let (code, old) = p
                .post_json(RESOURCE_PATH, &cred, json!({
                    "PageNumber": 1, "PageSize": 100, "ProductCode": "p_tcaca", "Status": [0, 3],
                    "PackageEndTimeRangeBegin": now.format("%Y-%m-%d %H:%M:%S").to_string(),
                    "PackageEndTimeRangeEnd": (now + chrono::Duration::days(365 * 101)).format("%Y-%m-%d %H:%M:%S").to_string(),
                }))
                .await
                .expect("旧资源接口请求失败");
            eprintln!("[old] http={code}");
            let candidates: Vec<Value> = [
                "data/resources",
                "data/data/Response/Data/Accounts",
                "data/Response/Data/Accounts",
                "data",
            ]
            .iter()
            .filter_map(|path| {
                let mut cur = &old;
                for seg in path.split('/') {
                    cur = cur.get(seg)?;
                }
                cur.as_array().cloned()
            })
            .next()
            .unwrap_or_default();
            let num = |v: &Value| -> f64 {
                v.as_f64()
                    .or_else(|| v.as_i64().map(|i| i as f64))
                    .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
                    .unwrap_or(0.0)
            };
            let old_remain: f64 = candidates
                .iter()
                .map(|x| num(&x["CycleCapacityRemainPrecise"]).max(num(&x["CycleCapacityRemain"])))
                .sum();
            eprintln!("[old] items={} remain_sum={old_remain}", candidates.len());
            for acc in &candidates {
                eprintln!(
                    "[pkg] code={} name={:?} status={} capType={} total={} remain={} pkgEnd={} cycleEnd={} reset={} deductEnd={} expired={}",
                    acc["PackageCode"].as_str().unwrap_or(""),
                    acc["PackageName"].as_str(),
                    acc["Status"], acc["CapacityType"],
                    num(&acc["CycleCapacitySizePrecise"]).max(num(&acc["CycleCapacitySize"])),
                    num(&acc["CycleCapacityRemainPrecise"]).max(num(&acc["CycleCapacityRemain"])),
                    acc["PackageEndTime"], acc["CycleEndTime"], acc["CycleResetTime"],
                    acc["DeductionEndTime"], acc["ExpiredTime"],
                );
            }

            let (code, summary) = p
                .post_json("/billing/meter/get-user-resource-summary", &cred, json!({}))
                .await
                .expect("summary 请求失败");
            eprintln!(
                "[summary] http={code} body={}",
                serde_json::to_string(&summary).unwrap_or_default()
            );

            let valid_body = json!({
                "PageNumber": 1, "PageSize": 200, "IsDisplayTotalInfo": true,
                "Status": [0, 3], "OnlyValidPeriod": true,
            });
            for (route, codes) in [
                (
                    "get-user-resource-paid-packages",
                    json!([
                        "TCACA_code_002_AkiJS3ZHF5",
                        "TCACA_code_005_maRGyrHhw1",
                        "TCACA_code_003_FAnt7lcmRT",
                        "TCACA_code_023_4xbGhMrE6q",
                        "TCACA_code_026_BaESVICNoi",
                        "TCACA_code_027_0FCGVA6vSa",
                        "TCACA_code_043_6oIkxT7Z0S",
                        "TCACA_code_009_0XmEQc2xOf",
                        "TCACA_code_038_OhvqZtiPKr",
                        "TCACA_code_036_lupO5WgNdG"
                    ]),
                ),
                (
                    "get-user-resource-free-packages",
                    json!([
                        "TCACA_code_001_PqouKr6QWV",
                        "TCACA_code_008_cfWoLwvjU4",
                        "TCACA_code_035_ArVxJcGDsm",
                        "TCACA_code_006_DbXS0lrypC",
                        "TCACA_code_039_KRcQj7wUat",
                        "TCACA_code_040_mi9rCYg46x",
                        "TCACA_code_007_nzdH5h4Nl0",
                        "TCACA_code_028_NtpWi0jzXs",
                        "TCACA_code_037_WxOD3MpI2o",
                        "TCACA_code_029_6wCGEWquYy",
                        "TCACA_code_030_BjSt89qTvr"
                    ]),
                ),
            ] {
                let mut body = valid_body.clone();
                body["PackageCodes"] = codes;
                let (code, detail) = p
                    .post_json(&format!("/billing/meter/{route}"), &cred, body)
                    .await
                    .expect("明细请求失败");
                let n = detail["data"]["Accounts"]
                    .as_array()
                    .map(|v| v.len())
                    .unwrap_or(0);
                eprintln!(
                    "[detail] {route} http={code} accounts={n} total={}",
                    detail["data"]["TotalCount"]
                );
                if route.ends_with("free-packages") {
                    if let Some(items) = detail["data"]["Accounts"].as_array() {
                        for acc in items.iter().take(3) {
                            eprintln!(
                                "[detail-pkg] {}",
                                serde_json::to_string(acc).unwrap_or_default()
                            );
                        }
                    }
                }
            }

            let start = (now - chrono::Duration::days(30))
                .format("%Y-%m-%d 00:00:00")
                .to_string();
            let end = now.format("%Y-%m-%d 23:59:59").to_string();
            let resp = p
                .http
                .post(format!(
                    "{}/billing/meter/get-user-request-usage",
                    base_of(&cred)
                ))
                .headers(WorkBuddyProvider::auth_headers(&cred))
                .header("content-type", "application/json")
                .header("X-Client-Platform", "workbuddy")
                .json(&json!({"startTime": start, "endTime": end, "pageNum": 1, "pageSize": 5}))
                .send()
                .await
                .expect("消耗接口请求失败");
            let code = resp.status();
            let usage: Value = resp.json().await.unwrap_or(Value::Null);
            eprintln!(
                "[usage] http={code} body={}",
                serde_json::to_string(&usage).unwrap_or_default()
            );
        }
    }
}
