//! 切换客户端实际登录的账号。
//!
//! 做法借鉴 Cockpit Tools：客户端把自己的登录态存在一个本地文件里，
//! 所谓「换账号」就是 **备份当前凭据 → 写入目标账号的凭据 → 启动客户端**，
//! 客户端启动后读到新凭据，等于换了个账号登录。
//!
//! Cockpit 在 `~/.codex/.cockpit_codex_auth.json` 里记了「这个客户端的凭据
//! 现在属于哪个账号」，并配了 `.cockpit-token-locks` 防止并发写坏 ——
//! 我们这边每次切换前会先把客户端原来的登录态备份成 `<file>.bak`，可回滚。
//!
//! **支持范围**：
//!   * 妙手 `~/.meituan-catpaw/auth.json` —— 明文，结构简单；本地数据还按 uid 分目录
//!   * MiniMax `~/.minimax/auth/<env>/<region>/<clientId>/auth.json` —— 明文 JSON
//!   * Qoder CN `%APPDATA%\com.qodercn.app.stable\auth.v1.dat` —— Electron
//!     safeStorage 加密（`v10` + AES-256-GCM，密钥在 `Local State` 里由 DPAPI
//!     保护）。实测同一 Windows 用户可以解出密钥，也就能重新加密写回，
//!     详见 `core/os_crypt.rs`。
//!
//! ⚠️ 这会改写客户端的登录凭据。客户端在运行时**先自动关闭再写入**
//!（`switch` 入口统一处理：优雅关闭 → 超时强制结束 → 写入 → 重启，
//! 借鉴 Cockpit 的关闭→写入→重启一条龙），全程不需要用户手动退出客户端。

use anyhow::{anyhow, Result};
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::path::PathBuf;

use super::os_crypt;
use crate::core::paths::atomic_write;

/// 客户端当前实际登录的账号。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedAccount {
    pub account_id: String,
    pub label: String,
}

/// 一次切换的结果。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SwitchOutcome {
    /// 客户端原登录态的备份路径（可回滚）
    pub backup: Option<String>,
    /// 写入的目标凭据文件
    pub target_file: String,
    /// 启动的客户端
    pub client_exe: String,
}

/// 这个平台能不能靠「写凭据文件」来切账号。
pub fn is_switchable(provider: &str) -> bool {
    matches!(provider, "minimax" | "miaoshou" | "qoder")
}

/// 读客户端当前登录的是哪个账号（只读，不改任何东西）。
pub fn detect(provider: &str) -> Result<Option<DetectedAccount>> {
    match provider {
        "minimax" => detect_minimax(),
        "miaoshou" => detect_miaoshou(),
        "qoder" => Ok(detect_qoder()),
        _ => Ok(None),
    }
}

/// 切换客户端登录的账号，然后以新账号启动客户端。
///
/// **关闭 → 写入 → 重启一条龙**（借鉴 Cockpit Tools 的 `launch_on_switch`）：
/// 客户端在运行时先自动收掉它（优雅关闭，超时强制结束），不再要求用户手动退出。
/// 客户端退出时会把内存里的登录态写回文件，所以**写凭据之前必须等进程完全退出**，
/// 否则写入的文件会被它覆盖（这是换号失败最常见的姿势）。
pub fn switch(provider: &str, cred: &Value, account_id: &str, label: &str) -> Result<SwitchOutcome> {
    if crate::core::launcher::is_running(provider)
        && !crate::core::launcher::close_client(provider)
    {
        return Err(anyhow!(
            "客户端正在运行且无法自动关闭（可能权限不足）：请手动退出后重试"
        ));
    }
    let outcome = match provider {
        "minimax" => switch_minimax(cred, account_id, label)?,
        "miaoshou" => switch_miaoshou(cred, account_id, label)?,
        "qoder" => switch_qoder(cred, account_id, label)?,
        other => return Err(anyhow!("{other} 不支持切换客户端账号")),
    };
    Ok(outcome)
}

// ---------------------------------------------------------------------------
// MiniMax Code
// ---------------------------------------------------------------------------

/// 递归找 MiniMax 的登录态文件（层级随版本变，不写死）。
fn minimax_auth_file() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let root = home.join(".minimax").join("auth");
    let mut out = Vec::new();
    collect(&root, &mut out, 0);
    out.into_iter().next()
}

fn collect(dir: &PathBuf, out: &mut Vec<PathBuf>, depth: usize) {
    if depth > 4 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, out, depth + 1);
        } else if path.file_name().and_then(|n| n.to_str()) == Some("auth.json") {
            out.push(path);
        }
    }
}

/// 记录键是客户端生成的**设备级**键（`<clientId>\0<设备hash>`，换号登录不变）。
fn detect_minimax() -> Result<Option<DetectedAccount>> {
    let Some(path) = minimax_auth_file() else {
        return Ok(None);
    };
    let raw = std::fs::read_to_string(&path)?;
    let parsed: Value = serde_json::from_str(&raw)?;
    let Some(records) = parsed["records"].as_object() else {
        return Ok(None);
    };
    for key in records.keys() {
        if let Some(rec) = records.get(key) {
            if rec["accessToken"].as_str().map(|s| !s.is_empty()).unwrap_or(false) {
                // 账号 id 用客户端配置里的 userID（**账号级**，换号会变）。
                // 记录键的 hash 是设备级的，换号后不变，不能当账号 id。
                let id = crate::providers::minimax::local_user_id()
                    .unwrap_or_else(|| key.rsplit('\u{0}').next().unwrap_or(key).to_string());
                return Ok(Some(DetectedAccount {
                    label: crate::providers::minimax::local_label(),
                    account_id: id,
                }));
            }
        }
    }
    Ok(None)
}

fn switch_minimax(cred: &Value, _account_id: &str, _label: &str) -> Result<SwitchOutcome> {
    // 优先用凭据里记的那个文件（就是我们当初导入时读的），找不到再全盘搜
    let path = cred
        .get("auth_file")
        .and_then(|v| v.as_str())
        .map(PathBuf::from)
        .filter(|p| p.is_file())
        .or_else(minimax_auth_file)
        .ok_or_else(|| anyhow!("没找到 MiniMax Code 的登录态文件"))?;

    let raw = std::fs::read_to_string(&path).unwrap_or_else(|_| "{\"schemaVersion\":1,\"records\":{}}".into());
    let parsed: Value =
        serde_json::from_str(&raw).unwrap_or_else(|_| json!({"schemaVersion": 1, "records": {}}));

    let records = parsed["records"].as_object().cloned().unwrap_or_default();

    // 记录键是客户端生成的**设备级**键（`<clientId>\0<设备hash>`，换号登录键不变，
    // 只有 token 换），不能自己拼 —— 必须沿用文件里已有的键。
    // 客户端从没登录过就没有键可用，此时切不了，如实报错。
    let target_key = records
        .iter()
        .find(|(_, rec)| rec["accessToken"].as_str().map(|t| !t.is_empty()).unwrap_or(false))
        .map(|(k, _)| k.clone())
        .or_else(|| records.keys().next().cloned())
        .ok_or_else(|| {
            anyhow!("MiniMax Code 登录态里没有可沿用的记录键：请先在客户端登录任意账号一次，再使用切换")
        })?;

    // 记录本体：沿用现有记录做模板（保留 clientId/scopes/audience/generation 等）
    let mut record = records.get(&target_key).cloned().unwrap_or_else(|| {
        json!({
            "schemaVersion": 1,
            "tokenType": "Bearer",
            "clientId": "mcode-public",
            "scopes": ["agent.default"],
            "audience": "agent-backend",
            "generation": 1
        })
    });

    // 新代数取「状态日记 / 现有凭据」里的最大值 +1 —— 客户端的状态机要求
    // auth-state.json 和 auth.json 的 generation 严格一致，否则判会话不可信、弹重新登录
    let state_path = path.with_file_name("auth-state.json");
    let state: Value = std::fs::read_to_string(&state_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| json!({}));
    let base_gen = state["generation"]
        .as_i64()
        .unwrap_or(0)
        .max(record["generation"].as_i64().unwrap_or(0));

    let obj = record.as_object_mut().ok_or_else(|| anyhow!("MiniMax 登录态格式异常"))?;
    obj.insert(
        "accessToken".into(),
        Value::String(cred["access_token"].as_str().unwrap_or("").to_string()),
    );
    obj.insert(
        "refreshToken".into(),
        Value::String(cred["refresh_token"].as_str().unwrap_or("").to_string()),
    );
    if let Some(ms) = cred.get("expires_at_ms").and_then(|v| v.as_i64()) {
        obj.insert("expiresAtMs".into(), json!(ms));
    }
    if let Some(epoch) = cred.get("login_epoch").and_then(|v| v.as_str()) {
        if !epoch.is_empty() {
            obj.insert("loginEpoch".into(), json!(epoch));
        }
    }
    obj.insert("generation".into(), json!(base_gen + 1));

    // 只留目标账号那一条 —— 客户端是「取第一条非空 token」，留着别的会被它抢先
    let mut new_records = Map::new();
    new_records.insert(target_key, record);
    let mut out = parsed;
    out["records"] = Value::Object(new_records);
    if out.get("schemaVersion").is_none() {
        out["schemaVersion"] = json!(1);
    }

    // 凭据和状态日记要一起写、generation 一致：
    // 只写凭据不写状态的话，客户端的状态机会判定会话不可信，启动直接弹重新登录
    //（2026-10-24 实测踩坑：切换后客户端弹登录框，根因就是状态日记对不上）。
    let new_state = json!({
        "schemaVersion": 2,
        "status": "authenticated",
        "storeKind": "file",
        "clientId": "mcode-public",
        "scopes": ["agent.default"],
        "audience": "agent-backend",
        "buildEnv": state["buildEnv"].as_str().unwrap_or("prod"),
        "region": state["region"].as_str().unwrap_or("cn"),
        "generation": base_gen + 1,
    });

    let backup = backup_file(&path)?;
    let _ = backup_file(&state_path);
    atomic_write(&path, serde_json::to_string_pretty(&out)?.as_bytes())?;
    atomic_write(&state_path, serde_json::to_string_pretty(&new_state)?.as_bytes())?;

    let client_exe = crate::core::launcher::launch("minimax")?;
    Ok(SwitchOutcome {
        backup,
        target_file: path.to_string_lossy().to_string(),
        client_exe: client_exe.to_string_lossy().to_string(),
    })
}

// ---------------------------------------------------------------------------
// 妙手（美团 CatPaw）
// ---------------------------------------------------------------------------

fn miaoshou_auth_file() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let path = home.join(".meituan-catpaw").join("auth.json");
    path.is_file().then_some(path)
}

fn detect_miaoshou() -> Result<Option<DetectedAccount>> {
    let Some(path) = miaoshou_auth_file() else {
        return Ok(None);
    };
    let raw = std::fs::read_to_string(&path)?;
    let parsed: Value = serde_json::from_str(&raw)?;
    let uid = parsed["account"]["uid"].as_str().unwrap_or("");
    if uid.is_empty() {
        return Ok(None);
    }
    let label = parsed["account"]["name"]
        .as_str()
        .or_else(|| parsed["account"]["loginName"].as_str())
        .unwrap_or("")
        .to_string();
    Ok(Some(DetectedAccount {
        account_id: uid.to_string(),
        label,
    }))
}

fn switch_miaoshou(cred: &Value, account_id: &str, label: &str) -> Result<SwitchOutcome> {
    let path = miaoshou_auth_file()
        .ok_or_else(|| anyhow!("没找到妙手的登录态文件（客户端还没登录过？）"))?;

    // 以客户端当前的文件为模板，保证 loginType / tokenType / entId 这些字段不丢
    let raw = std::fs::read_to_string(&path).unwrap_or_default();
    let mut parsed: Value =
        serde_json::from_str(&raw).unwrap_or_else(|_| json!({}));

    let uid = cred
        .get("uid")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| account_id.to_string());

    let token = cred["access_token"].as_str().unwrap_or("").to_string();

    // auth 段：保留原字段，只换 token
    let mut auth = parsed["auth"].as_object().cloned().unwrap_or_default();
    auth.insert("accessToken".into(), json!(token));
    if auth.get("loginType").is_none() {
        auth.insert("loginType".into(), json!("passport"));
    }
    if auth.get("tokenType").is_none() {
        auth.insert("tokenType".into(), json!("Bearer"));
    }

    // account 段：整体换成目标账号
    let mut account = parsed["account"].as_object().cloned().unwrap_or_default();
    account.insert("uid".into(), json!(uid));
    account.insert("name".into(), json!(label));
    account.insert("loginName".into(), json!(label));

    parsed["auth"] = Value::Object(auth);
    parsed["account"] = Value::Object(account);
    parsed["scopeSuffix"] = json!(format!("-{uid}"));
    parsed["updatedAt"] = json!(chrono::Utc::now().timestamp_millis());

    let backup = backup_file(&path)?;
    atomic_write(&path, serde_json::to_string_pretty(&parsed)?.as_bytes())?;

    let client_exe = crate::core::launcher::launch("miaoshou")?;
    Ok(SwitchOutcome {
        backup,
        target_file: path.to_string_lossy().to_string(),
        client_exe: client_exe.to_string_lossy().to_string(),
    })
}

// ---------------------------------------------------------------------------
// Qoder CN（Electron safeStorage 加密：可检测，也可注入）
// ---------------------------------------------------------------------------

/// Qoder CN 客户端的 Electron userData 目录。
fn qoder_user_data() -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA")?;
    let dir = PathBuf::from(appdata).join("com.qodercn.app.stable");
    dir.is_dir().then_some(dir)
}

/// 解密客户端的 `auth.v1.dat`（OSCrypt v10）。文件不存在返回 `Ok(None)`。
fn qoder_read_auth() -> Result<Option<Value>> {
    let Some(dir) = qoder_user_data() else {
        return Ok(None);
    };
    let auth_file = dir.join("auth.v1.dat");
    if !auth_file.is_file() {
        return Ok(None);
    }
    let key = os_crypt::read_key(&dir.join("Local State"))?;
    let data = std::fs::read(&auth_file)?;
    let plain = os_crypt::decrypt(&key, &data)?;
    let json = serde_json::from_slice(&plain)
        .map_err(|e| anyhow!("auth.v1.dat 解密成功但内容不是 JSON：{e}"))?;
    Ok(Some(json))
}

/// 账号 id 与设备授权时保持一致：sha256(uid) 前 16 位十六进制。
fn qoder_account_id(uid: &str) -> String {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(uid.as_bytes());
    let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
    hex[..16].to_string()
}

/// 毫秒时间戳 → RFC3339（auth.v1.dat 里的 `expiresAt` 用这种格式，如 `2026-11-12T09:16:12Z`）。
fn ms_to_rfc3339(ms: i64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(ms)
        .map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
        .unwrap_or_default()
}

/// RFC3339 → 毫秒时间戳（读 auth.v1.dat 时用）。
fn rfc3339_to_ms(s: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.timestamp_millis())
        .unwrap_or(0)
}

/// Qoder 客户端**当前登录**的账号 + 完整凭据（解密 `auth.v1.dat`，只读不写）。
///
/// 「切换客户端账号」切走前用它把当前登录捕获进账号池 —— 客户端同一时刻
/// 只能登一个号，切走后这份凭据就丢了；先捕获，谁都不丢。
/// 这里连 `authenticationContext` 一起存进凭据，比设备授权拿到的还全。
pub fn qoder_client_login() -> Option<(crate::core::provider::Account, Value)> {
    let auth = qoder_read_auth().ok()??;
    let uid = auth["user"]["id"].as_str().filter(|s| !s.is_empty())?;
    let name = auth["user"]["name"].as_str().unwrap_or("");
    let account = crate::core::provider::Account {
        id: qoder_account_id(uid),
        provider: "qoder".to_string(),
        label: if name.is_empty() {
            format!("Qoder {}", &uid[..uid.len().min(12)])
        } else {
            name.to_string()
        },
        created_at: chrono::Utc::now().timestamp(),
        last_used: None,
        origin: "Qoder 客户端登录态（切换时捕获）".to_string(),
    };
    let cred = json!({
        "access_token": auth["token"].as_str().unwrap_or(""),
        "refresh_token": auth["refreshToken"].as_str().unwrap_or(""),
        "user_id": uid,
        "expires_at": auth["expiresAt"].as_str().map(rfc3339_to_ms).unwrap_or(0),
        "refresh_expires_at": auth["refreshTokenExpiresAt"].as_str().map(rfc3339_to_ms).unwrap_or(0),
        "machine_id": crate::providers::qoder::local_machine_id(),
        "region": "cn",
        "authentication_context": auth.get("authenticationContext").cloned().unwrap_or(Value::Null),
    });
    Some((account, cred))
}

/// 客户端当前登录的账号。
///
/// 首选解密 `auth.v1.dat`（登录态本体，最准）；解不了再退回
/// 客户端写的 `.qoder-app-status.json`。但如果状态文件**明确**说
/// `logged_in == false`，以它为准 —— 登出后 auth.v1.dat 可能还留着
/// 上一次登录的内容，直接解密会把「已登出」误报成「还登着」。
fn detect_qoder() -> Option<DetectedAccount> {
    if let Some(logged_in) = qoder_status_logged_in() {
        if !logged_in {
            return None;
        }
    }
    if let Ok(Some(auth)) = qoder_read_auth() {
        let uid = auth["user"]["id"].as_str().unwrap_or("");
        if !uid.is_empty() {
            return Some(DetectedAccount {
                account_id: qoder_account_id(uid),
                label: auth["user"]["name"].as_str().unwrap_or("").to_string(),
            });
        }
    }
    detect_qoder_from_status()
}

/// 读状态文件的 `logged_in` 字段；文件不在 / 解析不了返回 None（当作「不知道」）。
fn qoder_status_logged_in() -> Option<bool> {
    let home = dirs::home_dir()?;
    let path = home.join(".qoder-cn").join(".qoder-app-status.json");
    let raw = std::fs::read_to_string(path).ok()?;
    let parsed: Value = serde_json::from_str(&raw).ok()?;
    parsed["logged_in"].as_bool()
}

/// 兜底：客户端最近一次运行时写的状态文件里带着 uid。
fn detect_qoder_from_status() -> Option<DetectedAccount> {
    let home = dirs::home_dir()?;
    let path = home.join(".qoder-cn").join(".qoder-app-status.json");
    let raw = std::fs::read_to_string(path).ok()?;
    let parsed: Value = serde_json::from_str(&raw).ok()?;
    if parsed["logged_in"].as_bool() != Some(true) {
        return None;
    }
    let url = parsed["avatar_url"].as_str()?;
    // 形如 https://qoder.com.cn/users/<uid>/default/avatars
    let uid = url.split('/').find(|s| s.len() >= 32 && s.contains('-'))?;
    Some(DetectedAccount {
        account_id: qoder_account_id(uid),
        label: parsed["name"].as_str().unwrap_or("").to_string(),
    })
}

/// `authenticationContext` 的默认值（受众列表，账号无关，实测值）。
/// 仅当客户端还没有 `auth.v1.dat`（从未登录过）时才用它，否则从现有文件里抄。
const QODER_DEFAULT_AUTH_CONTEXT: &str = r#"["https://qoder.cn","https://openapi.qoder.com.cn","732aef47-9cf2-46a2-95fe-4cebb5d0d1fa","prod"]"#;

fn switch_qoder(cred: &Value, _account_id: &str, label: &str) -> Result<SwitchOutcome> {
    let dir = qoder_user_data()
        .ok_or_else(|| anyhow!("没有找到 Qoder CN 客户端的数据目录（客户端从未运行过？）"))?;
    let auth_file = dir.join("auth.v1.dat");

    // AES 密钥来自 Local State（DPAPI 保护）；客户端至少完整启动过一次才会有
    let key = os_crypt::read_key(&dir.join("Local State"))?;

    // authenticationContext（账号无关的受众列表）：优先用凭据里存的
    //（从客户端捕获的凭据带原值），其次抄现有文件，最后用实测默认值
    let current = qoder_read_auth()?;
    let context = cred
        .get("authentication_context")
        .filter(|v| !v.is_null())
        .cloned()
        .or_else(|| {
            current
                .as_ref()
                .and_then(|c| c.get("authenticationContext").cloned())
        })
        .unwrap_or_else(|| json!(QODER_DEFAULT_AUTH_CONTEXT));

    let uid = cred
        .get("user_id")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("账号凭据里没有 user_id，无法写入客户端登录态"))?;

    let now_ms = chrono::Utc::now().timestamp_millis();
    let expires_ms = cred.get("expires_at").and_then(|v| v.as_i64()).unwrap_or(0);
    // refresh token 的有效期设备授权时不一定下发；实测约一年。
    // 拿不到就写「一年后」，宁可写宽也不把客户端逼成「登录态已过期」。
    let refresh_expires_ms = cred
        .get("refresh_expires_at")
        .and_then(|v| v.as_i64())
        .filter(|v| *v > 0)
        .unwrap_or_else(|| now_ms + 365 * 24 * 3600 * 1000);

    let target = json!({
        "schemaVersion": 1,
        "token": cred["access_token"].as_str().unwrap_or(""),
        "refreshToken": cred["refresh_token"].as_str().unwrap_or(""),
        "expiresAt": ms_to_rfc3339(if expires_ms > 0 { expires_ms } else { now_ms }),
        "refreshTokenExpiresAt": ms_to_rfc3339(refresh_expires_ms),
        "user": {
            "id": uid,
            "name": label,
            "email": "",
            "phone": "",
            "avatarUrl": "",
        },
        "authenticationContext": context,
    });

    let plain = serde_json::to_vec_pretty(&target)?;
    let blob = os_crypt::encrypt(&key, &plain)?;

    let backup = if auth_file.is_file() {
        backup_file(&auth_file)?
    } else {
        None
    };
    atomic_write(&auth_file, &blob)?;

    let client_exe = crate::core::launcher::launch("qoder")?;
    Ok(SwitchOutcome {
        backup,
        target_file: auth_file.to_string_lossy().to_string(),
        client_exe: client_exe.to_string_lossy().to_string(),
    })
}

// ---------------------------------------------------------------------------
// 备份
// ---------------------------------------------------------------------------

/// 备份成 `<file>.bak`；已经存在就先留着（不覆盖上一次备份，
/// 客户端自己也会写 .bak，我们最多留一份自己的时间戳备份）。
fn backup_file(path: &std::path::Path) -> Result<Option<String>> {
    let bak = path.with_extension("json.bak");
    if bak.exists() {
        return Ok(Some(bak.to_string_lossy().to_string()));
    }
    std::fs::copy(path, &bak)?;
    Ok(Some(bak.to_string_lossy().to_string()))
}
