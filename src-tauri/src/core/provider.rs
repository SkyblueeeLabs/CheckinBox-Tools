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
        self.label.starts_with("Qoder ") || self.label.starts_with("豆包 ")
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

/// 周期签到面板的一天（MiniMax 的 7 天连签等）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckinDay {
    /// 周期内的第几天（1 起）
    pub day_no: i32,
    pub is_today: bool,
    /// 这一天的奖励积分（已含上游给的加成）
    pub points: i64,
    #[serde(default)]
    pub bonus_points: i64,
    pub claimed: bool,
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
    /// 本轮周期面板（上游原样给出每天的奖励）；没有该数据的平台为空
    #[serde(default)]
    pub days: Vec<CheckinDay>,
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
            days: Vec::new(),
        }
    }
}

/// 单个积分发放批次。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreditItem {
    /// "purchased"（购买）/ "free"（签到赠送）；WorkBuddy 是分组 kind（plan_base 等）
    pub credit_type: String,
    pub granted: f64,
    pub consumed: f64,
    pub remaining: f64,
    pub granted_at_ms: i64,
    pub expire_at_ms: i64,
    /// 平台给的包名（WorkBuddy PackageName），没有为空串
    #[serde(default)]
    pub package_name: String,
    /// 本批次下一次刷新时间（毫秒；0 = 平台未提供）。WorkBuddy 取 CycleResetTime，
    /// 缺了按 CycleEndTime + 1s 推（套餐结束即刷新）。
    #[serde(default)]
    pub refresh_at_ms: i64,
}

/// WorkBuddy 官方积分分组（get-user-resource-summary 的 CreditGroups，
/// 服务端按 Kind 聚合，口径与官网一致）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreditGroup {
    /// plan_base / plan_bonus / benefit / addon
    pub kind: String,
    /// 服务端给的中文名（套餐基础积分 等）
    pub name: String,
    pub total: f64,
    pub used: f64,
    pub remaining: f64,
    /// 组内资源包数量
    pub count: i64,
    /// benefit/addon 取最近到期时间，plan_base/plan_bonus 取下次刷新时间
    ///（毫秒；0 = 没有可展示的）
    #[serde(default)]
    pub time_ms: i64,
    /// true = time_ms 是「到期时间」（benefit/addon），false =「下次刷新」
    #[serde(default)]
    pub uses_expiry: bool,
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
    /// 平台下一次额度刷新 / 发放时间（毫秒；0 = 平台未提供）。
    /// 目前妙手会返回（每月刷新），其他平台为 0。
    #[serde(default)]
    pub refresh_at_ms: i64,
    /// WorkBuddy 官方积分分组；其他平台 / summary 查询失败时为空。
    #[serde(default)]
    pub credit_groups: Vec<CreditGroup>,
}

impl Quota {
    pub fn empty(message: impl Into<String>) -> Self {
        Self {
            total_granted: 0.0,
            total_consumed: 0.0,
            total_remaining: 0.0,
            items: Vec::new(),
            message: message.into(),
            refresh_at_ms: 0,
            credit_groups: Vec::new(),
        }
    }

    /// 未来 `days` 天内到期的剩余积分合计，以及最早到期时刻。
    ///
    /// 只统计 `remaining > 0` 且到期时间在 `(now, now + days]` 区间内的批次；
    /// `expire_at_ms == 0`（平台没给到期时间）和已过期的批次都忽略。
    pub fn expiring_within(&self, days: i64, now_ms: i64) -> Option<(f64, i64)> {
        let limit = now_ms + days.max(0) * 86_400_000;
        let mut total = 0.0;
        let mut earliest = i64::MAX;
        for item in &self.items {
            if item.remaining <= 0.0 || item.expire_at_ms <= now_ms || item.expire_at_ms > limit {
                continue;
            }
            total += item.remaining;
            earliest = earliest.min(item.expire_at_ms);
        }
        (earliest != i64::MAX).then_some((total, earliest))
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

/// 平台每日福利的获取方式（决定 UI 分区与调度行为）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ClaimMode {
    /// 本工具可以自动签到（WorkBuddy / MiniMax / Trae / Qoder）
    Auto,
    /// 有每日福利，但必须在官方页面 / 客户端手动领取（ZCode / 豆包）
    Manual,
    /// 没有每日福利，只看额度（妙手）
    None,
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

    /// 这个平台每日福利的获取方式。默认按「可自动签到」处理。
    fn claim_mode(&self) -> ClaimMode {
        ClaimMode::Auto
    }

    /// 这个平台是否支持「自动领取」。
    ///
    /// `false` = 只展示状态 + 提醒，调度器和「一键全部签到」都不会调用
    /// `claim()`。给「领取必须走官方客户端」的平台用（例如 ZCode ——
    /// 领取要过阿里云验证码，第三方做不了）。
    fn supports_claim(&self) -> bool {
        self.claim_mode() == ClaimMode::Auto
    }

    /// 是否有「每日福利」可言（false = 只看额度，不参与签到分区与统计）。
    fn has_daily_benefit(&self) -> bool {
        self.claim_mode() != ClaimMode::None
    }

    /// 不支持自动领取时，面向用户的「为什么不能代签」。
    ///
    /// `Some` = UI 在账号卡上展示一个置灰不可点的签到按钮，并把这段文字
    /// 作为悬浮说明；`None` = 完全不展示签到入口（例如 ZCode 用「打开客户端领取」）。
    fn claim_blocked_reason(&self) -> Option<&'static str> {
        None
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 86_400_000;

    fn item(remaining: f64, expire_at_ms: i64) -> CreditItem {
        CreditItem {
            credit_type: "free".into(),
            granted: remaining,
            consumed: 0.0,
            remaining,
            granted_at_ms: 0,
            expire_at_ms,
            package_name: String::new(),
            refresh_at_ms: 0,
        }
    }

    fn quota(items: Vec<CreditItem>) -> Quota {
        Quota {
            total_granted: 0.0,
            total_consumed: 0.0,
            total_remaining: 0.0,
            items,
            message: String::new(),
            refresh_at_ms: 0,
            credit_groups: Vec::new(),
        }
    }

    #[test]
    fn 到期计算_只统计窗口内且未用完的批次() {
        let now = 1_700_000_000_000;
        let q = quota(vec![
            item(100.0, now + 2 * DAY),
            item(50.0, now + 6 * DAY),
            item(0.0, now + 1 * DAY),
            item(30.0, now + 8 * DAY),
            item(20.0, now - DAY),
            item(40.0, 0),
        ]);
        assert_eq!(q.expiring_within(7, now), Some((150.0, now + 2 * DAY)));
    }

    #[test]
    fn 到期计算_没有窗口内批次时返回none() {
        let now = 1_700_000_000_000;
        assert_eq!(quota(vec![]).expiring_within(7, now), None);
        assert_eq!(
            quota(vec![item(100.0, now + 30 * DAY)]).expiring_within(7, now),
            None
        );
    }
}
