//! 命令行自检：不启动 GUI，直接验证「发现账号 → 读签到状态 → DPAPI 加解密」。
//!
//! 用法：`CheckinBox --selftest`
//!
//! 注意：只读状态，不领取。领取接口是幂等的，但自检不主动调用它。

use serde_json::Value;

use crate::core::dpapi;
use crate::core::provider::Provider;
use crate::core::store::AccountStore;
use crate::providers::minimax::MiniMaxProvider;
use crate::providers::qoder::QoderProvider;

pub fn run() {
    println!("=== CheckinBox 自检 ===\n");

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
                            Ok(q) => println!(
                                "        积分: 剩余={:.0} | {}",
                                q.total_remaining, q.message
                            ),
                            Err(e) => println!("        积分读取失败：{e}"),
                        }
                    }
                }
                Err(e) => println!("    发现失败：{e}"),
            }
        }

        // ---- 3d. Trae CN ----
        println!("\n[3d] Trae CN");
        {
            let provider = crate::providers::trae::TraeProvider::new();
            match provider.discover().await {
                Ok(found) => {
                    println!("    发现本机账号 {} 个", found.len());
                    for (account, cred) in &found {
                        println!("      - {}（{}）", account.label, account.id);
                        match provider.status(account, cred).await {
                            Ok(st) => println!(
                                "        状态={:?} 今日={} {}",
                                st.state, st.points, st.message
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
        println!(
            "    machine_id = {}",
            crate::providers::qoder::local_machine_id()
        );
        let qoder = QoderProvider::new();
        let session = qoder.begin_authorization("qoder");
        match session {
            Ok(s) => println!(
                "    设备授权地址已生成 ✓（长度 {}，含 challenge/machine_id/nonce）",
                s.auth_url.len()
            ),
            Err(e) => println!("    授权会话生成失败：{e}"),
        }

        // ---- 4b. 账号池里的 Qoder 账号：状态 + 真实昵称 ----
        println!("\n[4b] Qoder CN 账号池账号");
        {
            let store = crate::core::store::AccountStore::load().unwrap_or_default();
            match store.list_of("qoder").first().cloned() {
                Some(account) => {
                    println!("     账号 {}（{}）", account.label, account.id);
                    match store.credential("qoder", &account.id) {
                        Ok(cred) => {
                            match qoder.status(&account, &cred).await {
                                Ok(s) => println!(
                                    "        状态={:?} 可领={} 消息={}",
                                    s.state, s.points, s.message
                                ),
                                Err(e) => println!("        状态读取失败：{e}"),
                            }
                            match qoder.quota(&account, &cred).await {
                                Ok(q) => println!(
                                    "        积分: 发放={:.0} 已用={:.0} 剩余={:.0}",
                                    q.total_granted, q.total_consumed, q.total_remaining
                                ),
                                Err(e) => println!("        积分读取失败：{e}"),
                            }
                            match qoder.profile(&account, &cred).await {
                                Ok(Some(p)) => {
                                    println!("        昵称={:?} 邮箱={:?}", p.name, p.email)
                                }
                                Ok(None) => println!("        昵称接口没返回数据"),
                                Err(e) => println!("        昵称读取失败：{e}"),
                            }
                        }
                        Err(e) => println!("        取凭据失败：{e}"),
                    }
                }
                None => println!("     账号池里没有 Qoder 账号"),
            }
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
                let is_str =
                    cred.get("access_token").and_then(|v| v.as_str()) == Some("demo-token");
                println!(
                    "    回读凭据 {}（{}）",
                    if is_str { "✓" } else { "✗" },
                    if is_str {
                        "内容一致"
                    } else {
                        "内容不一致"
                    }
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
    println!(
        "    账号文件 = {}",
        crate::core::paths::accounts_file().display()
    );

    // ---- 7. 客户端定位 + 进程检测 ----
    println!("\n[7] 客户端定位与进程检测");
    for provider in ["minimax", "qoder", "miaoshou"] {
        let exe = crate::core::launcher::client_exe(provider);
        match exe {
            Some(p) => println!("    {provider}: {}", p.display()),
            None => println!("    {provider}: 未找到客户端 exe"),
        }
        println!(
            "      正在运行 = {}",
            crate::core::launcher::is_running(provider)
        );
    }

    // ---- 8. Qoder OSCrypt（换号的核心链路）----
    // 对真实客户端文件做「解密 → 再加密 → 再解密」的内存 roundtrip，
    // 验证 DPAPI 密钥读取和 v10 加解密与客户端格式互通。不写任何文件。
    println!("\n[8] Qoder safeStorage roundtrip");
    {
        use crate::core::{client_switch, os_crypt};
        match client_switch::detect("qoder") {
            Ok(Some(d)) => println!("    客户端登录账号：{}（id {}）", d.label, d.account_id),
            Ok(None) => println!("    客户端未登录 / 读不到登录态"),
            Err(e) => println!("    登录态读取失败：{e}"),
        }

        let appdata = std::env::var("APPDATA").unwrap_or_default();
        let dir = std::path::PathBuf::from(appdata).join("com.qodercn.app.stable");
        let local_state = dir.join("Local State");
        let auth_file = dir.join("auth.v1.dat");
        if !auth_file.is_file() {
            println!("    没有 auth.v1.dat（客户端没登录过），跳过 roundtrip");
        } else {
            match os_crypt::read_key(&local_state).and_then(|key| {
                let raw = std::fs::read(&auth_file)?;
                let plain = os_crypt::decrypt(&key, &raw)?;
                let blob = os_crypt::encrypt(&key, &plain)?;
                let back = os_crypt::decrypt(&key, &blob)?;
                Ok((plain, back))
            }) {
                Ok((plain, back)) => {
                    let ok = plain == back;
                    let is_json = serde_json::from_slice::<serde_json::Value>(&plain).is_ok();
                    println!(
                        "    解密 {} · 重加密后解回 {} · 内容是 JSON {}",
                        if is_json { "✓" } else { "✗" },
                        if ok { "一致 ✓" } else { "不一致 ✗" },
                        if is_json { "✓" } else { "✗" }
                    );
                }
                Err(e) => println!("    roundtrip 失败：{e}"),
            }
        }
    }

    let _ = Value::Null;
    println!("\n=== 自检完成 ===");
}
