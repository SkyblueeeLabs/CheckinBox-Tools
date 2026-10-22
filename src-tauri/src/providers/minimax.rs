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

use crate::core::provider::{Account, CheckinStatus, ClaimOutcome, ClaimState, Provider};
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

/// 找本机 MiniMax Code 的登录态文件。
///
/// 结构是 `~/.minimax/auth/<region>/<clientId>/auth.json`，这里扫两层目录。
fn auth_files() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Some(home) = dirs::home_dir() else {
        return out;
    };
    let root = home.join(".minimax").join("auth");
    let Ok(regions) = std::fs::read_dir(&root) else {
        return out;
    };
    for region in regions.flatten() {
        if !region.path().is_dir() {
            continue;
        }
        let Ok(clients) = std::fs::read_dir(region.path()) else {
            continue;
        };
        for client in clients.flatten() {
            if !client.path().is_dir() {
                continue;
            }
            let candidate = client.path().join("auth.json");
            if candidate.is_file() {
                out.push(candidate);
            }
        }
    }
    out
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
        let token = cred["access_token"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow!("缺少 access token"))?;

        let tz = urlenc(&tz::iana_timezone());
        let url = format!("{BASE}{STATUS_PATH}?timezone_id={tz}");

        let body: Value = self
            .http
            .get(url)
            .bearer_auth(token)
            .header("accept", "application/json")
            .header("accept-language", "zh-CN,zh;q=0.9")
            .send()
            .await?
            .json()
            .await?;

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
        let token = cred["access_token"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow!("缺少 access token"))?;

        let tz = urlenc(&tz::iana_timezone());
        let url = format!("{BASE}{CLAIM_PATH}?timezone_id={tz}");

        let body: Value = self
            .http
            .post(url)
            .bearer_auth(token)
            .header("accept", "application/json")
            .header("content-type", "application/json")
            .json(&json!({}))
            .send()
            .await?
            .json()
            .await?;

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
}
