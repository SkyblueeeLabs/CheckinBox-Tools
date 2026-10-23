//! 平台适配器抽象。
//!
//! 每个 AI 编程工具（MiniMax Code / Qoder CN / ...）实现一个 `Provider`，
//! 上层（调度器、UI、命令）只依赖这个 trait，因此新增一个平台不需要改动其他层。

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 一个已登录的账号。`id` 在单个 provider 内唯一。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub provider: String,
    /// 展示用昵称（用户名 / 邮箱 / 备注）
    pub label: String,
    pub created_at: i64,
    pub last_used: Option<i64>,
    /// 账号来源说明，例如「本机 MiniMax Code」/「设备授权」
    #[serde(default)]
    pub origin: String,
}

impl Account {
    /// 当前 label 是否是「拿不到昵称时的兜底值」（平台名 + id 前缀）。
    ///
    /// 只有这种情况才值得为它多打一次 `profile` 接口去换真实昵称。
    pub fn label_is_fallback(&self) -> bool {
        self.label.starts_with("Qoder ")
    }
}

/// 账号真实资料（昵称 / 邮箱 / 头像）。
///
/// 有些平台（例如 Qoder）在授权时只给一个内部 uid，界面只能显示
/// `Qoder 01a1348c-3fc` 这种 id 前缀；这里补一个「按 token 查资料」的接口，
/// 拿到用户在平台上真实设置的昵称。取不到就返回 `Ok(None)`，不影响主流程。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountProfile {
    pub name: Option<String>,
    pub email: Option<String>,
    pub avatar: Option<String>,
}

/// 今日签到状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ClaimState {
    /// 今天可以签
    Claimable,
    /// 今天已经签过了
    Claimed,
    /// 今天没有可签的活动（例如上游还没下发）
    NotAvailable,
    /// 状态未知
    Unknown,
}

/// 签到状态快照。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckinStatus {
    pub state: ClaimState,
    /// 今日可领取的积分数
    pub points: i64,
    /// 当前连续签到天数
    pub streak_days: i32,
    /// 面向用户的一句话说明
    pub message: String,
    /// 今天是周期中的第几天（1-7），未知时为 None
    #[serde(default)]
    pub day_index: Option<i32>,
    /// 额外奖励积分（连签第 4/7 天那种）
    #[serde(default)]
    pub bonus_points: i64,
}

impl Default for CheckinStatus {
    fn default() -> Self {
        Self {
            state: ClaimState::Unknown,
            points: 0,
            streak_days: 0,
            message: String::new(),
            day_index: None,
            bonus_points: 0,
        }
    }
}

/// 单个积分发放批次。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreditItem {
    /// "purchased"（购买）/ "free"（签到赠送）
    pub credit_type: String,
    pub granted: f64,
    pub consumed: f64,
    pub remaining: f64,
    pub granted_at_ms: i64,
    pub expire_at_ms: i64,
}

/// 积分总览：发放 / 已用 / 剩余。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Quota {
    pub total_granted: f64,
    pub total_consumed: f64,
    pub total_remaining: f64,
    #[serde(default)]
    pub items: Vec<CreditItem>,
    /// 面向用户的一句话说明（例如"30 天内到期 400"）
    #[serde(default)]
    pub message: String,
}

impl Quota {
    pub fn empty(message: impl Into<String>) -> Self {
        Self {
            total_granted: 0.0,
            total_consumed: 0.0,
            total_remaining: 0.0,
            items: Vec::new(),
            message: message.into(),
        }
    }
}

/// 领取结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaimOutcome {
    pub state: ClaimState,
    pub points: i64,
    pub message: String,
}

impl ClaimOutcome {
    pub fn claimed(points: i64, message: impl Into<String>) -> Self {
        Self {
            state: ClaimState::Claimed,
            points,
            message: message.into(),
        }
    }

    pub fn already(points: i64, message: impl Into<String>) -> Self {
        Self {
            state: ClaimState::Claimed,
            points,
            message: message.into(),
        }
    }

    pub fn failed(message: impl Into<String>) -> Self {
        Self {
            state: ClaimState::Unknown,
            points: 0,
            message: message.into(),
        }
    }
}

/// 平台适配器。
///
/// `Credential` 用 `serde_json::Value` 而不是强类型，是为了让每个平台自由
/// 决定自己需要存哪些字段（MiniMax 要 access/refresh token，Qoder 还要
/// machine_id 和设备风控三件套）。凭据本身由 `store` 层用 DPAPI 加密落盘。
#[async_trait]
pub trait Provider: Send + Sync {
    /// 平台标识，如 `minimax` / `qoder`
    fn id(&self) -> &'static str;
    /// 展示名
    fn display_name(&self) -> &'static str;
    /// 主题色，供 UI 分组使用
    fn accent(&self) -> &'static str;
    /// 官方活动说明页
    fn docs_url(&self) -> &'static str;

    /// 从本机客户端自动发现已登录的账号（不写盘，仅返回候选）。
    ///
    /// 返回空数组表示该平台不支持自动发现，需要走 `authorize`。
    async fn discover(&self) -> Result<Vec<(Account, Value)>> {
        Ok(Vec::new())
    }

    /// 查询今日签到状态。
    async fn status(&self, account: &Account, cred: &Value) -> Result<CheckinStatus>;

    /// 执行领取。
    async fn claim(&self, account: &Account, cred: &Value) -> Result<ClaimOutcome>;

    /// 查询积分总览（发放 / 已用 / 剩余）。
    ///
    /// 默认返回空并给出说明；平台支持时覆盖实现。
    async fn quota(&self, _account: &Account, _cred: &Value) -> Result<Quota> {
        Ok(Quota::empty("该平台暂未提供积分查询"))
    }

    /// 查询账号真实资料（昵称 / 邮箱 / 头像）。
    ///
    /// 默认取不到；平台有对应接口时覆盖实现。用于把
    /// `Qoder 01a1348c-3fc` 这种 id 前缀换成用户真实设置的昵称。
    async fn profile(&self, _account: &Account, _cred: &Value) -> Result<Option<AccountProfile>> {
        Ok(None)
    }

    /// 凭据过期时尝试刷新，返回新的凭据；无法刷新返回 `Ok(None)`。
#[allow(dead_code)] // 预留给「凭据自动刷新」的扩展点，第一版未接线
    async fn refresh(&self, _account: &Account, _cred: &Value) -> Result<Option<Value>> {
        Ok(None)
    }
}
