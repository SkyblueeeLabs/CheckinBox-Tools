//! 客户端登录态的**多代备份**与回滚。
//!
//! 旧实现（`client_switch::backup_file`）只在 `.bak` 不存在时才复制 ——
//! 备份里永远是「第一次换号之前」的那份，第二次换号之后现场就丢了、无法回滚。
//! 这里改成「一次换号 = 一代备份」，并且**成组**保存：
//! MiniMax 的 `auth.json` 与 `auth-state.json` 必须同代，缺一半回滚会让客户端
//! 状态机判定会话不可信（generation 对不上 → 客户端弹登录框）。
//!
//! 落点刻意放在**应用数据目录**而不是客户端数据目录：客户端目录扫描是
//! 「递归找任何名为 `auth.json` 的文件」，备份目录里若放同名文件会被当成
//! 候选登录态文件 → 选错文件 → 串号。
//!
//! 目录结构：
//! ```text
//! <应用数据目录>/
//! └── client_login_backups/
//!     └── <平台>/
//!         └── <操作id>/            一次换号 = 一个操作id = 一代备份
//!             ├── auth.json        备份里的文件名 = 原始文件名
//!             ├── auth-state.json
//!             └── meta.json        来源路径、时间、原因
//! ```

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

use crate::core::paths;

/// 默认保留代数（超出的删最旧）。
pub const DEFAULT_KEEP_GENERATIONS: usize = 5;

/// 一代备份的对外描述。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientLoginBackup {
    pub provider: String,
    /// 操作 id（= 备份目录名）
    pub op_id: String,
    pub created_at_ms: i64,
    /// 被备份文件的**原始路径**（回滚时按它写回）
    pub source_files: Vec<String>,
    /// 这一代占用的字节数
    pub bytes: u64,
    /// 备份原因：`switch`（换号）/ `manual`（手动）/ `pre-restore`（回滚前）
    pub reason: String,
    /// 这一代备份目录的路径
    pub dir: String,
}

/// 备份目录里的 `meta.json`。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackupMeta {
    provider: String,
    op_id: String,
    created_at_ms: i64,
    reason: String,
    /// (原始路径, 备份内的文件名)
    source_files: Vec<(String, String)>,
}

/// 某平台的备份根目录。
pub fn backup_root(provider: &str) -> PathBuf {
    paths::data_dir().join("client_login_backups").join(provider)
}

/// 平台名来自内部调用，但仍按**外部输入**校验：混进 `..` 或分隔符
/// 会把备份写到任意位置，必须先拒掉。
fn validate_segment(value: &str, what: &str) -> Result<()> {
    if value.is_empty() || value != value.trim() {
        return Err(anyhow!("{what} 为空或含首尾空白"));
    }
    if value.contains('/') || value.contains('\\') || value.contains("..") || value.contains('\0') {
        return Err(anyhow!("{what} 含非法字符：{value}"));
    }
    Ok(())
}

/// 操作 id：时间戳 + 进程号，与聊天记录备份同一套命名，目录互不覆盖。
fn operation_id() -> String {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("{ts}-{}", std::process::id())
}

/// 备份一次换号前的现场。
///
/// `files` = [(原始路径, 备份内的文件名)]。不存在的文件会被跳过（不报错），
/// 备份失败**不阻断换号** —— 换号本身不依赖备份成功。
pub fn backup_client_login(
    provider: &str,
    files: &[(PathBuf, String)],
    reason: &str,
) -> Result<ClientLoginBackup, String> {
    let root = backup_root(provider);
    backup_client_login_in(&root, provider, files, reason)
}

/// 同上，但根目录可指定（测试用，避免写进真实数据目录）。
pub fn backup_client_login_in(
    root: &Path,
    provider: &str,
    files: &[(PathBuf, String)],
    reason: &str,
) -> Result<ClientLoginBackup, String> {
    validate_segment(provider, "平台名").map_err(|e| e.to_string())?;
    let op_id = operation_id();
    let dir = root.join(&op_id);
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建备份目录失败：{e}"))?;

    let mut source_files = Vec::new();
    let mut bytes = 0u64;
    for (src, name) in files {
        if !src.is_file() {
            continue;
        }
        validate_segment(name, "备份文件名").map_err(|e| e.to_string())?;
        let dest = dir.join(name);
        std::fs::copy(src, &dest).map_err(|e| format!("备份 {} 失败：{e}", src.display()))?;
        bytes += std::fs::metadata(&dest).map(|m| m.len()).unwrap_or(0);
        source_files.push((src.to_string_lossy().to_string(), name.clone()));
    }

    let meta = BackupMeta {
        provider: provider.to_string(),
        op_id: op_id.clone(),
        created_at_ms: chrono::Utc::now().timestamp_millis(),
        reason: reason.to_string(),
        source_files,
    };
    let meta_json =
        serde_json::to_string_pretty(&meta).map_err(|e| format!("序列化备份信息失败：{e}"))?;
    std::fs::write(dir.join("meta.json"), meta_json)
        .map_err(|e| format!("写备份信息失败：{e}"))?;

    // 顺手裁剪：超出保留代数就删最旧的
    let _ = prune_client_login_backups_in(root, provider, DEFAULT_KEEP_GENERATIONS);

    Ok(ClientLoginBackup {
        provider: provider.to_string(),
        op_id,
        created_at_ms: meta.created_at_ms,
        source_files: meta.source_files.iter().map(|(p, _)| p.clone()).collect(),
        bytes,
        reason: reason.to_string(),
        dir: dir.to_string_lossy().to_string(),
    })
}

/// 列出某平台的全部备份代（新的在前）。
pub fn list_client_login_backups(provider: &str) -> Vec<ClientLoginBackup> {
    let root = backup_root(provider);
    list_client_login_backups_in(&root)
}

/// 同上，但根目录可指定（测试用）。
pub fn list_client_login_backups_in(root: &Path) -> Vec<ClientLoginBackup> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let meta_path = dir.join("meta.json");
        let Ok(raw) = std::fs::read_to_string(&meta_path) else {
            continue;
        };
        let Ok(meta) = serde_json::from_str::<BackupMeta>(&raw) else {
            continue;
        };
        let bytes = dir_size(&dir);
        out.push(ClientLoginBackup {
            provider: meta.provider,
            op_id: meta.op_id,
            created_at_ms: meta.created_at_ms,
            source_files: meta.source_files.iter().map(|(p, _)| p.clone()).collect(),
            bytes,
            reason: meta.reason,
            dir: dir.to_string_lossy().to_string(),
        });
    }
    out.sort_by(|a, b| b.created_at_ms.cmp(&a.created_at_ms).then(b.op_id.cmp(&a.op_id)));
    out
}

/// 回滚到指定的一代备份。
///
/// 回滚前会**先把当前现场再备份一代**（允许备份套备份），
/// 这样「回滚错了」也能再回来。
pub fn restore_client_login_backup(provider: &str, op_id: &str) -> Result<(), String> {
    let root = backup_root(provider);
    restore_client_login_backup_in(&root, provider, op_id)
}

/// 同上，但根目录可指定（测试用）。
pub fn restore_client_login_backup_in(
    root: &Path,
    provider: &str,
    op_id: &str,
) -> Result<(), String> {
    validate_segment(provider, "平台名").map_err(|e| e.to_string())?;
    validate_segment(op_id, "操作 id").map_err(|e| e.to_string())?;

    let dir = root.join(op_id);
    let meta_path = dir.join("meta.json");
    let raw =
        std::fs::read_to_string(&meta_path).map_err(|e| format!("找不到该代备份：{e}"))?;
    let meta: BackupMeta = serde_json::from_str(&raw).map_err(|e| format!("备份信息损坏：{e}"))?;

    // 先给当前现场存一代，避免回滚本身不可逆
    let live: Vec<(PathBuf, String)> = meta
        .source_files
        .iter()
        .map(|(p, name)| (PathBuf::from(p), name.clone()))
        .collect();
    let _ = backup_client_login_in(root, provider, &live, "pre-restore");

    for (src, name) in &meta.source_files {
        let stored = dir.join(name);
        if !stored.is_file() {
            continue;
        }
        let bytes = std::fs::read(&stored).map_err(|e| format!("读取备份文件失败：{e}"))?;
        crate::core::paths::atomic_write(Path::new(src), &bytes)
            .map_err(|e| format!("写回 {} 失败：{e}", src))?;
    }
    Ok(())
}

/// 只保留最近 `keep` 代，其余删掉。返回删除的代数。
pub fn prune_client_login_backups(provider: &str, keep: usize) -> Result<usize, String> {
    let root = backup_root(provider);
    prune_client_login_backups_in(&root, provider, keep)
}

/// 同上，但根目录可指定（测试用）。
pub fn prune_client_login_backups_in(
    root: &Path,
    provider: &str,
    keep: usize,
) -> Result<usize, String> {
    validate_segment(provider, "平台名").map_err(|e| e.to_string())?;
    let all = list_client_login_backups_in(root);
    if all.len() <= keep {
        return Ok(0);
    }
    let mut removed = 0usize;
    for backup in all.into_iter().skip(keep) {
        let dir = PathBuf::from(&backup.dir);
        // 只允许删备份根目录下的东西
        if dir.parent() != Some(root) {
            continue;
        }
        if std::fs::remove_dir_all(&dir).is_ok() {
            removed += 1;
        }
    }
    Ok(removed)
}

/// 删除指定的一代备份。
pub fn delete_client_login_backup(provider: &str, op_id: &str) -> Result<(), String> {
    let root = backup_root(provider);
    delete_client_login_backup_in(&root, provider, op_id)
}

/// 同上，但根目录可指定（测试用）。
pub fn delete_client_login_backup_in(
    root: &Path,
    provider: &str,
    op_id: &str,
) -> Result<(), String> {
    validate_segment(provider, "平台名").map_err(|e| e.to_string())?;
    validate_segment(op_id, "操作 id").map_err(|e| e.to_string())?;
    let dir = root.join(op_id);
    if dir.parent() != Some(root) {
        return Err("操作 id 非法".to_string());
    }
    std::fs::remove_dir_all(&dir).map_err(|e| format!("删除备份失败：{e}"))
}

/// 某平台所有代备份里的**登录态文件**（文件名与原始一致）。
///
/// 给「找回客户端真实用过的记录键」用：客户端中途把 `records` 清空时，
/// 备份里那份就是它之前真正用的键（2026-11-02 实测踩坑）。
pub fn stored_login_state_files(provider: &str) -> Vec<PathBuf> {
    let root = backup_root(provider);
    stored_login_state_files_in(&root)
}

/// 同上，但根目录可指定（测试用）。
pub fn stored_login_state_files_in(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let Ok(files) = std::fs::read_dir(&dir) else {
            continue;
        };
        for f in files.flatten() {
            let path = f.path();
            if path.is_file() && path.file_name().and_then(|n| n.to_str()) == Some("auth.json") {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

fn dir_size(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|e| e.path().is_file())
        .filter_map(|e| e.metadata().ok().map(|m| m.len()))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "checkinbox-backup-test-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn 连续备份生成多代且回滚能回到那一代() {
        let root = temp_root("multi");
        let live = root.join("live-auth.json");
        std::fs::write(&live, b"{\"v\":1}").unwrap();

        let b1 = backup_client_login_in(
            &root,
            "minimax",
            &[(live.clone(), "auth.json".into())],
            "switch",
        )
        .unwrap();
        std::fs::write(&live, b"{\"v\":2}").unwrap();
        let b2 = backup_client_login_in(
            &root,
            "minimax",
            &[(live.clone(), "auth.json".into())],
            "switch",
        )
        .unwrap();

        assert_ne!(b1.op_id, b2.op_id, "两代备份必须是不同的操作 id");
        let all = list_client_login_backups_in(&root);
        assert_eq!(all.len(), 2, "应有两代备份");

        // 回滚第 1 代 → 内容应回到 v1
        restore_client_login_backup_in(&root, "minimax", &b1.op_id).unwrap();
        assert_eq!(
            std::fs::read_to_string(&live).unwrap(),
            "{\"v\":1}",
            "回滚后应恢复第 1 代的内容"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn 裁剪只保留最近若干代() {
        let root = temp_root("prune");
        let live = root.join("live-auth.json");
        std::fs::write(&live, b"x").unwrap();
        for _ in 0..4 {
            let _ = backup_client_login_in(
                &root,
                "minimax",
                &[(live.clone(), "auth.json".into())],
                "switch",
            );
        }
        let removed = prune_client_login_backups_in(&root, "minimax", 2).unwrap();
        let all = list_client_login_backups_in(&root);
        assert_eq!(all.len(), 2, "裁剪后应只剩 2 代，实际 {}", all.len());
        assert!(removed >= 2, "应删除多余代，实际删了 {removed}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn 操作id含分隔符会被拒绝() {
        let root = temp_root("guard");
        assert!(restore_client_login_backup_in(&root, "minimax", "../evil").is_err());
        assert!(restore_client_login_backup_in(&root, "../evil", "op").is_err());
        assert!(prune_client_login_backups_in(&root, "minimax/../x", 1).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn 备份里的登录态文件能被找出来() {
        let root = temp_root("scan");
        let live = root.join("live-auth.json");
        std::fs::write(&live, b"{\"records\":{}}").unwrap();
        let _ = backup_client_login_in(
            &root,
            "minimax",
            &[(live.clone(), "auth.json".into())],
            "switch",
        );
        let found = stored_login_state_files_in(&root);
        assert_eq!(found.len(), 1, "应找到 1 份备份里的登录态文件");
        let _ = std::fs::remove_dir_all(&root);
    }
}
