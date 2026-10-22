//! 应用入口（Tauri 2）。

mod commands;
mod core;
mod providers;
mod tray;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(commands::AppState::new())
        .invoke_handler(tauri::generate_handler![
            commands::overview,
            commands::checkin_account,
            commands::checkin_all,
            commands::discover_accounts,
            commands::add_discovered_account,
            commands::begin_qoder_auth,
            commands::poll_qoder_auth,
            commands::remove_account,
            commands::get_config,
            commands::set_config,
            commands::get_logs,
            commands::open_external,
        ])
        .setup(|app| {
            tray::setup(app.handle())?;
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(commands::scheduler_loop(handle));
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("启动聚合签到失败");
}
