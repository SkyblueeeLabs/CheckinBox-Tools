//! 应用配置。
//!
//! 调度模型参考 Cockpit Tools：不是「每天固定一个点全打一遍」，而是给一个
//! 时间窗（如 06:00–08:00），再给每个账号在窗内随机分配一个分钟偏移，
//! 错峰请求，避免同一时刻打同一个上游接口。
//!
//! 关键点：**调度配置按 agent 各自独立**（每个 agent 自己管自己的开关和时间窗），
//! 不再有一个全局时间窗。这样不同平台的活动刷新时间（例如 Qoder 每天 10:00）
//! 可以配不同的窗口。

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use super::paths::{atomic_write, config_file};

/// 单个 agent 的调度配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentScheduleConfig {
    /// 是否开启该 agent 的自动签到
    pub enabled: bool,
    /// 时间窗起（当天第几分钟，0-1439）
    pub start_minute: u32,
    /// 时间窗止（当天第几分钟，0-1439）
    pub end_minute: u32,
    /// 该 agent 的数据自动同步间隔（分钟）。
    ///
    /// 借鉴 Cockpit Tools 的 `*_auto_refresh_minutes`（默认 10）：
    /// 后台按此间隔刷新积分 / 签到状态 / 账号资料，
    /// 跨天时不看间隔，强制同步一次。
    #[serde(default = "default_refresh_interval")]
    pub refresh_interval_minutes: u32,
    /// 换号时是否同步客户端本地聊天记录（借鉴 Cockpit 的
    /// `share_sessions_on_switch`）。
    ///
    /// WorkBuddy 按 UID 过滤本地会话，需要切换时合并；Qoder 本地会话默认共享，不用此开关。
    /// 勾选后每次「切换客户端账号」把会话记录带给目标账号；
    /// 不勾选则完全不碰聊天数据（与旧版行为一致）。
    /// MiniMax 的会话在服务端自动同步，此开关对它不存在。
    #[serde(default)]
    pub chat_sync_enabled: bool,
}

fn default_refresh_interval() -> u32 {
    10
}

impl Default for AgentScheduleConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            start_minute: 6 * 60,
            end_minute: 8 * 60,
            refresh_interval_minutes: default_refresh_interval(),
            chat_sync_enabled: false,
        }
    }
}

impl AgentScheduleConfig {
    pub fn is_valid(&self) -> bool {
        self.start_minute < self.end_minute && self.end_minute <= 24 * 60
    }

    /// 某平台的默认时间窗。
    ///
    /// **必须按平台各自的「活动刷新时刻」来配**：Qoder 的每日活动 10:00（UTC+8）
    /// 才刷新，窗口放在 10:00 之前的话，到点只会拿到「未开始」，然后被标记成
    /// 「今天已处理」，一整天都不再重试。
    pub fn default_for(provider: &str) -> Self {
        let (start_minute, end_minute) = match provider {
            // Qoder CN：每日活动 10:00（UTC+8）刷新
            "qoder" => (10 * 60, 11 * 60),
            _ => (6 * 60, 8 * 60),
        };
        Self {
            enabled: true,
            start_minute,
            end_minute,
            refresh_interval_minutes: default_refresh_interval(),
            chat_sync_enabled: false,
        }
    }
}

/// 调度器配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SchedulerConfig {
    /// 总闸：关掉后所有 agent 都不跑（应急用）
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// 自动同步总开关：关掉后后台不再周期性拉取积分 / 签到状态 / 账号资料，
    /// 只保留「到点签到」。跨天时的强制同步也一并被关掉。
    #[serde(default = "default_true")]
    pub sync_enabled: bool,
    /// 每个 agent 自己的配置；没有条目的 agent 用默认值
    #[serde(default)]
    pub agents: BTreeMap<String, AgentScheduleConfig>,
}

fn default_true() -> bool {
    true
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            sync_enabled: true,
            agents: BTreeMap::new(),
        }
    }
}

impl SchedulerConfig {
    /// 取某 agent 的配置（缺省用该平台的默认时间窗）。
    pub fn agent(&self, id: &str) -> AgentScheduleConfig {
        self.agents
            .get(id)
            .cloned()
            .unwrap_or_else(|| AgentScheduleConfig::default_for(id))
    }

    /// 写回某 agent 的配置。
    pub fn set_agent(&mut self, id: &str, cfg: AgentScheduleConfig) {
        self.agents.insert(id.to_string(), cfg);
    }
}

/// 点窗口右上角 × 时的行为。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CloseAction {
    /// 每次关闭都弹确认框，由用户选择最小化还是退出
    #[default]
    Ask,
    /// 最小化到系统托盘，后台继续签到 / 同步
    Tray,
    /// 直接退出应用
    Exit,
}

/// 应用配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    #[serde(default)]
    pub scheduler: SchedulerConfig,
    /// 每个 agent 当前激活（默认使用）的账号 id。
    /// 「多账号切换」就是改这个值。
    #[serde(default)]
    pub active_accounts: BTreeMap<String, String>,
    /// 积分预警阈值（剩余低于该值就在界面上标红）。
    /// 借鉴 Cockpit Tools 的 `*_quota_alert_threshold`。
    #[serde(default)]
    pub alert_thresholds: BTreeMap<String, u32>,
    /// 侧栏 agent 的显示顺序（拖拽调序后落盘）。
    /// 没列出的平台排在最后（按注册顺序）。
    #[serde(default)]
    pub agent_order: Vec<String>,
    /// 关闭主窗口时的行为（默认每次询问）。
    #[serde(default)]
    pub close_action: CloseAction,
    /// 是否生成自动提醒（签到失败、低额度、到期、可领取）。
    #[serde(default = "default_true")]
    pub notifications_enabled: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            scheduler: SchedulerConfig::default(),
            active_accounts: BTreeMap::new(),
            alert_thresholds: BTreeMap::new(),
            agent_order: Vec::new(),
            close_action: CloseAction::default(),
            notifications_enabled: true,
        }
    }
}

impl AppConfig {
    pub fn load() -> Result<Self> {
        let path = config_file();
        if !path.exists() {
            // 首次运行：把默认配置写盘，方便用户直接打开文件调整
            let defaults = Self::default();
            let _ = defaults.save();
            return Ok(defaults);
        }
        let raw = std::fs::read_to_string(&path)?;
        if raw.trim().is_empty() {
            return Ok(Self::default());
        }
        Ok(serde_json::from_str(&raw).unwrap_or_default())
    }

    pub fn save(&self) -> Result<()> {
        let path = config_file();
        let text = serde_json::to_string_pretty(self)?;
        atomic_write(&path, text.as_bytes())
    }

    /// 取某 agent 当前激活的账号 id。
    pub fn active_of(&self, provider: &str) -> Option<String> {
        self.active_accounts.get(provider).cloned()
    }

    pub fn set_active(&mut self, provider: &str, account_id: &str) {
        self.active_accounts
            .insert(provider.to_string(), account_id.to_string());
    }

    /// 某 agent 的积分预警阈值（默认 200）。
    pub fn alert_threshold_of(&self, provider: &str) -> u32 {
        self.alert_thresholds.get(provider).copied().unwrap_or(200)
    }

    pub fn set_alert_threshold(&mut self, provider: &str, value: u32) {
        self.alert_thresholds.insert(provider.to_string(), value);
    }

    /// 某 agent 的数据自动同步间隔（分钟）。
    pub fn refresh_interval_of(&self, provider: &str) -> u32 {
        self.scheduler
            .agent(provider)
            .refresh_interval_minutes
            .max(1)
    }

    /// 按用户自定义顺序排序平台 id（未配置的按原顺序排在后面）。
    pub fn sort_provider_ids(&self, ids: Vec<String>) -> Vec<String> {
        let rank_of = |id: &str| self.agent_order.iter().position(|x| x == id);
        let mut sorted = ids;
        sorted.sort_by_key(|id| match rank_of(id) {
            Some(i) => (i as u32, 0u32),
            None => (u32::MAX, 0),
        });
        sorted
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 按配置顺序排序_未配置的排在后面() {
        let mut cfg = AppConfig::default();
        cfg.agent_order = vec!["qoder".into(), "minimax".into()];
        let sorted = cfg.sort_provider_ids(vec![
            "miaoshou".into(),
            "minimax".into(),
            "qoder".into(),
            "workbuddy".into(),
        ]);
        assert_eq!(sorted, vec!["qoder", "minimax", "miaoshou", "workbuddy"]);
    }

    #[test]
    fn 没有配置时保持原顺序() {
        let cfg = AppConfig::default();
        let ids = vec!["a".to_string(), "b".to_string()];
        assert_eq!(cfg.sort_provider_ids(ids.clone()), ids);
    }

    #[test]
    fn 配置里有多余项不影响排序() {
        let mut cfg = AppConfig::default();
        cfg.agent_order = vec!["gone".into(), "b".into(), "a".into()];
        let sorted = cfg.sort_provider_ids(vec!["a".into(), "b".into()]);
        assert_eq!(sorted, vec!["b", "a"]);
    }

    #[test]
    fn 关闭行为默认每次询问() {
        assert_eq!(AppConfig::default().close_action, CloseAction::Ask);
    }

    #[test]
    fn 旧配置缺少关闭行为字段时默认询问() {
        let cfg: AppConfig = serde_json::from_str(
            r#"{"scheduler":{"enabled":true,"syncEnabled":true,"agents":{}},"activeAccounts":{},"alertThresholds":{},"agentOrder":[]}"#,
        )
        .unwrap();
        assert_eq!(cfg.close_action, CloseAction::Ask);
        assert!(cfg.notifications_enabled, "旧配置没写通知开关时应默认开启");
    }

    #[test]
    fn 通知默认开启并按camelCase落盘() {
        assert!(AppConfig::default().notifications_enabled);
        let mut cfg = AppConfig::default();
        cfg.notifications_enabled = false;
        let json = serde_json::to_string(&cfg).unwrap();
        assert!(json.contains("\"notificationsEnabled\":false"));
        let back: AppConfig = serde_json::from_str(&json).unwrap();
        assert!(!back.notifications_enabled);
    }

    #[test]
    fn 关闭行为按camelCase落盘并可回读() {
        let mut cfg = AppConfig::default();
        cfg.close_action = CloseAction::Tray;
        let json = serde_json::to_string(&cfg).unwrap();
        assert!(json.contains("\"closeAction\":\"tray\""));
        let back: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.close_action, CloseAction::Tray);
    }
}
