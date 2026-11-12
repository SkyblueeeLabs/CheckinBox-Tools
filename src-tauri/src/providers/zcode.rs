//! Zcode（Z.ai / 智谱）适配器。
//!
//! 产品标识：`zcode`，数据目录 `~/.zcode`，客户端 `ZCode.exe`。
//!
//! 协议（已实测）：
//!   * 登录态：`~/.zcode/v2/credentials.json` —— AES-256-GCM 加密，
//!     键 `zcodejwttoken` 解密后是 JWT；
//!   * 设备标识：`~/.zcode/v2/onboarding-record.json`（**明文**），含
//!     `deviceMid` 和 `decisions[].userId`；
//!   * 余额：`GET https://zcode.z.ai/api/v1/zcode-plan/billing/balance`
//!     （只读，返回 `data.plans[]` 套餐 + `data.balances[]` 额度桶）。
//!
//! **鉴权方式**：请求头带 `authorization: Bearer <jwt>` + `x-device-mid` +
//! `x-zcode-app-version` 等（照抄官方客户端，见 dsh-zcode-free 的逆向）。
//!
//! **重要**：Zcode 的每日额度（3 亿 GLM-5.3-Flash Token）**只能在官方客户端
//! 领取** —— 领取接口要过阿里云验证码，第三方无法自动完成。所以本适配器
//! **只做「余额展示 + 领取提示」**，`supports_claim()` 返回 `false`，
//! 调度器和「一键全部签到」都不会调用 `claim()`。

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64URL;
use base64::Engine;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::core::provider::{
    Account, AccountProfile, CheckinStatus, ClaimOutcome, ClaimState, CreditItem, Provider, Quota,
};

const BASE: &str = "https://zcode.z.ai";
const BALANCE_PATH: &str = "/api/v1/zcode-plan/billing/balance";
/// 官方客户端版本号。上游可能按它做兼容判断，客户端大版本升级后需要同步。
const APP_VERSION: &str = "3.17.2";
/// `credentials.json` 里 JWT 所在的键（Zcode 自己的命名）。
const JWT_KEY: &str = "zcodejwttoken";
const CREDENTIALS_REL: &str = ".zcode/v2/credentials.json";
const ONBOARDING_REL: &str = ".zcode/v2/onboarding-record.json";
const ENC_PREFIX: &str = "enc:v1:";
const NONCE_LEN: usize = 12;

// ---------------------------------------------------------------------------
// 本机登录态读取 + 解密
// ---------------------------------------------------------------------------

fn credentials_path() -> Option<std::path::PathBuf> {
    dirs::home_dir().map(|h| h.join(CREDENTIALS_REL))
}

fn onboarding_path() -> Option<std::path::PathBuf> {
    dirs::home_dir().map(|h| h.join(ONBOARDING_REL))
}

/// 候选 seed，按优先级排列。
///
/// Zcode 桌面端用 `zcode-credential-fallback:<platform>:<homedir>:<username>`
/// 当 AES 密钥的 seed（见 dsh-zcode-free 的逆向）。两个坑：
///   * `os.platform()` 在 Windows 上是 `win32`，而 Rust 的
///     `std::env::consts::OS` 是 `windows`，需要映射；
///   * `os.homedir()` 与 `dirs::home_dir()` 理论上都是 `C:\Users\<name>`，
///     但保险起见再补一个正斜杠变体。
fn candidate_seeds() -> Vec<String> {
    let mut seeds = Vec::new();
    if let Ok(s) = std::env::var("ZCODE_CREDENTIAL_SECRET") {
        if !s.trim().is_empty() {
            seeds.push(s);
        }
    }
    let username = std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "unknown".to_string());
    let home = dirs::home_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let platform = match std::env::consts::OS {
        "windows" => "win32",
        "macos" => "darwin",
        other => other,
    };
    seeds.push(format!(
        "zcode-credential-fallback:{platform}:{home}:{username}"
    ));
    let home_fwd = home.replace('\\', "/");
    if home_fwd != home {
        seeds.push(format!(
            "zcode-credential-fallback:{platform}:{home_fwd}:{username}"
        ));
    }
    seeds
}

fn try_decrypt(key: &[u8], iv: &[u8], tag: &[u8], ct: &[u8]) -> Option<String> {
    if iv.len() != NONCE_LEN {
        return None;
    }
    let cipher = Aes256Gcm::new_from_slice(key).ok()?;
    // aes-gcm 的 decrypt 收 `ciphertext || tag`
    let mut combined = Vec::with_capacity(ct.len() + tag.len());
    combined.extend_from_slice(ct);
    combined.extend_from_slice(tag);
    let plain = cipher
        .decrypt(Nonce::from_slice(iv), Payload::from(combined.as_slice()))
        .ok()?;
    String::from_utf8(plain).ok()
}

/// 用固定 seed 解密一个 `enc:v1:<iv>.<tag>.<ct>` 字段（测试用）。
fn decrypt_with_seed(seed: &str, encoded: &str) -> Option<String> {
    let rest = encoded.strip_prefix(ENC_PREFIX)?;
    let parts: Vec<&str> = rest.split('.').collect();
    if parts.len() != 3 || parts[0].is_empty() || parts[1].is_empty() || parts[2].is_empty() {
        return None;
    }
    let iv = B64URL.decode(parts[0]).ok()?;
    let tag = B64URL.decode(parts[1]).ok()?;
    let ct = B64URL.decode(parts[2]).ok()?;
    let key = Sha256::digest(seed.as_bytes());
    try_decrypt(&key, &iv, &tag, &ct)
}

/// 解密一个 `enc:v1:` 字段：逐个候选 seed 试，命中即返回。
fn decrypt_field(encoded: &str) -> Option<String> {
    for seed in candidate_seeds() {
        if let Some(plain) = decrypt_with_seed(&seed, encoded) {
            return Some(plain);
        }
    }
    None
}

fn read_credentials() -> Option<Value> {
    let path = credentials_path()?;
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

/// 取 Zcode 的 JWT（每次实时读，客户端会自己续期）。
fn read_jwt() -> Option<String> {
    let json = read_credentials()?;
    let obj = json.as_object()?;
    let mut keys: Vec<String> = Vec::new();
    if obj.contains_key(JWT_KEY) {
        keys.push(JWT_KEY.to_string());
    }
    for k in obj.keys() {
        let lk = k.to_lowercase();
        if lk.contains("zcode") && lk.contains("jwt") && k != JWT_KEY {
            keys.push(k.clone());
        }
    }
    for k in keys {
        let Some(s) = obj[&k].as_str() else { continue };
        if let Some(plain) = decrypt_field(s) {
            // JWT 是三段式，顺手校验一下，避免把别的字段误当 token
            if plain.split('.').count() == 3 {
                return Some(plain);
            }
        }
    }
    None
}

fn read_onboarding() -> Option<Value> {
    let path = onboarding_path()?;
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

/// 往 HeaderMap 里塞一个头；名字/值不合法就跳过（不 panic）。
fn set_header(map: &mut reqwest::header::HeaderMap, name: &str, value: &str) {
    if let (Ok(n), Ok(v)) = (
        reqwest::header::HeaderName::from_bytes(name.as_bytes()),
        reqwest::header::HeaderValue::from_str(value),
    ) {
        map.insert(n, v);
    }
}

fn read_device_mid() -> Option<String> {
    let json = read_onboarding()?;
    json["deviceMid"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// 账号 id：优先 `onboarding-record.json` 的 `decisions[].userId`，
/// 兜底解 JWT payload。
fn read_user_id() -> Option<String> {
    if let Some(json) = read_onboarding() {
        if let Some(id) = json["decisions"][0]["userId"].as_str() {
            if !id.is_empty() {
                return Some(id.to_string());
            }
        }
    }
    let jwt = read_jwt()?;
    let payload = jwt.split('.').nth(1)?;
    let bytes = B64URL.decode(payload).ok()?;
    let v: Value = serde_json::from_slice(&bytes).ok()?;
    for k in ["sub", "user_id", "userId", "uid"] {
        if let Some(s) = v[k].as_str() {
            if !s.is_empty() {
                return Some(s.to_string());
            }
        }
    }
    None
}

/// 账号昵称：解密 `oauth:bigmodel:user_info`。
fn read_user_name() -> Option<String> {
    let json = read_credentials()?;
    let obj = json.as_object()?;
    for (k, v) in obj {
        if !k.contains("user_info") {
            continue;
        }
        let Some(s) = v.as_str() else { continue };
        let Some(plain) = decrypt_field(s) else {
            continue;
        };
        let Ok(info) = serde_json::from_str::<Value>(&plain) else {
            continue;
        };
        // 实测 user_info 形如
        // {"id":..,"username":"Skyblueee","displayName":"Skyblueee","avatarUrl":..}
        for key in [
            "displayName",
            "nickname",
            "name",
            "username",
            "userName",
            "login",
            "email",
        ] {
            if let Some(n) = info[key].as_str() {
                if !n.is_empty() {
                    return Some(n.to_string());
                }
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// 展示用格式化
// ---------------------------------------------------------------------------

/// Token 数缩写：300000000 → `3.00亿`，12345 → `1.2万`。
fn fmt_units(n: f64) -> String {
    if n >= 100_000_000.0 {
        format!("{:.2}亿", n / 100_000_000.0)
    } else if n >= 10_000.0 {
        format!("{:.1}万", n / 10_000.0)
    } else {
        format!("{:.0}", n)
    }
}

fn fmt_ts(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|d| {
            d.with_timezone(&chrono::Local)
                .format("%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_default()
}

/// 秒 / 毫秒时间戳统一成毫秒。
fn to_ms(ts: i64) -> i64 {
    if ts > 1_000_000_000_000 {
        ts
    } else {
        ts * 1000
    }
}

// ---------------------------------------------------------------------------
// 响应解析（纯函数，便于测试）
// ---------------------------------------------------------------------------

/// 当前生效的套餐：`status == active` 且还没过期。
fn active_plan<'a>(data: &'a Value, now: i64) -> Option<&'a Value> {
    data["plans"].as_array()?.iter().find(|p| {
        p["status"].as_str() == Some("active") && p["ends_at"].as_i64().unwrap_or(0) > now
    })
}

fn plan_name(p: &Value) -> String {
    p["name"].as_str().unwrap_or("Zcode 套餐").to_string()
}

fn plan_model(p: &Value) -> String {
    p["entitlements"][0]["show_name"]
        .as_str()
        .unwrap_or("GLM-5.3-Flash")
        .to_string()
}

/// 取套餐首个 entitlement 的发放额度与生效时刻。
fn plan_grant(p: &Value) -> (i64, i64) {
    let e = &p["entitlements"][0];
    (
        e["grant_units"].as_i64().unwrap_or(0),
        e["effective_at"].as_i64().unwrap_or(0),
    )
}

/// 今日领取状态：有生效套餐 = 已领取，否则待领取。
fn parse_status(data: &Value, now: i64) -> CheckinStatus {
    if let Some(p) = active_plan(data, now) {
        CheckinStatus {
            state: ClaimState::Claimed,
            points: 0,
            streak_days: 0,
            message: format!("已领取 {} · {}", plan_name(p), plan_model(p)),
            day_index: None,
            bonus_points: 0,
        }
    } else {
        CheckinStatus {
            state: ClaimState::Claimable,
            points: 0,
            streak_days: 0,
            message: "当前无生效中的套餐，点「打开 Zcode 领取」去官方客户端领取".to_string(),
            day_index: None,
            bonus_points: 0,
        }
    }
}

/// 「现在能领」的套餐（来自 `billing/preview`）。
///
/// preview = 可领取，balance = 已生效 —— 两个接口分工不同，
/// 判断「今天要不要提醒」必须用 preview。
fn parse_claimable(data: &Value) -> Option<CheckinStatus> {
    let p = data["plans"].as_array()?.first()?;
    let (grant, _) = plan_grant(p);
    Some(CheckinStatus {
        state: ClaimState::Claimable,
        points: 0,
        streak_days: 0,
        message: format!(
            "可领取 {} · {} {}，点「打开 Zcode 领取」",
            plan_name(p),
            plan_model(p),
            fmt_units(grant as f64)
        ),
        day_index: None,
        bonus_points: 0,
    })
}

/// 余额：优先用 `balances[]` 的实时桶；没有桶但有生效套餐时展示发放额度。
fn parse_quota(data: &Value, now: i64) -> Quota {
    let buckets = data["balances"].as_array().cloned().unwrap_or_default();

    if let Some(p) = active_plan(data, now) {
        let (grant, effective) = plan_grant(p);
        if buckets.is_empty() {
            // 已领取但还没产生桶（没到生效时刻，或还没消耗）
            return Quota {
                total_granted: grant as f64,
                total_consumed: 0.0,
                total_remaining: grant as f64,
                items: vec![CreditItem {
                    credit_type: "plan".to_string(),
                    granted: grant as f64,
                    consumed: 0.0,
                    remaining: grant as f64,
                    granted_at_ms: 0,
                    expire_at_ms: to_ms(p["ends_at"].as_i64().unwrap_or(0)),
                    package_name: String::new(),
                    refresh_at_ms: 0,
                }],
                message: format!("已领取 {}，{} 生效", plan_name(p), fmt_ts(to_ms(effective))),
                refresh_at_ms: 0,
                credit_groups: Vec::new(),
            };
        }
    }

    if buckets.is_empty() {
        return Quota::empty("暂无生效中的 Zcode 套餐，可打开客户端领取每日额度");
    }

    let mut items = Vec::new();
    let mut granted_sum = 0.0;
    let mut used_sum = 0.0;
    let mut remaining_sum = 0.0;
    let mut label = String::new();
    for b in &buckets {
        let granted = b["total_units"].as_f64().unwrap_or(0.0);
        let used = b["used_units"].as_f64().unwrap_or(0.0);
        let remaining = b["remaining_units"]
            .as_f64()
            .or_else(|| b["available_units"].as_f64())
            .unwrap_or(0.0);
        if label.is_empty() {
            label = b["show_name"].as_str().unwrap_or("").to_string();
        }
        items.push(CreditItem {
            credit_type: "plan".to_string(),
            granted,
            consumed: used,
            remaining,
            granted_at_ms: to_ms(b["period_start"].as_i64().unwrap_or(0)),
            expire_at_ms: to_ms(
                b["period_end"]
                    .as_i64()
                    .or_else(|| b["expires_at"].as_i64())
                    .unwrap_or(0),
            ),
            package_name: String::new(),
            refresh_at_ms: 0,
        });
        granted_sum += granted;
        used_sum += used;
        remaining_sum += remaining;
    }

    Quota {
        total_granted: granted_sum,
        total_consumed: used_sum,
        total_remaining: remaining_sum,
        items,
        message: format!(
            "{} 剩余 {} / 共 {}",
            if label.is_empty() {
                "Zcode 额度"
            } else {
                &label
            },
            fmt_units(remaining_sum),
            fmt_units(granted_sum)
        ),
        refresh_at_ms: 0,
        credit_groups: Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// Provider
// ---------------------------------------------------------------------------

pub struct ZcodeProvider {
    http: reqwest::Client,
}

impl ZcodeProvider {
    pub fn new() -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .user_agent(format!("ZCode/{APP_VERSION}"))
            .build()
            .unwrap_or_default();
        Self { http }
    }

    /// 拉一次余额。只读接口，不需要验证码。
    async fn fetch_balance(&self) -> Result<Value> {
        let jwt = read_jwt()
            .ok_or_else(|| anyhow!("未找到 Zcode 登录态：请先在本机登录 Zcode 客户端"))?;
        let device_mid = read_device_mid().unwrap_or_default();
        let tz = crate::core::tz::iana_timezone();

        let mut headers = reqwest::header::HeaderMap::new();
        set_header(&mut headers, "content-type", "application/json");
        set_header(&mut headers, "accept", "application/json");
        set_header(&mut headers, "anthropic-version", "2023-06-01");
        set_header(&mut headers, "user-agent", &format!("ZCode/{APP_VERSION}"));
        set_header(&mut headers, "x-zcode-app-version", APP_VERSION);
        set_header(&mut headers, "x-release-channel", "stable");
        set_header(&mut headers, "x-client-language", "zh-CN");
        set_header(&mut headers, "x-client-timezone", &tz);
        set_header(&mut headers, "x-device-mid", &device_mid);
        set_header(&mut headers, "x-platform", "win32");
        set_header(&mut headers, "x-os-category", "windows");
        set_header(&mut headers, "http-referer", "https://zcode.z.ai/");
        set_header(&mut headers, "authorization", &format!("Bearer {jwt}"));

        let resp = self
            .http
            .get(format!("{BASE}{BALANCE_PATH}"))
            .headers(headers)
            .send()
            .await?;
        let status = resp.status();
        let body: Value = resp.json().await.unwrap_or(Value::Null);

        if !status.is_success() {
            let code = body["code"].as_i64().unwrap_or(0);
            if status.as_u16() == 401 || code == 4001 {
                return Err(anyhow!("Zcode 登录态已过期，请打开 Zcode 客户端重新登录"));
            }
            return Err(anyhow!(
                "Zcode 余额接口返回 HTTP {}：{}",
                status.as_u16(),
                body["msg"].as_str().unwrap_or("")
            ));
        }
        if body["code"].as_i64().unwrap_or(-1) != 0 {
            return Err(anyhow!(
                "Zcode 余额接口返回 {}：{}",
                body["code"],
                body["msg"].as_str().unwrap_or("")
            ));
        }
        Ok(body)
    }

    /// 拉一次「可领取」预览。
    ///
    /// 和 `fetch_balance` 的分工：**balance = 已生效的套餐**，
    /// **preview = 现在能领的套餐**（官方 `getManualClaimPlanPreviews`）。
    async fn fetch_preview(&self) -> Result<Value> {
        let jwt = read_jwt()
            .ok_or_else(|| anyhow!("未找到 Zcode 登录态：请先在本机登录 Zcode 客户端"))?;
        let mut headers = reqwest::header::HeaderMap::new();
        set_header(&mut headers, "accept", "application/json");
        set_header(&mut headers, "authorization", &format!("Bearer {jwt}"));
        set_header(&mut headers, "x-zcode-app-version", APP_VERSION);
        set_header(&mut headers, "x-platform", "win32");

        let url = format!(
            "{BASE}/api/v1/zcode-plan/billing/preview?app_version={APP_VERSION}&platform=win32"
        );
        let resp = self.http.get(url).headers(headers).send().await?;
        let status = resp.status();
        let body: Value = resp.json().await.unwrap_or(Value::Null);
        if !status.is_success() {
            let code = body["code"].as_i64().unwrap_or(0);
            if status.as_u16() == 401 || code == 4001 {
                return Err(anyhow!("Zcode 登录态已过期，请打开 Zcode 客户端重新登录"));
            }
            return Err(anyhow!(
                "Zcode 预览接口返回 HTTP {}：{}",
                status.as_u16(),
                body["msg"].as_str().unwrap_or("")
            ));
        }
        if body["code"].as_i64().unwrap_or(-1) != 0 {
            return Err(anyhow!(
                "Zcode 预览接口返回 {}：{}",
                body["code"],
                body["msg"].as_str().unwrap_or("")
            ));
        }
        Ok(body)
    }
}

#[async_trait]
impl Provider for ZcodeProvider {
    fn id(&self) -> &'static str {
        "zcode"
    }

    fn display_name(&self) -> &'static str {
        "Zcode"
    }

    fn accent(&self) -> &'static str {
        "#6E5BFF"
    }

    fn docs_url(&self) -> &'static str {
        "https://zcode.z.ai"
    }

    /// Zcode 的每日额度只能走官方客户端的阿里云验证码领取，本工具不代领。
    fn supports_claim(&self) -> bool {
        false
    }

    async fn discover(&self) -> Result<Vec<(Account, Value)>> {
        let Some(jwt) = read_jwt() else {
            return Ok(Vec::new());
        };
        let user_id = read_user_id().unwrap_or_else(|| "zcode".to_string());
        let name = read_user_name().unwrap_or_else(|| "Zcode 账号".to_string());
        let account = Account {
            id: user_id.clone(),
            provider: "zcode".to_string(),
            label: name,
            created_at: chrono::Utc::now().timestamp(),
            last_used: None,
            origin: "本机 Zcode 登录态".to_string(),
        };
        let cred = json!({ "jwt": jwt, "user_id": user_id });
        Ok(vec![(account, cred)])
    }

    async fn status(&self, _account: &Account, _cred: &Value) -> Result<CheckinStatus> {
        // 先看「现在能不能领」（preview）。官方客户端只查 balance，所以
        // 光看 balance 分不出「还没开放领取」和「已经领过了」。
        // preview 查询失败不致命：继续走 balance。
        if let Ok(body) = self.fetch_preview().await {
            if let Some(s) = parse_claimable(&body["data"]) {
                return Ok(s);
            }
        }
        // 没有可领取的：再分「已领取（有生效套餐）」和「现在没得领」
        let body = self.fetch_balance().await?;
        let now = chrono::Utc::now().timestamp();
        Ok(parse_status(&body["data"], now))
    }

    async fn claim(&self, _account: &Account, _cred: &Value) -> Result<ClaimOutcome> {
        // 不会被调用（supports_claim() == false），这里只是如实说明。
        Ok(ClaimOutcome::failed(
            "Zcode 需在官方客户端领取，本工具不提供自动领取",
        ))
    }

    async fn quota(&self, _account: &Account, _cred: &Value) -> Result<Quota> {
        let body = self.fetch_balance().await?;
        let now = chrono::Utc::now().timestamp();
        Ok(parse_quota(&body["data"], now))
    }

    async fn profile(&self, _account: &Account, _cred: &Value) -> Result<Option<AccountProfile>> {
        Ok(read_user_name().map(|name| AccountProfile {
            name: Some(name),
            ..Default::default()
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 本机日志里的真实 payload（zcode-v3-start-plan-1027），时间戳调成未来。
    fn sample_data() -> Value {
        serde_json::json!({
          "plans": [{
            "user_plan_id": "upl_demo",
            "plan_id": "zcode-v3-start-plan-1027",
            "name": "ZCODE GLOBAL BUILD",
            "status": "active",
            "starts_at": 1793100812,
            "ends_at": 4102444800i64,
            "entitlements": [{
              "show_name": "GLM-5.3-Flash",
              "grant_units": 300000000,
              "effective_at": 1793113200
            }]
          }],
          "balances": [{
            "show_name": "GLM-5.3-Flash",
            "total_units": 300000000,
            "used_units": 1109567,
            "remaining_units": 298890433,
            "period_start": 1793100812,
            "period_end": 4102444800i64,
            "expires_at": 4102444800i64
          }]
        })
    }

    #[test]
    fn 生效套餐判为已领取() {
        let s = parse_status(&sample_data(), 1793100900);
        assert_eq!(s.state, ClaimState::Claimed);
        assert!(s.message.contains("ZCODE GLOBAL BUILD"));
        assert!(s.message.contains("GLM-5.3-Flash"));
    }

    #[test]
    fn 套餐过期后判为待领取() {
        let s = parse_status(&sample_data(), 4102444900i64);
        assert_eq!(s.state, ClaimState::Claimable);
        assert!(s.message.contains("无生效中的套餐"));
    }

    #[test]
    fn 余额映射到quota() {
        let q = parse_quota(&sample_data(), 1793100900);
        assert_eq!(q.total_granted, 300_000_000.0);
        assert_eq!(q.total_consumed, 1_109_567.0);
        assert_eq!(q.total_remaining, 298_890_433.0);
        assert_eq!(q.items.len(), 1);
        assert_eq!(q.items[0].expire_at_ms, 4_102_444_800_000);
        assert!(q.message.contains("2.99亿"));
    }

    #[test]
    fn 无套餐时余额为空并给出说明() {
        let q = parse_quota(&serde_json::json!({"plans": [], "balances": []}), 0);
        assert_eq!(q.total_remaining, 0.0);
        assert!(q.message.contains("暂无生效中"));
    }

    #[test]
    fn 可领取预览解析为待领取() {
        let data = serde_json::json!({
          "plans": [{
            "plan_id": "zcode-v3-start-plan-1104",
            "name": "ZCODE GLOBAL BUILD",
            "entitlements": [{
              "show_name": "GLM-5.3-Flash",
              "grant_units": 300000000,
              "effective_at": 4102444800i64
            }]
          }]
        });
        let s = parse_claimable(&data).expect("应解析出可领取");
        assert_eq!(s.state, ClaimState::Claimable);
        assert!(s.message.contains("可领取"));
        assert!(s.message.contains("3.00亿"));
    }

    #[test]
    fn 无可领取预览返回none() {
        assert!(parse_claimable(&serde_json::json!({"plans": []})).is_none());
        assert!(parse_claimable(&serde_json::json!({})).is_none());
    }

    #[test]
    fn 已领取未生效时展示发放额度() {
        let data = serde_json::json!({
          "plans": [{
            "name": "ZCODE GLOBAL BUILD",
            "status": "active",
            "ends_at": 4102444800i64,
            "entitlements": [{
              "show_name": "GLM-5.3-Flash",
              "grant_units": 300000000,
              "effective_at": 4102444800i64
            }]
          }],
          "balances": []
        });
        let q = parse_quota(&data, 1793100900);
        assert_eq!(q.total_remaining, 300_000_000.0);
        assert!(q.message.contains("生效"));
    }

    #[test]
    fn 解密往返() {
        let seed = "test-seed";
        let key = Sha256::digest(seed.as_bytes());
        let plain = "hello zcode";
        let mut nonce = [0u8; NONCE_LEN];
        getrandom::getrandom(&mut nonce).unwrap();
        let cipher = Aes256Gcm::new_from_slice(&key).unwrap();
        let sealed = cipher
            .encrypt(Nonce::from_slice(&nonce), Payload::from(plain.as_bytes()))
            .unwrap();
        let (ct, tag) = sealed.split_at(sealed.len() - 16);
        let encoded = format!(
            "{ENC_PREFIX}{}.{}.{}",
            B64URL.encode(nonce),
            B64URL.encode(tag),
            B64URL.encode(ct)
        );
        assert_eq!(decrypt_with_seed(seed, &encoded).as_deref(), Some(plain));
        // 错 seed 解不开
        assert!(decrypt_with_seed("wrong-seed", &encoded).is_none());
    }

    #[test]
    fn 非enc前缀直接返回none() {
        assert!(decrypt_with_seed("s", "not-encrypted").is_none());
    }

    #[test]
    fn 种子fallback形态正确() {
        let seeds = candidate_seeds();
        assert!(!seeds.is_empty());
        let fallback = seeds
            .iter()
            .find(|s| s.starts_with("zcode-credential-fallback:"))
            .expect("应有 fallback seed");
        let rest = fallback.trim_start_matches("zcode-credential-fallback:");
        // 剩下是 <platform>:<home>:<username>。Windows 的 home 自带盘符冒号
        // （C:\Users\...），所以只校验「非空 + 至少两段」，不数死冒号个数。
        assert!(!rest.is_empty());
        assert!(
            rest.matches(':').count() >= 2,
            "应为 platform/home/username 三段：{fallback}"
        );
    }

    #[test]
    fn 数字缩写() {
        assert_eq!(fmt_units(300_000_000.0), "3.00亿");
        assert_eq!(fmt_units(298_890_433.0), "2.99亿");
        assert_eq!(fmt_units(12_345.0), "1.2万");
        assert_eq!(fmt_units(999.0), "999");
    }

    /// 诊断用：确认本机的 `credentials.json` 真能解出字段（即 seed 猜对了）。
    ///
    /// 默认不跑（依赖本机 Zcode 登录态）。手动验证：
    /// `cargo test -- --ignored 真实凭据能解出`
    #[test]
    #[ignore = "依赖本机 Zcode 登录态，手动跑：cargo test -- --ignored 真实凭据能解出"]
    fn 真实凭据能解出() {
        let Some(json) = read_credentials() else {
            eprintln!("跳过：本机没有 {}", CREDENTIALS_REL);
            return;
        };
        let Some(obj) = json.as_object() else {
            panic!("credentials.json 不是对象");
        };
        let mut ok = 0;
        for (k, v) in obj {
            let Some(s) = v.as_str() else { continue };
            if !s.starts_with(ENC_PREFIX) {
                continue;
            }
            match decrypt_field(s) {
                Some(plain) => {
                    // 只打印长度和头尾，避免把 token 明文写进测试输出
                    let head = plain.chars().take(12).collect::<String>();
                    eprintln!("  {k}: 解密成功（{} 字节，开头 {head}…）", plain.len());
                    ok += 1;
                }
                None => eprintln!("  {k}: 解密失败（seed 不匹配？）"),
            }
        }
        assert!(ok > 0, "一个字段都没解出来：seed 可能不对");
    }

    /// 诊断用：打一次真实的余额接口，确认 headers / 解析都对。
    ///
    /// 默认不跑。手动验证：
    /// `cargo test -- --ignored 真实余额接口能取到 --nocapture`
    #[tokio::test]
    #[ignore = "依赖本机网络 + Zcode 登录态"]
    async fn 真实余额接口能取到() {
        let body = match ZcodeProvider::new().fetch_balance().await {
            Ok(v) => v,
            Err(e) => panic!("余额接口失败：{e}"),
        };
        let data = &body["data"];
        eprintln!(
            "plans {} 个 / balances {} 个",
            data["plans"].as_array().map(|a| a.len()).unwrap_or(0),
            data["balances"].as_array().map(|a| a.len()).unwrap_or(0)
        );
        let now = chrono::Utc::now().timestamp();
        let status = parse_status(data, now);
        let quota = parse_quota(data, now);
        eprintln!("status: {:?} · {}", status.state, status.message);
        eprintln!(
            "quota: 剩余 {} / 已用 {} / 共 {} · {}",
            quota.total_remaining, quota.total_consumed, quota.total_granted, quota.message
        );
    }

    /// 诊断用：打一次「可领取」预览，看现在有没有能领的套餐、生效时刻是几点。
    ///
    /// `cargo test -- --ignored 真实预览接口能取到 --nocapture`
    #[tokio::test]
    #[ignore = "依赖本机网络 + Zcode 登录态"]
    async fn 真实预览接口能取到() {
        let body = match ZcodeProvider::new().fetch_preview().await {
            Ok(v) => v,
            Err(e) => panic!("预览接口失败：{e}"),
        };
        let data = &body["data"];
        let plans = data["plans"].as_array().cloned().unwrap_or_default();
        eprintln!("可领取套餐 {} 个", plans.len());
        for p in &plans {
            let grant = p["entitlements"][0]["grant_units"].as_i64().unwrap_or(0);
            let eff = p["entitlements"][0]["effective_at"].as_i64().unwrap_or(0);
            eprintln!(
                "  {} · {} · {} Token · {}",
                p["plan_id"].as_str().unwrap_or("?"),
                p["name"].as_str().unwrap_or("?"),
                grant,
                fmt_ts(to_ms(eff))
            );
        }
    }
}
