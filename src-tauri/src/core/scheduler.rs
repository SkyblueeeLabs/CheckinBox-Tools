//! 调度计划。
//!
//! 模型参考 Cockpit Tools：
//!   * 每天在 `[start_minute, end_minute]` 时间窗内触发；
//!   * 每个账号在窗内有一个**确定性随机**的分钟偏移（同一个账号、同一天，
//!     重启软件后计划不变），避免所有账号同一秒打同一个上游；
//!   * 跨天时重新分配偏移，并清空「今日已签」标记。

use anyhow::Result;
use chrono::{Local, Timelike};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use super::paths::{atomic_write, scheduler_state_file};

/// 单个账号的当日计划。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountSchedule {
    /// 当天第几分钟执行
    pub scheduled_minute: u32,
    /// 已成功签到的日期（`YYYY-MM-DD`）
    pub last_checked_date: Option<String>,
    /// 计划生成时的日期
    #[serde(default)]
    pub scheduled_date: String,
}

/// 调度状态。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SchedulerState {
    /// 当前计划所属日期
    pub date: String,
    #[serde(default)]
    pub accounts: BTreeMap<String, AccountSchedule>,
}

/// FNV-1a：把字符串稳定地映射成一个 u64。
fn fnv1a(seed: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in seed.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    hash
}

/// 在时间窗内为 `key` 分配一个确定性的分钟偏移。
fn pick_minute(key: &str, date: &str, start: u32, end: u32) -> u32 {
    let span = end.saturating_sub(start).max(1);
    let mixed = format!("{date}:{key}");
    let hash = fnv1a(&mixed);
    start + (hash % span as u64) as u32
}

impl SchedulerState {
    pub fn load() -> Result<Self> {
        let path = scheduler_state_file();
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
        let path = scheduler_state_file();
        let text = serde_json::to_string_pretty(self)?;
        atomic_write(&path, text.as_bytes())
    }

    /// 跨天重排：给不在计划里的账号补上当天偏移，保留「已签」标记以便 UI 展示。
    pub fn roll_over(&mut self, date: &str, keys: &[String], start: u32, end: u32) {
        if self.date != date {
            self.date = date.to_string();
        }
        for key in keys {
            let entry = self.accounts.entry(key.clone()).or_default();
            if entry.scheduled_date != date {
                entry.scheduled_date = date.to_string();
                entry.scheduled_minute = pick_minute(key, date, start, end);
            }
        }
        // 清理已删除账号的残留计划
        let stale: Vec<String> = self
            .accounts
            .keys()
            .filter(|k| !keys.contains(k))
            .cloned()
            .collect();
        for key in stale {
            self.accounts.remove(&key);
        }
    }

    /// 该账号今天是否已经处理过。
    pub fn is_done(&self, key: &str, date: &str) -> bool {
        self.accounts
            .get(key)
            .and_then(|s| s.last_checked_date.as_deref())
            .map(|d| d == date)
            .unwrap_or(false)
    }

    /// 该账号今天的计划分钟。
    pub fn minute_of(&self, key: &str) -> Option<u32> {
        self.accounts.get(key).map(|s| s.scheduled_minute)
    }

    /// 标记已签。
    pub fn mark_done(&mut self, key: &str, date: &str) {
        if let Some(entry) = self.accounts.get_mut(key) {
            entry.last_checked_date = Some(date.to_string());
        }
    }

    /// 清除「已签」标记（手动重试时用）。
    pub fn clear_done(&mut self, key: &str) {
        if let Some(entry) = self.accounts.get_mut(key) {
            entry.last_checked_date = None;
        }
    }
}

/// 今天的日期字符串。
pub fn today() -> String {
    Local::now().format("%Y-%m-%d").to_string()
}

/// 当前是当天第几分钟。
pub fn now_minute() -> u32 {
    let now = Local::now();
    now.hour() * 60 + now.minute()
}
