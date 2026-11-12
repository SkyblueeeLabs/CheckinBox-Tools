//! 妙手（美团 CatPaw）适配器。
//!
//! 产品标识：`catpaw-moon`，数据目录 `~/.meituan-catpaw`，CLI 名 `paw`。
//!
//! 协议（已实测）：
//!   * 登录态：`~/.meituan-catpaw/auth.json` —— **明文 JSON**，含
//!     `auth.accessToken` / `auth.tokenType` / `account.uid` / `account.name`；
//!   * 积分余额：`GET https://catx.nocode.cn/api/gateway/credit/balance`
//!   * 注册奖励：`POST https://catx.nocode.cn/api/gateway/credit/register`
//!
//! **鉴权方式**：请求头同时带 `X-Auth-Token` 和 `access-token`（两者同值），
//! 少了任意一个都会返回 `4011 Authentication failed`。
//!
//! **重要**：妙手的积分中心**没有每日签到**。`credit/register` 是「向积分中心
//! 注册用户并发放注册奖励」，只对新用户生效（`newUser=false` 时返回
//! 「用户已注册」）。积分来自套餐（体验版 200 Credits 等）。所以本适配器
//! 只做「账号接入 + 积分展示」，不做每日领取。

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::core::provider::{
    Account, CheckinStatus, ClaimOutcome, ClaimState, CreditItem, Provider, Quota,
};

const BASE: &str = "https://catx.nocode.cn";
const BALANCE_PATH: &str = "/api/gateway/credit/balance";
const REGISTER_PATH: &str = "/api/gateway/credit/register";
const SUBSCRIPTION_CURRENT_PATH: &str = "/api/gateway/credit/subscription/current";
/// 客户端标识，和服务端 `registerChannel` 一致
const REGISTER_CHANNEL: &str = "CATPAW_PC";

pub struct MiaoshouProvider {
    http: reqwest::Client,
}

impl MiaoshouProvider {
    pub fn new() -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .user_agent("CatPaw")
            .build()
            .unwrap_or_default();
        Self { http }
    }

    /// 读一次登录态（每次请求前重读，客户端会自己续期）。
    fn local_auth() -> Option<Value> {
        let home = dirs::home_dir()?;
        let path = home.join(".meituan-catpaw").join("auth.json");
        let raw = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&raw).ok()
    }

    /// 取当前可用的 access token。
    ///
    /// 优先读本机客户端的实时登录态（客户端会自动续期）。但客户端是**单账号**的：
    /// 只有文件里的 uid 就是这个账号时才能用文件里的 token —— 否则会拿别人的
    /// token 发请求，把别的账号的积分当成自己的展示（数据串号）。
    fn current_token(account: &Account, cred: &Value) -> Result<String> {
        if let Some(auth) = Self::local_auth() {
            let uid = auth["account"]["uid"].as_str().unwrap_or("");
            if !uid.is_empty() && uid == account.id {
                if let Some(t) = auth["auth"]["accessToken"].as_str() {
                    if !t.is_empty() {
                        return Ok(t.to_string());
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
}

#[async_trait]
impl Provider for MiaoshouProvider {
    fn id(&self) -> &'static str {
        "miaoshou"
    }

    fn display_name(&self) -> &'static str {
        "妙手"
    }

    fn accent(&self) -> &'static str {
        "#ff6a00"
    }

    fn docs_url(&self) -> &'static str {
        "https://catpaw.meituan.com"
    }

    async fn discover(&self) -> Result<Vec<(Account, Value)>> {
        let Some(auth) = Self::local_auth() else {
            return Ok(Vec::new());
        };
        let token = auth["auth"]["accessToken"].as_str().unwrap_or("");
        if token.is_empty() {
            return Ok(Vec::new());
        }
        let uid = auth["account"]["uid"].as_str().unwrap_or("");
        let name = ["name", "loginName"]
            .iter()
            .find_map(|k| auth["account"][*k].as_str())
            .unwrap_or("妙手账号");

        let account = Account {
            id: if uid.is_empty() {
                "miaoshou".to_string()
            } else {
                uid.to_string()
            },
            provider: "miaoshou".to_string(),
            label: name.to_string(),
            created_at: chrono::Utc::now().timestamp(),
            last_used: None,
            origin: "本机妙手登录态".to_string(),
        };
        let cred = json!({
            "access_token": token,
            "uid": uid,
            "name": name,
        });
        Ok(vec![(account, cred)])
    }

    async fn status(&self, _account: &Account, _cred: &Value) -> Result<CheckinStatus> {
        // 妙手没有每日签到：如实说明，不假装能签。
        Ok(CheckinStatus {
            state: ClaimState::NotAvailable,
            points: 0,
            streak_days: 0,
            message: "妙手无每日签到，积分来自套餐与注册奖励".to_string(),
            day_index: None,
            bonus_points: 0,
        })
    }

    async fn claim(&self, account: &Account, cred: &Value) -> Result<ClaimOutcome> {
        // 幂等的注册奖励：新用户拿奖励，老用户返回「用户已注册」。
        let token = Self::current_token(account, cred)?;
        let mut headers = reqwest::header::HeaderMap::new();
        let value = token.parse::<reqwest::header::HeaderValue>().unwrap();
        headers.insert("X-Auth-Token", value.clone());
        headers.insert("access-token", value);
        headers.insert("accept", "application/json".parse().unwrap());
        headers.insert("content-type", "application/json".parse().unwrap());

        let body: Value = self
            .http
            .post(format!("{BASE}{REGISTER_PATH}"))
            .headers(headers)
            .json(&json!({ "registerChannel": REGISTER_CHANNEL }))
            .send()
            .await?
            .json()
            .await?;

        if body["code"].as_i64().unwrap_or(-1) != 0 {
            return Err(anyhow!(
                "注册接口返回 {}：{}",
                body["code"],
                body["message"].as_str().unwrap_or("")
            ));
        }

        let data = &body["data"];
        let is_new = data["newUser"].as_bool().unwrap_or(false);
        let bonus = data["registrationBonus"].as_i64().unwrap_or(0);
        if is_new {
            Ok(ClaimOutcome::claimed(
                bonus,
                format!("注册奖励 +{bonus} Credits"),
            ))
        } else {
            Ok(ClaimOutcome::already(
                0,
                data["message"].as_str().unwrap_or("用户已注册，无注册奖励"),
            ))
        }
    }

    async fn quota(&self, account: &Account, cred: &Value) -> Result<Quota> {
        let token = Self::current_token(account, cred)?;
        let mut headers = reqwest::header::HeaderMap::new();
        let value = token.parse::<reqwest::header::HeaderValue>().unwrap();
        headers.insert("X-Auth-Token", value.clone());
        headers.insert("access-token", value);
        headers.insert("accept", "application/json".parse().unwrap());

        let balance: Value = self
            .http
            .get(format!("{BASE}{BALANCE_PATH}"))
            .headers(headers.clone())
            .send()
            .await?
            .json()
            .await?;

        if balance["code"].as_i64().unwrap_or(-1) != 0 {
            return Err(anyhow!(
                "积分查询失败 {}：{}",
                balance["code"],
                balance["message"].as_str().unwrap_or("")
            ));
        }

        let data = &balance["data"];
        let available = data["availableCredits"]
            .as_str()
            .and_then(|s| s.replace(',', "").parse::<f64>().ok())
            .or_else(|| data["availableCredits"].as_f64())
            .unwrap_or(0.0);
        let plan_name = data["userPlan"]["planName"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let next_refresh_raw = data["userPlan"]["nextRefreshTime"].as_i64().unwrap_or(0);
        // 兼容秒级时间戳（界面按毫秒展示）
        let next_refresh = if next_refresh_raw > 0 && next_refresh_raw < 1_000_000_000_000 {
            next_refresh_raw * 1000
        } else {
            next_refresh_raw
        };

        // 订阅详情（失败不影响主结果）
        let mut message = format!("{plan_name} · 可用 {available:.0} Credits");
        // 真正的「积分到期」来自订阅的 expireTime；nextRefreshTime 只是套餐刷新时间，
        // 不能当到期用（否则会误报「N 天后到期」）。
        let mut subscription_expire_ms = 0i64;
        if let Ok(sub) = self
            .http
            .get(format!("{BASE}{SUBSCRIPTION_CURRENT_PATH}"))
            .headers(headers)
            .send()
            .await
        {
            if let Ok(v) = sub.json::<Value>().await {
                if v["code"].as_i64() == Some(0) {
                    if let Some(exp) = v["data"]["expireTime"].as_i64() {
                        if exp > 0 {
                            let exp_ms = if exp < 1_000_000_000_000 {
                                exp * 1000
                            } else {
                                exp
                            };
                            subscription_expire_ms = exp_ms;
                            message.push_str(&format!(" · {} 到期", fmt_ts(exp_ms)));
                        }
                    }
                }
            }
        }
        // 下次刷新时间单独用 refresh_at_ms 返回，界面独立展示，避免和到期混在一起。
        Ok(Quota {
            total_granted: 0.0,
            total_consumed: 0.0,
            total_remaining: available,
            items: vec![CreditItem {
                credit_type: "plan".to_string(),
                granted: 0.0,
                consumed: 0.0,
                remaining: available,
                granted_at_ms: 0,
                expire_at_ms: subscription_expire_ms,
                package_name: String::new(),
                refresh_at_ms: 0,
            }],
            message,
            refresh_at_ms: next_refresh,
            credit_groups: Vec::new(),
        })
    }
}

fn fmt_ts(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|d| {
            d.with_timezone(&chrono::Local)
                .format("%Y-%m-%d")
                .to_string()
        })
        .unwrap_or_else(|| ms.to_string())
}
