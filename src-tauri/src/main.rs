// Windows release 构建时不弹控制台窗口
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    agent_checkin_tools_lib::run()
}
