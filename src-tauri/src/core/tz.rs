//! 时区解析。
//!
//! MiniMax 的签到接口要求带 `timezone_id`（IANA 名，如 `Asia/Shanghai`），
//! 否则返回 `1406010011 invalid timezone_id`。
//!
//! Windows 本身用的是「China Standard Time」这类 Windows 时区名，需要映射到
//! IANA 名。这里读注册表里的 `TimeZoneKeyName`，再走一张常见映射表；
//! 表里没有的退回 UTC+8（中国用户绝大多数场景都合适，且服务端接受 UTC）。

use std::sync::OnceLock;

fn windows_tz_key_name() -> Option<String> {
    let out = std::process::Command::new("reg")
        .args([
            "query",
            r"HKLM\SYSTEM\CurrentControlSet\Control\TimeZoneInformation",
            "/v",
            "TimeZoneKeyName",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        if let Some((_, value)) = line.split_once("REG_SZ") {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
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
