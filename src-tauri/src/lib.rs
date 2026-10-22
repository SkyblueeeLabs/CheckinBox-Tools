//! 应用入口（Tauri 2）。

mod commands;
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
            commands::set_agent_schedule,
            commands::set_scheduler_enabled,
            commands::set_active_account,
            commands::set_alert_threshold,
            commands::launch_client,
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
