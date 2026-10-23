// Windows release 构建时不弹控制台窗口
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use agent_checkin_tools_lib::commands;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.iter().any(|a| a == "--selftest") {
        agent_checkin_tools_lib::selftest::run();
        return;
    }

    // 把本机客户端登录态的账号全部导入（MiniMax / 妙手）
    if args.iter().any(|a| a == "--import-local") {
        println!("=== 导入本机客户端登录态 ===");
        match commands::cli_import_local() {
            Ok(()) => println!("=== 完成 ==="),
            Err(e) => {
                eprintln!("失败：{e}");
                std::process::exit(1);
            }
        }
        return;
    }

    // 命令行版设备授权：发起会话并落盘，轮询交给应用
    if let Some(pos) = args.iter().position(|a| a == "--device-auth") {
        let provider = args
            .get(pos + 1)
            .cloned()
            .unwrap_or_else(|| "qoder".to_string());
        if let Err(e) = commands::cli_device_auth(&provider) {
            eprintln!("失败：{e}");
            std::process::exit(1);
        }
        return;
    }

    // 命令行版切换客户端账号（会备份原登录态并启动客户端）
    if let Some(pos) = args.iter().position(|a| a == "--switch-client") {
        let provider = args.get(pos + 1).cloned().unwrap_or_default();
        let account = args.get(pos + 2).cloned().unwrap_or_default();
        if provider.is_empty() || account.is_empty() {
            eprintln!("用法：--switch-client <minimax|miaoshou> <account_id>");
            std::process::exit(1);
        }
        if let Err(e) = commands::cli_switch_client(&provider, &account) {
            eprintln!("失败：{e}");
            std::process::exit(1);
        }
        return;
    }

    agent_checkin_tools_lib::run()
}
