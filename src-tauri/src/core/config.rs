//! 应用配置。
//!
//! 调度模型参考 Cockpit Tools：不是「每天固定一个点全打一遍」，而是给一个
//! 时间窗（如 06:00–08:00），再给每个账号在窗内随机分配一个分钟偏移，
//! 错峰请求，避免同一时刻打同一个上游接口。

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use super::paths::{atomic_write, config_file};

/// 调度器配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SchedulerConfig {
    /// 是否开启自动签到
    pub enabled: bool,
    /// 时间窗起（当天第几分钟，0-1439）
    pub start_minute: u32,
    /// 时间窗止（当天第几分钟，0-1439）
    pub end_minute: u32,
    /// 各平台是否参与自动签到
    pub providers: BTreeMap<String, bool>,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        let mut providers = BTreeMap::new();
        for id in ["minimax", "qoder"] {
            providers.insert(id.to_string(), true);
        }
        Self {
            enabled: true,
            start_minute: 6 * 60,
            end_minute: 8 * 60,
            providers,
        }
    }
}

impl SchedulerConfig {
    /// 时间窗是否合法（起 < 止）。
    pub fn is_valid(&self) -> bool {
        self.start_minute < self.end_minute && self.end_minute <= 24 * 60
    }
}

/// 应用配置。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    #[serde(default)]
    pub scheduler: SchedulerConfig,
}

impl AppConfig {
    pub fn load() -> Result<Self> {
        let path = config_file();
        if !path.exists() {
            return Ok(Self::default());
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
}
