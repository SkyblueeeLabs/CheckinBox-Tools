//! 积分历史快照。
//!
//! 借鉴 Tokdash / syrtis 的做法：每次刷新积分时落一条快照，
//! 用来画趋势、看「这个号每天掉多少」。
//!
//! 存储：`%APPDATA%\CheckinBox\quota_history.jsonl`，只保留最近 N 天。

use anyhow::Result;
use chrono::Local;
use serde::{Deserialize, Serialize};
use std::io::Write;

use super::paths::{ensure_dir, quota_history_file};

/// 单条积分快照。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaSnapshot {
    /// `2026-10-22 22:30:05`
    pub timestamp: String,
    pub provider: String,
    pub account_id: String,
    pub label: String,
    pub remaining: f64,
    pub granted: f64,
    pub consumed: f64,
}

impl QuotaSnapshot {
    pub fn new(
        provider: impl Into<String>,
        account_id: impl Into<String>,
        label: impl Into<String>,
        remaining: f64,
        granted: f64,
        consumed: f64,
    ) -> Self {
        Self {
            timestamp: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            provider: provider.into(),
            account_id: account_id.into(),
            label: label.into(),
            remaining,
            granted,
            consumed,
        }
    }
}

const MAX_LINES: usize = 4000;

/// 追加一条快照。文件过大时裁剪最旧的。
pub fn append(snap: &QuotaSnapshot) -> Result<()> {
    ensure_dir(&super::paths::data_dir())?;
    let path = quota_history_file();
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    writeln!(file, "{}", serde_json::to_string(snap)?)?;

    // 简单裁剪：超过上限就重写文件，只留后半部分
    if let Ok(raw) = std::fs::read_to_string(&path) {
        let lines: Vec<&str> = raw.lines().filter(|l| !l.trim().is_empty()).collect();
        if lines.len() > MAX_LINES {
            let keep = &lines[lines.len() - MAX_LINES..];
            std::fs::write(&path, format!("{}\n", keep.join("\n")))?;
        }
    }
    Ok(())
}

/// 读某账号最近 `days` 天的快照，按时间升序。
pub fn recent_of(provider: &str, account_id: &str, days: i64) -> Result<Vec<QuotaSnapshot>> {
    let path = quota_history_file();
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return Ok(Vec::new());
    };
    let cutoff = Local::now() - chrono::Duration::days(days.max(1));

    let mut out = Vec::new();
    for line in raw.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let Ok(s) = serde_json::from_str::<QuotaSnapshot>(line) else {
            continue;
        };
        if &s.provider != provider || &s.account_id != account_id {
            continue;
        }
        // 时间戳是本地 YYYY-MM-DD HH:MM:SS，直接按字符串比较即可
        if s.timestamp.as_str() >= cutoff.format("%Y-%m-%d %H:%M:%S").to_string().as_str() {
            out.push(s);
        }
    }
    Ok(out)
}
