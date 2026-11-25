//! 版本更新说明。
//!
//! 每个版本发布时在 `NOTES` 里补一条，App 首次启动会写进「应用内提醒」
//! （首页右上角铃铛），配置里的标记保证同一版本只发一次。

use tauri::{AppHandle, Manager};

use crate::commands::AppState;
use crate::notifications::{self, NoticeKind};

/// (版本号, 说明正文)。正文按行分段展示。
const NOTES: &[(&str, &str)] = &[(
    "0.2.2",
    "新增：Qoder CN 免费模型实时核对，从官方活动文档读取进行中的限时免费活动（如 HungerG）。\n\
     新增：各平台设置里可选客户端路径；首页公告支持从官网更新。\n\
     修复：ZCode 本机导入改用 Windows 凭据管理器里的密钥解密，失败时给出明确提示。\n\
     调整：平台设置入口移到头部，更新提示移到侧栏，Trae CN 图标换高清，Zcode 统一为 ZCode。",
)];

fn notes_for(version: &str) -> Option<&'static str> {
    NOTES
        .iter()
        .find(|(v, _)| *v == version)
        .map(|(_, body)| *body)
}

/// 当前版本有更新说明且还没发过时，写入应用内提醒。
pub fn publish_if_new(app: &AppHandle) -> Result<(), String> {
    let version = env!("CARGO_PKG_VERSION");
    let Some(body) = notes_for(version) else {
        return Ok(());
    };
    let state = app.state::<AppState>();
    let already = state
        .config
        .lock()
        .map(|cfg| cfg.release_notes_version.as_deref() == Some(version))
        .unwrap_or(false);
    if already {
        return Ok(());
    }
    notifications::publish(
        app,
        NoticeKind::Ok,
        &format!("v{version} 更新说明"),
        body,
        Some(format!("release-notes-{version}")),
    )?;
    state.update_config(|cfg| {
        cfg.release_notes_version = Some(version.to_string());
        Ok(())
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn 当前版本必须有更新说明() {
        assert!(
            super::notes_for(env!("CARGO_PKG_VERSION")).is_some(),
            "发版前忘写 release_notes::NOTES"
        );
    }
}