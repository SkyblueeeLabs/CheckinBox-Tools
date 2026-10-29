//! 换号时同步客户端本地聊天记录（机制调研自上游 Cockpit Tools）。
//!
//! 上游的做法（`workbuddy_session_transfer.rs`）：WorkBuddy 的会话正文按 **UID**
//! 分目录存在 `%LOCALAPPDATA%\CodeBuddyExtension\Data\<uid>\<产品>\<uid>\history\…`，
//! 切号前把来源账号的会话**合并**进目标账号名下（同 id 比 lastMessageAt 新者胜，
//! 新 id 直接复制），再重映射会话库的 user_id；开关 `share_sessions_on_switch` 控制。
//!
//! 本项目的两个客户端形态不同，分别落地为：
//!
//! * **WorkBuddy（CodeBuddy IDE）**——与上游同构：会话正文按 uid 分目录，
//!   直接迁移上游的「目录级合并」算法（本机无 `workbuddy.db`，
//!   旧版 `codebuddy-sessions.vscdb` 是空库，数据库重映射做成无行则跳过的防御逻辑）。
//! * **Qoder CN**——Electron 应用，聊天库 `main.sqlite`（`chat_sessions` 表
//!   **没有 user_id 列**，会话是本机全局的，重映射不可行），改用**文件级换库**：
//!   换号前把当前聊天库归档给来源账号，再回填目标账号的归档；目标没有归档
//!   就清空（新账号从空白开始，与需求「未勾选保持空白」的直觉一致）。
//!
//! 触发时机与上游一致：**换号写凭据之前**（客户端已完全退出），见
//! `client_switch::switch`。MiniMax 的会话在服务端、客户端登录即自动拉取，
//! 不在本模块职责内（也永不开启此开关）。
//!
//! 安全边界（**每次同步都先备份，宁可不同步也不丢数据**）：
//!   * 动手前把涉及的现有文件**先备份**到
//!     `chat_history\<provider>\_backups\<操作id>\`（每次操作独立目录，
//!     不覆盖旧备份、不自动清理）；
//!   * Qoder 的现场库归档时还会另存一份到 `chat_history\qoder\<账号id>\payload\`，
//!     同一份历史至少同时存在「当次备份 + 账号归档」两份；
//!   * WorkBuddy **只读来源账号**，目标账号的会话只增不删；替换任何会话/索引前，
//!     先把原目录/原文件整个移进备份；
//!   * sqlite 必须**主库 + `-wal`/`-shm` 成组**搬，否则数据不一致；
//!   * 归档用 staging + rename（旧归档先改名让位再删），回填失败自动把现场移回，
//!     进程任何时刻被强杀，旧数据或新数据总有一份完整存在；
//!   * uid/账号 id 拼路径前做安全校验（单级路径，拒绝 `..`、分隔符、符号链接）；
//!   * 客户端必须在运行检测不通过（`launcher::is_running`）时拒绝执行。

use anyhow::{anyhow, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};

use super::paths::{data_dir, ensure_dir};

/// 一次聊天记录同步的结果（给前端 toast 和日志）。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ChatSyncReport {
    /// swapped（Qoder 换库）/ merged（WorkBuddy 合并）/ cleared（目标无归档，清空现场）/ skipped
    pub action: String,
    /// 人话说明，直接可展示
    pub detail: String,
    /// 搬动/合并的文件数
    pub files: usize,
    /// 涉及的字节数
    pub bytes: u64,
    /// WorkBuddy 合并：新带入的会话数
    pub added: usize,
    /// WorkBuddy 合并：以新版本替换的会话数
    pub replaced: usize,
}

/// 这个平台的客户端要不要（能不能）做聊天记录同步。
///
/// 只有「聊天记录存本地」且工具有换号能力的平台才有意义：
/// MiniMax 的会话在服务端自动跟随账号，不需要也不提供此开关。
pub fn supports(provider: &str) -> bool {
    matches!(provider, "workbuddy" | "qoder")
}

/// 换号入口：把聊天记录从 `from` 账号带到 `to` 账号名下。
///
/// `from` 为 `None`（客户端当前登录识别失败）时 Qoder 跳过（无法归属的
/// 历史不能乱动），WorkBuddy 同样跳过 —— 宁可不同步，不能弄丢。
/// 调用方保证客户端已完全退出；这里再做一次进程防御。
pub fn sync_on_switch(provider: &str, from: Option<&str>, to: &str) -> Result<ChatSyncReport> {
    if !supports(provider) {
        return Ok(skipped("该平台不需要/不支持聊天记录同步"));
    }
    if super::launcher::is_running(provider) {
        return Err(anyhow!("客户端正在运行，不能同步聊天记录"));
    }
    let Some(from) = from.filter(|f| !f.is_empty()) else {
        return Ok(skipped("没识别出客户端当前登录的账号，聊天记录保持原样"));
    };
    if from == to {
        return Ok(skipped("目标账号与当前登录账号相同，无需同步"));
    }

    let op = operation_id();
    match provider {
        "qoder" => swap_qoder_chat(&qoder_user_data()?, &qoder_archive_root(), from, to, &op),
        "workbuddy" => {
            let Some(root) = workbuddy_extension_data_dir() else {
                return Ok(skipped("本机没有 CodeBuddy 会话数据目录"));
            };
            let backup = backup_root("workbuddy", &op);
            ensure_dir(&backup)?;
            merge_uid_history(&root, from, to, &backup)
        }
        _ => Ok(skipped("未知平台")),
    }
}

fn skipped(reason: &str) -> ChatSyncReport {
    ChatSyncReport {
        action: "skipped".into(),
        detail: reason.to_string(),
        ..Default::default()
    }
}

/// 操作 id：时间戳 + 进程号，备份目录互不覆盖。
fn operation_id() -> String {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("{ts}-{}", std::process::id())
}

/// 账号 id 拼进路径前校验：只允许单级普通路径段。
///
/// 账号 id 来自客户端文件/上游响应，属于**外部输入**；混进 `..` 或分隔符
/// 会把归档写到任意位置，必须先拒掉。
fn validate_path_segment(id: &str) -> Result<()> {
    if id.is_empty() || id != id.trim() {
        return Err(anyhow!("账号 id 为空或含首尾空白"));
    }
    if id.contains('/') || id.contains('\\') || id.contains("..") || id.contains('\0') {
        return Err(anyhow!("账号 id 含不安全的路径字符"));
    }
    match Path::new(id).components().next() {
        Some(std::path::Component::Normal(_)) => Ok(()),
        _ => Err(anyhow!("账号 id 不是安全的单级路径")),
    }
}

/// 存在即拒绝的符号链接检查（上游同款防护：防目录被链到别处被误写）。
fn reject_symlink(path: &Path) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            Err(anyhow!("拒绝通过符号链接读写：{}", path.display()))
        }
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(anyhow!("读取路径属性失败 {}: {e}", path.display())),
    }
}

/// 递归复制目录（跳过符号链接），返回（文件数, 字节数）。
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(usize, u64)> {
    ensure_dir(dst)?;
    let mut files = 0usize;
    let mut bytes = 0u64;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let meta = std::fs::symlink_metadata(&path)?;
        if meta.file_type().is_symlink() {
            continue;
        }
        let target = dst.join(entry.file_name());
        if meta.is_dir() {
            let (f, b) = copy_dir_recursive(&path, &target)?;
            files += f;
            bytes += b;
        } else if meta.is_file() {
            std::fs::copy(&path, &target)?;
            files += 1;
            bytes += meta.len();
        }
    }
    Ok((files, bytes))
}

/// 删除目录（不存在不算错）。
fn remove_dir_all_quiet(path: &Path) {
    let _ = std::fs::remove_dir_all(path);
}

/// 把现场聊天库文件移进 trash（同盘 rename 优先，失败退回复制+删除）。
fn move_live_to_trash(_root: &Path, live: &[Vec<PathBuf>], trash: &Path) {
    for group in live {
        for src in group.iter().filter(|p| p.is_file()) {
            let name = src.file_name().unwrap();
            let dst = trash.join(name);
            let moved = std::fs::rename(src, &dst)
                .or_else(|_| std::fs::copy(src, &dst).and_then(|_| std::fs::remove_file(src)));
            if moved.is_err() {
                // 移不走就留着：宁可同步不完整，不能删用户的聊天库
            }
        }
    }
}

/// 回填失败时把 trash 里的文件尽量移回现场。
fn restore_from_trash(root: &Path, trash: &Path) {
    if let Ok(entries) = std::fs::read_dir(trash) {
        for entry in entries.flatten() {
            let _ = std::fs::rename(entry.path(), root.join(entry.file_name()));
        }
    }
}

/// 从归档 payload 回填聊天库到 userData 根（主文件 + sidecar 成组）。
fn fill_from_payload(
    root: &Path,
    payload: &Path,
    files: &mut usize,
    bytes: &mut u64,
) -> Result<()> {
    for f in QODER_CHAT_FILES {
        let src = payload.join(f);
        if src.is_file() {
            std::fs::copy(&src, root.join(f))?;
            *files += 1;
            *bytes += std::fs::metadata(&src).map(|m| m.len()).unwrap_or(0);
        }
        for side in [format!("{f}-wal"), format!("{f}-shm")] {
            let side_src = payload.join(&side);
            if side_src.is_file() {
                std::fs::copy(&side_src, root.join(&side))?;
            } else {
                let _ = std::fs::remove_file(root.join(&side));
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Qoder CN：文件级换库
// ---------------------------------------------------------------------------

/// Qoder 的聊天库（相对 userData 根）。会话正文/消息全在 `main.sqlite`，
/// 另两个是聊天相关的配套库；一律**主文件 + `-wal`/`-shm` 成组**处理。
/// 不搬迁移台账（sessionMigration/memoryMigration，设备级状态）与
/// `qoder-data.v1.json`（agent 运行时配置，非聊天，且含 deviceId）。
const QODER_CHAT_FILES: &[&str] = &[
    "main.sqlite",
    "chat-session-turn-payload-buffer.sqlite",
    "collaboration-message-cache.sqlite",
];

fn qoder_user_data() -> Result<PathBuf> {
    let appdata = std::env::var_os("APPDATA").ok_or_else(|| anyhow!("没有 APPDATA 环境变量"))?;
    let dir = PathBuf::from(appdata).join("com.qodercn.app.stable");
    dir.is_dir()
        .then_some(dir)
        .ok_or_else(|| anyhow!("没有找到 Qoder CN 客户端的数据目录"))
}

fn qoder_archive_root() -> PathBuf {
    data_dir().join("chat_history").join("qoder")
}

/// 一个聊天库文件的「成组」路径：主文件 + -wal + -shm。
fn sqlite_group(root: &Path, name: &str) -> Vec<PathBuf> {
    [name.to_string(), format!("{name}-wal"), format!("{name}-shm")]
        .iter()
        .map(|n| root.join(n))
        .collect()
}

/// Qoder 换号：把当前聊天库归档给 `from`，再回填 `to` 的归档（没有就清空）。
///
/// 布局：`chat_history\qoder\<账号id>\payload\`（与 userData 根同构）。
/// 所有步骤先备份、staging 后 rename，任何一步失败都保留原状并如实报错。
fn swap_qoder_chat(
    root: &Path,
    arc_root: &Path,
    from: &str,
    to: &str,
    op: &str,
) -> Result<ChatSyncReport> {
    validate_path_segment(from)?;
    validate_path_segment(to)?;
    if !root.is_dir() {
        return Ok(skipped("Qoder 数据目录不存在"));
    }
    for f in QODER_CHAT_FILES {
        reject_symlink(&root.join(f))?;
    }

    // 现场上有哪些库文件（主文件存在才算一组）
    let live: Vec<Vec<PathBuf>> = QODER_CHAT_FILES
        .iter()
        .map(|f| sqlite_group(root, f))
        .filter(|group| group[0].is_file())
        .collect();
    let live_bytes: u64 = live
        .iter()
        .flatten()
        .filter_map(|p| std::fs::metadata(p).ok())
        .map(|m| m.len())
        .sum();

    let backup_dir = arc_root.join("_backups").join(op);
    let trash = arc_root.join(format!(".trash-{op}"));

    // 1) 备份现场（保命：后面任何一步出错，用户可以从这里恢复）
    if !live.is_empty() {
        ensure_dir(&backup_dir)?;
        for group in &live {
            for src in group.iter().filter(|p| p.is_file()) {
                std::fs::copy(src, backup_dir.join(src.file_name().unwrap()))?;
            }
        }
    }

    // 2) 归档当前聊天库给来源账号
    //    先完整写到 staging，再把旧归档改名让位、staging 顶上、最后删旧的 ——
    //    任何时刻进程被强杀，旧归档或新归档总有一份完整存在（不会「先删后改名」留空窗）。
    let mut files = 0usize;
    let mut bytes = 0u64;
    if !live.is_empty() {
        let staging = arc_root.join(from).join(format!(".staging-{op}"));
        remove_dir_all_quiet(&staging);
        ensure_dir(&staging)?;
        for group in &live {
            for src in group.iter().filter(|p| p.is_file()) {
                std::fs::copy(src, staging.join(src.file_name().unwrap()))?;
            }
        }
        let payload = arc_root.join(from).join("payload");
        let old = arc_root.join(from).join(format!(".old-{op}"));
        remove_dir_all_quiet(&old);
        if payload.exists() {
            std::fs::rename(&payload, &old)?;
        }
        std::fs::rename(&staging, &payload)?;
        remove_dir_all_quiet(&old);
        files += live.len();
        bytes += live_bytes;
    }

    // 3) 回填目标账号的归档；没有就清空现场（新账号从空白开始）
    let target_payload = arc_root.join(to).join("payload");
    let has_target = QODER_CHAT_FILES
        .iter()
        .any(|f| target_payload.join(f).is_file());
    if !has_target && live.is_empty() {
        return Ok(skipped("客户端没有本地聊天记录，目标账号也没有归档，无需同步"));
    }

    ensure_dir(arc_root)?;
    ensure_dir(&trash)?;
    let mut action = "cleared";
    if has_target {
        // 现场文件移进 trash（同盘 rename，快且原子），回填失败再移回来
        move_live_to_trash(root, &live, &trash);
        let fill = fill_from_payload(root, &target_payload, &mut files, &mut bytes);
        if let Err(e) = fill {
            restore_from_trash(root, &trash);
            remove_dir_all_quiet(&trash);
            return Err(e);
        }
        action = "swapped";
    } else if !live.is_empty() {
        move_live_to_trash(root, &live, &trash);
    }

    remove_dir_all_quiet(&trash);

    Ok(match action {
        "swapped" => ChatSyncReport {
            action: action.into(),
            detail: format!(
                "已把 {from} 的聊天记录归档，并回填 {to} 的历史会话（原库已备份）"
            ),
            files,
            bytes,
            ..Default::default()
        },
        _ => ChatSyncReport {
            action: "cleared".into(),
            detail: format!(
                "已归档 {from} 的聊天记录；{to} 没有历史归档，客户端将以空白会话启动（原库已备份）"
            ),
            files,
            bytes,
            ..Default::default()
        },
    })
}

// ---------------------------------------------------------------------------
// 备份管理（设置页「历史备份」tab）
// ---------------------------------------------------------------------------

/// 一条换号备份的清单项。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatBackupEntry {
    /// workbuddy / qoder
    pub provider: String,
    /// 操作 id（`<毫秒时间戳>-<进程号>`）
    pub op_id: String,
    /// 备份时间（毫秒时间戳；解析失败为 0）
    pub created_at_ms: i64,
    /// 文件数
    pub files: usize,
    /// 占用字节数
    pub bytes: u64,
    /// 顶层内容（给用户看备份了什么：`main.sqlite` / `history` 等）
    pub items: Vec<String>,
}

/// 备份根目录：`<数据目录>\chat_history`。
///
/// 布局：
/// ```text
/// chat_history\
///   ├── qoder\
///   │   ├── <账号id>\payload\      账号聊天库归档（同步功能使用，不在本列表里）
///   │   └── _backups\<操作id>\     每次换号前的现场备份（可删）
///   └── workbuddy\
///       └── _backups\<操作id>\     被替换的会话/索引备份（可删）
/// ```
pub fn chat_history_root() -> PathBuf {
    data_dir().join("chat_history")
}

/// 列出所有平台的换号备份（按时间倒序）。
///
/// 只列 `_backups\<操作id>\`，**不列**账号归档（`<账号id>\payload`）——
/// 后者是同步功能本身的数据源，删了对应账号的历史就回不来了。
pub fn list_backups() -> Vec<ChatBackupEntry> {
    list_backups_in(&chat_history_root())
}

fn list_backups_in(root: &Path) -> Vec<ChatBackupEntry> {
    let mut out = Vec::new();
    for provider in ["workbuddy", "qoder"] {
        let dir = root.join(provider).join("_backups");
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            // 跳过 staging/trash 这类临时目录
            if name.starts_with('.') {
                continue;
            }
            let (files, bytes) = dir_stats(&path);
            let mut items: Vec<String> = std::fs::read_dir(&path)
                .map(|it| {
                    it.flatten()
                        .map(|e| e.file_name().to_string_lossy().to_string())
                        .collect()
                })
                .unwrap_or_default();
            items.sort();
            out.push(ChatBackupEntry {
                provider: provider.to_string(),
                created_at_ms: op_timestamp_ms(&name),
                op_id: name,
                files,
                bytes,
                items,
            });
        }
    }
    out.sort_by(|a, b| b.created_at_ms.cmp(&a.created_at_ms));
    out
}

/// 操作 id 里的毫秒时间戳（`<ms>-<pid>`）。解析不出来返回 0（排序靠后）。
fn op_timestamp_ms(op_id: &str) -> i64 {
    op_id
        .split('-')
        .next()
        .and_then(|s| s.parse::<i64>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(0)
}

/// 递归统计目录的（文件数, 字节数）。
fn dir_stats(dir: &Path) -> (usize, u64) {
    let mut files = 0usize;
    let mut bytes = 0u64;
    let Ok(entries) = std::fs::read_dir(dir) else {
        return (0, 0);
    };
    for entry in entries.flatten() {
        let Ok(meta) = std::fs::symlink_metadata(entry.path()) else {
            continue;
        };
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            let (f, b) = dir_stats(&entry.path());
            files += f;
            bytes += b;
        } else if meta.is_file() {
            files += 1;
            bytes += meta.len();
        }
    }
    (files, bytes)
}

/// 删除一条备份（只允许删 `_backups` 下的目录）。
pub fn delete_backup(provider: &str, op_id: &str) -> Result<()> {
    delete_backup_in(&chat_history_root(), provider, op_id)
}

fn delete_backup_in(root: &Path, provider: &str, op_id: &str) -> Result<()> {
    if !matches!(provider, "workbuddy" | "qoder") {
        return Err(anyhow!("未知平台：{provider}"));
    }
    // 外部输入拼路径前必须校验：只允许单级普通路径段（拒绝 .. / 分隔符）
    validate_path_segment(op_id)?;
    if op_id.starts_with('.') {
        return Err(anyhow!("非法的备份目录名"));
    }
    let target = root.join(provider).join("_backups").join(op_id);
    reject_symlink(&target)?;
    if !target.is_dir() {
        return Err(anyhow!("备份不存在：{provider}/{op_id}"));
    }
    std::fs::remove_dir_all(&target)?;
    Ok(())
}

/// 删除早于 `days` 天的备份，返回删除条数。时间解析不出来的条目**不删**
///（宁可留着让用户手动处理，也不猜）。
pub fn delete_backups_older_than(days: u32) -> Result<usize> {
    let cutoff = chrono::Utc::now().timestamp_millis() - (days as i64) * 24 * 3600 * 1000;
    let mut removed = 0usize;
    for entry in list_backups() {
        if entry.created_at_ms > 0 && entry.created_at_ms < cutoff {
            if delete_backup(&entry.provider, &entry.op_id).is_ok() {
                removed += 1;
            }
        }
    }
    Ok(removed)
}

// ---------------------------------------------------------------------------
// WorkBuddy（CodeBuddy IDE）：按 uid 合并会话目录
// ---------------------------------------------------------------------------

/// 会话正文根目录：WorkBuddy 5.x 用 `WorkBuddyExtension\Data`，
/// CodeBuddy（本机实测）用 `CodeBuddyExtension\Data`，存在哪个用哪个。
fn workbuddy_extension_data_dir() -> Option<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")?;
    for name in ["WorkBuddyExtension", "CodeBuddyExtension"] {
        let dir = PathBuf::from(&base).join(name).join("Data");
        if dir.is_dir() {
            return Some(dir);
        }
    }
    None
}

fn backup_root(provider: &str, op: &str) -> PathBuf {
    data_dir()
        .join("chat_history")
        .join(provider)
        .join("_backups")
        .join(op)
}

/// 把来源账号 uid 的本地会话合并进目标账号 uid 名下（上游同款算法）。
///
/// 目录结构（本机实测）：`Data\<uid>\<产品>\<uid>\history\<工作区hash>\
/// index.json + <会话id>\`，另有 `check-point\<工作区>\<会话id>\` 辅助目录。
/// 合并规则：会话 id 相同比 `lastMessageAt` **新者胜**（替换），目标缺失的
/// 直接复制（added）；目标已有且不旧于来源的保留不动。
fn merge_uid_history(root: &Path, from: &str, to: &str, backup_root: &Path) -> Result<ChatSyncReport> {
    validate_path_segment(from)?;
    validate_path_segment(to)?;
    let source_outer = root.join(from);
    if !source_outer.is_dir() {
        return Ok(skipped("来源账号没有本地会话目录"));
    }
    reject_symlink(&source_outer)?;
    reject_symlink(&root.join(to))?;

    let mut report = ChatSyncReport {
        action: "merged".into(),
        ..Default::default()
    };
    let mut merged_any = false;

    // 来源账号下的每个「产品」目录（CodeBuddyIDE 等）
    for ide in std::fs::read_dir(&source_outer)? {
        let ide = ide?;
        let ide_meta = std::fs::symlink_metadata(ide.path())?;
        if !ide_meta.is_dir() || ide_meta.file_type().is_symlink() {
            continue;
        }
        let ide_name = ide.file_name();
        let source_account = source_outer.join(&ide_name).join(from);
        let source_history = source_account.join("history");
        if !source_history.is_dir() {
            continue;
        }
        let target_account = root.join(to).join(&ide_name).join(to);
        let target_history = target_account.join("history");
        reject_symlink(&target_account)?;

        for ws in std::fs::read_dir(&source_history)? {
            let ws = ws?;
            let ws_meta = std::fs::symlink_metadata(ws.path())?;
            if !ws_meta.is_dir() || ws_meta.file_type().is_symlink() {
                continue;
            }
            let ws_name = ws.file_name();
            merge_workspace(
                &ws.path(),
                &target_history.join(&ws_name),
                &source_account,
                &target_account,
                &ws_name.to_string_lossy(),
                backup_root,
                &mut report,
            )?;
            merged_any = true;
        }
    }

    if !merged_any {
        return Ok(skipped("来源账号没有可合并的本地会话"));
    }
    report.detail = format!(
        "已把 {} 的本地会话合并进 {}：新增 {} 条、更新 {} 条（被替换的会话已备份）",
        from, to, report.added, report.replaced
    );
    Ok(report)
}

/// 合并一个工作区的会话（index.json 按会话 id 去重合并 + 会话目录/检查点复制）。
fn merge_workspace(
    source_ws: &Path,
    target_ws: &Path,
    source_account: &Path,
    target_account: &Path,
    ws_name: &str,
    backup_root: &Path,
    report: &mut ChatSyncReport,
) -> Result<()> {
    let source_index_path = source_ws.join("index.json");
    let target_index_path = target_ws.join("index.json");

    // 来源没有 index.json 就没有可合并的元数据，保守跳过（不动未知目录）
    if !source_index_path.is_file() {
        return Ok(());
    }
    let source_index = read_json(&source_index_path)?;
    let target_index = if target_index_path.is_file() {
        read_json(&target_index_path)?
    } else {
        serde_json::Value::Null
    };

    let source_convs = conversations(&source_index);
    if source_convs.is_empty() {
        return Ok(());
    }

    let mut target_convs = conversations(&target_index);
    let had_target = target_index_path.is_file();
    ensure_dir(target_ws)?;

    for conv in &source_convs {
        let Some(id) = conv["id"].as_str() else {
            continue;
        };
        validate_path_segment(id)?;
        let existing = target_convs
            .iter()
            .position(|c| c["id"].as_str() == Some(id));
        match existing {
            None => {
                // 新会话：复制会话目录 + 检查点。
                // 正常情况下目标目录不存在；万一存在（索引缺行等异常），
                // 也走「先备份再覆盖」，绝不无备份删除用户数据。
                let src_dir = source_ws.join(id);
                if src_dir.is_dir() {
                    let rel = format!("history/{ws_name}/{id}");
                    backup_then_copy_dir(&src_dir, &target_ws.join(id), backup_root, Some(&rel))?;
                }
                copy_checkpoint(source_account, target_account, ws_name, id, backup_root)?;
                target_convs.push(conv.clone());
                report.added += 1;
                report.files += 1;
            }
            Some(pos) => {
                // 同 id：来源更新才替换
                if conversation_is_newer(conv, &target_convs[pos]) {
                    let src_dir = source_ws.join(id);
                    if src_dir.is_dir() {
                        let rel = format!("history/{ws_name}/{id}");
                        backup_then_copy_dir(&src_dir, &target_ws.join(id), backup_root, Some(&rel))?;
                        copy_checkpoint(source_account, target_account, ws_name, id, backup_root)?;
                        target_convs[pos] = conv.clone();
                        report.replaced += 1;
                        report.files += 1;
                    }
                }
            }
        }
    }

    // 合并后的 index.json：保留目标原有的 current（用户正在看的会话不跳）
    let current = target_index["current"]
        .as_str()
        .map(|s| serde_json::json!(s))
        .unwrap_or_else(|| source_index["current"].clone());
    let merged = serde_json::json!({
        "conversations": target_convs,
        "current": if had_target { current } else { source_index["current"].clone() },
    });
    write_json_backup_then(&target_index_path, &merged, backup_root, &format!("history/{ws_name}/index.json"))?;
    Ok(())
}

/// 复制 check-point 辅助目录（存在才复制，缺失的补上）。
fn copy_checkpoint(
    source_account: &Path,
    target_account: &Path,
    ws_name: &str,
    conv_id: &str,
    backup_root: &Path,
) -> Result<()> {
    let src = source_account.join("check-point").join(ws_name).join(conv_id);
    let dst = target_account.join("check-point").join(ws_name).join(conv_id);
    if src.is_dir() && !dst.exists() {
        backup_then_copy_dir(&src, &dst, backup_root, None)?;
    }
    Ok(())
}

/// 目录复制，带可选备份：`rel` 给出被覆盖项的相对路径，覆盖前先移进备份目录。
fn backup_then_copy_dir(src: &Path, dst: &Path, backup_root: &Path, rel: Option<&str>) -> Result<()> {
    if dst.exists() {
        if let Some(rel) = rel {
            let bak = backup_root.join(rel);
            ensure_dir(bak.parent().unwrap_or(backup_root))?;
            remove_dir_all_quiet(&bak);
            std::fs::rename(dst, &bak).or_else(|_| copy_dir_recursive(dst, &bak).map(|_| ()))?;
            remove_dir_all_quiet(dst);
        } else {
            remove_dir_all_quiet(dst);
        }
    }
    copy_dir_recursive(src, dst)?;
    Ok(())
}

fn read_json(path: &Path) -> Result<serde_json::Value> {
    let raw = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&raw).unwrap_or(serde_json::Value::Null))
}

/// 原子写 index.json，原文件先备份。
fn write_json_backup_then(path: &Path, value: &serde_json::Value, backup_root: &Path, rel: &str) -> Result<()> {
    if path.is_file() {
        let bak = backup_root.join(rel);
        ensure_dir(bak.parent().unwrap_or(backup_root))?;
        std::fs::copy(path, &bak)?;
    }
    let text = serde_json::to_string_pretty(value)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn conversations(index: &serde_json::Value) -> Vec<serde_json::Value> {
    index["conversations"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

/// 来源会话是否比目标的新（lastMessageAt，RFC3339；解析失败当 0）。
fn conversation_is_newer(source: &serde_json::Value, target: &serde_json::Value) -> bool {
    timestamp_of(source) > timestamp_of(target)
}

fn timestamp_of(conv: &serde_json::Value) -> i64 {
    for key in ["lastMessageAt", "updatedAt", "createdAt"] {
        match conv.get(key) {
            // RFC3339 字符串（本机实测格式）
            Some(serde_json::Value::String(s)) => {
                if let Ok(t) = chrono::DateTime::parse_from_rfc3339(s) {
                    return t.timestamp_millis();
                }
                if let Ok(n) = s.parse::<i64>() {
                    return n;
                }
            }
            // 兼容直接存数字（秒/毫秒）的版本
            Some(serde_json::Value::Number(n)) => {
                if let Some(ms) = n.as_i64() {
                    // 10 位按秒，13 位按毫秒
                    return if ms > 0 && ms < 1_000_000_000_000 { ms * 1000 } else { ms };
                }
            }
            _ => {}
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);
    impl TempDir {
        fn new(tag: &str) -> Self {
            let id = operation_id();
            let path = std::env::temp_dir().join(format!("checkinbox-chat-sync-{tag}-{}-{id}", std::process::id()));
            remove_dir_all_quiet(&path);
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            remove_dir_all_quiet(&self.0);
        }
    }

    fn write_file(path: &Path, content: &[u8]) {
        ensure_dir(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    #[test]
    fn 账号id路径校验() {
        assert!(validate_path_segment("384c6dd0-uid").is_ok());
        assert!(validate_path_segment("").is_err());
        assert!(validate_path_segment("../x").is_err());
        assert!(validate_path_segment("a/b").is_err());
        assert!(validate_path_segment("a\\b").is_err());
        assert!(validate_path_segment("a..b").is_err());
        assert!(validate_path_segment(" a").is_err());
    }

    #[test]
    fn qoder换库_归档并回填() {
        let tmp = TempDir::new("qoder-swap");
        let root = tmp.0.join("com.qodercn.app.stable");
        let arc = tmp.0.join("chat_history").join("qoder");
        ensure_dir(&root).unwrap();
        write_file(&root.join("main.sqlite"), b"chat-A-v1");

        // 第一次：from=A -> B，B 没有归档 => 清空 + 归档 A
        let report = swap_qoder_chat(&root, &arc, "accA", "accB", "op1").unwrap();
        assert_eq!(report.action, "cleared");
        assert!(!root.join("main.sqlite").exists(), "现场应被清空");
        assert!(arc.join("accA").join("payload").join("main.sqlite").is_file());
        // 保底备份：动现场之前的原文件必须留在 _backups\<op>\ 里
        let bak = arc.join("_backups").join("op1").join("main.sqlite");
        assert!(bak.is_file(), "同步前必须先备份原聊天库");
        assert_eq!(std::fs::read(&bak).unwrap(), b"chat-A-v1");

        // A 又聊了点东西，切回 B -> A：A 的归档回填
        write_file(&root.join("main.sqlite"), b"chat-B-v1");
        let report = swap_qoder_chat(&root, &arc, "accB", "accA", "op2").unwrap();
        assert_eq!(report.action, "swapped");
        assert_eq!(
            std::fs::read(root.join("main.sqlite")).unwrap(),
            b"chat-A-v1",
            "应回填 A 的归档"
        );
        assert!(arc.join("accB").join("payload").join("main.sqlite").is_file());
    }

    #[test]
    fn qoder换库_wal_sidecar成组搬() {
        let tmp = TempDir::new("qoder-wal");
        let root = tmp.0.join("data");
        let arc = tmp.0.join("arc");
        ensure_dir(&root).unwrap();
        write_file(&root.join("main.sqlite"), b"db");
        write_file(&root.join("main.sqlite-wal"), b"wal");
        write_file(&root.join("main.sqlite-shm"), b"shm");

        swap_qoder_chat(&root, &arc, "u1", "u2", "op").unwrap();
        let payload = arc.join("u1").join("payload");
        assert!(payload.join("main.sqlite").is_file());
        assert!(payload.join("main.sqlite-wal").is_file());
        assert!(payload.join("main.sqlite-shm").is_file());
        assert!(!root.join("main.sqlite").exists());
    }

    #[test]
    fn workbuddy合并_新会话带入_旧会话保留() {
        let tmp = TempDir::new("wb-merge");
        let root = tmp.0.join("CodeBuddyExtension").join("Data");
        let backup = tmp.0.join("backup");
        let src = root.join("uidA").join("CodeBuddyIDE").join("uidA");
        let tgt = root.join("uidB").join("CodeBuddyIDE").join("uidB");

        // 来源账号：一个工作区两条会话
        let src_ws = src.join("history").join("ws1");
        write_file(
            &src_ws.join("index.json"),
            br#"{"conversations":[
                {"id":"conv1","createdAt":"2026-10-01T00:00:00Z","lastMessageAt":"2026-10-02T00:00:00Z"},
                {"id":"conv2","createdAt":"2026-10-03T00:00:00Z","lastMessageAt":"2026-10-04T00:00:00Z"}],
                "current":"conv2"}"#,
        );
        write_file(&src_ws.join("conv1").join("messages.json"), b"src-conv1");
        write_file(&src_ws.join("conv2").join("messages.json"), b"src-conv2");
        write_file(&src.join("check-point").join("ws1").join("conv2").join("meta.json"), b"cp");

        // 目标账号：已有自己的 conv2（更旧）
        let tgt_ws = tgt.join("history").join("ws1");
        write_file(
            &tgt_ws.join("index.json"),
            br#"{"conversations":[
                {"id":"conv2","createdAt":"2026-10-01T00:00:00Z","lastMessageAt":"2026-10-01T00:00:00Z"}],
                "current":"conv2"}"#,
        );
        write_file(&tgt_ws.join("conv2").join("messages.json"), b"tgt-conv2-old");

        let report = merge_uid_history(&root, "uidA", "uidB", &backup).unwrap();
        assert_eq!(report.action, "merged");
        assert_eq!(report.added, 1, "conv1 应作为新会话带入");
        assert_eq!(report.replaced, 1, "conv2 来源更新应替换");

        // conv1 带入
        assert_eq!(
            std::fs::read(tgt_ws.join("conv1").join("messages.json")).unwrap(),
            b"src-conv1"
        );
        // conv2 被新版本替换
        assert_eq!(
            std::fs::read(tgt_ws.join("conv2").join("messages.json")).unwrap(),
            b"src-conv2"
        );
        // check-point 跟着过来
        assert!(tgt.join("check-point").join("ws1").join("conv2").join("meta.json").is_file());
        // 合并后的 index：两条会话，current 保留目标的
        let merged = read_json(&tgt_ws.join("index.json")).unwrap();
        assert_eq!(merged["conversations"].as_array().unwrap().len(), 2);
        assert_eq!(merged["current"], "conv2");
        // 被替换的目标会话有备份
        assert!(backup.join("history").join("ws1").join("conv2").join("messages.json").is_file());
    }

    #[test]
    fn workbuddy合并_目标较新则不动() {
        let tmp = TempDir::new("wb-keep");
        let root = tmp.0.join("Data");
        let src = root.join("uidA").join("IDE").join("uidA").join("history").join("ws");
        let tgt = root.join("uidB").join("IDE").join("uidB").join("history").join("ws");
        write_file(
            &src.join("index.json"),
            br#"{"conversations":[{"id":"c1","lastMessageAt":"2026-10-01T00:00:00Z"}]}"#,
        );
        write_file(&src.join("c1").join("m.json"), b"src");
        write_file(
            &tgt.join("index.json"),
            br#"{"conversations":[{"id":"c1","lastMessageAt":"2026-10-05T00:00:00Z"}]}"#,
        );
        write_file(&tgt.join("c1").join("m.json"), b"tgt");

        let report = merge_uid_history(&root, "uidA", "uidB", &tmp.0.join("bk")).unwrap();
        assert_eq!(report.replaced, 0, "目标更新时不应替换");
        assert_eq!(std::fs::read(tgt.join("c1").join("m.json")).unwrap(), b"tgt");
    }

    #[test]
    fn workbuddy_来源无目录时跳过() {
        let tmp = TempDir::new("wb-skip");
        let root = tmp.0.join("Data");
        ensure_dir(&root).unwrap();
        let report = merge_uid_history(&root, "ghost", "uidB", &tmp.0.join("bk")).unwrap();
        assert_eq!(report.action, "skipped");
    }

    #[test]
    fn 时间戳解析_字符串与数字() {
        let a = serde_json::json!({"lastMessageAt": "2026-10-02T00:00:00Z"});
        let b = serde_json::json!({"lastMessageAt": "2026-10-01T00:00:00Z"});
        assert!(conversation_is_newer(&a, &b));
        assert!(!conversation_is_newer(&b, &a));
        let big = serde_json::json!({"lastMessageAt": 9999999999999i64});
        assert!(conversation_is_newer(&big, &a));
    }

    #[test]
    fn 备份列表_按时间倒序并统计大小() {
        let tmp = TempDir::new("backup-list");
        let root = tmp.0.join("chat_history");
        write_file(&root.join("qoder/_backups/1700000000000-1/main.sqlite"), b"12345");
        write_file(&root.join("qoder/_backups/1700000000000-1/main.sqlite-wal"), b"67");
        write_file(
            &root.join("workbuddy/_backups/1800000000000-2/history/ws/conv/m.json"),
            b"x",
        );
        // 账号归档不在列表里（它是同步功能的数据源，不是可删备份）
        write_file(&root.join("qoder/accA/payload/main.sqlite"), b"archive");
        // staging/trash 这类临时目录也不出现
        write_file(&root.join("qoder/_backups/.staging-1/x"), b"tmp");

        let list = list_backups_in(&root);
        assert_eq!(list.len(), 2, "只列 _backups 下的正式备份");
        assert_eq!(list[0].op_id, "1800000000000-2", "时间大的在前");
        assert_eq!(list[0].provider, "workbuddy");
        assert_eq!(list[0].files, 1);
        assert_eq!(list[0].bytes, 1);
        assert_eq!(list[0].items, vec!["history".to_string()]);
        assert_eq!(list[1].provider, "qoder");
        assert_eq!(list[1].files, 2);
        assert_eq!(list[1].bytes, 7);
        assert_eq!(list[1].created_at_ms, 1_700_000_000_000);
    }

    #[test]
    fn 删除备份_只允许backups下的单级目录() {
        let tmp = TempDir::new("backup-del");
        let root = tmp.0.join("chat_history");
        write_file(&root.join("qoder/_backups/1700000000000-1/main.sqlite"), b"x");
        write_file(&root.join("qoder/accA/payload/main.sqlite"), b"keep");

        // 外部输入校验：路径穿越 / 分隔符 / 未知平台 / 临时目录一律拒绝
        assert!(delete_backup_in(&root, "qoder", "../accA").is_err());
        assert!(delete_backup_in(&root, "qoder", "a/b").is_err());
        assert!(delete_backup_in(&root, "minimax", "1700000000000-1").is_err());
        assert!(delete_backup_in(&root, "qoder", ".staging-1").is_err());
        assert!(delete_backup_in(&root, "qoder", "不存在").is_err());

        delete_backup_in(&root, "qoder", "1700000000000-1").unwrap();
        assert!(!root.join("qoder/_backups/1700000000000-1").exists());
        // 账号归档不能被删
        assert!(root.join("qoder/accA/payload/main.sqlite").is_file());
    }

    #[test]
    fn 操作id时间戳解析() {
        assert_eq!(op_timestamp_ms("1700000000000-1234"), 1_700_000_000_000);
        assert_eq!(op_timestamp_ms("bogus"), 0);
        assert_eq!(op_timestamp_ms(""), 0);
    }
}
