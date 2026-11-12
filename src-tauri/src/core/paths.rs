//! 数据目录布局。
//!
//! ```text
//! %APPDATA%\CheckinBox\
//!   ├── config.json            应用配置（调度时间窗等）
//!   ├── accounts.json          账号池（凭据经 DPAPI 加密后 base64）
//!   ├── scheduler_state.json   当日调度计划（每账号随机分钟偏移）
//!   └── logs\
//!       └── checkin-2026-10.jsonl
//! ```
//!
//! 旧名目录 `AgentCheckinTools` 在首次访问时整体复制到新目录，旧目录保留作备份。

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Once;

const DATA_DIR_NAME: &str = "CheckinBox";
const LEGACY_DIR_NAME: &str = "AgentCheckinTools";

static MIGRATE_LEGACY: Once = Once::new();

fn base_dir() -> PathBuf {
    dirs::data_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// 应用数据根目录。
///
/// 首次访问时若新目录不存在而旧目录存在，会先做一次性复制迁移。
/// 迁移失败时退回旧目录，避免用户看到“空账号池”。
pub fn data_dir() -> PathBuf {
    MIGRATE_LEGACY.call_once(|| {
        if let Err(err) = migrate_legacy_in(&base_dir()) {
            eprintln!("迁移旧数据目录失败：{err}");
        }
    });
    resolve_dir(&base_dir())
}

fn resolve_dir(base: &Path) -> PathBuf {
    let current = base.join(DATA_DIR_NAME);
    if current.exists() {
        return current;
    }
    let legacy = base.join(LEGACY_DIR_NAME);
    if legacy.is_dir() {
        return legacy;
    }
    current
}

/// 把旧目录整目录复制到新目录；新目录已存在时不做任何事（绝不覆盖）。
///
/// 先复制到临时目录再 rename，复制中途失败不会留下半份 `CheckinBox\`。
fn migrate_legacy_in(base: &Path) -> std::io::Result<()> {
    let legacy = base.join(LEGACY_DIR_NAME);
    let current = base.join(DATA_DIR_NAME);
    if !legacy.is_dir() || current.exists() {
        return Ok(());
    }
    let staging = base.join(format!("{DATA_DIR_NAME}.migrating-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&staging);
    if let Err(err) = copy_dir(&legacy, &staging) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(err);
    }
    match std::fs::rename(&staging, &current) {
        Ok(()) => Ok(()),
        Err(err) => {
            let _ = std::fs::remove_dir_all(&staging);
            // 另一个进程刚好抢先完成迁移时，目标已存在也算成功
            if current.exists() {
                Ok(())
            } else {
                Err(err)
            }
        }
    }
}

fn copy_dir(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let target = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
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

/// 待完成的设备授权会话（Qoder 用）。
///
/// 设备授权需要用户在浏览器里点一次确认，这期间应用可能被重启或前端被热更新。
/// 把会话落盘，启动时恢复，轮询才不会白等。
pub fn auth_pending_file() -> PathBuf {
    data_dir().join("auth_pending.json")
}

pub fn quota_history_file() -> PathBuf {
    data_dir().join("quota_history.jsonl")
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

/// 在同一目录完整写入临时文件，再原子替换目标文件。
pub fn atomic_write(path: &std::path::Path, contents: &[u8]) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    ensure_dir(parent)?;
    let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
    tmp.write_all(contents)?;
    tmp.as_file().sync_all()?;
    tmp.persist(path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_base(tag: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "checkinbox-paths-{tag}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn 旧目录存在且新目录不存在时整目录复制() {
        let base = temp_base("migrate");
        let legacy = base.join(LEGACY_DIR_NAME);
        let nested = legacy.join("chat_history").join("qoder").join("acc");
        std::fs::create_dir_all(legacy.join("logs")).unwrap();
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(legacy.join("accounts.json"), b"pool").unwrap();
        std::fs::write(legacy.join("logs").join("checkin-2026-10.jsonl"), b"log").unwrap();
        std::fs::write(nested.join("main.sqlite"), b"db").unwrap();

        migrate_legacy_in(&base).unwrap();

        let current = base.join(DATA_DIR_NAME);
        assert_eq!(
            std::fs::read(current.join("accounts.json")).unwrap(),
            b"pool"
        );
        assert_eq!(
            std::fs::read(current.join("logs").join("checkin-2026-10.jsonl")).unwrap(),
            b"log"
        );
        assert!(current
            .join("chat_history")
            .join("qoder")
            .join("acc")
            .join("main.sqlite")
            .is_file());
        assert!(
            legacy.join("accounts.json").is_file(),
            "旧目录应保留作备份，不能搬空"
        );
        let leftovers: Vec<_> = std::fs::read_dir(&base)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with(&format!("{DATA_DIR_NAME}.migrating"))
            })
            .collect();
        assert!(leftovers.is_empty(), "迁移完成后不应残留临时目录");
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn 新目录已存在时不覆盖() {
        let base = temp_base("keep-new");
        let legacy = base.join(LEGACY_DIR_NAME);
        let current = base.join(DATA_DIR_NAME);
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::create_dir_all(&current).unwrap();
        std::fs::write(legacy.join("accounts.json"), b"old").unwrap();
        std::fs::write(current.join("accounts.json"), b"new").unwrap();

        migrate_legacy_in(&base).unwrap();

        assert_eq!(
            std::fs::read(current.join("accounts.json")).unwrap(),
            b"new"
        );
        assert_eq!(std::fs::read(legacy.join("accounts.json")).unwrap(), b"old");
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn 没有旧目录时不创建新目录() {
        let base = temp_base("none");
        migrate_legacy_in(&base).unwrap();
        assert!(!base.join(DATA_DIR_NAME).exists());
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn 目录选择_新目录优先_迁移失败退回旧目录() {
        let base = temp_base("resolve");
        assert_eq!(
            resolve_dir(&base),
            base.join(DATA_DIR_NAME),
            "两者都不存在时用新目录"
        );
        std::fs::create_dir_all(base.join(LEGACY_DIR_NAME)).unwrap();
        assert_eq!(
            resolve_dir(&base),
            base.join(LEGACY_DIR_NAME),
            "旧目录还在（迁移失败）时应退回旧目录"
        );
        std::fs::create_dir_all(base.join(DATA_DIR_NAME)).unwrap();
        assert_eq!(
            resolve_dir(&base),
            base.join(DATA_DIR_NAME),
            "新目录存在时用新目录"
        );
        std::fs::remove_dir_all(&base).ok();
    }
}
