//! 命令行自检：不启动 GUI，直接验证「发现账号 → 读签到状态 → DPAPI 加解密」。
//!
//! 用法：`agent-checkin-tools --selftest`
//!
//! 注意：只读状态，不领取。领取接口是幂等的，但自检不主动调用它。

use serde_json::Value;

use crate::core::dpapi;
use crate::core::provider::Provider;
use crate::core::store::AccountStore;
use crate::providers::minimax::MiniMaxProvider;
use crate::providers::qoder::QoderProvider;

pub fn run() {
    println!("=== 聚合签到自检 ===\n");

    // ---- 1. DPAPI 加解密 ----
    println!("[1] DPAPI 凭据加解密");
    match dpapi::protect(b"secret-token-123") {
        Ok(cipher) => match dpapi::unprotect(&cipher) {
            Ok(plain) => {
                let ok = plain == b"secret-token-123";
                println!(
                    "    明文 {} 字节 -> 密文 {} 字节 -> 解密 {}",
                    b"secret-token-123".len(),
                    cipher.len(),
                    if ok { "一致 ✓" } else { "不一致 ✗" }
                );
            }
            Err(e) => println!("    解密失败：{e}"),
        },
        Err(e) => println!("    加密失败：{e}"),
    }

    // ---- 2. 时区解析 ----
    println!("\n[2] 时区解析");
    println!("    IANA 时区 = {}", crate::core::tz::iana_timezone());

    // ---- 3. MiniMax 发现 + 状态 ----
    println!("\n[3] MiniMax Code");
    let rt = tokio::runtime::Runtime::new().expect("启动 tokio 运行时");
    rt.block_on(async {
        let provider = MiniMaxProvider::new();
        match provider.discover().await {
            Ok(found) => {
                println!("    发现本机账号 {} 个", found.len());
                for (account, cred) in &found {
                    println!("      - {}（{}）", account.label, account.id);
                    let _ = cred;
                    match provider.status(account, cred).await {
                        Ok(s) => println!(
                            "        状态={:?} 今日可领={} 连签={}天 消息={}",
                            s.state, s.points, s.streak_days, s.message
                        ),
                        Err(e) => println!("        状态读取失败：{e}"),
                    }
                    match provider.quota(account, cred).await {
                        Ok(q) => println!(
                            "        积分: 发放={:.0} 已用={:.0} 剩余={:.0} | {}",
                            q.total_granted, q.total_consumed, q.total_remaining, q.message
                        ),
                        Err(e) => println!("        积分读取失败：{e}"),
                    }
                }
            }
            Err(e) => println!("    发现失败：{e}"),
        }

        // ---- 3b. 妙手（美团 CatPaw）----
        println!("\n[3b] 妙手 CatPaw");
        {
            let provider = crate::providers::miaoshou::MiaoshouProvider::new();
            match provider.discover().await {
                Ok(found) => {
                    println!("    发现本机账号 {} 个", found.len());
                    for (account, cred) in &found {
                        println!("      - {}（{}）", account.label, account.id);
                        match provider.status(account, cred).await {
                            Ok(st) => println!("        状态={:?} {}", st.state, st.message),
                            Err(e) => println!("        状态读取失败：{e}"),
                        }
                        match provider.quota(account, cred).await {
                            Ok(q) => println!("        积分: 剩余={:.0} | {}", q.total_remaining, q.message),
                            Err(e) => println!("        积分读取失败：{e}"),
                        }
                    }
                }
                Err(e) => println!("    发现失败：{e}"),
            }
        }

        // ---- 3c. 千问办公 ----
        println!("\n[3c] 千问办公 QwenWorkCN");
        {
            let provider = crate::providers::qwenwork::QwenWorkProvider::new();
            let store = crate::core::store::AccountStore::load().unwrap_or_default();
            if let Some(account) = store.list_of("qwenwork").first().cloned() {
                match store.credential("qwenwork", &account.id) {
                    Ok(cred) => {
                        match provider.status(&account, &cred).await {
                            Ok(st) => println!("     状态={:?} {}", st.state, st.message),
                            Err(e) => println!("     状态读取失败：{e}"),
                        }
                        match provider.quota(&account, &cred).await {
                            Ok(q) => println!("     积分: 剩余={:.0} 已用={:.0} 发放={:.0}", q.total_remaining, q.total_consumed, q.total_granted),
                            Err(e) => println!("     积分读取失败：{e}"),
                        }
                    }
                    Err(e) => println!("     取凭据失败：{e}"),
                }
            } else {
                println!("     账号池里没有千问办公账号（点界面「添加账号」走一次设备授权）");
            }
        }

        // ---- 4. Qoder 本机环境 ----
        println!("\n[4] Qoder CN 本机环境");
        let umid = crate::providers::qoder::umid_identity();
        match umid {
            Some(u) => println!(
                "    UMID 三件套 ✓  token 长度={}  code 长度={}",
                u.token.len(),
                u.code.len()
            ),
            None => println!("    未找到 umid-cache.json（活动可能不下发）"),
        }
        println!("    machine_id = {}", crate::providers::qoder::local_machine_id());
        let qoder = QoderProvider::new();
        let session = qoder.begin_authorization("qoder");
        match session {
            Ok(s) => println!(
                "    设备授权地址已生成 ✓（长度 {}，含 challenge/machine_id/nonce）",
                s.auth_url.len()
            ),
            Err(e) => println!("    授权会话生成失败：{e}"),
        }
    });

    // ---- 5. 账号池读写 ----
    println!("\n[5] 账号池读写（DPAPI 加密落盘）");
    let mut store = AccountStore::load().unwrap_or_default();
    let before = store.list().len();
    println!("    现有账号 {} 个", before);
    let sample = serde_json::json!({
        "access_token": "demo-token",
        "refresh_token": "demo-refresh",
    });
    if before == 0 {
        let account = crate::core::provider::Account {
            id: "selftest".to_string(),
            provider: "minimax".to_string(),
            label: "自检临时账号".to_string(),
            created_at: chrono::Utc::now().timestamp(),
            last_used: None,
            origin: "自检".to_string(),
        };
        match store.upsert(account, &sample) {
            Ok(()) => println!("    写入临时账号成功 ✓"),
            Err(e) => println!("    写入失败：{e}"),
        }
        match store.credential("minimax", "selftest") {
            Ok(cred) => {
                let is_str = cred.get("access_token").and_then(|v| v.as_str()) == Some("demo-token");
                println!(
                    "    回读凭据 {}（{}）",
                    if is_str { "✓" } else { "✗" },
                    if is_str { "内容一致" } else { "内容不一致" }
                );
            }
            Err(e) => println!("    回读失败：{e}"),
        }
        let _ = store.remove("minimax", "selftest");
        println!("    临时账号已清理");
    } else {
        println!("    账号池非空，跳过写入测试");
    }

    // ---- 6. 配置文件 ----
    println!("\n[6] 数据目录");
    println!("    根目录 = {}", crate::core::paths::data_dir().display());
    println!("    账号文件 = {}", crate::core::paths::accounts_file().display());

    let _ = Value::Null;
    println!("\n=== 自检完成 ===");
}
