//! 打开对应的 agent 客户端。
//!
//! 「多账号切换」在这里落地为两件事：
//!   1. 账号池里标记哪个账号是该 agent 的**当前账号**（`config.activeAccounts`）；
//!   2. 一键**打开客户端**，直接进到该账号的会话里继续干活。
//!
//! 说明：本机各客户端（MiniMax Code / Qoder CN）默认是**单账号**的，切换账号
//! 需要客户端自己重新登录。这里做的是「把客户端拉起来并带上账号上下文」，
//! 而不是像 Cockpit 的 workbuddy 那样给每个账号一份独立数据目录 ——
//! 那需要每个账号一个隔离的 userData，属于更大的架构改动。

use anyhow::{anyhow, Result};
use std::path::PathBuf;

/// 某 agent 客户端的可执行文件候选路径（按优先级）。
fn candidates(provider: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let local = dirs::data_local_dir().unwrap_or_default();

    match provider {
        "minimax" => {
            out.push(PathBuf::from(r"D:\Programs\MiniMax Code\MiniMax Code.exe"));
            out.push(PathBuf::from(r"C:\Program Files\MiniMax Code\MiniMax Code.exe"));
            out.push(local.join("Programs").join("MiniMax Code").join("MiniMax Code.exe"));
        }
        "qoder" => {
            // Qoder 的实际 exe 在 .qoder-versions\<版本>\ 下，取版本号最大的那个
            let versions = PathBuf::from(r"D:\Programs\Qoder CN\.qoder-versions");
            let mut found: Vec<(u64, u64, u64, PathBuf)> = Vec::new();
            if let Ok(entries) = std::fs::read_dir(&versions) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    let mut parts = name.split('.');
                    let a = parts.next().and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
                    let b = parts.next().and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
                    let c = parts.next().and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
                    found.push((a, b, c, entry.path()));
                }
            }
            if let Some((_, _, _, dir)) = found.into_iter().max() {
                out.push(dir.join("Qoder CN.exe"));
            }
            out.push(PathBuf::from(
                r"C:\Program Files\Qoder CN\Qoder CN.exe",
            ));
            // 官方启动器（会自动定位到最新已安装版本）
            out.push(
                local
                    .join("Qoder CN")
                    .join("Qoder CN Launcher")
                    .join("Qoder CN Launcher.exe"),
            );
        }
        "miaoshou" => {
            out.push(PathBuf::from(r"D:\meituan\妙手\妙手.exe"));
            out.push(PathBuf::from(r"C:\Program Files\妙手\妙手.exe"));
        }
        _ => {}
    }
    out
}

/// 找某 agent 的客户端可执行文件。
pub fn client_exe(provider: &str) -> Option<PathBuf> {
    candidates(provider)
        .into_iter()
        .find(|p| p.is_file())
}

/// 客户端本地数据目录（账号的项目/会话都在这里）。
pub fn data_dir(provider: &str) -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let dir = match provider {
        "minimax" => home.join(".minimax"),
        "qoder" => home.join(".qoder-cn"),
        "miaoshou" => home.join(".meituan-catpaw"),
        _ => return None,
    };
    if dir.is_dir() {
        Some(dir)
    } else {
        None
    }
}

/// 启动客户端。返回实际启动的可执行文件路径。
pub fn launch(provider: &str) -> Result<PathBuf> {
    let exe = client_exe(provider)
        .ok_or_else(|| anyhow!("没有找到 {} 的客户端可执行文件", provider))?;
    std::process::Command::new(&exe)
        .spawn()
        .map_err(|e| anyhow!("启动失败：{e}"))?;
    Ok(exe)
}

/// 统计数据目录里的项目/会话数量（只读，给 UI 一个「这个账号有多少东西」的直观感受）。
pub fn account_stats(provider: &str) -> Option<(usize, usize)> {
    let home = dirs::home_dir()?;
    match provider {
        // MiniMax 的数据分两处：.minimax（会话/鉴权）与 .minimax-agent-cn（项目/配置）
        "minimax" => {
            let projects = home.join(".minimax-agent-cn").join("projects");
            let sessions = home.join(".minimax").join("v2").join("sessions");
            Some((count_dirs(&projects), count_dirs(&sessions)))
        }
        "qoder" => {
            let projects = home.join(".qoder-cn").join("projects");
            let sessions = home.join(".qoder-cn").join("logs").join("sessions");
            Some((count_dirs(&projects), count_dirs(&sessions)))
        }
        // 妙手的工作区在用户 id 目录下，结构因版本而异，只数一层
        "miaoshou" => {
            let root = home.join(".meituan-catpaw");
            Some((count_dirs(&root), count_files(&root, "auth.json")))
        }
        _ => None,
    }
}

fn count_files(dir: &std::path::Path, name: &str) -> usize {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| e.path().file_name().and_then(|n| n.to_str()) == Some(name))
                .count()
        })
        .unwrap_or(0)
}

fn count_dirs(dir: &std::path::Path) -> usize {
    std::fs::read_dir(dir)
        .map(|entries| entries.flatten().filter(|e| e.path().is_dir()).count())
        .unwrap_or(0)
}
