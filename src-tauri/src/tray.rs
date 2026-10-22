//! 系统托盘：常驻后台 + 快捷菜单。
//!
//! 交互约定：
//!   * 左键点击托盘图标 → 显示/聚焦主窗口；
//!   * 右键 → 菜单：显示主窗口 / 立即全部签到 / 退出。

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Runtime};

use crate::commands;

fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// 建立托盘图标。
pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
    let checkin = MenuItem::with_id(app, "checkin", "立即全部签到", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;

    let menu = Menu::with_items(app, &[&show, &checkin, &separator, &quit])?;

    let mut builder = TrayIconBuilder::with_id("main")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip("聚合签到")
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => show_main_window(app),
            "checkin" => {
                let handle = app.clone();
                tauri::async_runtime::spawn(async move {
                    let Some(state) = handle.try_state::<commands::AppState>() else {
                        return;
                    };
                    let _ = commands::checkin_all_inner(handle.clone(), &state).await;
                });
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        });

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }

    builder.build(app)?;
    Ok(())
}
