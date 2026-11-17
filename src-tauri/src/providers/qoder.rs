//! Qoder CN（中国版）每日签到适配器。
//!
//! 协议要点：
//!   * **签到是「活动领取制」**，不是固定接口：先拉活动列表，再对可领取的活动
//!     调 claim。
//!   * 设备授权登录（无验证码、无需抓包）：
//!       打开 `https://qoder.com.cn/device/selectAccounts?challenge=..&challenge_method=S256&machine_id=..&nonce=..`
//!       浏览器里用目标账号确认后，轮询
//!       `https://openapi.qoder.com.cn/api/v1/deviceToken/poll?nonce=&verifier=&challenge_method=S256`
//!       拿到 access token / refresh token。
//!   * 签到接口：
//!       `GET  /sash/api/v1/me/campaigns?forceRefresh=true`
//!       `POST /sash/api/v1/me/campaigns/{campaignId}/claim`
//!   * 请求必须带设备风控头 `Cosy-MachineToken` / `Cosy-MachineCode` /
//!     `Cosy-MachineType`（本机 `~/.qoder-cn/umid-cache.json` 里就有），
//!     否则活动不会下发。
//!   * **活动周期按「10:00 刷新」算，不是按自然日**：活动行的
//!     `startAt`/`endAt` 是一个跨天窗口（如 10-22 10:00 → 10-23 09:59）。
//!     「今天能不能签」= 今天 10:00 起的那个窗口，不能拿昨天窗口的
//!     `CLAIMED` 当成今天已签。
//!   * 账号真实昵称：`GET /api/v1/userinfo`（返回 `name` / `username`）。
//!   * 已领取的判定：HTTP 409 / `result == ALREADY_CLAIMED` / `replayed == true`。

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64URL;
use base64::Engine;
use chrono::Timelike;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::core::provider::{
    Account, AccountProfile, CheckinStatus, ClaimOutcome, ClaimState, CreditItem, Provider, Quota,
};

const WEB_BASE: &str = "https://qoder.com.cn";
const OPENAPI_BASE: &str = "https://openapi.qoder.com.cn";
const CAMPAIGNS_PATH: &str = "/sash/api/v1/me/campaigns";
const DEVICE_POLL_PATH: &str = "/api/v1/deviceToken/poll";
const DEVICE_REFRESH_PATH: &str = "/api/v1/deviceToken/refresh";
const DEVICE_LOGIN_PATH: &str = "/device/selectAccounts";
const USERINFO_PATH: &str = "/api/v1/userinfo";
const SASH_CLIENT_TYPE: &str = "10";
const SASH_USER_AGENT: &str = "Qoder";

/// 设备授权会话。`verifier` / `nonce` 只留在后端，不下发给前端。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthSession {
    pub id: String,
    /// 发起授权的 provider
    pub provider: String,
    /// 让用户在浏览器里打开的授权地址
    pub auth_url: String,
    pub verifier: String,
    pub nonce: String,
    pub machine_id: String,
    pub created_at: i64,
    /// 各平台自己的附加信息（如 MiniMax 设备码轮询的参数名 / 间隔）
    #[serde(default)]
    pub extra: Value,
}

pub struct QoderProvider {
    http: reqwest::Client,
}

impl QoderProvider {
    pub fn new() -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(25))
            .user_agent(SASH_USER_AGENT)
            .build()
            .unwrap_or_default();
        Self { http }
    }

    /// 构造一次设备授权会话。
    pub fn begin_authorization(&self, provider: &str) -> Result<AuthSession> {
        let machine_id = local_machine_id();
        let verifier = b64url(&random_bytes(32)?);
        let challenge = b64url(&Sha256::digest(verifier.as_bytes()));
        let nonce = uuid::Uuid::new_v4().simple().to_string();

        let auth_url = format!(
            "{WEB_BASE}{DEVICE_LOGIN_PATH}?challenge={}&challenge_method=S256&machine_id={}&nonce={}",
            urlenc(&challenge),
            urlenc(&machine_id),
            urlenc(&nonce)
        );

        Ok(AuthSession {
            id: uuid::Uuid::new_v4().to_string(),
            provider: provider.to_string(),
            auth_url,
            verifier,
            nonce,
            machine_id,
            created_at: chrono::Utc::now().timestamp(),
            extra: Value::Null,
        })
    }

    /// 轮询授权结果；`Ok(None)` 表示用户还没在浏览器里确认。
    pub async fn poll_authorization(
        &self,
        session: &AuthSession,
    ) -> Result<Option<(Account, Value)>> {
        let url = format!(
            "{OPENAPI_BASE}{DEVICE_POLL_PATH}?nonce={}&verifier={}&challenge_method=S256",
            urlenc(&session.nonce),
            urlenc(&session.verifier)
        );
        let resp = self
            .http
            .get(url)
            .header("Cosy-Version", "1.0.1")
            .header("Cosy-ClientType", "5")
            .header("User-Agent", "qoder-local-proxy")
            .send()
            .await?;

        if resp.status() == 202 || resp.status() == 404 {
            return Ok(None);
        }
        let status = resp.status();
        let body: Value = resp.json().await.unwrap_or(Value::Null);

        if status.as_u16() >= 400 {
            let msg = body["message"]
                .as_str()
                .or_else(|| body["errorMessage"].as_str())
                .or_else(|| body["msg"].as_str())
                .unwrap_or("授权失败");
            return Err(anyhow!("授权失败（HTTP {}）：{msg}", status.as_u16()));
        }

        // 注意：这个接口的响应**没有 data 包装层**，token 直接在顶层。
        // （实测：{"token":"dt-...","user_id":"...","expires_at":"..."}）
        // 所以 data 缺失时回退到整个 body，不能当成"还没授权"。
        let empty = serde_json::Map::new();
        let data = body
            .get("data")
            .and_then(|d| d.as_object())
            .unwrap_or(&empty);
        let lookup = |keys: &[&str]| -> Option<String> {
            // 先在有 data 包装时找，再在整个 body 里找
            for k in keys {
                if let Some(v) = data.get(*k).and_then(|v| v.as_str()) {
                    if !v.is_empty() {
                        return Some(v.to_string());
                    }
                }
            }
            for k in keys {
                if let Some(v) = body.get(*k).and_then(|v| v.as_str()) {
                    if !v.is_empty() {
                        return Some(v.to_string());
                    }
                }
            }
            None
        };

        let token = lookup(&["token", "access_token", "device_token"]).unwrap_or_default();
        if token.is_empty() {
            return Ok(None);
        }

        let refresh = lookup(&["refresh_token", "refreshToken"]).unwrap_or_default();
        let refresh_id = lookup(&["refresh_token_id", "refreshTokenId"]).unwrap_or_default();
        let user_id = lookup(&["user_id", "userId"]).unwrap_or_default();
        // expires_at 实测是 ISO 字符串（"2026-11-21T13:10:38Z"），不是数字
        let expires_at = lookup(&["expires_at", "expiresAt"])
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
            .map(|dt| dt.timestamp_millis())
            .unwrap_or(0);
        // refresh token 的有效期：不一定下发，有就存下来
        //（「切换客户端账号」写 auth.v1.dat 时需要，见 core/client_switch.rs）
        let refresh_expires_at = lookup(&["refreshTokenExpiresAt", "refresh_token_expires_at"])
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
            .map(|dt| dt.timestamp_millis())
            .unwrap_or(0);

        let label = if user_id.is_empty() {
            "Qoder 账号".to_string()
        } else {
            format!("Qoder {}", &user_id[..user_id.len().min(12)])
        };

        let account = Account {
            id: format!("{:x}", Sha256::digest(user_id.as_bytes()))[..16].to_string(),
            provider: "qoder".to_string(),
            label,
            created_at: chrono::Utc::now().timestamp(),
            last_used: None,
            origin: "设备授权登录".to_string(),
        };
        let cred = json!({
            "access_token": token,
            "refresh_token": refresh,
            "refresh_token_id": refresh_id,
            "user_id": user_id,
            "expires_at": expires_at,
            "refresh_expires_at": refresh_expires_at,
            "machine_id": session.machine_id,
            "region": "cn",
        });
        Ok(Some((account, cred)))
    }

    /// 拼装 SASH 接口需要的请求头（含设备风控三件套）。
    fn sash_headers(&self, cred: &Value) -> reqwest::header::HeaderMap {
        let mut headers = reqwest::header::HeaderMap::new();
        if let Some(token) = cred["access_token"].as_str() {
            if let Ok(value) = format!("Bearer {token}").parse() {
                headers.insert(reqwest::header::AUTHORIZATION, value);
            }
        }
        headers.insert(
            "cosy-clienttype",
            SASH_CLIENT_TYPE
                .parse::<reqwest::header::HeaderValue>()
                .unwrap(),
        );
        headers.insert(
            "accept",
            "application/json"
                .parse::<reqwest::header::HeaderValue>()
                .unwrap(),
        );
        headers.insert(
            "accept-language",
            "zh-CN".parse::<reqwest::header::HeaderValue>().unwrap(),
        );
        headers.insert(
            "user-agent",
            SASH_USER_AGENT
                .parse::<reqwest::header::HeaderValue>()
                .unwrap(),
        );
        headers.insert(
            "Cosy-Version",
            client_version()
                .parse::<reqwest::header::HeaderValue>()
                .unwrap(),
        );
        headers.insert(
            "Cosy-MachineOS",
            machine_os()
                .parse::<reqwest::header::HeaderValue>()
                .unwrap(),
        );
        headers.insert(
            "Cosy-MachineId",
            cred["machine_id"]
                .as_str()
                .unwrap_or("")
                .parse::<reqwest::header::HeaderValue>()
                .unwrap(),
        );
        let hostname = machine_hostname();
        if !hostname.is_empty() {
            headers.insert(
                "Cosy-MachineHostname",
                hostname.parse::<reqwest::header::HeaderValue>().unwrap(),
            );
        }
        if let Some(risk) = umid_identity() {
            headers.insert(
                "Cosy-MachineToken",
                risk.token.parse::<reqwest::header::HeaderValue>().unwrap(),
            );
            headers.insert(
                "Cosy-MachineCode",
                risk.code.parse::<reqwest::header::HeaderValue>().unwrap(),
            );
            headers.insert(
                "Cosy-MachineType",
                risk.r#type.parse::<reqwest::header::HeaderValue>().unwrap(),
            );
        }
        headers
    }

    /// 拉活动列表。
    async fn fetch_campaigns(&self, cred: &Value) -> Result<(reqwest::StatusCode, Value)> {
        let url = format!("{OPENAPI_BASE}{CAMPAIGNS_PATH}?forceRefresh=true");
        let resp = self
            .http
            .get(url)
            .headers(self.sash_headers(cred))
            .send()
            .await?;
        let status = resp.status();
        let body: Value = resp.json().await.unwrap_or(Value::Null);
        Ok((status, body))
    }

    /// 从活动列表响应里取 `campaigns` 数组。
    fn campaign_rows(&self, body: &Value) -> Vec<Value> {
        body.get("campaigns")
            .and_then(|v| v.as_array())
            .or_else(|| {
                body.get("data")
                    .and_then(|d| d.get("campaigns"))
                    .and_then(|v| v.as_array())
            })
            .cloned()
            .unwrap_or_default()
    }
}

/// Qoder's userinfo has appeared both as a flat object and under response
/// wrappers. Accept the profile object itself, common wrapper keys, and the
/// equivalent casing used by different API generations.
fn profile_text<'a>(value: &'a Value, keys: &[&str], depth: u8) -> Option<&'a str> {
    if let Some(object) = value.as_object() {
        for key in keys {
            if let Some(text) = object.get(*key).and_then(Value::as_str) {
                let text = text.trim();
                if !text.is_empty() {
                    return Some(text);
                }
            }
        }
        if depth > 0 {
            for key in ["data", "user", "profile", "result", "account"] {
                if let Some(child) = object.get(key) {
                    if let Some(text) = profile_text(child, keys, depth - 1) {
                        return Some(text);
                    }
                }
            }
        }
    }
    None
}

/// 是不是「每日积分」活动行。
///
/// 活动列表里还混着别的 `CLAIM_BENEFIT` 活动（例如「发布站点领奶茶兑换卡」，
/// `benefit.kind == REDEMPTION_CODE`），不能当成每日签到，否则会领错东西。
fn is_daily_credits(row: &Value) -> bool {
    row.get("actionType").and_then(|v| v.as_str()) == Some("CLAIM_BENEFIT")
        && row
            .get("benefit")
            .and_then(|b| b.get("kind"))
            .and_then(|v| v.as_str())
            == Some("CREDITS")
}

/// 活动的 `[startAt, endAt]` 窗口（Unix 秒）。
fn campaign_window(row: &Value) -> Option<(i64, i64)> {
    let start = row.get("startAt").and_then(|v| v.as_i64())?;
    let end = row.get("endAt").and_then(|v| v.as_i64())?;
    Some((start, end))
}

/// 把 Unix 秒换成本地时间。
fn to_local(ts: i64) -> chrono::DateTime<chrono::Local> {
    chrono::DateTime::<chrono::Utc>::from_timestamp(ts, 0)
        .unwrap_or_default()
        .with_timezone(&chrono::Local)
}

/// 从活动行推断「每日刷新时刻」（本地时间的当天分钟数）。
///
/// 实测：每日活动的 `startAt` 是 10:00（UTC+8），窗口到次日 09:59 结束。
/// 取不到时按 10:00 兜底。
fn reset_minute_of_day(row: &Value) -> Option<u32> {
    let (start, _) = campaign_window(row)?;
    let local = to_local(start);
    Some(local.hour() * 60 + local.minute())
}

/// 一行活动里取可领取积分数。
fn row_points(row: &Value) -> i64 {
    row.get("benefit")
        .and_then(|b| b.get("amount"))
        .and_then(|v| v.as_i64())
        .unwrap_or(100)
}

/// 「今天这个周期」的判定结果。
#[derive(Debug)]
enum Cycle {
    /// 今天可以领，带可领积分数
    Claimable(i64),
    /// 今天已经领过了（带当天积分数 —— 首页福利展示要显示「每天大概多少」，
    /// 已领取时也要能看出来，不能只报 0）
    Claimed(i64),
    /// 今天还没到刷新点
    NotYet { reset_minute: u32 },
    /// 已经过了刷新点，但上游还没下发今天的活动行
    NotIssued { reset_minute: u32 },
}

/// 判断「今天能不能签」。
///
/// 关键点：活动行的 `[startAt, endAt]` 是**跨天窗口**（如 10-22 10:00 → 10-23 09:59）。
/// 只看「有没有 CLAIMED 行」会把上一个周期的已领记录当成今天已签，
/// 导致 10:00 之前一直显示「今日已签」。所以这里取的是
/// 「startAt 是今天、且已经过了刷新点」的那一行。
fn daily_cycle(rows: &[Value], now: &chrono::DateTime<chrono::Local>) -> Cycle {
    let daily: Vec<&Value> = rows.iter().filter(|r| is_daily_credits(r)).collect();

    // 刷新时刻：由每日活动的 startAt 推断（默认 10:00 UTC+8）
    let reset_minute = daily
        .iter()
        .find_map(|row| reset_minute_of_day(row))
        .unwrap_or(10 * 60);

    let today = now.format("%Y-%m-%d").to_string();
    let now_minute = now.hour() * 60 + now.minute();
    let now_ts = now.timestamp();

    // 「今天这个周期」= startAt 是今天、已经过了刷新点、**并且已经开窗**
    // （startAt <= now）的那一行。三个条件缺一不可：
    //   * 只看「有没有 CLAIMED 行」→ 会把上一个周期的已领记录当成今天已签；
    //   * 不看 startAt 是否已到 → 活动行提前下发时会显示「可签」，实际还没开窗。
    let today_row = daily.iter().find(|row| {
        campaign_window(row)
            .map(|(start, _)| {
                let local = to_local(start);
                local.format("%Y-%m-%d").to_string() == today
                    && local.hour() * 60 + local.minute() >= reset_minute
                    && start <= now_ts
            })
            .unwrap_or(false)
    });

    if let Some(row) = today_row {
        return match row.get("claimStatus").and_then(|v| v.as_str()) {
            Some("CLAIMABLE") => Cycle::Claimable(row_points(row)),
            Some("CLAIMED") => Cycle::Claimed(row_points(row)),
            _ => Cycle::NotIssued { reset_minute },
        };
    }

    if now_minute < reset_minute {
        Cycle::NotYet { reset_minute }
    } else {
        Cycle::NotIssued { reset_minute }
    }
}

/// 解析续期响应：兼容顶层 / `data` 包装，返回更新后的凭据。
/// 响应示例：`{device_token, refresh_token, expires_at, refresh_token_expires_at}`。
fn parse_refresh_response(cred: &Value, body: &Value) -> Result<Value> {
    let data = body
        .get("data")
        .filter(|d| d.is_object())
        .unwrap_or(body);
    let token = data["device_token"]
        .as_str()
        .or_else(|| data["token"].as_str())
        .or_else(|| data["access_token"].as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("续期响应里没有 device_token（上游可能已变更）"))?;

    let now_ms = chrono::Utc::now().timestamp_millis();
    let mut fresh = cred.clone();
    fresh["access_token"] = json!(token);
    if let Some(rt) = data["refresh_token"].as_str().filter(|s| !s.is_empty()) {
        fresh["refresh_token"] = json!(rt);
    }
    if let Some(uid) = data["user_id"].as_str().filter(|s| !s.is_empty()) {
        fresh["user_id"] = json!(uid);
    }

    let expires_at = data["expires_at"]
        .as_str()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.timestamp_millis())
        .or_else(|| data["expires_in"].as_i64().map(|secs| now_ms + secs * 1000))
        .or_else(|| data["expires_in_ms"].as_i64().map(|ms| now_ms + ms));
    if let Some(ms) = expires_at.filter(|ms| *ms > 0) {
        fresh["expires_at"] = json!(ms);
    }

    let refresh_expires_at = data["refresh_token_expires_at"]
        .as_str()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.timestamp_millis())
        .or_else(|| {
            data["refresh_token_expires_in"]
                .as_i64()
                .map(|secs| now_ms + secs * 1000)
        });
    if let Some(ms) = refresh_expires_at.filter(|ms| *ms > 0) {
        fresh["refresh_expires_at"] = json!(ms);
    }
    Ok(fresh)
}

#[async_trait]
impl Provider for QoderProvider {
    fn id(&self) -> &'static str {
        "qoder"
    }

    fn display_name(&self) -> &'static str {
        "Qoder CN"
    }

    fn accent(&self) -> &'static str {
        "#6366f1"
    }

    fn docs_url(&self) -> &'static str {
        "https://docs.qoder.cn/"
    }

    /// 从本机客户端登录态导入（解密 `auth.v1.dat`，只读）。
    /// access token 临期时由 `ensure_fresh_cred` 调用 `refresh` 续期。
    async fn discover(&self) -> Result<Vec<(Account, Value)>> {
        Ok(crate::core::client_switch::qoder_client_login()
            .into_iter()
            .collect())
    }

    /// 用 refresh token 续期；服务端会轮换 refresh token，新值一并回写。
    ///
    /// 协议来自实测：`POST /api/v1/deviceToken/refresh`，body `{refresh_token}`，
    /// 响应含 `device_token` / `refresh_token` / `expires_at`。
    /// 旧 refresh token 不会立即失效，因此不会把桌面客户端顶下线。
    async fn refresh(&self, _account: &Account, cred: &Value) -> Result<Option<Value>> {
        let refresh_token = cred["refresh_token"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow!("凭据里没有 refresh token，无法续期；请在 Qoder 客户端重新登录后导入"))?;
        let refresh_expires_at = cred["refresh_expires_at"].as_i64().unwrap_or(0);
        if refresh_expires_at > 0 && refresh_expires_at <= chrono::Utc::now().timestamp_millis() {
            return Err(anyhow!(
                "refresh token 已过期，请在 Qoder 客户端重新登录后重新导入"
            ));
        }

        let resp = self
            .http
            .post(format!("{OPENAPI_BASE}{DEVICE_REFRESH_PATH}"))
            .header("Accept", "application/json")
            .json(&json!({ "refresh_token": refresh_token }))
            .send()
            .await?;
        let status = resp.status();
        let body: Value = resp.json().await.unwrap_or(Value::Null);
        if !status.is_success() {
            let msg = body["message"]
                .as_str()
                .or_else(|| body["errorMessage"].as_str())
                .or_else(|| body["msg"].as_str())
                .unwrap_or("");
            return Err(anyhow!(
                "Qoder 续期失败（HTTP {} {}）：请在客户端重新登录后重新导入",
                status.as_u16(),
                msg
            ));
        }
        parse_refresh_response(cred, &body).map(Some)
    }

    async fn status(&self, _account: &Account, cred: &Value) -> Result<CheckinStatus> {
        let (status, body) = self.fetch_campaigns(cred).await?;

        if status.as_u16() == 401 || status.as_u16() == 403 {
            return Err(anyhow!(
                "签到活动查询失败（HTTP {}）：凭据无效或地区不匹配",
                status.as_u16()
            ));
        }
        if status.as_u16() == 429 {
            return Err(anyhow!("签到活动查询失败（HTTP 429）：请稍后重试"));
        }
        if !status.is_success() {
            return Err(anyhow!("活动列表返回 HTTP {}", status.as_u16()));
        }

        let rows = self.campaign_rows(&body);
        let now = chrono::Local::now();

        return Ok(match daily_cycle(&rows, &now) {
            Cycle::Claimable(points) => CheckinStatus {
                state: ClaimState::Claimable,
                points,
                streak_days: 0,
                message: format!("可签到 +{points} Credits"),
                day_index: None,
                bonus_points: 0,
                days: Vec::new(),
            },
            Cycle::Claimed(points) => CheckinStatus {
                state: ClaimState::Claimed,
                points,
                streak_days: 0,
                message: format!("今日已领取 +{points} Credits"),
                day_index: None,
                bonus_points: 0,
                days: Vec::new(),
            },
            Cycle::NotYet { reset_minute } => {
                let reset_text = format!("{:02}:{:02}", reset_minute / 60, reset_minute % 60);
                CheckinStatus {
                    state: ClaimState::NotAvailable,
                    points: 0,
                    streak_days: 0,
                    message: format!("今日活动 {reset_text}（UTC+8）开启"),
                    day_index: None,
                    bonus_points: 0,
                    days: Vec::new(),
                }
            }
            Cycle::NotIssued { reset_minute } => {
                let reset_text = format!("{:02}:{:02}", reset_minute / 60, reset_minute % 60);
                CheckinStatus {
                    state: ClaimState::NotAvailable,
                    points: 0,
                    streak_days: 0,
                    message: format!("已过 {reset_text}，但今天的活动尚未下发（上游可能已变更）"),
                    day_index: None,
                    bonus_points: 0,
                    days: Vec::new(),
                }
            }
        });
    }

    async fn claim(&self, _account: &Account, cred: &Value) -> Result<ClaimOutcome> {
        let (status, body) = self.fetch_campaigns(cred).await?;
        if !status.is_success() {
            return Err(anyhow!("活动列表返回 HTTP {}", status.as_u16()));
        }

        let rows = self.campaign_rows(&body);

        // 只认「每日积分」活动，并且优先取窗口更晚的那个
        // （跨天窗口下今天/昨天的可能同时开着，晚的那个才是当前该领的）。
        let mut candidates: Vec<Value> = rows
            .into_iter()
            .filter(|row| is_daily_credits(row))
            .collect();
        candidates.sort_by_key(|row| campaign_window(row).map(|(s, _)| s).unwrap_or(0));

        let target = candidates
            .iter()
            .rev()
            .find(|row| row.get("claimStatus").and_then(|v| v.as_str()) == Some("CLAIMABLE"));

        let Some(target) = target else {
            let already = candidates
                .iter()
                .any(|row| row.get("claimStatus").and_then(|v| v.as_str()) == Some("CLAIMED"));
            return if already {
                Ok(ClaimOutcome::already(0, "当前周期已领取"))
            } else {
                Ok(ClaimOutcome::failed("当前没有可领取的每日积分活动"))
            };
        };

        let campaign_id = target
            .get("campaignId")
            .or_else(|| target.get("campaignKey"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("活动数据里没有 campaignId，无法领取"))?;

        let url = format!("{OPENAPI_BASE}{CAMPAIGNS_PATH}/{campaign_id}/claim");
        let mut headers = self.sash_headers(cred);
        headers.insert(
            "content-type",
            "application/json"
                .parse::<reqwest::header::HeaderValue>()
                .unwrap(),
        );
        headers.insert(
            "origin",
            OPENAPI_BASE
                .parse::<reqwest::header::HeaderValue>()
                .unwrap(),
        );

        let resp = self
            .http
            .post(url)
            .headers(headers)
            .json(&json!({}))
            .send()
            .await?;
        let status = resp.status();
        let body: Value = resp.json().await.unwrap_or(Value::Null);

        let already = status.as_u16() == 409
            || body.get("result").and_then(|v| v.as_str()) == Some("ALREADY_CLAIMED")
            || body.get("replayed").and_then(|v| v.as_bool()) == Some(true);
        if already {
            return Ok(ClaimOutcome::already(0, "当前周期已领取"));
        }
        if !status.is_success() {
            let msg = body
                .get("errorMessage")
                .or_else(|| body.get("message"))
                .or_else(|| body.get("msg"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            return Err(anyhow!("领取失败 HTTP {} {msg}", status.as_u16()));
        }
        if body.get("status").and_then(|v| v.as_str()) != Some("CLAIMED") {
            let s = body
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("未知");
            return Err(anyhow!("签到未完成（上游状态 {s}）"));
        }

        let amount = body
            .get("benefit")
            .and_then(|b| b.get("amount"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let days = body
            .get("benefit")
            .and_then(|b| b.get("validity"))
            .and_then(|v| v.get("days"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let mut message = format!("签到成功 +{amount} Credits");
        if days > 0 {
            message.push_str(&format!("，{days} 天有效"));
        }
        Ok(ClaimOutcome::claimed(amount, message))
    }

    async fn quota(&self, _account: &Account, cred: &Value) -> Result<Quota> {
        // 客户端自己用的就是 /sash/api/v2/me/usage（见其 main.log）。
        // 实测响应结构：
        //   { "qoderUsage": {
        //       "userQuota":  {"total":300,"used":15,"remaining":285,"unit":"credits"},
        //       "addOnQuota": {"total":500,"used":0, "remaining":500,"unit":"credits"} } }
        // 两块配额（套餐 + 加购）合并展示。
        let url = format!("{OPENAPI_BASE}/sash/api/v2/me/usage");
        let resp = self
            .http
            .get(url)
            .headers(self.sash_headers(cred))
            .send()
            .await?;

        let status = resp.status();
        let body: Value = resp.json().await.unwrap_or(Value::Null);

        if !status.is_success() {
            let msg = body
                .get("message")
                .or_else(|| body.get("errorMessage"))
                .or_else(|| body.get("msg"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            return Err(anyhow!("积分查询失败 HTTP {} {msg}", status.as_u16()));
        }

        let num = |v: &Value| -> f64 {
            v.as_f64()
                .or_else(|| v.as_i64().map(|i| i as f64))
                .or_else(|| {
                    v.as_str()
                        .and_then(|s| s.replace(',', "").trim().parse::<f64>().ok())
                })
                .unwrap_or(0.0)
        };

        let usage = &body["qoderUsage"];
        let user_q = &usage["userQuota"];
        let addon_q = &usage["addOnQuota"];

        // 结构不符时如实说明，不猜数
        if usage.is_null() || user_q.is_null() {
            return Ok(Quota::empty(
                "积分接口返回了数据，但结构不是预期的 qoderUsage.userQuota（上游可能已变更）",
            ));
        }

        let mut items = Vec::new();
        let mut total_granted = 0.0;
        let mut total_consumed = 0.0;
        let mut total_remaining = 0.0;

        // expiresAt 实测是毫秒；有的账号返回 9999 年（视为长期有效）。
        // 保险起见兼容秒级时间戳。
        let expires_raw = usage["expiresAt"].as_i64().unwrap_or(0);
        let expires_at_ms = if expires_raw > 0 && expires_raw < 1_000_000_000_000 {
            expires_raw * 1000
        } else {
            expires_raw
        };

        let mut absorb = |label: &str, q: &Value, items: &mut Vec<CreditItem>| {
            let granted = num(&q["total"]);
            let consumed = num(&q["used"]);
            let remaining = num(&q["remaining"]);
            total_granted += granted;
            total_consumed += consumed;
            total_remaining += remaining;
            items.push(CreditItem {
                credit_type: label.to_string(),
                granted,
                consumed,
                remaining,
                granted_at_ms: 0,
                expire_at_ms: expires_at_ms,
                package_name: String::new(),
                refresh_at_ms: 0,
            });
        };

        absorb("plan", user_q, &mut items);
        if !addon_q.is_null() {
            absorb("addon", addon_q, &mut items);
        }

        let message = if total_remaining > 0.0 {
            format!("剩余 {total_remaining:.0} / 共 {total_granted:.0} Credits")
        } else {
            "当前无剩余 Credits".to_string()
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

    /// 账号真实资料。
    ///
    /// 设备授权只给一个内部 uid（界面会显示成 `Qoder 01a1348c-3fc` 这种 id 前缀），
    /// 这个接口能拿回用户在 Qoder 里真实设置的昵称：
    ///   `GET /api/v1/userinfo` → `{ id, name, username, avatar, source, ... }`
    async fn profile(&self, _account: &Account, cred: &Value) -> Result<Option<AccountProfile>> {
        let token = cred["access_token"].as_str().unwrap_or("");
        if token.is_empty() {
            return Ok(None);
        }
        let url = format!("{OPENAPI_BASE}{USERINFO_PATH}");
        let resp = self
            .http
            .get(url)
            .header(reqwest::header::AUTHORIZATION, format!("Bearer {token}"))
            .header("accept", "application/json")
            .header("user-agent", SASH_USER_AGENT)
            .send()
            .await?;

        let status = resp.status();
        let body: Value = resp.json().await.unwrap_or(Value::Null);
        if !status.is_success() {
            return Ok(None);
        }

        let name = profile_text(
            &body,
            &["name", "nickname", "displayName", "display_name", "username", "loginName", "login_name"],
            3,
        )
        .map(str::to_string);
        let email = profile_text(&body, &["email", "emailAddress", "email_address"], 3)
            .map(str::to_string);
        let avatar = profile_text(&body, &["avatar", "avatarUrl", "avatar_url", "picture"], 3)
            .map(str::to_string);
        if name.is_none() && email.is_none() {
            return Ok(None);
        }
        Ok(Some(AccountProfile {
            name,
            email,
            avatar,
        }))
    }
}

// ---------------------------------------------------------------------------
// 本机环境辅助
// ---------------------------------------------------------------------------

/// 设备风控身份。
#[derive(Debug, Clone)]
pub struct UmidIdentity {
    pub token: String,
    pub code: String,
    pub r#type: String,
}

/// 读取本机 Qoder 客户端缓存的 UMID 三件套。
///
/// 客户端启动过就会写这个文件；没有的话返回 None（此时活动可能不下发）。
pub fn umid_identity() -> Option<UmidIdentity> {
    let home = dirs::home_dir()?;
    for candidate in [
        home.join(".qoder-cn").join("umid-cache.json"),
        home.join(".qoder").join("umid-cache.json"),
    ] {
        let Ok(raw) = std::fs::read_to_string(&candidate) else {
            continue;
        };
        let Ok(parsed) = serde_json::from_str::<Value>(&raw) else {
            continue;
        };
        let info = &parsed["info"];
        let token = info["machineToken"].as_str().unwrap_or("").to_string();
        let code = info["machineCode"].as_str().unwrap_or("").to_string();
        let r#type = info["machineType"].as_str().unwrap_or("").to_string();
        if !token.is_empty() && !code.is_empty() {
            return Some(UmidIdentity {
                token,
                code,
                r#type,
            });
        }
    }
    None
}

/// 本机 machine id（客户端自己缓存了一份）。
pub fn local_machine_id() -> String {
    let home = dirs::home_dir().unwrap_or_default();
    for candidate in [
        home.join(".qoder-cn").join(".auth").join("machine_id"),
        home.join(".qoder").join(".auth").join("machine_id"),
    ] {
        if let Ok(value) = std::fs::read_to_string(&candidate) {
            let value = value.trim();
            if !value.is_empty() {
                return value.to_string();
            }
        }
    }
    uuid::Uuid::new_v4().simple().to_string()
}

/// `Cosy-MachineOS`，形如 `x86_64_win32`。
fn machine_os() -> String {
    let arch = if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        "unknown"
    };
    let system = if cfg!(target_os = "windows") {
        "win32"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else {
        "linux"
    };
    format!("{arch}_{system}")
}

/// `Cosy-MachineHostname`。
fn machine_hostname() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// 客户端版本号（`Cosy-Version` 头）。
///
/// 从本机能定位到的所有 Qoder 安装根里的 `.qoder-versions\<版本>\` 目录取
/// 语义化版本最高的（定位逻辑见 `core/launcher.rs`）。
fn client_version() -> String {
    crate::core::launcher::qoder_versions()
        .into_iter()
        .filter_map(|v| {
            let mut parts = v.split('.');
            let a = parts.next().and_then(|x| x.parse::<u64>().ok())?;
            let b = parts
                .next()
                .and_then(|x| x.parse::<u64>().ok())
                .unwrap_or(0);
            let c = parts
                .next()
                .and_then(|x| x.parse::<u64>().ok())
                .unwrap_or(0);
            Some((a, b, c, v))
        })
        .max()
        .map(|(_, _, _, v)| v)
        .unwrap_or_else(|| "0.4.9".to_string())
}

// ---------------------------------------------------------------------------
// 小工具
// ---------------------------------------------------------------------------

fn b64url(bytes: &[u8]) -> String {
    B64URL.encode(bytes)
}

/// 用系统随机源取随机字节。
fn random_bytes(len: usize) -> Result<Vec<u8>> {
    let mut buf = vec![0u8; len];
    getrandom::getrandom(&mut buf).map_err(|e| anyhow!("读取系统随机源失败：{e}"))?;
    Ok(buf)
}

fn urlenc(value: &str) -> String {
    value
        .chars()
        .map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
            _ => format!("%{:02X}", c as u32),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    /// 构造一个本地时间点。
    fn at(y: i32, m: u32, d: u32, hh: u32, mm: u32) -> chrono::DateTime<chrono::Local> {
        chrono::Local
            .with_ymd_and_hms(y, m, d, hh, mm, 0)
            .single()
            .expect("本地时间不唯一（夏令时重叠）")
    }

    /// 一条每日积分活动行。`start` 为该周期起点的时间戳。
    fn daily_row(start: i64, status: &str) -> Value {
        json!({
            "campaignId": "camp-1",
            "campaignKey": "act-20261023-993",
            "actionType": "CLAIM_BENEFIT",
            "startAt": start,
            "endAt": start + 24 * 3600 - 60,
            "claimStatus": status,
            "benefit": { "kind": "CREDITS", "amount": 100 }
        })
    }

    /// 非每日积分的活动行（例如「发布站点领奶茶兑换卡」），不该被当成签到。
    fn voucher_row(status: &str) -> Value {
        json!({
            "campaignId": "camp-2",
            "actionType": "CLAIM_BENEFIT",
            "startAt": 1_790_697_600,
            "endAt": 1_793_289_540,
            "claimStatus": status,
            "benefit": { "kind": "REDEMPTION_CODE", "amount": 1 }
        })
    }

    #[test]
    fn 十点前拿昨天周期的已领记录不能算今天已签() {
        // 10-23 09:23：当前窗口是 10-22 10:00 起的那一个，已领
        let rows = vec![daily_row(at(2026, 10, 22, 10, 0).timestamp(), "CLAIMED")];
        assert!(matches!(
            daily_cycle(&rows, &at(2026, 10, 23, 9, 23)),
            Cycle::NotYet { .. }
        ));
    }

    #[test]
    fn 十点后今天的活动可领() {
        let rows = vec![
            daily_row(at(2026, 10, 22, 10, 0).timestamp(), "CLAIMED"),
            daily_row(at(2026, 10, 23, 10, 0).timestamp(), "CLAIMABLE"),
        ];
        match daily_cycle(&rows, &at(2026, 10, 23, 10, 5)) {
            Cycle::Claimable(points) => assert_eq!(points, 100),
            other => panic!("应为 Claimable，实际 {other:?}"),
        }
    }

    #[test]
    fn 十点后今天已领() {
        let rows = vec![daily_row(at(2026, 10, 23, 10, 0).timestamp(), "CLAIMED")];
        match daily_cycle(&rows, &at(2026, 10, 23, 10, 5)) {
            Cycle::Claimed(points) => assert_eq!(points, 100, "已领取也要带当天积分数"),
            other => panic!("应为 Claimed，实际 {other:?}"),
        }
    }

    #[test]
    fn 兑换码活动不算签到() {
        // 只有奶茶兑换卡（REDEMPTION_CODE）时，10:00 前应显示「未开始」
        let rows = vec![voucher_row("CLAIMED")];
        assert!(matches!(
            daily_cycle(&rows, &at(2026, 10, 23, 9, 0)),
            Cycle::NotYet { .. }
        ));
        // 10:00 后也没有每日活动行 → 未下发
        assert!(matches!(
            daily_cycle(&rows, &at(2026, 10, 23, 10, 5)),
            Cycle::NotIssued { .. }
        ));
    }

    #[test]
    fn 过点但上游没下发() {
        let rows: Vec<Value> = Vec::new();
        match daily_cycle(&rows, &at(2026, 10, 23, 10, 5)) {
            Cycle::NotIssued { reset_minute } => assert_eq!(reset_minute, 10 * 60),
            other => panic!("应为 NotIssued，实际 {other:?}"),
        }
    }

    #[test]
    fn 刷新时刻从活动行推断() {
        let rows = vec![daily_row(at(2026, 10, 23, 10, 0).timestamp(), "CLAIMABLE")];
        match daily_cycle(&rows, &at(2026, 10, 23, 9, 59)) {
            Cycle::NotYet { reset_minute } => {
                assert_eq!(reset_minute, 600);
            }
            other => panic!("应为 NotYet，实际 {other:?}"),
        }
    }

    /// 真机验证：本机 Qoder 客户端登录态能解出账号（只读，不写账号池）。
    #[test]
    #[ignore = "依赖本机 Qoder 登录态，手动跑：cargo test -- --ignored 本机Qoder登录态能解出"]
    fn 本机Qoder登录态能解出() {
        let Some((account, cred)) = crate::core::client_switch::qoder_client_login() else {
            panic!("本机没有可解的 Qoder 登录态");
        };
        assert_eq!(account.provider, "qoder");
        assert!(!account.id.is_empty());
        assert!(cred["access_token"].as_str().is_some_and(|s| !s.is_empty()));
    }

    #[test]
    fn 续期响应解析_轮换rt并保留其他字段() {
        let old = json!({
            "access_token": "dt-old",
            "refresh_token": "drt-old",
            "machine_id": "m1",
            "region": "cn",
            "expires_at": 1_000,
        });
        let fresh = parse_refresh_response(
            &old,
            &json!({
                "device_token": "dt-new",
                "refresh_token": "drt-new",
                "expires_at": "2026-12-16T00:00:00Z",
                "refresh_token_expires_at": "2027-12-16T00:00:00Z",
                "user_id": "u1",
            }),
        )
        .expect("应能解析续期响应");
        assert_eq!(fresh["access_token"], "dt-new");
        assert_eq!(fresh["refresh_token"], "drt-new");
        assert_eq!(fresh["machine_id"], "m1");
        assert_eq!(fresh["region"], "cn");
        assert_eq!(fresh["user_id"], "u1");
        assert!(fresh["expires_at"].as_i64().unwrap() > 1_000);
        assert!(fresh["refresh_expires_at"].as_i64().unwrap() > 0);
    }

    #[test]
    fn 续期响应缺少token时报错() {
        let err = parse_refresh_response(&json!({}), &json!({ "refresh_token": "x" }));
        assert!(err.is_err());
    }

    /// 真机验证：用本机 Qoder 登录态真实调用续期接口（不写账号池）。
    /// 服务端会轮换 refresh token，但旧值不会立即失效，桌面客户端不受影响。
    #[test]
    #[ignore = "会真实调用 Qoder 续期接口，手动跑：cargo test -- --ignored 本机Qoder续期接口能刷新"]
    fn 本机Qoder续期接口能刷新() {
        let Some((account, cred)) = crate::core::client_switch::qoder_client_login() else {
            panic!("本机没有可用的 Qoder 登录态");
        };
        let rt = tokio::runtime::Runtime::new().unwrap();
        let fresh = rt
            .block_on(QoderProvider::new().refresh(&account, &cred))
            .expect("续期请求失败")
            .expect("续期没有返回新凭据");
        assert!(fresh["access_token"].as_str().is_some_and(|s| !s.is_empty()));
        assert_ne!(
            fresh["access_token"].as_str(),
            cred["access_token"].as_str(),
            "续期后 access token 应变化"
        );
    }
}
