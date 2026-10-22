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
//!   * 活动窗口每天 10:00（UTC+8）开启；没到点时如实提示，不谎报失败。
//!   * 已领取的判定：HTTP 409 / `result == ALREADY_CLAIMED` / `replayed == true`。

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64URL;
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

use crate::core::provider::{
    Account, CheckinStatus, ClaimOutcome, ClaimState, Provider, Quota,
};

const WEB_BASE: &str = "https://qoder.com.cn";
const OPENAPI_BASE: &str = "https://openapi.qoder.com.cn";
const CAMPAIGNS_PATH: &str = "/sash/api/v1/me/campaigns";
const DEVICE_POLL_PATH: &str = "/api/v1/deviceToken/poll";
const DEVICE_LOGIN_PATH: &str = "/device/selectAccounts";
const SASH_CLIENT_TYPE: &str = "10";
const SASH_USER_AGENT: &str = "Qoder";

/// 设备授权会话。`verifier` / `nonce` 只留在后端，不下发给前端。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthSession {
    pub id: String,
    /// 让用户在浏览器里打开的授权地址
    pub auth_url: String,
    pub verifier: String,
    pub nonce: String,
    pub machine_id: String,
    pub created_at: i64,
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
    pub fn begin_authorization(&self) -> Result<AuthSession> {
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
            auth_url,
            verifier,
            nonce,
            machine_id,
            created_at: chrono::Utc::now().timestamp(),
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

        let data = body
            .get("data")
            .and_then(|d| d.as_object())
            .cloned()
            .unwrap_or_default();
        let token = data
            .get("token")
            .or_else(|| data.get("access_token"))
            .or_else(|| data.get("device_token"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if token.is_empty() {
            return Ok(None);
        }

        let refresh = data
            .get("refresh_token")
            .or_else(|| data.get("refreshToken"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let user_id = data
            .get("user_id")
            .or_else(|| data.get("userId"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let expires_at = data
            .get("expires_at")
            .or_else(|| data.get("expiresAt"))
            .and_then(|v| v.as_i64())
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
            "user_id": user_id,
            "expires_at": expires_at,
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
            SASH_CLIENT_TYPE.parse::<reqwest::header::HeaderValue>().unwrap(),
        );
        headers.insert(
            "accept",
            "application/json".parse::<reqwest::header::HeaderValue>().unwrap(),
        );
        headers.insert(
            "accept-language",
            "zh-CN".parse::<reqwest::header::HeaderValue>().unwrap(),
        );
        headers.insert(
            "user-agent",
            SASH_USER_AGENT.parse::<reqwest::header::HeaderValue>().unwrap(),
        );
        headers.insert(
            "Cosy-Version",
            client_version().parse::<reqwest::header::HeaderValue>().unwrap(),
        );
        headers.insert(
            "Cosy-MachineOS",
            machine_os().parse::<reqwest::header::HeaderValue>().unwrap(),
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
    async fn fetch_campaigns(
        &self,
        cred: &Value,
    ) -> Result<(reqwest::StatusCode, Value)> {
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

        let rows = body
            .get("campaigns")
            .and_then(|v| v.as_array())
            .or_else(|| {
                body.get("data")
                    .and_then(|d| d.get("campaigns"))
                    .and_then(|v| v.as_array())
            })
            .cloned()
            .unwrap_or_default();

        let mut target: Option<Value> = None;
        let mut claimed = false;
        for row in rows.iter() {
            if row.get("actionType").and_then(|v| v.as_str()) != Some("CLAIM_BENEFIT") {
                continue;
            }
            match row.get("claimStatus").and_then(|v| v.as_str()) {
                Some("CLAIMABLE") => {
                    if target.is_none() {
                        target = Some(row.clone());
                    }
                }
                Some("CLAIMED") => claimed = true,
                _ => {}
            }
        }

        if let Some(row) = target {
            let points = row
                .get("benefit")
                .and_then(|b| b.get("amount"))
                .and_then(|v| v.as_i64())
                .unwrap_or(100);
            return Ok(CheckinStatus {
                state: ClaimState::Claimable,
                points,
                streak_days: 0,
                message: format!("可签到 +{points} Credits"),
                day_index: None,
                bonus_points: 0,
            });
        }

        if claimed {
            return Ok(CheckinStatus {
                state: ClaimState::Claimed,
                points: 0,
                streak_days: 0,
                message: "今日已领取".to_string(),
                day_index: None,
                bonus_points: 0,
            });
        }

        Ok(CheckinStatus {
            state: ClaimState::NotAvailable,
            points: 0,
            streak_days: 0,
            message: "上游暂未下发每日活动（官方活动窗口每天 10:00 UTC+8 开启；今天若已在客户端领过，活动行会消失）".to_string(),
            day_index: None,
            bonus_points: 0,
        })
    }

    async fn claim(&self, _account: &Account, cred: &Value) -> Result<ClaimOutcome> {
        let (status, body) = self.fetch_campaigns(cred).await?;
        if !status.is_success() {
            return Err(anyhow!("活动列表返回 HTTP {}", status.as_u16()));
        }

        let rows = body
            .get("campaigns")
            .and_then(|v| v.as_array())
            .or_else(|| {
                body.get("data")
                    .and_then(|d| d.get("campaigns"))
                    .and_then(|v| v.as_array())
            })
            .cloned()
            .unwrap_or_default();

        let target = rows.iter().find(|row| {
            row.get("actionType").and_then(|v| v.as_str()) == Some("CLAIM_BENEFIT")
                && row.get("claimStatus").and_then(|v| v.as_str()) == Some("CLAIMABLE")
        });
        let Some(target) = target else {
            let already = rows.iter().any(|row| {
                row.get("actionType").and_then(|v| v.as_str()) == Some("CLAIM_BENEFIT")
                    && row.get("claimStatus").and_then(|v| v.as_str()) == Some("CLAIMED")
            });
            return if already {
                Ok(ClaimOutcome::already(0, "今日已领取"))
            } else {
                Ok(ClaimOutcome::failed("当前没有可领取的签到活动"))
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
            "application/json".parse::<reqwest::header::HeaderValue>().unwrap(),
        );
        headers.insert(
            "origin",
            OPENAPI_BASE.parse::<reqwest::header::HeaderValue>().unwrap(),
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
            return Ok(ClaimOutcome::already(0, "今日已领取"));
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
            let s = body.get("status").and_then(|v| v.as_str()).unwrap_or("未知");
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
        // 响应结构未在文档中公布，这里做防御式解析：能认出多少算多少，
        // 认不出就如实说明，不猜数。
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

        let num = |v: &Value| -> Option<f64> {
            v.as_f64()
                .or_else(|| v.as_i64().map(|i| i as f64))
                .or_else(|| {
                    v.as_str()
                        .and_then(|s| s.replace(',', "").trim().parse::<f64>().ok())
                })
        };

        // 常见字段名（不同版本可能不同）
        let candidates: [(&str, &str, &str); 3] = [
            ("total_credits", "used_credits", "remaining_credits"),
            ("total", "used", "remaining"),
            ("granted_amount", "consumed_amount", "remaining_amount"),
        ];

        for (tg, uc, rm) in candidates {
            if let (Some(t), Some(u), Some(r)) = (
                body.get(tg).and_then(num),
                body.get(uc).and_then(num),
                body.get(rm).and_then(num),
            ) {
                return Ok(Quota {
                    total_granted: t,
                    total_consumed: u,
                    total_remaining: r,
                    items: Vec::new(),
                    message: format!("剩余 {r:.0} / 共 {t:.0} Credits"),
                });
            }
        }

        // 也可能挂在 data / summary / resource_packages 之类的子对象里
        for key in ["data", "summary", "usage", "result"] {
            if let Some(sub) = body.get(key) {
                if let Some(sub) = sub.as_object() {
                    for (tg, uc, rm) in candidates {
                        if let (Some(t), Some(u), Some(r)) = (
                            sub.get(tg).and_then(num),
                            sub.get(uc).and_then(num),
                            sub.get(rm).and_then(num),
                        ) {
                            return Ok(Quota {
                                total_granted: t,
                                total_consumed: u,
                                total_remaining: r,
                                items: Vec::new(),
                                message: format!("剩余 {r:.0} / 共 {t:.0} Credits"),
                            });
                        }
                    }
                }
            }
        }

        Ok(Quota::empty(
            "积分接口返回了数据，但字段名与预期不符（上游结构可能已变更）",
        ))
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
            return Some(UmidIdentity { token, code, r#type });
        }
    }
    None
}

/// 本机 machine id。
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
fn client_version() -> String {
    // Qoder 的安装布局是 `<root>/.qoder-versions/<version>/`，取其中语义化最高的。
    let mut candidates: Vec<(u64, u64, u64, String)> = Vec::new();
    let roots = [
        PathBuf::from(r"D:\Programs\Qoder CN"),
        PathBuf::from(r"C:\Program Files\Qoder"),
        PathBuf::from(r"C:\Program Files\Qoder CN"),
    ];
    if let Some(local) = dirs::data_local_dir() {
        candidates_root(&local.join("Qoder CN"), &mut candidates);
    }
    for root in roots {
        candidates_root(&root, &mut candidates);
    }
    if let Some(appdata) = std::env::var("APPDATA").ok() {
        candidates_root(&PathBuf::from(appdata).join("Qoder CN"), &mut candidates);
    }

    candidates
        .into_iter()
        .max()
        .map(|(_, _, _, v)| v)
        .unwrap_or_else(|| "0.4.9".to_string())
}

fn candidates_root(root: &std::path::Path, out: &mut Vec<(u64, u64, u64, String)>) {
    let versions_dir = root.join(".qoder-versions");
    let Ok(entries) = std::fs::read_dir(&versions_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let mut parts = name.split('.');
        let a = parts.next().and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
        let b = parts.next().and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
        let c = parts.next().and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
        out.push((a, b, c, name));
    }
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
