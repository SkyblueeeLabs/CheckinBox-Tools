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
}

impl Default for AgentScheduleConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            start_minute: 6 * 60,
            end_minute: 8 * 60,
        }
    }
}

impl AgentScheduleConfig {
    pub fn is_valid(&self) -> bool {
        self.start_minute < self.end_minute && self.end_minute <= 24 * 60
    }
}

/// 调度器配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SchedulerConfig {
    /// 总闸：关掉后所有 agent 都不跑（应急用）
    #[serde(default = "default_true")]
    pub enabled: bool,
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
            agents: BTreeMap::new(),
        }
    }
}

impl SchedulerConfig {
    /// 取某 agent 的配置（缺省用默认值）。
    pub fn agent(&self, id: &str) -> AgentScheduleConfig {
        self.agents.get(id).cloned().unwrap_or_default()
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
}
