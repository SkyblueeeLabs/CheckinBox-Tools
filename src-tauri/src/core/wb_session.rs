//! WorkBuddy 5.6+ 的共享登录态（`workbuddy-desktop.info`）读写。
//!
//! **背景**：新版官方客户端把登录态从 `%APPDATA%\WorkBuddy`（VS Code fork 的
//! `state.vscdb` secret 键）迁到了
//! `%LOCALAPPDATA%\CodeBuddyExtension\Data\Public\auth\workbuddy-desktop.info`
//! （桌面端与 IDE 插件共享的 auth 目录，客户端运行时会持续回写并产生轮转备份）。
//! 旧实现只写 state.vscdb，新客户端根本不读 —— 表现就是「提示切换成功、
//! 客户端账号没变」。
//!
//! **官方字段加密**（实测本机 5.6 客户端）：`auth.accessToken` / `auth.refreshToken` /
//! `account.nickname` / `account.phoneNumber` / `account.departmentFullName` 不是
//! 明文字符串，而是 `{"$wbEncrypted":1,"envelope":"<base64>"}` 信封 ——
//! envelope 解开是 `{suite:1,keyId,nonce,authTag,ciphertext}`，AES-256-GCM。
//!
//! **密钥来源**：密钥不在任何文件里，在客户端自带的 native binding 里
//! （`electron_browser_workbuddy_storage`）。取法（与 Cockpit Tools 一致）：
//! 以 `ELECTRON_RUN_AS_NODE=1` 跑一次客户端 exe，让它以 Node 模式调自己的
//! binding 输出 `atRestSecretKey`（base64 的 32 字节），再 `sha256` 派生 AES 密钥。
//! 纯本地、无窗口、无网络；密钥缓存 60 秒，避免频繁起辅助进程。
//!
//! **字段范围**：只动上面列出的已知字段；解密后若还有未知加密字段，**拒绝写回**
//! （宁可不切，也不把密文当明文落盘弄坏客户端登录态）。
//!
//! 协议与做法参考 jlcodes99/cockpit-tools 的 workbuddy 实现（含 Issue #2567 的
//! 教训），在本机客户端上独立实现并验证。

use aes_gcm::aead::{Aead, Payload};
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use anyhow::{anyhow, Result};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// 共享 auth 目录（Windows：`%LOCALAPPDATA%\CodeBuddyExtension\Data\Public\auth`）。
const AUTH_DIR_REL: &str = r"CodeBuddyExtension\Data\Public\auth";
/// 登录态文件名（客户端自己会写轮转备份 `workbuddy-desktop.<ts>.<pid>.<uuid>.info`）。
const AUTH_FILE_NAME: &str = "workbuddy-desktop.info";
/// 登出标记：客户端登出时会写 `<file>.logged-out`，在就视为「没登录」。
const LOGOUT_MARKER_SUFFIX: &str = ".logged-out";

const KEY_TIMEOUT: Duration = Duration::from_secs(5);
const KEY_TTL: Duration = Duration::from_secs(60);

/// 取密钥的 Node 脚本：让客户端 exe 以 Node 模式调自己的 binding，输出
/// `sha256(atRestSecretKey)` 的 base64（44 字符）。密钥只走匿名管道，
/// 不进参数/环境变量/日志。
const KEY_SCRIPT: &str = r#"
try {
  const c = require('crypto');
  const p = JSON.parse(process._linkedBinding('electron_browser_workbuddy_storage').loggerGet());
  if (p.version !== 1 || typeof p.atRestSecretKey !== 'string') process.exit(2);
  const b = Buffer.from(p.atRestSecretKey, 'base64');
  if (b.length !== 32 || b.toString('base64') !== p.atRestSecretKey || b.every(x => x === 0)) process.exit(2);
  const key = c.createHash('sha256').update(p.atRestSecretKey, 'utf8').digest();
  process.stdout.write(key.toString('base64'));
  key.fill(0); b.fill(0);
} catch (_) { process.exit(2); }
"#;

// ---------------------------------------------------------------------------
// 路径
// ---------------------------------------------------------------------------

/// 共享登录态文件路径；本机没有 `CodeBuddyExtension` 目录时返回 None
///（说明客户端太旧，还走 state.vscdb 的老路子）。
pub fn auth_file() -> Option<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA")?;
    let path = PathBuf::from(local).join(AUTH_DIR_REL).join(AUTH_FILE_NAME);
    Some(path)
}

fn logout_marker(auth_file: &std::path::Path) -> PathBuf {
    let mut s = auth_file.as_os_str().to_os_string();
    s.push(LOGOUT_MARKER_SUFFIX);
    PathBuf::from(s)
}

/// 读登录态原文（不解密）。文件不存在 / 有登出标记 / JSON 坏了 → None。
pub fn read_session_raw() -> Option<Value> {
    let path = auth_file()?;
    if !path.is_file() || logout_marker(&path).exists() {
        return None;
    }
    let raw = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&raw).ok()
}

// ---------------------------------------------------------------------------
// 官方加密信封
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Envelope {
    suite: u32,
    key_id: String,
    nonce: String,
    auth_tag: String,
    ciphertext: String,
}

/// 递归判断 JSON 里还有没有 `$wbEncrypted` 信封（编/解码后的完整性检查用）。
pub fn contains_encrypted_wrapper(value: &Value) -> bool {
    match value {
        Value::Object(obj) => {
            obj.contains_key("$wbEncrypted") || obj.values().any(contains_encrypted_wrapper)
        }
        Value::Array(items) => items.iter().any(contains_encrypted_wrapper),
        _ => false,
    }
}

fn decode_b64_strict(encoded: &str) -> Result<Vec<u8>> {
    if encoded.len() > 16 * 1024 * 1024 {
        return Err(anyhow!("官方加密字段超长"));
    }
    let bytes = B64
        .decode(encoded)
        .map_err(|_| anyhow!("官方加密字段不是合法 base64"))?;
    // base64 有两种字母表变体，重编码不一致说明客户端用了我们不认的变体
    if B64.encode(&bytes) != encoded {
        return Err(anyhow!("官方加密字段 base64 不是规范形式"));
    }
    Ok(bytes)
}

/// 官方 AES-256-GCM 密钥（来自客户端 native binding）。
#[derive(Clone)]
pub struct WbKey {
    bytes: [u8; 32],
    /// keyId = sha256(key) 前 16 位 hex —— envelope 里的 `keyId` 与之匹配，
    /// 不匹配说明客户端换了密钥（升级/重登），缓存要作废。
    id: String,
}

impl WbKey {
    fn new(bytes: [u8; 32]) -> Self {
        let id = format!("{:x}", Sha256::digest(bytes))[..16].to_owned();
        Self { bytes, id }
    }

    /// 官方客户端的 AAD 帧（逆向实测，一字节都不能差，否则 GCM 校验不过）。
    fn aad(&self) -> Vec<u8> {
        let mut bytes = b"WB-AAD\0\x01".to_vec();
        for value in ["WBEV1", "sym-v1"] {
            bytes.extend_from_slice(&(value.len() as u32).to_be_bytes());
            bytes.extend_from_slice(value.as_bytes());
        }
        bytes.extend_from_slice(&1u32.to_be_bytes());
        bytes.extend_from_slice(&(self.id.len() as u32).to_be_bytes());
        bytes.extend_from_slice(self.id.as_bytes());
        bytes.extend_from_slice(&[2, 0, 0]); // 字段帧类型，无 sequence/final
        bytes
    }

    /// 解一个信封，返回明文字符串。
    pub fn open(&self, wrapper: &Value) -> Result<String> {
        let obj = wrapper
            .as_object()
            .ok_or_else(|| anyhow!("官方加密字段形态不对"))?;
        if obj.len() != 2 || obj.get("$wbEncrypted") != Some(&json!(1)) {
            return Err(anyhow!("官方加密格式不受支持（$wbEncrypted != 1）"));
        }
        let encoded = obj
            .get("envelope")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("官方加密字段缺 envelope"))?;
        let envelope: Envelope = serde_json::from_slice(&decode_b64_strict(encoded)?)
            .map_err(|_| anyhow!("官方加密信封解析失败"))?;
        if envelope.suite != 1 {
            return Err(anyhow!("官方加密 suite 不受支持：{}", envelope.suite));
        }
        if envelope.key_id != self.id {
            return Err(anyhow!("KEY_MISMATCH：客户端密钥已变化"));
        }
        let nonce = decode_b64_strict(&envelope.nonce)?;
        let tag = decode_b64_strict(&envelope.auth_tag)?;
        if nonce.len() != 12 || tag.len() != 16 {
            return Err(anyhow!("官方加密 nonce/tag 长度不对"));
        }
        let mut ciphertext = decode_b64_strict(&envelope.ciphertext)?;
        ciphertext.extend_from_slice(&tag);
        let cipher = Aes256Gcm::new_from_slice(&self.bytes)?;
        let plain = cipher
            .decrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &ciphertext,
                    aad: &self.aad(),
                },
            )
            .map_err(|_| anyhow!("官方加密字段校验失败（密钥或数据不对）"))?;
        String::from_utf8(plain).map_err(|_| anyhow!("官方加密字段解出非 UTF-8"))
    }

    /// 加密成信封（随机 nonce）。
    pub fn seal(&self, plaintext: &str) -> Result<Value> {
        let mut nonce = [0u8; 12];
        getrandom::getrandom(&mut nonce).map_err(|e| anyhow!("随机数生成失败：{e}"))?;
        self.seal_with_nonce(plaintext, &nonce)
    }

    /// 固定 nonce 的加密（测试用，生产走 `seal`）。
    fn seal_with_nonce(&self, plaintext: &str, nonce: &[u8; 12]) -> Result<Value> {
        let cipher = Aes256Gcm::new_from_slice(&self.bytes)?;
        let sealed = cipher
            .encrypt(
                Nonce::from_slice(nonce),
                Payload {
                    msg: plaintext.as_bytes(),
                    aad: &self.aad(),
                },
            )
            .map_err(|_| anyhow!("官方加密失败"))?;
        let split = sealed.len() - 16;
        let envelope = Envelope {
            suite: 1,
            key_id: self.id.clone(),
            nonce: B64.encode(nonce),
            auth_tag: B64.encode(&sealed[split..]),
            ciphertext: B64.encode(&sealed[..split]),
        };
        let bytes = serde_json::to_vec(&envelope)?;
        Ok(json!({ "$wbEncrypted": 1, "envelope": B64.encode(bytes) }))
    }
}

// ---------------------------------------------------------------------------
// 密钥获取（跑客户端 exe 的 Node 模式）+ 60 秒缓存
// ---------------------------------------------------------------------------

static KEY_CACHE: Mutex<Option<(WbKey, Instant, PathBuf)>> = Mutex::new(None);

/// 密钥缓存失效（客户端升级换了密钥时调）。
pub fn invalidate_key() {
    if let Ok(mut cache) = KEY_CACHE.lock() {
        *cache = None;
    }
}

/// 拿官方密钥：缓存 60 秒内直接用，否则跑一次客户端 exe 的 Node 模式。
pub fn prepare_key() -> Result<WbKey> {
    prepare_key_for(None)
}

/// 关闭客户端前确认加密登录态可读，并保留本次切换已定位的可执行文件。
pub fn prepare_switch_key(exe: &Path) -> Result<()> {
    if read_session_raw().as_ref().is_some_and(contains_encrypted_wrapper) {
        prepare_key_for(Some(exe))?;
    }
    Ok(())
}

fn prepare_key_for(exe: Option<&Path>) -> Result<WbKey> {
    if let Ok(cache) = KEY_CACHE.lock() {
        if let Some((key, at, path)) = &*cache {
            if at.elapsed() < KEY_TTL && exe.map_or(true, |exe| exe == path) {
                return Ok(key.clone());
            }
        }
    }
    let exe = exe.map(Path::to_path_buf)
        .or_else(|| crate::core::launcher::client_exe("workbuddy"))
        .ok_or_else(|| anyhow!("没有找到 WorkBuddy 客户端，无法读取官方加密密钥"))?;
    let key = load_key(&exe)?;
    if let Ok(mut cache) = KEY_CACHE.lock() {
        *cache = Some((key.clone(), Instant::now(), exe));
    }
    Ok(key)
}

/// 跑客户端 exe 取密钥。客户端已关闭或正在运行都行 —— Node 模式不碰 GUI/单实例锁。
fn load_key(exe: &Path) -> Result<WbKey> {
    let mut command = std::process::Command::new(exe);
    command
        .env("ELECTRON_RUN_AS_NODE", "1")
        .env_remove("NODE_OPTIONS")
        .args(["-e", KEY_SCRIPT])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = command
        .spawn()
        .map_err(|e| anyhow!("启动 WorkBuddy 密钥辅助进程失败：{e}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("密钥辅助进程没有可读输出"))?;

    // 在辅助线程读输出：客户端挂死时主线程能按超时收掉它
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        use std::io::Read;
        let mut buf = Vec::new();
        let mut stdout = stdout;
        let _ = stdout.read_to_end(&mut buf);
        let _ = tx.send(buf);
    });

    let output = match rx.recv_timeout(KEY_TIMEOUT) {
        Ok(buf) => buf,
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(anyhow!("读取 WorkBuddy 官方密钥超时，请重试"));
        }
    };
    let status = child.wait()?;

    if !status.success() {
        return Err(anyhow!(
            "当前 WorkBuddy 客户端没有提供官方密钥接口（可能版本过旧或过新），已停止覆盖以避免破坏登录态"
        ));
    }
    // 期望 44 字符 base64 = 32 字节密钥
    if output.len() != 44 {
        return Err(anyhow!(
            "官方密钥输出长度不对（{} 字节），客户端版本可能已变更",
            output.len()
        ));
    }
    let encoded = String::from_utf8(output).map_err(|_| anyhow!("官方密钥不是 UTF-8"))?;
    let decoded = decode_b64_strict(encoded.trim())?;
    let bytes = <[u8; 32]>::try_from(decoded.as_slice())
        .map_err(|_| anyhow!("官方密钥长度不是 32 字节"))?;
    Ok(WbKey::new(bytes))
}

// ---------------------------------------------------------------------------
// 会话级解码 / 编码（只动已知字段）
// ---------------------------------------------------------------------------

/// 会话里带敏感字段的路径（与官方客户端写入范围一致）。
const AUTH_SECRET_FIELDS: [&str; 2] = ["accessToken", "refreshToken"];
const ACCOUNT_SECRET_FIELDS: [&str; 3] = ["nickname", "phoneNumber", "departmentFullName"];

fn decode_field(value: &mut Value, container: &str, field: &str, key: &WbKey) -> Result<()> {
    if let Some(item) = value.get_mut(container) {
        if let Some(slot) = item.get_mut(field) {
            if slot.get("$wbEncrypted").is_some() {
                *slot = json!(key.open(slot)?);
            }
        }
    }
    Ok(())
}

fn encode_field(value: &mut Value, container: &str, field: &str, key: &WbKey) -> Result<()> {
    if let Some(item) = value.get_mut(container) {
        if let Some(slot) = item.get_mut(field) {
            if let Some(plain) = slot.as_str() {
                *slot = key.seal(plain)?;
            } else if !slot.is_null() {
                return Err(anyhow!(
                    "官方加密字段 {container}.{field} 不是字符串，无法加密"
                ));
            }
        }
    }
    Ok(())
}

/// 解密会话里的已知加密字段。没有加密字段时原样返回（旧版明文文件不需要密钥）。
pub fn decode_session(value: &Value) -> Result<Value> {
    let mut decoded = value.clone();
    if !contains_encrypted_wrapper(&decoded) {
        return Ok(decoded);
    }
    let key = prepare_key()?;
    for field in AUTH_SECRET_FIELDS {
        decode_field(&mut decoded, "auth", field, &key)?;
    }
    for field in ACCOUNT_SECRET_FIELDS {
        decode_field(&mut decoded, "account", field, &key)?;
    }
    for list in ["accounts", "allAccounts"] {
        if let Some(items) = decoded.get_mut(list).and_then(Value::as_array_mut) {
            for item in items.iter_mut() {
                for field in ACCOUNT_SECRET_FIELDS {
                    if let Some(slot) = item.get_mut(field) {
                        if slot.get("$wbEncrypted").is_some() {
                            *slot = json!(key.open(slot)?);
                        }
                    }
                }
            }
        }
    }
    // 解完还有没认出来的加密字段：拒绝继续，避免把未知密文当明文落盘
    if contains_encrypted_wrapper(&decoded) {
        invalidate_key();
        return Err(anyhow!(
            "登录态里存在未识别的官方加密字段，已停止覆盖以避免破坏登录状态（请更新本工具或重新登录客户端）"
        ));
    }
    Ok(decoded)
}

/// 把会话里的已知敏感字段重新加密。输入里不允许已有加密字段（先 decode）。
pub fn encode_session(value: &Value) -> Result<Value> {
    if contains_encrypted_wrapper(value) {
        return Err(anyhow!("会话里仍有未解密的官方加密字段，拒绝写回"));
    }
    let key = prepare_key()?;
    let mut encoded = value.clone();
    for field in AUTH_SECRET_FIELDS {
        encode_field(&mut encoded, "auth", field, &key)?;
    }
    for field in ACCOUNT_SECRET_FIELDS {
        encode_field(&mut encoded, "account", field, &key)?;
    }
    for list in ["accounts", "allAccounts"] {
        if let Some(items) = encoded.get_mut(list).and_then(Value::as_array_mut) {
            for item in items.iter_mut() {
                for field in ACCOUNT_SECRET_FIELDS {
                    if let Some(slot) = item.get_mut(field) {
                        if let Some(plain) = slot.as_str() {
                            *slot = key.seal(plain)?;
                        }
                    }
                }
            }
        }
    }
    Ok(encoded)
}

// ---------------------------------------------------------------------------
// 会话构造（换号 = 用目标账号改写会话，模板来自客户端当前文件）
// ---------------------------------------------------------------------------

fn seconds_until_ms(timestamp_ms: i64, now_ms: i64) -> i64 {
    if timestamp_ms > now_ms {
        (timestamp_ms - now_ms) / 1000
    } else {
        0
    }
}

/// accounts / allAccounts 列表合并：目标账号 upsert 进去（lastLogin=true），
/// 其它账号标记 lastLogin=false。没有列表字段时建一个只含目标账号的列表。
fn merge_account_list(items: Option<Vec<Value>>, account_value: &Value) -> Vec<Value> {
    let target_uid = account_value.get("uid").and_then(Value::as_str);
    let mut found = false;
    let mut merged = items
        .unwrap_or_default()
        .into_iter()
        .map(|mut item| {
            let matches =
                target_uid.is_some() && item.get("uid").and_then(Value::as_str) == target_uid;
            if matches {
                found = true;
                return account_value.clone();
            }
            if let Some(obj) = item.as_object_mut() {
                obj.insert("lastLogin".to_string(), json!(false));
            }
            item
        })
        .collect::<Vec<_>>();
    if !found {
        merged.push(account_value.clone());
    }
    merged
}

/// 用目标账号凭据改写登录会话。
///
/// * `base`：客户端当前登录态（**已解密**），只当模板 —— 保留 sessionState、
///   deployStatus 等账号无关字段；`account` 块**不用**旧账号的（uin/oneid 这些
///   是身份字段，张冠李戴会让客户端认错人），优先用凭据里存的
///   `account_profile`（捕获该账号时存下的它自己的资料）。
/// * 返回的会话是**明文**，调用方负责 `encode_session` 后落盘。
pub fn build_session(cred: &Value, label: &str, base: Option<&Value>) -> Result<Value> {
    let uid = cred
        .get("uid")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("账号凭据里没有 uid，无法写入 WorkBuddy 登录态"))?;

    let base_obj = base.and_then(Value::as_object);

    // account：目标账号自己的档案，没有就用最小集（uid + nickname）。
    // 档案必须已经是解密后的明文 —— 还带官方信封的话整体弃用
    // （build 之后会再加密，带着信封进去会被 encode_session 拒绝）
    let mut account = cred
        .get("account_profile")
        .filter(|v| !contains_encrypted_wrapper(v))
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    account.insert("uid".to_string(), json!(uid));
    account.insert("nickname".to_string(), json!(label));
    // 模板里有 lastLogin 字段就补上（目标账号 = 当前登录）
    if base_obj
        .and_then(|obj| obj.get("account"))
        .and_then(Value::as_object)
        .is_some_and(|obj| obj.contains_key("lastLogin"))
    {
        account.insert("lastLogin".to_string(), json!(true));
    }
    let account_value = Value::Object(account);

    // auth：旧文件的 auth 当模板（保留 sessionState 等），换掉身份与 token
    let mut auth = base_obj
        .and_then(|obj| obj.get("auth"))
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let now_ms = chrono::Utc::now().timestamp_millis();
    auth.insert(
        "accessToken".to_string(),
        json!(cred["access_token"].as_str().unwrap_or("")),
    );
    auth.insert(
        "refreshToken".to_string(),
        json!(cred["refresh_token"].as_str().unwrap_or("")),
    );
    auth.insert("tokenType".to_string(), json!("Bearer"));
    auth.insert(
        "domain".to_string(),
        json!(cred["domain"].as_str().unwrap_or("")),
    );
    auth.insert("lastRefreshTime".to_string(), json!(now_ms));

    let expires_at = cred.get("expires_at").and_then(Value::as_i64).unwrap_or(0);
    if expires_at > 0 {
        auth.insert("expiresAt".to_string(), json!(expires_at));
        auth.insert(
            "expiresIn".to_string(),
            json!(seconds_until_ms(expires_at, now_ms)),
        );
        // refresh 有效期凭据里没有；旧文件里有就沿用，没有就跟 access 一致
        let refresh_expires_at = auth
            .get("refreshExpiresAt")
            .and_then(Value::as_i64)
            .filter(|v| *v > 0)
            .unwrap_or(expires_at);
        auth.insert("refreshExpiresAt".to_string(), json!(refresh_expires_at));
        auth.insert(
            "refreshExpiresIn".to_string(),
            json!(seconds_until_ms(refresh_expires_at, now_ms)),
        );
    }

    let mut session = base_obj.cloned().unwrap_or_default();
    session.insert("account".to_string(), account_value.clone());
    session.insert("auth".to_string(), Value::Object(auth));
    session.insert(
        "accounts".to_string(),
        json!(merge_account_list(
            base_obj
                .and_then(|obj| obj.get("accounts"))
                .and_then(Value::as_array)
                .cloned(),
            &account_value
        )),
    );
    session.insert(
        "allAccounts".to_string(),
        json!(merge_account_list(
            base_obj
                .and_then(|obj| obj.get("allAccounts"))
                .and_then(Value::as_array)
                .cloned(),
            &account_value
        )),
    );
    Ok(Value::Object(session))
}

// ---------------------------------------------------------------------------
// 换号写入（备份 + 防并发覆盖）
// ---------------------------------------------------------------------------

/// 把目标账号写入共享登录态文件，返回 `(写入文件, 备份文件)`。
///
/// 流程（对齐 Cockpit 的 `write_account_to_auth_file`）：
/// 读当前文件 → 有官方加密就解密 → 用目标凭据改写 → 敏感字段重新加密 →
/// 备份 → 写盘前复查文件没被并发改动 → 原子写。
pub fn write_switch(cred: &Value, label: &str) -> Result<(PathBuf, Option<PathBuf>)> {
    let path = auth_file().ok_or_else(|| anyhow!("本机没有 WorkBuddy 共享登录态目录"))?;
    std::fs::create_dir_all(path.parent().unwrap_or(&path))?;

    let before = if path.is_file() {
        Some(std::fs::read(&path)?)
    } else {
        None
    };

    // 现有文件带官方加密 → 解密改写后重新加密回写；明文文件（罕见）原样明文回写；
    // 全新文件（客户端从未登录过）写明文，客户端下次刷新会自己加密
    let (base, encrypt) = match &before {
        Some(bytes) => {
            let raw: Value = serde_json::from_slice(bytes)
                .map_err(|_| anyhow!("现有登录态不是合法 JSON，已停止覆盖"))?;
            let encrypt = contains_encrypted_wrapper(&raw);
            (Some(decode_session(&raw)?), encrypt)
        }
        None => (None, false),
    };

    let session = build_session(cred, label, base.as_ref())?;
    let session = if encrypt {
        encode_session(&session)?
    } else {
        session
    };

    let content = serde_json::to_string_pretty(&session)?;
    let bytes = content.as_bytes();

    // 写盘前复查：客户端（哪怕刚被我们关掉）可能还有收尾写入
    if let Some(expected) = &before {
        let current = std::fs::read(&path).unwrap_or_default();
        if &current != expected {
            return Err(anyhow!("登录态文件在切换期间被官方客户端更新，请重试"));
        }
    }

    // 备份（留一份我们自己的，可回滚；客户端自己还有轮转备份）
    let backup = match &before {
        Some(expected) => {
            let bak = backup_path(&path);
            if !bak.exists() {
                std::fs::write(&bak, expected)?;
            }
            Some(bak)
        }
        None => None,
    };

    crate::core::paths::atomic_write(&path, bytes)?;
    Ok((path, backup))
}

fn backup_path(path: &std::path::Path) -> PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(".bak");
    PathBuf::from(s)
}

/// 从登录态文件读「客户端当前登录」的账号身份（uid + 明文昵称）。
///
/// **不起密钥辅助进程**：uid 在官方文件里就是明文，overview 每 45 秒轮询一次
/// 检测，不能每轮都拉起客户端 exe。昵称是加密的就拿 uid 兜底。
pub fn detect_identity() -> Option<(String, String)> {
    let session = read_session_raw()?;
    let uid = session["account"]["uid"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())?
        .to_string();
    let label = match session["account"]["nickname"] {
        Value::String(ref s) if !s.trim().is_empty() => s.clone(),
        _ => format!("WorkBuddy {}", &uid[..uid.len().min(8)]),
    };
    Some((uid, label))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试密钥 [7;32] 的 keyId（Cockpit 的公开测试向量同源，帧一致）。
    fn test_key() -> WbKey {
        WbKey::new([7; 32])
    }

    /// 独立构造的信封（key [7;32]、nonce [3;12]、明文 "fixture-token-测试"），
    /// 钉死 AAD 帧 / 密钥派生 / 信封结构 —— 一处不对 open() 就失败。
    #[test]
    fn 信封解密_官方帧向量() {
        let envelope = r#"{"suite":1,"keyId":"4bb06f8e4e3a7715","nonce":"AwMDAwMDAwMDAwMD","authTag":"8mXI80XfjtTaNENkKD5E5Q==","ciphertext":"Q5fbdy9aO28OLyg5hXoZuWbUT3o="}"#;
        let wrapper = json!({"$wbEncrypted":1,"envelope":B64.encode(envelope)});
        assert_eq!(test_key().open(&wrapper).unwrap(), "fixture-token-测试");
    }

    #[test]
    fn 信封加密解密_往返一致() {
        let key = test_key();
        let wrapper = key.seal("往返测试-token").unwrap();
        assert_eq!(key.open(&wrapper).unwrap(), "往返测试-token");
        // 信封结构齐全
        let env: Envelope =
            serde_json::from_slice(&B64.decode(wrapper["envelope"].as_str().unwrap()).unwrap())
                .unwrap();
        assert_eq!(env.suite, 1);
        assert_eq!(env.key_id, key.id);
    }

    #[test]
    fn 错密钥与篡改都解不开() {
        let key = test_key();
        let wrapper = key.seal("secret").unwrap();
        // 错密钥
        assert!(WbKey::new([8; 32]).open(&wrapper).is_err());
        // 篡改 ciphertext / nonce / tag / keyId
        let mut env: Envelope =
            serde_json::from_slice(&B64.decode(wrapper["envelope"].as_str().unwrap()).unwrap())
                .unwrap();
        env.ciphertext = B64.encode([0u8; 32]);
        let tampered =
            json!({"$wbEncrypted":1,"envelope":B64.encode(serde_json::to_vec(&env).unwrap())});
        assert!(key.open(&tampered).is_err());
        // 非 $wbEncrypted:1
        assert!(key.open(&json!({"$wbEncrypted":2,"envelope":""})).is_err());
    }

    #[test]
    fn 固定nonce加密_与解密向量一致() {
        // seal_with_nonce([3;12]) + key [7;32] 必须复现测试向量（AAD 帧一致）
        let key = test_key();
        let nonce = [3u8; 12];
        let wrapper = key.seal_with_nonce("fixture-token-测试", &nonce).unwrap();
        let env: Envelope =
            serde_json::from_slice(&B64.decode(wrapper["envelope"].as_str().unwrap()).unwrap())
                .unwrap();
        assert_eq!(env.nonce, "AwMDAwMDAwMDAwMD");
        assert_eq!(env.auth_tag, "8mXI80XfjtTaNENkKD5E5Q==");
        assert_eq!(env.ciphertext, "Q5fbdy9aO28OLyg5hXoZuWbUT3o=");
    }

    #[test]
    fn 会话解码编码_只动已知字段() {
        let key = test_key();
        let plain = json!({
            "auth": {"accessToken": "access", "refreshToken": "refresh", "expiresAt": 42, "sessionState": "s1"},
            "account": {"uid": "u1", "nickname": "测试", "phoneNumber": "13800000000", "departmentFullName": "team"},
            "accounts": [{"uid": "u2", "nickname": "Other", "lastLogin": false}],
            "allAccounts": [{"uid": "u1", "nickname": "测试"}],
            "unknown": {"revision": 1}
        });
        // 编码：敏感字段变信封，其它字段原样
        let encoded = encode_with_key_for_test(&plain, &key).unwrap();
        assert!(encoded["auth"]["accessToken"].is_object());
        assert!(encoded["account"]["phoneNumber"].is_object());
        assert!(encoded["accounts"][0]["nickname"].is_object());
        assert_eq!(encoded["auth"]["expiresAt"], 42);
        assert_eq!(encoded["auth"]["sessionState"], "s1");
        assert_eq!(encoded["unknown"]["revision"], 1);
        // 解码：还原明文
        let decoded = decode_with_key_for_test(&encoded, &key).unwrap();
        assert_eq!(decoded, plain);
        // 再编码 nonce 是新的，密文不同但内容一致
        let again = encode_with_key_for_test(&plain, &key).unwrap();
        assert_ne!(again, encoded);
        assert_eq!(decode_with_key_for_test(&again, &key).unwrap(), plain);
    }

    #[test]
    fn 未识别的加密字段拒绝解码() {
        let key = test_key();
        let mut session = json!({"auth": {"accessToken": key.seal("t").unwrap()}});
        // 解密已知字段没问题
        assert!(decode_with_key_for_test(&session, &key).is_ok());
        // 塞一个未知路径的加密字段 → 拒绝
        session["mystery"] = json!({"v": key.seal("x").unwrap()});
        assert!(decode_with_key_for_test(&session, &key).is_err());
        // 编码入口拒绝带信封的输入
        assert!(encode_with_key_for_test(&session, &key).is_err());
    }

    /// 测试直连版（绕开密钥缓存进程），只做字段编解码。
    fn decode_with_key_for_test(value: &Value, key: &WbKey) -> Result<Value> {
        let mut decoded = value.clone();
        for field in AUTH_SECRET_FIELDS {
            decode_field(&mut decoded, "auth", field, key)?;
        }
        for field in ACCOUNT_SECRET_FIELDS {
            decode_field(&mut decoded, "account", field, key)?;
        }
        for list in ["accounts", "allAccounts"] {
            if let Some(items) = decoded.get_mut(list).and_then(Value::as_array_mut) {
                for item in items.iter_mut() {
                    for field in ACCOUNT_SECRET_FIELDS {
                        if let Some(slot) = item.get_mut(field) {
                            if slot.get("$wbEncrypted").is_some() {
                                *slot = json!(key.open(slot)?);
                            }
                        }
                    }
                }
            }
        }
        if contains_encrypted_wrapper(&decoded) {
            return Err(anyhow!("存在未识别的加密字段"));
        }
        Ok(decoded)
    }

    fn encode_with_key_for_test(value: &Value, key: &WbKey) -> Result<Value> {
        if contains_encrypted_wrapper(value) {
            return Err(anyhow!("输入仍带加密字段"));
        }
        let mut encoded = value.clone();
        for field in AUTH_SECRET_FIELDS {
            encode_field(&mut encoded, "auth", field, key)?;
        }
        for field in ACCOUNT_SECRET_FIELDS {
            encode_field(&mut encoded, "account", field, key)?;
        }
        for list in ["accounts", "allAccounts"] {
            if let Some(items) = encoded.get_mut(list).and_then(Value::as_array_mut) {
                for item in items.iter_mut() {
                    for field in ACCOUNT_SECRET_FIELDS {
                        if let Some(slot) = item.get_mut(field) {
                            if let Some(plain) = slot.as_str() {
                                *slot = key.seal(plain)?;
                            }
                        }
                    }
                }
            }
        }
        Ok(encoded)
    }

    #[test]
    fn 会话构造_换uid换token_列表合并() {
        let base = json!({
            "account": {"uid": "old", "uin": "old_uin", "nickname": "旧账号", "accountType": "personal", "lastLogin": true},
            "auth": {"accessToken": "old", "refreshToken": "oldr", "sessionState": "ss",
                     "expiresAt": 1, "refreshExpiresAt": 2, "domain": "www.codebuddy.cn"},
            "accounts": [{"uid": "old", "nickname": "旧账号", "lastLogin": true},
                         {"uid": "u9", "nickname": "路人"}],
            "allAccounts": [{"uid": "old", "lastLogin": true}]
        });
        let cred = json!({
            "uid": "new_uid",
            "access_token": "at",
            "refresh_token": "rt",
            "expires_at": 4_102_444_800_000i64,
            "domain": "www.workbuddy.cn"
        });
        let session = build_session(&cred, "新账号", Some(&base)).unwrap();

        // account 不带旧账号的身份字段（uin/accountType 是旧号的）
        let account = &session["account"];
        assert_eq!(account["uid"], "new_uid");
        assert_eq!(account["nickname"], "新账号");
        assert!(account.get("uin").is_none(), "旧账号的 uin 不能留下来");

        // auth 换了 token，保留了 sessionState，有效期重算
        let auth = &session["auth"];
        assert_eq!(auth["accessToken"], "at");
        assert_eq!(auth["refreshToken"], "rt");
        assert_eq!(auth["domain"], "www.workbuddy.cn");
        assert_eq!(auth["sessionState"], "ss");
        assert_eq!(auth["expiresAt"], 4_102_444_800_000i64);
        assert!(auth["expiresIn"].as_i64().unwrap() > 0);
        assert_eq!(auth["refreshExpiresAt"], 2, "沿用旧文件的 refresh 有效期");

        // accounts / allAccounts：目标不在旧列表里 → 追加；旧账号保留但降级
        // （列表 = 这台机器登录过的所有账号，目标标 lastLogin=true）
        let accounts = session["accounts"].as_array().unwrap();
        assert_eq!(accounts.len(), 3);
        let target = accounts.iter().find(|a| a["uid"] == "new_uid").unwrap();
        assert_eq!(target["lastLogin"], true);
        assert_eq!(
            accounts.iter().find(|a| a["uid"] == "u9").unwrap()["lastLogin"],
            false
        );
        assert_eq!(
            accounts.iter().find(|a| a["uid"] == "old").unwrap()["lastLogin"],
            false
        );
        let all = session["allAccounts"].as_array().unwrap();
        assert_eq!(all.len(), 2, "allAccounts 也是追加");
        assert_eq!(all[1]["uid"], "new_uid");
        assert_eq!(all[0]["lastLogin"], false);
    }

    #[test]
    fn 会话构造_没有档案时用最小account() {
        let cred = json!({
            "uid": "u1", "access_token": "at", "refresh_token": "rt",
            "expires_at": 0, "domain": ""
        });
        let session = build_session(&cred, "某人", None).unwrap();
        assert_eq!(session["account"]["uid"], "u1");
        assert_eq!(session["account"]["nickname"], "某人");
        assert_eq!(session["auth"]["accessToken"], "at");
        // expires_at == 0：不写有效期字段（客户端刷新时会自己算）
        assert!(session["auth"].get("expiresAt").is_none());
        // 没有 base：列表只含目标账号
        assert_eq!(session["accounts"].as_array().unwrap().len(), 1);
        assert_eq!(session["allAccounts"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn 会话构造_凭据档案带加密字段时弃用() {
        // account_profile 必须是解密后的明文；万一还带信封就整体弃用
        let cred = json!({
            "uid": "u1", "access_token": "at", "refresh_token": "rt",
            "account_profile": {"uid": "u1", "nickname": {"$wbEncrypted": 1, "envelope": "xxx"}}
        });
        let session = build_session(&cred, "某人", None).unwrap();
        assert_eq!(
            session["account"]["nickname"], "某人",
            "带信封的档案不能直接写回"
        );
    }

    /// 诊断用：真实客户端的密钥读取 + 会话解密（**只读**，不写任何文件）。
    ///
    /// 验证三件事：密钥辅助进程能跑通、现有登录态能按官方帧解密、
    /// 我们的再加密能被同一套帧解回来（不落盘）。
    /// 手动验证：`cargo test -- --ignored 真实WorkBuddy会话能解密 --nocapture`
    #[test]
    #[ignore = "依赖本机 WorkBuddy 客户端与登录态，手动跑"]
    fn 真实WorkBuddy会话能解密() {
        let key = prepare_key().expect("密钥读取失败");
        eprintln!("key_id = {}", key.id);
        let raw = read_session_raw().expect("没有读到共享登录态文件");
        assert!(contains_encrypted_wrapper(&raw), "现有文件应带官方加密信封");
        let session = decode_session(&raw).expect("解密失败");
        eprintln!(
            "uid = {} · nickname = {} · domain = {}",
            session["account"]["uid"], session["account"]["nickname"], session["auth"]["domain"]
        );
        assert!(
            session["auth"]["accessToken"]
                .as_str()
                .is_some_and(|s| !s.is_empty()),
            "accessToken 应解出非空明文"
        );
        // 再加密 → 再解密：证明我们的编码帧与官方一致（全程只在内存里）
        let encoded = encode_session(&session).expect("重新加密失败");
        assert!(contains_encrypted_wrapper(&encoded));
        assert_eq!(decode_session(&encoded).unwrap(), session);
    }
}
