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
//!   * Qoder CN 只能**检测**（客户端 token 是 Electron safeStorage 加密的，格式不同，没法注入）
//!
//! ⚠️ 这会改写客户端的登录凭据。调用前必须确认客户端没在运行 ——
//! 否则客户端内存里的旧登录态会覆盖我们写进去的文件。

use anyhow::{anyhow, Result};
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::path::PathBuf;

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
    matches!(provider, "minimax" | "miaoshou")
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

/// 切换客户端登录的账号，然后启动客户端。
pub fn switch(provider: &str, cred: &Value, account_id: &str, label: &str) -> Result<SwitchOutcome> {
    let outcome = match provider {
        "minimax" => switch_minimax(cred, account_id, label)?,
        "miaoshou" => switch_miaoshou(cred, account_id, label)?,
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

/// 记录键是 `<clientId 前缀>\0<账号 id>`，取前缀用来拼新账号的键。
fn record_prefix(records: &Map<String, Value>) -> Option<String> {
    records.keys().next().and_then(|k| {
        k.rsplitn(2, '\u{0}')
            .last()
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty())
    })
}

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
                let id = key.rsplit('\u{0}').next().unwrap_or(key).to_string();
                return Ok(Some(DetectedAccount {
                    label: crate::providers::minimax::local_label(),
                    account_id: id,
                }));
            }
        }
    }
    Ok(None)
}

fn switch_minimax(cred: &Value, account_id: &str, _label: &str) -> Result<SwitchOutcome> {
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

    // 备份客户端原来的登录态
    let backup = backup_file(&path)?;

    let records = parsed["records"].as_object().cloned().unwrap_or_default();

    // 键前缀：沿用文件里已有的；没有就用已知的 prod/cn 组合
    let prefix = record_prefix(&records)
        .unwrap_or_else(|| "com.minimax.mcode.oauth.prod.cn".to_string());
    let target_key = format!("{prefix}\u{0}{account_id}");

    // 记录本体：优先复用同账号的旧记录（保留 clientId/scopes/audience/generation），
    // 没有就拿任意一条当模板，再没有就现场造一个。
    let mut record = records
        .get(&target_key)
        .cloned()
        .or_else(|| records.values().next().cloned())
        .unwrap_or_else(|| {
            json!({
                "schemaVersion": 1,
                "tokenType": "Bearer",
                "clientId": "mcode-public",
                "scopes": ["agent.default"],
                "audience": "agent-backend",
                "generation": 1
            })
        });

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
    // 续期代数 +1，让客户端知道这是新写入的一版
    let gen = obj.get("generation").and_then(|v| v.as_i64()).unwrap_or(0);
    obj.insert("generation".into(), json!(gen + 1));

    // 只留目标账号那一条 —— 客户端是「取第一条非空 token」，留着别的会被它抢先
    let mut new_records = Map::new();
    new_records.insert(target_key, record);
    let mut out = parsed;
    out["records"] = Value::Object(new_records);
    if out.get("schemaVersion").is_none() {
        out["schemaVersion"] = json!(1);
    }

    atomic_write(&path, serde_json::to_string_pretty(&out)?.as_bytes())?;

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
// Qoder CN（只能检测，不能注入）
// ---------------------------------------------------------------------------

/// Qoder 客户端把当前登录账号写进了 `~/.qoder-cn/.qoder-app-status.json`，
/// `avatar_url` 里带着 uid：`.../users/<uid>/default/avatars`。
fn detect_qoder() -> Option<DetectedAccount> {
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
    // 账号 id 与设备授权时一致：sha256(uid) 的前 16 位十六进制
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(uid.as_bytes());
    let id: String = hash.iter().map(|b| format!("{b:02x}")).collect();
    let id = id[..16].to_string();
    let label = parsed["name"].as_str().unwrap_or("").to_string();
    Some(DetectedAccount {
        account_id: id,
        label,
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
