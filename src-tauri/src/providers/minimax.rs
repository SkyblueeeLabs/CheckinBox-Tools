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

use crate::core::provider::{
    Account, CheckinStatus, ClaimOutcome, ClaimState, CreditItem, Provider, Quota,
};
use crate::core::tz;

const BASE: &str = "https://agent.minimax.cn";
const STATUS_PATH: &str = "/minimax-cloud/api/v1/signin/status";
const CLAIM_PATH: &str = "/minimax-cloud/api/v1/signin/claim";
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

fn local_label() -> String {
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

impl MiniMaxProvider {
    /// 带一次重试的 GET。
    ///
    /// MiniMax 的 token 约 1 小时过期，客户端续期有几分钟空窗；
    /// 空窗内请求会失败，等 2 秒重读本机登录态再试一次就能恢复。
    async fn get_json(&self, url: &str, cred: &Value) -> Result<Value> {
        self.request_with_retry(url, cred, false).await
    }

    /// 带重试的 POST（领取用）。
    async fn post_json(&self, url: &str, cred: &Value) -> Result<Value> {
        self.request_with_retry(url, cred, true).await
    }

    /// 统一的请求 + 重试。
    ///
    /// MiniMax 的 token 约 1 小时过期，客户端会在到期前后续期，中间有几十秒空窗。
    /// 空窗内请求会拿到 401，所以 **401/403 时多重试几次**（间隔递增），
    /// 其他错误只重试一次。
    async fn request_with_retry(&self, url: &str, cred: &Value, is_post: bool) -> Result<Value> {
        // 鉴权类错误的退避：2s / 5s / 10s；其他错误只试两次
        let backoff = [2000u64, 5000, 10000];
        let mut last_err = anyhow!("请求失败");
        let mut attempt = 0usize;

        loop {
            let token = current_token(cred)?;
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

        Err(last_err)
    }
}

/// 取当前可用的 access token。
///
/// MiniMax 的 token 生命周期很短（实测约 1 小时），而本机客户端会自动续期
/// 并把新 token 写回 `auth.json`。所以每次请求前都去读一遍本机登录态，
/// 读不到再回退到账号池里存的 token。
fn current_token(cred: &Value) -> Result<String> {
    if let Some(path) = cred.get("auth_file").and_then(|v| v.as_str()) {
        if let Ok(raw) = std::fs::read_to_string(path) {
            if let Ok(parsed) = serde_json::from_str::<Value>(&raw) {
                if let Some(records) = parsed["records"].as_object() {
                    for rec in records.values() {
                        if let Some(t) = rec["accessToken"].as_str() {
                            if !t.is_empty() {
                                return Ok(t.to_string());
                            }
                        }
                    }
                }
            }
        }
    }
    cred["access_token"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("缺少 access token"))
}

/// 从记录键里取出稳定的账号 id。
///
/// 键形如 `com.minimax.mcode.oauth.prod.cn\u{0}<hash>`，用后半段做 id。
fn account_id_from(key: &str) -> String {
    key.rsplit('\u{0}').next().unwrap_or(key).to_string()
}

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
        "#f97316"
    }

    fn docs_url(&self) -> &'static str {
        "https://agent.minimax.io/docs/changelog"
    }

    async fn discover(&self) -> Result<Vec<(Account, Value)>> {
        let now = chrono::Utc::now().timestamp();
        let mut out = Vec::new();

        for path in auth_files() {
            let Ok(raw) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Ok(parsed) = serde_json::from_str::<Value>(&raw) else {
                continue;
            };
            let Some(records) = parsed["records"].as_object() else {
                continue;
            };
            for (key, rec) in records {
                let Some(access) = rec["accessToken"].as_str() else {
                    continue;
                };
                if access.is_empty() {
                    continue;
                }
                let label = local_label();
                let account = Account {
                    id: account_id_from(key),
                    provider: "minimax".to_string(),
                    label: label.clone(),
                    created_at: now,
                    last_used: None,
                    origin: "本机 MiniMax Code 登录态".to_string(),
                };
                let cred = json!({
                    "access_token": access,
                    "refresh_token": rec["refreshToken"].as_str().unwrap_or(""),
                    "expires_at_ms": rec["expiresAtMs"].as_i64().unwrap_or(0),
                    "login_epoch": rec["loginEpoch"].as_str().unwrap_or(""),
                    "auth_file": path.to_string_lossy().to_string(),
                });
                out.push((account, cred));
            }
        }
        Ok(out)
    }

    async fn status(&self, _account: &Account, cred: &Value) -> Result<CheckinStatus> {
        let tz = urlenc(&tz::iana_timezone());
        let url = format!("{BASE}{STATUS_PATH}?timezone_id={tz}");

        let body: Value = self.get_json(&url, cred).await?;

        check_base_resp(&body)?;

        let empty = Vec::new();
        let days = body["data"]["days"].as_array().unwrap_or(&empty);
        let Some(today) = days.iter().find(|d| d["is_today"].as_bool().unwrap_or(false)) else {
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

    async fn claim(&self, _account: &Account, cred: &Value) -> Result<ClaimOutcome> {
        let tz = urlenc(&tz::iana_timezone());
        let url = format!("{BASE}{CLAIM_PATH}?timezone_id={tz}");

        let body: Value = self.post_json(&url, cred).await?;

        check_base_resp(&body)?;

        let data = &body["data"];
        let result = data["claim_result"].as_i64().unwrap_or(0);
        let points = data["points"].as_i64().unwrap_or(0);

        match result {
            1 => Ok(ClaimOutcome::claimed(points, format!("签到成功 +{points} 积分"))),
            2 => Ok(ClaimOutcome::already(points, "今日已签到")),
            _ => Ok(ClaimOutcome::failed(
                data["claim_id"].as_str().unwrap_or("上游未返回领取结果"),
            )),
        }
    }

    async fn quota(&self, _account: &Account, cred: &Value) -> Result<Quota> {
        let tz = urlenc(&tz::iana_timezone());
        let url = format!("{BASE}/minimax-cloud/api/v1/credit/details?timezone_id={tz}");

        let body: Value = self.get_json(&url, cred).await?;

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
            if n == 0.0 { 0.0 } else { n }
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
        })
    }
}
