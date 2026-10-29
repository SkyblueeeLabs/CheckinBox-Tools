//! 应用入口（Tauri 2）。

// ---------------------------------------------------------------------------
// 构建护栏（2026-10-26 实际踩坑后加的，别删）：
// release 构建且未启用 `custom-protocol` 特性时，`generate_context!` 不会把
// ../dist 嵌入 exe，运行时去连 devUrl（http://localhost:5173）——没开
// `npm run dev` 就是「localhost 拒绝连接 / ERR_CONNECTION_REFUSED」错误页。
// 正确姿势：`npx tauri build --no-bundle --features custom-protocol`
//       或  `cargo build --release --features custom-protocol`
// ⚠️ tauri-cli 2.12+ 不再自动带这个特性，`--features` 必须显式写。
// 日常开发（npm run tauri:dev / cargo test 走 debug profile）不受影响。
// ---------------------------------------------------------------------------
#[cfg(all(not(debug_assertions), not(test), not(feature = "custom-protocol")))]
compile_error!(
    "release 构建缺少 custom-protocol 特性：界面不会嵌入 exe，运行时会去连 \
     http://localhost:5173 导致「localhost 拒绝连接」。\
     请改用 `npx tauri build --no-bundle`，或加 `--features custom-protocol`"
);

use tauri::{Emitter, Manager};

use crate::core::config::CloseAction;

pub mod commands;
mod core;
mod providers;
pub mod selftest;
mod tray;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(commands::AppState::new())
        .invoke_handler(tauri::generate_handler![
            commands::overview,
            commands::refresh_quota,
            commands::refresh_all_quotas,
            commands::checkin_account,
            commands::checkin_all,
            commands::discover_accounts,
            commands::add_discovered_account,
            commands::begin_device_auth,
            commands::pending_auth,
            commands::poll_device_auth,
            commands::remove_account,
            commands::get_config,
            commands::set_config,
            commands::set_close_action,
            commands::resolve_close_action,
            commands::set_agent_order,
            commands::set_agent_schedule,
            commands::set_scheduler_enabled,
            commands::set_sync_enabled,
            commands::set_refresh_interval,
            commands::set_active_account,
            commands::set_alert_threshold,
            commands::set_chat_sync,
            commands::list_chat_backups,
            commands::delete_chat_backup,
            commands::delete_old_chat_backups,
            commands::launch_client,
            commands::detect_client_account,
            commands::client_running,
            commands::switch_client_account,
            commands::sync_now,
            commands::open_data_dir,
            commands::get_logs,
            commands::open_external,
        ])
        .on_window_event(|window, event| {
            if window.label() != "main" {
                return;
            }
            let tauri::WindowEvent::CloseRequested { api, .. } = event else {
                return;
            };
            let app = window.app_handle();
            let action = app
                .try_state::<commands::AppState>()
                .map(|state| state.config.lock().unwrap().close_action)
                .unwrap_or_default();
            match action {
                // 后台驻留：拦下关闭，只把窗口藏起来（托盘和调度器继续跑）
                CloseAction::Tray => {
                    api.prevent_close();
                    let _ = window.hide();
                }
                // 放行：关掉最后一个窗口后 Tauri 正常退出
                CloseAction::Exit => {}
                // 首次 / 每次询问：拦下关闭，交给前端弹确认框
                CloseAction::Ask => {
                    api.prevent_close();
                    // 必须用 app 广播：window.emit 的目标是 Window，
                    // 前端 listen 监听的是 Webview，收不到。
                    let _ = app.emit("close-requested", ());
                }
            }
        })
        .setup(|app| {
            tray::setup(app.handle())?;
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(commands::scheduler_loop(handle));
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("启动 CheckinBox 失败");
}
