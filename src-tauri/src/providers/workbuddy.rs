//! WorkBuddy（腾讯 CodeBuddy CN）每日签到适配器。
//!
//! 凭据存储（本机实测）：`%APPDATA%\CodeBuddy\User\globalStorage\state.vscdb`
//! （SQLite 的 ItemTable），键 `secret://{"extensionId":"tencent-cloud.coding-copilot",
//! "key":"planning-genie.new.accessToken"}`，值是 `{type:"Buffer",data:[…]}`，
//! 字节是 **Electron safeStorage**（`v10` + AES-256-GCM，密钥在 `Local State`
//! 里由 DPAPI 保护）—— 与 Qoder 的 auth.v1.dat 同一套加密，复用 `core/os_crypt.rs`。
//! 解密后的 JSON：`{account:{uid,nickname,uin,…}, token, refreshToken, expiresAt, …}`。
//!
//! 协议（接口参考 jlcodes99/cockpit-tools 的 codebuddy_cn 实现，CC BY-NC-SA，
//! 仅作协议参考独立实现；本机实测调通）：
//! ```text
//! 签到状态  POST www.codebuddy.cn/v2/billing/meter/checkin-activity-status
//! 每日签到  POST www.codebuddy.cn/v2/billing/meter/daily-checkin
//! 积分      POST www.codebuddy.cn/v2/billing/meter/get-user-resource
//! 刷新      POST www.codebuddy.cn/v2/plugin/auth/token/refresh（X-Refresh-Token 头）
//! 浏览器授权 POST /v2/plugin/auth/state?platform=ide → authUrl → GET /v2/plugin/auth/token?state=
//! ```

use anyhow::{anyhow, Result};
use base64::Engine;
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::core::os_crypt;
use crate::core::provider::{
    Account, CheckinStatus, ClaimOutcome, ClaimState, CreditItem, Provider, Quota,
};

/// 默认基址（CN 版）。实际调用时跟随账号的 domain 字段（国际版是 codebuddy.ai）
const BASE_DEFAULT: &str = "https://www.codebuddy.cn";
/// 浏览器 UA（网关会校验，非浏览器 UA 直接 401）
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36";

/// 请求基址：跟随账号 domain（如 www.codebuddy.ai），没有就用 CN 默认。
fn base_of(cred: &Value) -> String {
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

    fn auth_headers(cred: &Value) -> reqwest::header::HeaderMap {
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

    async fn post_json(&self, path: &str, cred: &Value, body: Value) -> Result<(reqwest::StatusCode, Value)> {
        let resp = self
            .http
            .post(format!("{}{path}", base_of(cred)))
            .headers(Self::auth_headers(cred))
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?;
        let status = resp.status();
        let body: Value = resp.json().await.unwrap_or(Value::Null);
        Ok((status, body))
    }
}

// ---------------------------------------------------------------------------
// 本机凭据读取（state.vscdb + safeStorage）
// ---------------------------------------------------------------------------

/// CodeBuddy 客户端的 Electron userData 目录。
pub fn user_data_dir() -> Option<std::path::PathBuf> {
    let appdata = std::env::var_os("APPDATA")?;
    let dir = std::path::PathBuf::from(appdata).join("CodeBuddy");
    dir.is_dir().then_some(dir)
}

/// state.vscdb 里 safeStorage 密文的键名（换号写回时也要用）。
pub fn secret_key() -> String {
    r#"secret://{"extensionId":"tencent-cloud.coding-copilot","key":"planning-genie.new.accessToken"}"#.to_string()
}

/// 读出并解密客户端当前登录的凭据 JSON（只读，不写任何文件）。
/// 换号（`core/client_switch.rs::switch_workbuddy`）也用它拿模板和判身份。
pub fn read_local_secret() -> Option<Value> {
    let dir = user_data_dir()?;
    let key = os_crypt::read_key(&dir.join("Local State")).ok()?;
    let conn = rusqlite::Connection::open(
        dir.join("User").join("globalStorage").join("state.vscdb"),
    )
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
    let bytes: Vec<u8> = wrapper["data"].as_array()?.iter().filter_map(|b| b.as_u64().map(|v| v as u8)).collect();
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
    let key_file = home.join(".cockpit_tools").join("secure-account-storage.key");
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
            base64::engine::general_purpose::STANDARD.decode(envelope["nonce"].as_str().unwrap_or("")),
            base64::engine::general_purpose::STANDARD.decode(envelope["ciphertext"].as_str().unwrap_or("")),
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
    let resp: Value = self.http
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
    let resp = self.http
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
    let info: Value = self.http
            .get(format!("{BASE_DEFAULT}/v2/plugin/login/account?state={state}"))
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
        let hex: String = Sha256::digest(access.as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
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
        // 没有再退回本机 CodeBuddy 客户端的当前登录态（单账号）。
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

        let Some(secret) = read_local_secret() else {
            return Ok(Vec::new());
        };
        let uid = secret["account"]["uid"].as_str().unwrap_or("");
        if uid.is_empty() {
            return Ok(Vec::new());
        }
        let account = Account {
            id: uid.to_string(),
            provider: "workbuddy".to_string(),
            label: secret["account"]["nickname"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(String::from)
                .unwrap_or_else(|| format!("WorkBuddy {}", &uid[..uid.len().min(8)])),
            created_at: chrono::Utc::now().timestamp(),
            last_used: None,
            origin: "本机 CodeBuddy 登录态".to_string(),
        };
        let cred = json!({
            "access_token": secret["token"].as_str().unwrap_or(""),
            "refresh_token": secret["refreshToken"].as_str().unwrap_or(""),
            "expires_at": secret["expiresAt"].as_i64().unwrap_or(0),
            "uid": uid,
            "domain": secret["domain"].as_str().unwrap_or(""),
        });
        Ok(vec![(account, cred)])
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
        let checked = data["todayCheckedIn"].as_bool()
            .or_else(|| data["today_checked_in"].as_bool())
            .or_else(|| data["todayChecked"].as_bool())
            .unwrap_or(false);
        let active = data["active"].as_bool().unwrap_or(true);
        let streak = data["streakDays"].as_i64().or_else(|| data["streak_days"].as_i64()).unwrap_or(0);
        let daily = data["dailyCredit"].as_i64().or_else(|| data["daily_credit"].as_i64()).unwrap_or(0);
        let bonus = data["todayCredit"].as_i64()
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
            let msg = body["message"].as_str().or_else(|| body["msg"].as_str()).unwrap_or("");
            return Err(anyhow!("签到失败 HTTP {} {msg}", code.as_u16()));
        }
        let data = body.get("data").cloned().unwrap_or_else(|| body.clone());
        let success = data["success"].as_bool().unwrap_or(true);
        let credit = data["credit"].as_i64()
            .or_else(|| data["reward"].as_i64())
            .or_else(|| data["credit"].as_f64().map(|f| f as i64))
            .unwrap_or(0);
        if success {
            Ok(ClaimOutcome::claimed(credit, format!("签到成功 +{credit} 积分")))
        } else {
            Ok(ClaimOutcome::already(credit, data["message"].as_str().unwrap_or("今日已签到")))
        }
    }

    async fn quota(&self, _account: &Account, cred: &Value) -> Result<Quota> {
        let (code, body) = self
            .post_json(
                RESOURCE_PATH,
                cred,
                json!({
                    "PageNumber": 1,
                    "PageSize": 100,
                    "ProductCode": "p_tcaca",
                    "Status": [0, 3],
                    "PackageEndTimeRangeBegin": chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
                    "PackageEndTimeRangeEnd": (chrono::Local::now() + chrono::Duration::days(365 * 101)).format("%Y-%m-%d %H:%M:%S").to_string(),
                }),
            )
            .await?;
        if !code.is_success() {
            return Err(anyhow!("积分接口返回 HTTP {}", code.as_u16()));
        }

        // 响应结构有几种可能的嵌套路径，按 Cockpit 的经验逐个找 accounts 数组
        let candidates: Vec<Value> = ["data/resources", "data/data/Response/Data/Accounts", "data/Response/Data/Accounts", "data"]
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

        let num = |v: &Value| -> f64 {
            v.as_f64()
                .or_else(|| v.as_i64().map(|i| i as f64))
                .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
                .unwrap_or(0.0)
        };

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
                credit_type: acc["PackageCode"].as_str().unwrap_or("plan").to_string(),
                granted: g,
                consumed: u,
                remaining: r,
                granted_at_ms: 0,
                expire_at_ms: acc["CycleEndTime"].as_i64().unwrap_or(0),
            });
        }
        if items.is_empty() {
            return Ok(Quota::empty("积分接口返回了数据，但没找到资源列表（上游可能已变更）"));
        }
        let message = format!("剩余 {remaining:.0} / 共 {granted:.0} credits");
        Ok(Quota {
            total_granted: granted,
            total_consumed: used,
            total_remaining: remaining,
            items,
            message,
        })
    }

    /// token 过期时用 X-Refresh-Token 换新（实测 token 有效期很长，极少触发）。
    async fn refresh(&self, _account: &Account, cred: &Value) -> Result<Option<Value>> {
        let rt = cred["refresh_token"].as_str().filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow!("凭据里没有 refresh token"))?;
        let resp = self
            .http
            .post(format!("{}{REFRESH_PATH}", base_of(cred)))
            .header(reqwest::header::AUTHORIZATION, format!("Bearer {}", cred["access_token"].as_str().unwrap_or("")))
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
        let access = data["accessToken"].as_str().or_else(|| data["access_token"].as_str()).unwrap_or("");
        if access.is_empty() {
            return Err(anyhow!("刷新响应里没有 accessToken（上游可能已变更）"));
        }
        let mut fresh = cred.clone();
        fresh["access_token"] = json!(access);
        if let Some(new_rt) = data["refreshToken"].as_str().or_else(|| data["refresh_token"].as_str()).filter(|s| !s.is_empty()) {
            fresh["refresh_token"] = json!(new_rt);
        }
        Ok(Some(fresh))
    }
}
