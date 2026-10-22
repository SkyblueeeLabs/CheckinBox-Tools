//! 签到流水。
//!
//! 每次签到（含「今日已签」和失败）都落一条 JSONL，按月分文件：
//! `logs/checkin-2026-10.jsonl`。UI 读最近 N 天。

use anyhow::Result;
use chrono::Local;
use serde::{Deserialize, Serialize};
use std::io::Write;

use super::paths::{ensure_dir, log_file, logs_dir};

/// 单条签到记录。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub id: String,
    /// `2026-10-22 07:53:17`
    pub timestamp: String,
    /// `2026-10-22`
    pub date: String,
    pub provider: String,
    pub account_id: String,
    pub label: String,
    /// `success` / `already` / `failed`
    pub status: String,
    pub points: i64,
    pub message: String,
}

impl LogEntry {
    pub fn new(
        provider: impl Into<String>,
        account_id: impl Into<String>,
        label: impl Into<String>,
        status: impl Into<String>,
        points: i64,
        message: impl Into<String>,
    ) -> Self {
        let now = Local::now();
        Self {
            id: format!("log_{}_{}", now.timestamp_millis(), uuid::Uuid::new_v4().simple()),
            timestamp: now.format("%Y-%m-%d %H:%M:%S").to_string(),
            date: now.format("%Y-%m-%d").to_string(),
            provider: provider.into(),
            account_id: account_id.into(),
            label: label.into(),
            status: status.into(),
            points,
            message: message.into(),
        }
    }
}

/// 追加一条流水。
pub fn append(entry: &LogEntry) -> Result<()> {
    ensure_dir(&logs_dir())?;
    let month = entry.date.get(0..7).unwrap_or("unknown");
    let path = log_file(month);
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    let line = serde_json::to_string(entry)?;
    writeln!(file, "{line}")?;
    Ok(())
}

/// 读取最近 `days` 天的流水，按时间倒序。
///
/// 实现要点：按月分文件，**每个月只读一次**，再按日期区间过滤。
/// 早先的版本对区间内每天都重读一遍整月文件，导致同一条记录重复 N 次。
pub fn recent(days: i64) -> Result<Vec<LogEntry>> {
    let days = days.max(1);
    let today = Local::now().date_naive();
    let oldest = today - chrono::Duration::days(days - 1);
    let oldest_str = oldest.format("%Y-%m-%d").to_string();
    let today_str = today.format("%Y-%m-%d").to_string();

    // 收集区间覆盖到的月份（去重）
    let mut months: Vec<String> = Vec::new();
    let mut cursor = oldest;
    while cursor <= today {
        let month = cursor.format("%Y-%m").to_string();
        if months.last().map(|m| m != &month).unwrap_or(true) {
            months.push(month);
        }
        cursor += chrono::Duration::days(1);
    }

    let mut out: Vec<LogEntry> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    for month in months {
        let path = log_file(&month);
        if !path.exists() {
            continue;
        }
        let raw = std::fs::read_to_string(&path)?;
        for line in raw.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let Ok(entry) = serde_json::from_str::<LogEntry>(line) else {
                continue;
            };
            if entry.date < oldest_str || entry.date > today_str {
                continue;
            }
            if seen.insert(entry.id.clone()) {
                out.push(entry);
            }
        }
    }

    out.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    Ok(out)
}
