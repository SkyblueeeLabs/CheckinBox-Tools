//! 千问办公（QwenWorkCN，阿里系）适配器。
//!
//! 与 Qoder CN 同属阿里「灵码」家族，**共用 sash 网关与设备授权流程**：
//!
//!   设备授权（和 Qoder 完全同一套 URL）：
//!     GET https://qoder.com.cn/device/selectAccounts?challenge=..&challenge_method=S256
//!         &machine_id=..&nonce=..&client_id=..
//!     轮询 GET https://openapi.qoder.com.cn/api/v1/deviceToken/poll?nonce=&verifier=&challenge_method=S256
//!
//!   每日签到（比 Qoder 的「活动领取制」更规整，是固定接口）：
//!     GET  https://openapi.qoder.com.cn/sash/api/v1/me/daily-check-in/status
//!     POST https://openapi.qoder.com.cn/sash/api/v1/me/daily-check-in/claim
//!
//!   积分：
//!     GET  https://openapi.qoder.com.cn/sash/api/v2/me/usage
//!
//! 签到状态枚举：`CLAIMABLE` / `CLAIMED_TODAY` / `DISABLED` / `NOT_STARTED` / `ENDED`
//! 领取结果枚举：`CLAIMED` / `ALREADY_CLAIMED`
//!
//! 注意：千问办公与 Qoder 是**两个独立账号**，需要各自做一次设备授权。

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::core::provider::{Account, CheckinStatus, ClaimOutcome, ClaimState, CreditItem, Provider, Quota};

const OPENAPI_BASE: &str = "https://openapi.qoder.com.cn";
const STATUS_PATH: &str = "/sash/api/v1/me/daily-check-in/status";
const CLAIM_PATH: &str = "/sash/api/v1/me/daily-check-in/claim";
const USAGE_PATH: &str = "/sash/api/v2/me/usage";
const USER_AGENT: &str = "QoderWork";

pub struct QwenWorkProvider {
    http: reqwest::Client,
}

impl QwenWorkProvider {
    pub fn new() -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .user_agent(USER_AGENT)
            .build()
            .unwrap_or_default();
        Self { http }
    }

    /// SASH 请求头：Bearer + 设备风控三件套 + 千问客户端标识。
    fn sash_headers(&self, cred: &Value) -> reqwest::header::HeaderMap {
        let mut headers = reqwest::header::HeaderMap::new();
        if let Some(token) = cred["access_token"].as_str() {
            if let Ok(v) = format!("Bearer {token}").parse() {
                headers.insert(reqwest::header::AUTHORIZATION, v);
            }
        }
        headers.insert("accept", "application/json".parse().unwrap());
        headers.insert("content-type", "application/json".parse().unwrap());
        headers.insert("user-agent", USER_AGENT.parse().unwrap());
        headers.insert(
            "cosy-clienttype",
            "10".parse::<reqwest::header::HeaderValue>().unwrap(),
        );
        headers.insert("Cosy-Version", "1.2.5".parse().unwrap());
        headers.insert("Cosy-MachineOS", "x86_64_win32".parse().unwrap());
        headers.insert(
            "Cosy-MachineId",
            cred["machine_id"]
                .as_str()
                .unwrap_or("")
                .parse::<reqwest::header::HeaderValue>()
                .unwrap(),
        );
        // 千问客户端标识
        headers.insert("X-QwenWork-Version", "1.2.5".parse().unwrap());
        headers.insert("X-QwenWork-Platform", "win32".parse().unwrap());
        headers.insert("X-QwenWork-Arch", "x64".parse().unwrap());
        headers.insert("X-QwenWork-Channel", "stable".parse().unwrap());
        // 设备风控（缺了活动可能不下发）
        if let Some(risk) = crate::providers::qoder::umid_identity() {
            headers.insert("Cosy-MachineToken", risk.token.parse().unwrap());
            headers.insert("Cosy-MachineCode", risk.code.parse().unwrap());
            headers.insert("Cosy-MachineType", risk.r#type.parse().unwrap());
        }
        headers
    }
}

#[async_trait]
impl Provider for QwenWorkProvider {
    fn id(&self) -> &'static str {
        "qwenwork"
    }

    fn display_name(&self) -> &'static str {
        "千问办公"
    }

    fn accent(&self) -> &'static str {
        "#0ea5e9"
    }

    fn docs_url(&self) -> &'static str {
        "https://docs.qoder.cn/"
    }

    async fn status(&self, _account: &Account, cred: &Value) -> Result<CheckinStatus> {
        let url = format!("{OPENAPI_BASE}{STATUS_PATH}");
        let body: Value = self
            .http
            .get(url)
            .headers(self.sash_headers(cred))
            .send()
            .await?
            .json()
            .await?;

        // 顶层即数据（和 Qoder 的 deviceToken/poll 一样没有 data 包装）
        let data = if body.get("data").is_some() {
            &body["data"]
        } else {
            &body
        };

        let status = data["status"].as_str().unwrap_or("");
        let points = data["rewardCredits"].as_i64().unwrap_or(0);
        let streak = data["currentStreakDays"].as_i64().unwrap_or(0);

        let (state, message) = match status {
            "CLAIMABLE" => (
                ClaimState::Claimable,
                format!("可签到 +{points} Credits · 连续 {streak} 天"),
            ),
            "CLAIMED_TODAY" => (
                ClaimState::Claimed,
                format!("今日已签 · 连续 {streak} 天"),
            ),
            "NOT_STARTED" => (ClaimState::NotAvailable, "活动未开始".to_string()),
            "ENDED" => (ClaimState::NotAvailable, "活动已结束".to_string()),
            _ => (
                ClaimState::NotAvailable,
                format!("当前不可签到（{status}）"),
            ),
        };

        Ok(CheckinStatus {
            state,
            points,
            streak_days: streak as i32,
            message,
            day_index: Some(streak as i32),
            bonus_points: 0,
        })
    }

    async fn claim(&self, _account: &Account, cred: &Value) -> Result<ClaimOutcome> {
        let url = format!("{OPENAPI_BASE}{CLAIM_PATH}");
        let resp = self
            .http
            .post(url)
            .headers(self.sash_headers(cred))
            .json(&json!({}))
            .send()
            .await?;

        let status = resp.status();
        let body: Value = resp.json().await.unwrap_or(Value::Null);

        // 409 / ALREADY_CLAIMED 都算「今日已领」
        let already = status.as_u16() == 409
            || body["dailyCheckInClaim"]["result"].as_str() == Some("ALREADY_CLAIMED")
            || body["errorCode"].as_str() == Some("AlreadyExists");
        if already {
            return Ok(ClaimOutcome::already(0, "今日已领取"));
        }
        if !status.is_success() {
            let msg = body["errorMessage"]
                .as_str()
                .or_else(|| body["message"].as_str())
                .unwrap_or("");
            return Err(anyhow!("领取失败 HTTP {} {msg}", status.as_u16()));
        }

        let result = body["dailyCheckInClaim"]["result"].as_str().unwrap_or("");
        let points = body["dailyCheckInClaim"]["rewardCredits"]
            .as_i64()
            .unwrap_or(0);
        if result == "CLAIMED" {
            Ok(ClaimOutcome::claimed(points, format!("签到成功 +{points} Credits")))
        } else {
            Ok(ClaimOutcome::failed(format!("上游返回 {result}")))
        }
    }

    async fn quota(&self, _account: &Account, cred: &Value) -> Result<Quota> {
        let url = format!("{OPENAPI_BASE}{USAGE_PATH}");
        let body: Value = self
            .http
            .get(url)
            .headers(self.sash_headers(cred))
            .send()
            .await?
            .json()
            .await?;

        let usage = &body["qoderUsage"];
        if usage.is_null() {
            return Ok(Quota::empty("积分接口返回了数据，但结构不是预期的 qoderUsage"));
        }
        let user_q = &usage["userQuota"];
        let addon_q = &usage["addOnQuota"];
        if user_q.is_null() {
            return Ok(Quota::empty("积分接口返回了数据，但结构不是预期的 qoderUsage.userQuota"));
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

        let mut items = Vec::new();
        let mut granted = 0.0;
        let mut consumed = 0.0;
        let mut remaining = 0.0;

        for (label, q) in [("plan", user_q), ("addon", addon_q)] {
            if q.is_null() {
                continue;
            }
            let g = num(&q["total"]);
            let u = num(&q["used"]);
            let r = num(&q["remaining"]);
            granted += g;
            consumed += u;
            remaining += r;
            items.push(CreditItem {
                credit_type: label.to_string(),
                granted: g,
                consumed: u,
                remaining: r,
                granted_at_ms: 0,
                expire_at_ms: usage["expiresAt"].as_i64().unwrap_or(0),
            });
        }

        Ok(Quota {
            total_granted: granted,
            total_consumed: consumed,
            total_remaining: remaining,
            items,
            message: format!("剩余 {remaining:.0} / 共 {granted:.0} Credits"),
        })
    }
}
