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
            // 千问办公的签到活动已停用，窗口仅作占位
            "qwenwork" => (6 * 60, 8 * 60),
            _ => (6 * 60, 8 * 60),
        };
        Self {
            enabled: true,
            start_minute,
            end_minute,
            refresh_interval_minutes: default_refresh_interval(),
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

/// 应用配置。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
        self.alert_thresholds
            .get(provider)
            .copied()
            .unwrap_or(200)
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
}
