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
    /// 每个 agent 最近一次**自动同步**的 Unix 秒（积分 / 状态 / 资料）。
    /// 跨天时无视间隔强制同步，所以这里只用于「距上次同步多久」。
    #[serde(default)]
    pub last_sync: BTreeMap<String, i64>,
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

    /// 跨天重排：给传入的账号补上当天偏移，保留「已签」标记以便 UI 展示。
    ///
    /// **注意**：本方法**不做**清理。调用方可能按 agent 分批调用（每个 agent
    /// 有自己的时间窗），如果在这里清掉不在 `keys` 里的条目，后一批就会把
    /// 前一批的计划删掉。清理统一由 `prune` 做。
    ///
    /// 除了跨天，**时间窗变了导致原计划分钟落在窗外**时也会重排 ——
    /// 否则「今天已经按旧窗口跑过一次」的账号会一直用旧的、可能在活动
    /// 刷新之前的分钟，新窗口永远等不到它。
    pub fn roll_over(&mut self, date: &str, keys: &[String], start: u32, end: u32) {
        if self.date != date {
            self.date = date.to_string();
        }
        for key in keys {
            let entry = self.accounts.entry(key.clone()).or_default();
            let out_of_window = entry.scheduled_minute < start || entry.scheduled_minute > end;
            if entry.scheduled_date != date || out_of_window {
                entry.scheduled_date = date.to_string();
                entry.scheduled_minute = pick_minute(key, date, start, end);
                // 计划被重排，说明之前那次执行是按**旧窗口**来的（跨天也算），
                // 它的「已处理」标记不再可信，清掉让新计划重新跑一次。
                entry.last_checked_date = None;
            }
        }
    }

    /// 清掉不在 `live_keys` 里的残留计划（账号被移除时）。
    pub fn prune(&mut self, live_keys: &[String]) {
        let stale: Vec<String> = self
            .accounts
            .keys()
            .filter(|k| !live_keys.contains(k))
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

    /// 某 agent 最近一次自动同步的 Unix 秒；从没同步过返回 None。
    pub fn last_sync_of(&self, provider: &str) -> Option<i64> {
        self.last_sync.get(provider).copied()
    }

    /// 记录某 agent 的同步时刻。
    pub fn mark_synced(&mut self, provider: &str, ts: i64) {
        self.last_sync.insert(provider.to_string(), ts);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 跨天重排并清掉已处理标记() {
        let mut s = SchedulerState::default();
        s.roll_over("2026-10-22", &["a".to_string()], 360, 480);
        s.mark_done("a", "2026-10-22");
        assert!(s.is_done("a", "2026-10-22"));

        s.roll_over("2026-10-23", &["a".to_string()], 360, 480);
        assert!(!s.is_done("a", "2026-10-23"), "新的一天不该算已处理");
    }

    #[test]
    fn 时间窗外的计划会被重排并重新执行() {
        let mut s = SchedulerState::default();
        // 旧窗口 06:00–08:00 下生成的计划
        s.roll_over("2026-10-23", &["qoder".to_string()], 360, 480);
        let old = s.minute_of("qoder").unwrap();
        assert!((360..=480).contains(&old));
        s.mark_done("qoder", "2026-10-23");
        assert!(s.is_done("qoder", "2026-10-23"));

        // 新窗口 10:00–11:00：旧计划落在窗外 → 重排 + 清掉已处理标记
        s.roll_over("2026-10-23", &["qoder".to_string()], 600, 660);
        let new = s.minute_of("qoder").unwrap();
        assert!(
            (600..=660).contains(&new),
            "新计划应落在 10:00–11:00 内，实际 {new}"
        );
        assert_ne!(old, new);
        assert!(!s.is_done("qoder", "2026-10-23"), "重排后应重新执行");
    }

    #[test]
    fn 同一窗口内不重排() {
        let mut s = SchedulerState::default();
        s.roll_over("2026-10-23", &["a".to_string()], 600, 660);
        let first = s.minute_of("a").unwrap();
        s.roll_over("2026-10-23", &["a".to_string()], 600, 660);
        assert_eq!(
            s.minute_of("a").unwrap(),
            first,
            "同一天重启多次，计划应保持不变"
        );
    }
}
