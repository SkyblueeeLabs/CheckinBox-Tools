//! 定位 / 启动各 agent 的客户端，以及「客户端是否在运行」的检测。
//!
//! 「多账号切换」在这里落地为两件事：
//!   1. 账号池里标记哪个账号是该 agent 的**当前账号**（`config.activeAccounts`）；
//!   2. 一键**打开客户端**，直接进到该账号的会话里继续干活。
//!
//! 说明：本机各客户端（MiniMax Code / Qoder CN）默认是**单账号**的，切换账号
//! 需要客户端自己重新登录。这里做的是「把客户端拉起来并带上账号上下文」，
//! 而不是像 Cockpit 的 workbuddy 那样给每个账号一份独立数据目录 ——
//! 那需要每个账号一个隔离的 userData，属于更大的架构改动。
//!
//! exe 定位策略（按优先级）：
//!   1. **App Paths 注册表**（安装器注册的，最准；用 `winreg` crate 读，无编码问题）；
//!   2. **标准安装根两级目录扫描**：Program Files / Program Files (x86) /
//!      ProgramData / LocalAppData / 各盘符 `:\Programs`，按目录名关键词匹配；
//!      Qoder 的版本化布局（`<安装根>\.qoder-versions\<版本>\`）单独处理；
//!   3. 旧版本写死的路径，作为最后兜底（换机器后 1、2 基本能兜住）。

use anyhow::{anyhow, Result};
use std::path::{Path, PathBuf};

/// 各平台客户端的静态描述。
struct ClientSpec {
    /// 候选 exe 文件名（第一个是主程序，其余是启动器这类次要候选）
    exe_names: &'static [&'static str],
    /// 安装目录名关键词（小写包含匹配）
    dir_keywords: &'static [&'static str],
    /// 旧版本写死的路径（最后的兜底）
    legacy_paths: &'static [&'static str],
}

fn spec_of(provider: &str) -> Option<ClientSpec> {
    match provider {
        "minimax" => Some(ClientSpec {
            exe_names: &["MiniMax Code.exe"],
            dir_keywords: &["minimax"],
            legacy_paths: &[
                r"D:\Programs\MiniMax Code\MiniMax Code.exe",
                r"C:\Program Files\MiniMax Code\MiniMax Code.exe",
            ],
        }),
        "qoder" => Some(ClientSpec {
            // Launcher 是次要候选：真正的客户端在 .qoder-versions\<版本>\ 下
            exe_names: &["Qoder CN.exe", "Qoder CN Launcher.exe"],
            dir_keywords: &["qoder"],
            legacy_paths: &[r"C:\Program Files\Qoder CN\Qoder CN.exe"],
        }),
        "miaoshou" => Some(ClientSpec {
            exe_names: &["妙手.exe"],
            dir_keywords: &["妙手", "catpaw", "meituan"],
            legacy_paths: &[r"D:\meituan\妙手\妙手.exe", r"C:\Program Files\妙手\妙手.exe"],
        }),
        "workbuddy" => Some(ClientSpec {
            // 腾讯 CodeBuddy CN（桌面端品牌是 WorkBuddy），exe 两个名字都认
            exe_names: &["CodeBuddy.exe", "WorkBuddy.exe"],
            dir_keywords: &["codebuddy", "workbuddy"],
            legacy_paths: &[],
        }),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// exe 定位
// ---------------------------------------------------------------------------

/// App Paths 注册表：安装器会在这里注册 exe 的完整路径。
///
/// 用 `winreg` 直接调 W 版 API，中文路径不会有 `reg query` 的控制台编码问题。
fn app_paths_lookup(exe_name: &str) -> Option<PathBuf> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;

    let key = format!(r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\{exe_name}");
    for hive in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
        let Ok(sub) = RegKey::predef(hive).open_subkey(&key) else {
            continue;
        };
        if let Ok(path) = sub.get_value::<String, _>("") {
            let p = PathBuf::from(path.trim_matches('"'));
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// 要扫描的标准安装根。
fn scan_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for var in ["ProgramFiles", "ProgramFiles(x86)", "ProgramData"] {
        if let Some(v) = std::env::var_os(var) {
            roots.push(PathBuf::from(v));
        }
    }
    if let Some(local) = dirs::data_local_dir() {
        roots.push(local.clone());
        roots.push(local.join("Programs"));
    }
    if let Some(appdata) = dirs::data_dir() {
        roots.push(appdata);
    }
    // 各盘符根下的 \Programs（覆盖 D:\Programs 这类自装位置）
    for letter in b'C'..=b'Z' {
        let programs = PathBuf::from(format!("{}:\\Programs", letter as char));
        if programs.is_dir() {
            roots.push(programs);
        }
    }
    roots.sort();
    roots.dedup();
    roots
}

/// 找出本机所有「名字像这个客户端」的安装目录。
///
/// 两级扫描：根下的目录名匹配关键词就纳入，并继续看它子目录里匹配的
/// （覆盖 `LocalAppData\Qoder CN\Qoder CN Launcher\` 这种嵌套布局）。
/// App Paths 注册表定位到的 exe，其父目录链也一并纳入。
fn matching_dirs(spec: &ClientSpec) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();

    for exe in spec.exe_names {
        if let Some(p) = app_paths_lookup(exe) {
            let mut cur = p.parent().map(Path::to_path_buf);
            while let Some(dir) = cur {
                out.push(dir.clone());
                cur = dir.parent().map(Path::to_path_buf);
            }
        }
    }

    for root in scan_roots() {
        let mut level = vec![root];
        for _ in 0..2 {
            let mut next = Vec::new();
            for dir in &level {
                let Ok(entries) = std::fs::read_dir(dir) else {
                    continue;
                };
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_lowercase();
                    if !spec.dir_keywords.iter().any(|k| name.contains(k)) {
                        continue;
                    }
                    let path = entry.path();
                    if path.is_dir() {
                        out.push(path.clone());
                        next.push(path);
                    }
                }
            }
            level = next;
            if level.is_empty() {
                break;
            }
        }
    }

    out.sort();
    out.dedup();
    out
}

/// Qoder 的版本化布局：真实客户端在 `<安装根>\.qoder-versions\<版本>\` 下，
/// 取语义化版本最大的那个。
fn qoder_versioned_exe(root: &Path) -> Option<PathBuf> {
    let versions = root.join(".qoder-versions");
    let Ok(entries) = std::fs::read_dir(&versions) else {
        return None;
    };
    let mut found: Vec<(u64, u64, u64, PathBuf)> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let mut parts = name.split('.');
        let a = parts.next().and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
        let b = parts.next().and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
        let c = parts.next().and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
        found.push((a, b, c, entry.path()));
    }
    found
        .into_iter()
        .max()
        .map(|(_, _, _, dir)| dir.join("Qoder CN.exe"))
        .filter(|p| p.is_file())
}

/// 找某 agent 的客户端可执行文件。
pub fn client_exe(provider: &str) -> Option<PathBuf> {
    let Some(spec) = spec_of(provider) else {
        return None;
    };
    let dirs = matching_dirs(&spec);

    // 先只找主 exe（含 Qoder 版本化布局），避免被「启动器」抢先
    if let Some(primary) = spec.exe_names.first() {
        for dir in &dirs {
            let p = dir.join(primary);
            if p.is_file() {
                return Some(p);
            }
            if let Some(hit) = qoder_versioned_exe(dir) {
                return Some(hit);
            }
        }
    }
    // 再找次要候选（Qoder CN Launcher 这类）
    for dir in &dirs {
        for exe in spec.exe_names.iter().skip(1) {
            let p = dir.join(exe);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    // 最后：旧版本写死的路径
    spec.legacy_paths
        .iter()
        .map(PathBuf::from)
        .find(|p| p.is_file())
}

/// 本机能找到的全部 Qoder 版本号（`Cosy-Version` 请求头用）。
pub fn qoder_versions() -> Vec<String> {
    let Some(spec) = spec_of("qoder") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for dir in matching_dirs(&spec) {
        let Ok(entries) = std::fs::read_dir(dir.join(".qoder-versions")) else {
            continue;
        };
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                out.push(name.to_string());
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// 客户端本地数据目录（账号的项目/会话都在这里）。
pub fn data_dir(provider: &str) -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let dir = match provider {
        "minimax" => home.join(".minimax"),
        "qoder" => home.join(".qoder-cn"),
        "miaoshou" => home.join(".meituan-catpaw"),
        "workbuddy" => home.join(".workbuddy"),
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

// ---------------------------------------------------------------------------
// 进程检测（换号前必须确认客户端已完全退出）
// ---------------------------------------------------------------------------

/// 客户端是否正在运行。
///
/// 用 Toolhelp32 快照枚举进程名（W 版 API，中文 exe 名也正确），
/// 与该平台的候选 exe 名做不区分大小写的**全等**比较。
/// Electron 客户端的全部子进程都与其主 exe 同名，比到一个就算在运行。
pub fn is_running(provider: &str) -> bool {
    let Some(spec) = spec_of(provider) else {
        return false;
    };
    let targets: Vec<String> = spec
        .exe_names
        .iter()
        .map(|n| n.to_lowercase())
        .collect();
    for name in winproc::process_names() {
        if targets.iter().any(|t| name == *t) {
            return true;
        }
    }
    false
}

/// 关闭客户端：先请求**优雅关闭**（等价于点窗口的 X），超时再强制结束树。
///
/// 「切换客户端账号」的一条龙（借鉴 Cockpit 的关闭→写入→重启）用它先收掉
/// 正在运行的客户端 —— 客户端退出时会把内存里的登录态写回文件，所以写凭据
/// 之前必须等进程**完全退出**。返回 false 表示实在关不掉（权限不足等）。
pub fn close_client(provider: &str) -> bool {
    let Some(spec) = spec_of(provider) else {
        return false;
    };
    if !is_running(provider) {
        return true;
    }

    // 1) 优雅关闭：taskkill 不带 /F 时给所有窗口发 WM_CLOSE。
    //    注意 Electron 应用可能「关到托盘」，所以等一小段再看。
    for exe in spec.exe_names {
        run_hidden("taskkill", &["/IM", exe, "/T"]);
    }
    if wait_until_stopped(provider, std::time::Duration::from_secs(4))
        && confirm_stable_exit(provider)
    {
        return true;
    }

    // 2) 强制结束进程树（客户端的鉴权日记本身就是按崩溃恢复设计的，能兜住）。
    //    Electron 的渲染/工具子进程可能比主进程晚退，收不干净就多收几轮。
    for _ in 0..3 {
        for exe in spec.exe_names {
            run_hidden("taskkill", &["/IM", exe, "/F", "/T"]);
        }
        if wait_until_stopped(provider, std::time::Duration::from_secs(4))
            && confirm_stable_exit(provider)
        {
            return true;
        }
    }
    false
}

/// 进程消失后确认「不再复活、没有晚退的子进程」。
///
/// 2026-10-25 实测踩坑：优雅关闭时客户端退出前会把内存会话写回登录态文件，
/// 子进程/收尾写盘可能比主进程晚一两拍 —— 只看「进程没了」就写凭据，
/// 会被它的收尾写入覆盖，切换白做。静默 1.5 秒复查两轮都干净才算真退出。
fn confirm_stable_exit(provider: &str) -> bool {
    for _ in 0..2 {
        std::thread::sleep(std::time::Duration::from_millis(750));
        if is_running(provider) {
            return false;
        }
    }
    true
}

/// 轮询等待客户端完全退出。
fn wait_until_stopped(provider: &str, timeout: std::time::Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if !is_running(provider) {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
    !is_running(provider)
}

/// 静默跑一个命令（不弹控制台窗口）。
#[cfg(windows)]
fn run_hidden(program: &str, args: &[&str]) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let _ = std::process::Command::new(program)
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .status();
}

#[cfg(not(windows))]
fn run_hidden(program: &str, args: &[&str]) {
    let _ = std::process::Command::new(program).args(args).status();
}

#[cfg(windows)]
mod winproc {
    //! Toolhelp32 进程快照的 FFI。沿用 `dpapi.rs` 的做法：直接声明
    //! `extern "system"`，不引入 windows-sys 这类绑定 crate。

    use std::ffi::c_void;

    const TH32CS_SNAPPROCESS: u32 = 0x2;
    const INVALID_HANDLE_VALUE: *mut c_void = usize::MAX as *mut c_void;

    #[repr(C)]
    struct ProcessEntry32W {
        dw_size: u32,
        cnt_usage: u32,
        th32_process_id: u32,
        th32_default_heap_id: usize,
        th32_module_id: u32,
        cnt_threads: u32,
        th32_parent_process_id: u32,
        pc_pri_class_base: i32,
        dw_flags: u32,
        sz_exe_file: [u16; 260],
    }

    impl ProcessEntry32W {
        fn zeroed() -> Self {
            Self {
                dw_size: std::mem::size_of::<ProcessEntry32W>() as u32,
                cnt_usage: 0,
                th32_process_id: 0,
                th32_default_heap_id: 0,
                th32_module_id: 0,
                cnt_threads: 0,
                th32_parent_process_id: 0,
                pc_pri_class_base: 0,
                dw_flags: 0,
                sz_exe_file: [0; 260],
            }
        }
    }

    extern "system" {
        fn CreateToolhelp32Snapshot(dw_flags: u32, th32_process_id: u32) -> *mut c_void;
        fn Process32FirstW(h_snapshot: *mut c_void, lppe: *mut ProcessEntry32W) -> i32;
        fn Process32NextW(h_snapshot: *mut c_void, lppe: *mut ProcessEntry32W) -> i32;
        fn CloseHandle(h_object: *mut c_void) -> i32;
    }

    /// 全部进程名（小写）。枚举失败返回空（宁可放行也不误报客户端在运行）。
    pub fn process_names() -> Vec<String> {
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snap == INVALID_HANDLE_VALUE || snap.is_null() {
                return Vec::new();
            }
            let mut out = Vec::new();
            let mut entry = ProcessEntry32W::zeroed();
            if Process32FirstW(snap, &mut entry) != 0 {
                loop {
                    let len = entry
                        .sz_exe_file
                        .iter()
                        .position(|c| *c == 0)
                        .unwrap_or(entry.sz_exe_file.len());
                    let name = String::from_utf16_lossy(&entry.sz_exe_file[..len]).to_lowercase();
                    out.push(name);
                    if Process32NextW(snap, &mut entry) == 0 {
                        break;
                    }
                }
            }
            CloseHandle(snap);
            out
        }
    }
}

#[cfg(not(windows))]
mod winproc {
    pub fn process_names() -> Vec<String> {
        Vec::new()
    }
}

// ---------------------------------------------------------------------------
// 统计（只读，给 UI 一个「这个账号有多少东西」的直观感受）
// ---------------------------------------------------------------------------

/// 统计数据目录里的项目/会话数量。
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

fn count_files(dir: &Path, name: &str) -> usize {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| e.path().file_name().and_then(|n| n.to_str()) == Some(name))
                .count()
        })
        .unwrap_or(0)
}

fn count_dirs(dir: &Path) -> usize {
    std::fs::read_dir(dir)
        .map(|entries| entries.flatten().filter(|e| e.path().is_dir()).count())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 客户端exe能定位或至少不panic() {
        // 本机装了 MiniMax / Qoder / 妙手，定位应该成功；没装的机器只要求不 panic
        for provider in ["minimax", "qoder", "miaoshou", "unknown"] {
            let _ = client_exe(provider);
            let _ = is_running(provider);
        }
    }

    #[test]
    fn 进程名可枚举() {
        // 至少能列出当前进程（测试进程自身）
        let names = winproc::process_names();
        assert!(!names.is_empty(), "Toolhelp32 应该能枚举到进程");
    }
}
