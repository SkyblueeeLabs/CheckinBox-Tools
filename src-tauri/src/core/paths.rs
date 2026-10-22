//! 数据目录布局。
//!
//! ```text
//! %APPDATA%\AgentCheckinTools\
//!   ├── config.json            应用配置（调度时间窗等）
//!   ├── accounts.json          账号池（凭据经 DPAPI 加密后 base64）
//!   ├── scheduler_state.json   当日调度计划（每账号随机分钟偏移）
//!   └── logs\
//!       └── checkin-2026-10.jsonl
//! ```

use std::path::PathBuf;

/// 应用数据根目录。
pub fn data_dir() -> PathBuf {
    let base = dirs::data_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("AgentCheckinTools")
}

pub fn accounts_file() -> PathBuf {
    data_dir().join("accounts.json")
}

pub fn config_file() -> PathBuf {
    data_dir().join("config.json")
}

pub fn scheduler_state_file() -> PathBuf {
    data_dir().join("scheduler_state.json")
}

pub fn logs_dir() -> PathBuf {
    data_dir().join("logs")
}

pub fn log_file(month: &str) -> PathBuf {
    logs_dir().join(format!("checkin-{month}.jsonl"))
}

/// 确保目录存在。
pub fn ensure_dir(path: &std::path::Path) -> anyhow::Result<()> {
    if !path.exists() {
        std::fs::create_dir_all(path)?;
    }
    Ok(())
}

/// 原子写文件：先写临时文件再 rename，避免掉电写坏配置。
pub fn atomic_write(path: &std::path::Path, contents: &[u8]) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        ensure_dir(parent)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, contents)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}
