//! 时区解析。
//!
//! MiniMax 的签到接口要求带 `timezone_id`（IANA 名，如 `Asia/Shanghai`），
//! 否则返回 `1406010011 invalid timezone_id`。
//!
//! Windows 本身用的是「China Standard Time」这类 Windows 时区名，需要映射到
//! IANA 名。这里读注册表里的 `TimeZoneKeyName`，再走一张常见映射表；
//! 表里没有的退回 UTC+8（中国用户绝大多数场景都合适，且服务端接受 UTC）。

use std::sync::OnceLock;

#[cfg(windows)]
fn windows_tz_key_name() -> Option<String> {
    // 直接读注册表（winreg），不起 reg.exe 子进程 —— 既是 GUI 无控制台应用，
    // 起控制台子进程会闪 cmd 窗，而且 reg 输出还有控制台编码问题
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;

    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let key = hklm
        .open_subkey(r"SYSTEM\CurrentControlSet\Control\TimeZoneInformation")
        .ok()?;
    let value: String = key.get_value("TimeZoneKeyName").ok()?;
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

#[cfg(not(windows))]
fn windows_tz_key_name() -> Option<String> {
    None
}

fn map_windows_to_iana(key: &str) -> Option<&'static str> {
    let table: &[(&str, &str)] = &[
        ("China Standard Time", "Asia/Shanghai"),
        ("Taipei Standard Time", "Asia/Taipei"),
        ("Tokyo Standard Time", "Asia/Tokyo"),
        ("Korea Standard Time", "Asia/Seoul"),
        ("Singapore Standard Time", "Asia/Singapore"),
        ("SE Asia Standard Time", "Asia/Bangkok"),
        ("W. Australia Standard Time", "Australia/Perth"),
        ("AUS Eastern Standard Time", "Australia/Sydney"),
        ("India Standard Time", "Asia/Kolkata"),
        ("Eastern Standard Time", "America/New_York"),
        ("Central Standard Time", "America/Chicago"),
        ("Mountain Standard Time", "America/Denver"),
        ("Pacific Standard Time", "America/Los_Angeles"),
        ("GMT Standard Time", "Europe/London"),
        ("W. Europe Standard Time", "Europe/Berlin"),
        ("Romance Standard Time", "Europe/Paris"),
        ("Russian Standard Time", "Europe/Moscow"),
        ("UTC", "UTC"),
    ];
    table
        .iter()
        .find(|(win, _)| win.eq_ignore_ascii_case(key))
        .map(|(_, iana)| *iana)
}

/// 取当前系统的 IANA 时区名。
pub fn iana_timezone() -> String {
    static CACHE: OnceLock<String> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            windows_tz_key_name()
                .and_then(|k| map_windows_to_iana(&k).map(|s| s.to_string()))
                .unwrap_or_else(|| "Asia/Shanghai".to_string())
        })
        .clone()
}
