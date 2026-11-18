//! Trae CN（字节跳动 TRAE 国内版）适配器。
//!
//! 协议来源：本机客户端 2.3.103559 的 `out/main.js` + 实测（2026-11-17）。
//!
//! 关键事实：
//!   * 签到接口：`POST https://api.trae.cn/trae/api/v2/ug/checkin_credits/{status,claim}`
//!     body `{"req_source":1}`（IDE 谱系）/ `{"req_source":2}`（SOLO 谱系）。
//!   * 请求头必须带 `Authorization: Cloud-IDE-JWT <token>`、`X-User-Region: CN`、
//!     `x-device-id: <12~20 位纯数字 Aha 设备号>`。
//!   * 积分：`POST /trae/api/v2/pay/ide_user_ent_usage`，真实余额取
//!     `usage_summary.total_amount - consumed_amount`，`total_balance` 只做交叉校验。
//!   * 本机登录态在 `%APPDATA%\Trae CN\User\globalStorage\storage.json`，
//!     `iCubeAuthInfo://*` 是 `tc` 格式（AES-128-CBC + SHA-512 完整性校验），
//!     设备条目 `iCubeAuthInfo://icube-dc:<设备号>` 里还存着 EC P-256 设备密钥。
//!   * 刷新 token 用当前客户端的 v3 接口，必须用设备私钥对
//!     `POST\n/path\nClientID\nRefreshToken\nTimestamp\nNonce` 做 ECDSA P-256 签名。
//!   * 一台设备一天只能签一个账号（code=9095）；设备号不被认可是 code=9074，
//!     文案「当前参与用户太多」是幌子，不要当拥塞重试。

use anyhow::{anyhow, bail, Context, Result};
use async_trait::async_trait;
use base64::{
    engine::general_purpose::{STANDARD as B64, URL_SAFE_NO_PAD as B64URL},
    Engine as _,
};
use cbc::cipher::{block_padding::Pkcs7, BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use p256::ecdsa::{signature::Signer, Signature, SigningKey};
use p256::pkcs8::{DecodePrivateKey, EncodePrivateKey, EncodePublicKey, LineEnding};
use serde_json::{json, Value};
use sha2::{Digest, Sha256, Sha512};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::core::provider::{
    Account, AccountProfile, CheckinStatus, ClaimOutcome, ClaimState, CreditItem, Provider, Quota,
};
use crate::providers::qoder::AuthSession;

const DEFAULT_API_HOST: &str = "https://api.trae.cn";
const STATUS_PATH: &str = "/trae/api/v2/ug/checkin_credits/status";
const CLAIM_PATH: &str = "/trae/api/v2/ug/checkin_credits/claim";
const USAGE_PATH: &str = "/trae/api/v2/pay/ide_user_ent_usage";
const REFRESH_PATH: &str = "/trae/api/v3/oauth/ExchangeToken";
const LEGACY_REFRESH_PATH: &str = "/cloudide/api/v3/trae/oauth/ExchangeToken";

const CLIENT_ID_IDE: &str = "ono9krqynydwx5";
const CLIENT_ID_SOLO: &str = "en1oxy7wnw8j9n";
const FALLBACK_CLIENT_VERSION: &str = "3.4.20";
const FALLBACK_TRON_VERSION: &str = "2.3.103559";

const APP_DIRS: &[&str] = &["Trae CN", "TRAE SOLO CN", "Trae", "Trae Solo"];

// ---------------------------------------------------------------------------
// tc 存储格式（Trae 客户端自己的 byteCrypto）
// ---------------------------------------------------------------------------

const TC_BLOCK: usize = 16;
const TC_HEADER: usize = 6;
const TC_HASH: usize = 64;
const TC_KEY_MATERIAL: usize = 32;
const TC_MAGIC: [u8; TC_HEADER] = [b't', b'c', 0x05, 0x10, 0x00, 0x00];
const TC_PRIVATE_MAGIC: [u8; TC_HEADER] = [18, 57, 32, 32, 2, 3];

/// 标准 `tc` 格式的硬编码 KDF 材料（客户端 byteCrypto.js 里的 JG）。
const TC_AES_A: [u8; 64] = [
    82, 9, 106, 213, 48, 54, 165, 56, 191, 64, 163, 158, 129, 243, 215, 251, 124, 227, 57, 130,
    155, 47, 255, 135, 52, 142, 67, 68, 196, 222, 233, 203, 84, 123, 148, 50, 166, 194, 35, 61,
    238, 76, 149, 11, 66, 250, 195, 78, 8, 46, 161, 102, 40, 217, 36, 178, 118, 91, 162, 73, 109,
    139, 209, 37,
];
/// 标准 `tc` 格式的硬编码 KDF 材料（客户端 byteCrypto.js 里的 KG）。
const TC_AES_B: [u8; 64] = [
    31, 221, 168, 51, 136, 7, 199, 49, 177, 18, 16, 89, 39, 128, 236, 95, 96, 81, 127, 169, 25,
    181, 74, 13, 45, 229, 122, 159, 147, 201, 156, 239, 160, 224, 59, 77, 174, 42, 245, 176, 200,
    235, 187, 60, 131, 83, 153, 97, 23, 43, 4, 126, 186, 119, 214, 38, 225, 105, 20, 99, 85, 33,
    12, 125,
];
/// 少数客户端版本使用的 private 变体（magic `[18,57,32,32,2,3]`）。
const TC_PRIVATE_A: [u8; 64] = [
    191, 192, 216, 250, 122, 246, 220, 97, 31, 254, 98, 27, 8, 72, 71, 176, 135, 99, 96, 18, 127,
    101, 203, 104, 211, 102, 191, 125, 37, 72, 150, 156, 51, 229, 121, 35, 17, 153, 141, 177, 110,
    131, 150, 128, 172, 255, 254, 6, 18, 140, 55, 62, 236, 249, 135, 64, 135, 12, 117, 4, 89, 149,
    168, 209,
];
const TC_PRIVATE_B: [u8; 64] = [
    246, 204, 26, 232, 232, 70, 129, 109, 223, 146, 169, 242, 23, 241, 105, 145, 50, 196, 165, 42,
    254, 120, 3, 54, 244, 207, 209, 85, 53, 6, 138, 106, 175, 148, 31, 204, 186, 186, 165, 182, 87,
    142, 49, 10, 39, 110, 26, 154, 86, 56, 173, 125, 18, 64, 198, 225, 99, 99, 83, 82, 191, 134,
    76, 170,
];

type Aes128CbcEnc = cbc::Encryptor<aes::Aes128>;
type Aes128CbcDec = cbc::Decryptor<aes::Aes128>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TcVersion {
    Aes,
    AesPrivate,
}

impl TcVersion {
    fn from_header(header: &[u8]) -> Option<Self> {
        if header == TC_MAGIC {
            Some(Self::Aes)
        } else if header == TC_PRIVATE_MAGIC {
            Some(Self::AesPrivate)
        } else {
            None
        }
    }

    fn salt(self) -> [u8; 64] {
        let (left, right) = match self {
            Self::Aes => (&TC_AES_A, &TC_AES_B),
            Self::AesPrivate => (&TC_PRIVATE_A, &TC_PRIVATE_B),
        };
        let mut out = [0u8; 64];
        for i in 0..64 {
            out[i] = left[i] ^ right[i];
        }
        out
    }
}

fn tc_derive_key_iv(key_material: &[u8], version: TcVersion) -> Option<([u8; 16], [u8; 16])> {
    if key_material.len() != TC_KEY_MATERIAL {
        return None;
    }
    let key_hash = Sha512::digest(key_material);
    let mut merged = [0u8; 128];
    merged[..TC_HASH].copy_from_slice(&key_hash);
    merged[TC_HASH..].copy_from_slice(&version.salt());
    let out = Sha512::digest(merged);
    let mut key = [0u8; 16];
    let mut iv = [0u8; 16];
    key.copy_from_slice(&out[..16]);
    iv.copy_from_slice(&out[16..32]);
    Some((key, iv))
}

fn tc_decrypt(raw: &[u8]) -> Result<Vec<u8>> {
    if raw.len() < TC_HEADER + TC_KEY_MATERIAL + TC_BLOCK {
        bail!("Trae 登录态太短");
    }
    let version = TcVersion::from_header(&raw[..TC_HEADER])
        .ok_or_else(|| anyhow!("不支持的 Trae 登录态加密格式"))?;
    let key_material = &raw[TC_HEADER..TC_HEADER + TC_KEY_MATERIAL];
    let ciphertext = &raw[TC_HEADER + TC_KEY_MATERIAL..];
    if ciphertext.is_empty() || ciphertext.len() % TC_BLOCK != 0 {
        bail!("Trae 登录态密文长度异常");
    }
    let (key, iv) = tc_derive_key_iv(key_material, version)
        .ok_or_else(|| anyhow!("Trae 登录态密钥派生失败"))?;
    let mut buf = ciphertext.to_vec();
    let plain = Aes128CbcDec::new_from_slices(&key, &iv)
        .map_err(|e| anyhow!("Trae 登录态解密器初始化失败：{e}"))?
        .decrypt_padded_mut::<Pkcs7>(&mut buf)
        .map_err(|_| anyhow!("Trae 登录态解密失败（PKCS7）"))?;
    if plain.len() < TC_HASH {
        bail!("Trae 登录态明文太短");
    }
    let (hash, data) = plain.split_at(TC_HASH);
    if Sha512::digest(data).as_slice() != hash {
        bail!("Trae 登录态完整性校验失败");
    }
    Ok(data.to_vec())
}

fn tc_decrypt_json(b64: &str) -> Result<Value> {
    let raw = B64
        .decode(b64.trim())
        .context("Trae 登录态 base64 解码失败")?;
    let plain = tc_decrypt(&raw)?;
    serde_json::from_slice(&plain).context("Trae 登录态不是合法 JSON")
}

fn tc_encrypt_bytes(plain: &[u8]) -> Result<Vec<u8>> {
    let mut key_material = [0u8; TC_KEY_MATERIAL];
    getrandom::getrandom(&mut key_material)
        .map_err(|e| anyhow!("生成 Trae 登录态密钥失败：{e}"))?;
    let (key, iv) = tc_derive_key_iv(&key_material, TcVersion::Aes)
        .ok_or_else(|| anyhow!("Trae 登录态密钥派生失败"))?;

    let mut payload = Vec::with_capacity(TC_HASH + plain.len() + TC_BLOCK);
    payload.extend_from_slice(&Sha512::digest(plain));
    payload.extend_from_slice(plain);
    let msg_len = payload.len();
    payload.resize(msg_len + TC_BLOCK, 0);
    let encrypted = Aes128CbcEnc::new_from_slices(&key, &iv)
        .map_err(|e| anyhow!("Trae 登录态加密器初始化失败：{e}"))?
        .encrypt_padded_mut::<Pkcs7>(&mut payload, msg_len)
        .map_err(|_| anyhow!("Trae 登录态加密失败"))?;

    let mut out = Vec::with_capacity(TC_HEADER + TC_KEY_MATERIAL + encrypted.len());
    out.extend_from_slice(&TC_MAGIC);
    out.extend_from_slice(&key_material);
    out.extend_from_slice(encrypted);
    Ok(out)
}

fn tc_encrypt_json(value: &Value) -> Result<String> {
    let plain = serde_json::to_vec(value)?;
    Ok(B64.encode(tc_encrypt_bytes(&plain)?))
}

// ---------------------------------------------------------------------------
// 本机客户端读取
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct ClientMeta {
    product_line: &'static str,
    client_id: &'static str,
    req_source: i64,
    client_version: String,
    tron_version: String,
}

#[derive(Debug, Clone)]
struct LocalAuth {
    app_dir: PathBuf,
    auth: Value,
    device_id: String,
    device: Value,
    meta: ClientMeta,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientLoginState {
    Same(Value),
    Other,
    Unknown,
}

#[derive(Debug, Clone)]
struct OAuthContext {
    verifier: String,
    login_trace: String,
    device_id: String,
    machine_id: String,
    product_line: &'static str,
    client_id: String,
    req_source: i64,
    client_version: String,
    tron_version: String,
    private_key_pem: String,
    public_key_pem: String,
    created_at: i64,
}

#[derive(Debug, Clone)]
struct OAuthCallback {
    login_trace: String,
    query: serde_json::Map<String, Value>,
}

pub struct TraeProvider {
    http: reqwest::Client,
    /// OAuth 会话的私钥只放内存，不随 `pending_auth` 落盘。
    oauth_sessions: Mutex<HashMap<String, OAuthContext>>,
    oauth_callback: Arc<Mutex<Option<OAuthCallback>>>,
}

impl TraeProvider {
    pub fn new() -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .user_agent(format!("Trae/{FALLBACK_CLIENT_VERSION}"))
            .build()
            .unwrap_or_default();
        Self {
            http,
            oauth_sessions: Mutex::new(HashMap::new()),
            oauth_callback: Arc::new(Mutex::new(None)),
        }
    }

    async fn post(
        &self,
        cred: &Value,
        path: &str,
        body: Value,
    ) -> Result<(reqwest::StatusCode, Value)> {
        let url = format!("{}{}", api_host(cred), path);
        let resp = self
            .http
            .post(url)
            .headers(request_headers(cred))
            .json(&body)
            .send()
            .await?;
        let status = resp.status();
        let text = resp.text().await?;
        let value = serde_json::from_str(&text).unwrap_or(Value::Null);
        Ok((status, value))
    }
}

impl Default for TraeProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for TraeProvider {
    fn id(&self) -> &'static str {
        "trae"
    }

    fn display_name(&self) -> &'static str {
        "Trae CN"
    }

    fn accent(&self) -> &'static str {
        "#4f6ef7"
    }

    fn docs_url(&self) -> &'static str {
        "https://docs.trae.cn/ide_plans-and-billing"
    }

    async fn discover(&self) -> Result<Vec<(Account, Value)>> {
        let Some(local) = read_local_auth() else {
            return Ok(Vec::new());
        };
        let cred = cred_from_local(&local);
        let mut account = account_from_cred(&cred);
        account.origin = "本机 Trae CN".to_string();
        Ok(vec![(account, cred)])
    }

    async fn status(&self, _account: &Account, cred: &Value) -> Result<CheckinStatus> {
        let (status, body) = self
            .post(cred, STATUS_PATH, json!({ "req_source": req_source(cred) }))
            .await?;
        classify_http(status, "签到状态查询")?;
        parse_status_body(&body)
    }

    async fn claim(&self, account: &Account, cred: &Value) -> Result<ClaimOutcome> {
        let current = self.status(account, cred).await?;
        if current.state == ClaimState::Claimed {
            return Ok(ClaimOutcome::already(current.points, "今日已签到"));
        }
        if current.state != ClaimState::Claimable {
            return Ok(ClaimOutcome::failed(current.message));
        }

        let (status, body) = self
            .post(cred, CLAIM_PATH, json!({ "req_source": req_source(cred) }))
            .await?;
        classify_http(status, "签到")?;
        if status.as_u16() >= 400 {
            let message = body["message"].as_str().unwrap_or("");
            if message.contains("已签到") || message.to_lowercase().contains("already check") {
                return Ok(ClaimOutcome::already(current.points, "今日已签到"));
            }
            bail!("Trae 签到失败：HTTP {} {}", status.as_u16(), message);
        }
        parse_claim_body(&body, current.points)
    }

    async fn quota(&self, _account: &Account, cred: &Value) -> Result<Quota> {
        let (status, body) = self
            .post(
                cred,
                USAGE_PATH,
                json!({ "require_usage": true, "req_source": req_source(cred) }),
            )
            .await?;
        classify_http(status, "积分查询")?;
        if status.as_u16() >= 400 {
            bail!("Trae 积分查询失败：HTTP {}", status.as_u16());
        }
        Ok(parse_quota_body(&body))
    }

    async fn profile(&self, _account: &Account, cred: &Value) -> Result<Option<AccountProfile>> {
        let name = cred["nickname"].as_str().filter(|s| !s.trim().is_empty());
        let email = cred["email"].as_str().filter(|s| !s.trim().is_empty());
        let avatar = cred["avatar"].as_str().filter(|s| !s.trim().is_empty());
        if name.is_none() && email.is_none() && avatar.is_none() {
            return Ok(None);
        }
        Ok(Some(AccountProfile {
            name: name.map(str::to_string),
            email: email.map(str::to_string),
            avatar: avatar.map(str::to_string),
        }))
    }

    async fn refresh(&self, _account: &Account, cred: &Value) -> Result<Option<Value>> {
        let refresh_token = cred["refresh_token"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow!("Trae 凭据缺少 refresh token"))?;
        let private_pem = cred["device_private_key_pem"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow!("Trae 凭据缺少设备私钥"))?;
        let client_id = cred["client_id"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or(CLIENT_ID_IDE);

        let timestamp = chrono::Utc::now().timestamp();
        let nonce = random_hex(16)?;
        let canonical = format!(
            "POST\n{}\n{}\n{}\n{}\n{}",
            REFRESH_PATH, client_id, refresh_token, timestamp, nonce
        );
        let signature = sign_der_base64(private_pem, canonical.as_bytes())?;

        let body = json!({
            "ClientID": client_id,
            "ClientSecret": "",
            "RefreshToken": refresh_token,
            "DeviceInfo": device_info(cred),
            "DeviceProof": {
                "Signature": signature,
                "Timestamp": timestamp,
                "Nonce": nonce,
            },
            "IDEVersion": client_version(cred),
        });

        let url = format!("{}{}", api_host(cred), REFRESH_PATH);
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("content-type", "application/json".parse().unwrap());
        if let Some(token) = cred["access_token"].as_str().filter(|s| !s.is_empty()) {
            if let Ok(value) = token.parse() {
                headers.insert("x-cloudide-token", value);
            }
        }
        let resp = self
            .http
            .post(&url)
            .headers(headers)
            .json(&body)
            .send()
            .await?;
        let status = resp.status();
        let text = resp.text().await?;
        let value: Value = serde_json::from_str(&text).unwrap_or(Value::Null);

        // 只对「接口不存在」回落旧路径；401/400/业务错误不回落，避免掩盖真实原因。
        if matches!(status.as_u16(), 404 | 405 | 501) {
            return self.refresh_legacy(cred, refresh_token, client_id).await;
        }
        if status.as_u16() == 401 || status.as_u16() == 403 {
            bail!("Trae refresh token 已失效，请重新导入或重新授权");
        }
        if !status.is_success() {
            let msg = value["ResponseMetadata"]["Error"]["Message"]
                .as_str()
                .or_else(|| value["message"].as_str())
                .unwrap_or("");
            bail!("Trae token 刷新失败：HTTP {} {}", status.as_u16(), msg);
        }
        parse_refresh_result(cred, &value)
    }
}

impl TraeProvider {
    /// 发起浏览器授权：本地监听 127.0.0.1 的回调，返回授权页地址。
    ///
    /// 私钥只放在内存的 `oauth_sessions` 里；`AuthSession` 会持久化到 pending_auth，
    /// 不能把私钥写进去。
    pub async fn begin_authorization(&self) -> Result<AuthSession> {
        let local = read_local_auth();
        let meta = local
            .as_ref()
            .map(|item| item.meta.clone())
            .unwrap_or_else(|| client_meta(None));
        let device_id = local
            .as_ref()
            .map(|item| item.device_id.clone())
            .filter(|id| is_valid_device_id(id))
            .unwrap_or_else(|| machine_device_id(Path::new("trae")));
        let machine_id = local
            .as_ref()
            .map(|item| stable_machine_id(&item.app_dir))
            .unwrap_or_else(|| stable_machine_id(Path::new("trae")));
        // 复用客户端设备密钥：OAuth token 绑定这把公钥，之后切进客户端续期才不会被拒。
        let (private_key_pem, public_key_pem) = local
            .as_ref()
            .and_then(|item| {
                let private = item.device["privateKeyPEM"].as_str()?.to_string();
                let public = item.device["publicKeyPEM"].as_str()?.to_string();
                (!private.is_empty() && !public.is_empty()).then_some((private, public))
            })
            .or_else(generate_keypair)
            .ok_or_else(|| anyhow!("生成 Trae 设备密钥失败"))?;

        let verifier = random_base64url(48)?;
        let challenge = code_challenge(&verifier);
        let login_trace = random_hex(32)?;
        let session_id = uuid::Uuid::new_v4().to_string();

        let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        let port = listener.local_addr()?.port();
        let callback_url = format!("http://127.0.0.1:{port}/authorize");
        let auth_url = build_login_url(
            &meta,
            &device_id,
            &machine_id,
            &login_trace,
            &callback_url,
            &challenge,
        );

        let callback_slot = self.oauth_callback.clone();
        tokio::spawn(run_callback_server(listener, callback_slot));

        let now = chrono::Utc::now().timestamp();
        {
            let mut sessions = self.oauth_sessions.lock().unwrap();
            sessions.retain(|_, ctx| now - ctx.created_at < 300);
            sessions.insert(
                session_id.clone(),
                OAuthContext {
                    verifier: verifier.clone(),
                    login_trace: login_trace.clone(),
                    device_id,
                    machine_id: machine_id.clone(),
                    product_line: meta.product_line,
                    client_id: meta.client_id.to_string(),
                    req_source: meta.req_source,
                    client_version: meta.client_version.clone(),
                    tron_version: meta.tron_version.clone(),
                    private_key_pem,
                    public_key_pem,
                    created_at: now,
                },
            );
        }

        Ok(AuthSession {
            id: session_id,
            provider: "trae".to_string(),
            auth_url,
            verifier,
            nonce: login_trace,
            machine_id,
            created_at: now,
            extra: json!({ "expires_in": 300 }),
        })
    }

    /// 轮询本地回调，拿到 AuthCode 后换 token。`None` = 用户还没完成授权。
    pub async fn poll_authorization(
        &self,
        session: &AuthSession,
    ) -> Result<Option<(Account, Value)>> {
        let context = self
            .oauth_sessions
            .lock()
            .unwrap()
            .get(&session.id)
            .cloned();
        let Some(context) = context else {
            bail!("Trae 授权会话已过期，请重新发起");
        };
        let callback = self.oauth_callback.lock().unwrap().clone();
        let Some(callback) = callback else {
            return Ok(None);
        };
        if callback.login_trace != context.login_trace {
            return Ok(None);
        }

        if let Some(error) = callback
            .query
            .get("error_code")
            .and_then(|value| value.as_str())
            .filter(|value| !value.is_empty())
        {
            let message = callback
                .query
                .get("error_msg")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            self.oauth_callback.lock().unwrap().take();
            self.oauth_sessions.lock().unwrap().remove(&session.id);
            bail!("Trae 授权失败（{error}）：{message}");
        }

        let auth_code_info = callback_json(&callback.query, "authCodeInfo")
            .ok_or_else(|| anyhow!("Trae 授权回调缺少 authCodeInfo"))?;
        let auth_code = auth_code_info["AuthCode"]
            .as_str()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow!("Trae 授权回调缺少 AuthCode"))?;
        let verifier = callback
            .query
            .get("codeVerifier")
            .and_then(|value| value.as_str())
            .filter(|value| !value.is_empty())
            .unwrap_or(&context.verifier)
            .to_string();
        let user_info = callback_json(&callback.query, "userInfo").unwrap_or(Value::Null);

        let cred = match self
            .exchange_auth_code(&context, auth_code, &verifier, &user_info)
            .await
        {
            Ok(cred) => cred,
            Err(error) => {
                self.oauth_callback.lock().unwrap().take();
                self.oauth_sessions.lock().unwrap().remove(&session.id);
                return Err(error);
            }
        };
        self.oauth_callback.lock().unwrap().take();
        self.oauth_sessions.lock().unwrap().remove(&session.id);
        let account = account_from_cred(&cred);
        let mut account = account;
        account.origin = "浏览器授权".to_string();
        Ok(Some((account, cred)))
    }

    async fn exchange_auth_code(
        &self,
        context: &OAuthContext,
        auth_code: &str,
        verifier: &str,
        user_info: &Value,
    ) -> Result<Value> {
        let body = json!({
            "ClientID": context.client_id.clone(),
            "AuthCode": auth_code,
            "CodeVerifier": verifier,
            "DeviceInfo": oauth_device_info(context),
            "IDEVersion": context.client_version.clone(),
        });
        let url = format!("{}{}", DEFAULT_API_HOST, REFRESH_PATH);
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("content-type", "application/json".parse().unwrap());
        headers.insert("x-cloudide-token", "".parse().unwrap());
        let resp = self
            .http
            .post(url)
            .headers(headers)
            .json(&body)
            .send()
            .await?;
        let status = resp.status();
        let text = resp.text().await?;
        if status.as_u16() == 401 || status.as_u16() == 403 {
            bail!("Trae 授权码已失效，请重新发起授权");
        }
        if !status.is_success() {
            bail!("Trae 授权失败：HTTP {}", status.as_u16());
        }
        let value: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        let base = json!({ "refresh_token": "" });
        let fresh = parse_refresh_result(&base, &value)?
            .ok_or_else(|| anyhow!("Trae 授权响应缺少 token"))?;
        let mut user_id = fresh["user_id"]
            .as_str()
            .filter(|value| !value.is_empty())
            .unwrap_or("")
            .to_string();
        if user_id.is_empty() {
            user_id = user_field(user_info, &["UserID", "userId", "uid"]);
        }
        if user_id.is_empty() {
            bail!("Trae 授权响应缺少 user_id");
        }
        let nickname = user_field(user_info, &["ScreenName", "screenName", "username", "name"]);
        let avatar = user_field(
            user_info,
            &["AvatarUrl", "avatarUrl", "avatar_url", "avatar"],
        );
        let email = user_field(user_info, &["NonPlainTextEmail", "email", "Email"]);
        Ok(json!({
            "access_token": fresh["access_token"].clone(),
            "refresh_token": fresh["refresh_token"].clone(),
            "expires_at_ms": fresh["expires_at_ms"].clone(),
            "refresh_expires_at_ms": fresh["refresh_expires_at_ms"].clone(),
            "user_id": user_id,
            "nickname": nickname,
            "avatar": avatar,
            "email": email,
            "host": DEFAULT_API_HOST,
            "region": "CN",
            "product_line": context.product_line,
            "client_id": context.client_id.clone(),
            "req_source": context.req_source,
            "client_version": context.client_version.clone(),
            "tron_version": context.tron_version.clone(),
            "login_device_id": context.device_id.clone(),
            "checkin_device_id": context.device_id.clone(),
            "machine_id": context.machine_id.clone(),
            "device_private_key_pem": context.private_key_pem.clone(),
            "device_public_key_pem": context.public_key_pem.clone(),
            "device_source": "oauth",
            "cred_source": "oauth",
            "device_rotated_at": 0,
            "last_refresh_at": 0,
            "account_json": oauth_account_json(user_info),
            "user_region_json": {"region": "CN", "_aiRegion": "CN"},
        }))
    }

    /// 客户端启动时同款登录态校验。`false` = 会话已被作废，不能写进客户端。
    pub async fn check_login(&self, cred: &Value) -> Result<bool> {
        let url = format!("{}{}", api_host(cred), "/cloudide/api/v3/trae/CheckLogin");
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("content-type", "application/json".parse().unwrap());
        if let Some(token) = cred["access_token"].as_str().filter(|s| !s.is_empty()) {
            if let Ok(value) = token.parse() {
                headers.insert("x-cloudide-token", value);
            }
        }
        let body = json!({
            "IDEVersion": client_version(cred),
            "ReqSource": if cred["product_line"].as_str() == Some("solo") { "Lite" } else { "IDE" },
            "GetAIPayHost": true,
        });
        let resp = self.http.post(url).headers(headers).json(&body).send().await?;
        let status = resp.status();
        let text = resp.text().await?;
        if status.as_u16() == 401 || status.as_u16() == 403 {
            return Ok(false);
        }
        if !status.is_success() {
            bail!("Trae 登录态校验失败：HTTP {}", status.as_u16());
        }
        let value: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        Ok(value["Result"]["IsLogin"].as_bool().unwrap_or(false))
    }

    async fn refresh_legacy(
        &self,
        cred: &Value,
        refresh_token: &str,
        client_id: &str,
    ) -> Result<Option<Value>> {
        let url = format!("{}{}", api_host(cred), LEGACY_REFRESH_PATH);
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("content-type", "application/json".parse().unwrap());
        if let Some(token) = cred["access_token"].as_str().filter(|s| !s.is_empty()) {
            if let Ok(value) = token.parse() {
                headers.insert("x-cloudide-token", value);
            }
        }
        let body = json!({
            "ClientID": client_id,
            "RefreshToken": refresh_token,
            "ClientSecret": "-",
            "UserID": "",
        });
        let resp = self
            .http
            .post(&url)
            .headers(headers)
            .json(&body)
            .send()
            .await?;
        let status = resp.status();
        let text = resp.text().await?;
        let value: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        if status.as_u16() == 401 || status.as_u16() == 403 {
            bail!("Trae refresh token 已失效，请重新导入或重新授权");
        }
        if !status.is_success() {
            bail!("Trae token 刷新失败（legacy）：HTTP {}", status.as_u16());
        }
        parse_refresh_result(cred, &value)
    }

    /// 客户端确认未运行时，把刷新后的 token 回写当前登录账号。
    ///
    /// 目标不是客户端当前账号时不动 storage.json，避免把别的账号顶掉。
    pub fn write_back_local_auth(cred: &Value) -> Result<bool> {
        let Some(local) = read_local_auth() else {
            return Ok(false);
        };
        if local.auth["userId"].as_str() != cred["user_id"].as_str() {
            return Ok(false);
        }
        write_local_credential(cred, false, "refresh")?;
        Ok(true)
    }
}

/// 回写结果（换号时 UI / 备份需要知道实际写入的文件）。
pub struct LocalCredentialWrite {
    pub storage_path: PathBuf,
    pub backup: Option<String>,
}

/// 把凭据写入本机 Trae 的 `storage.json`。
///
/// - `allow_switch=false`：只允许当前登录账号的 token 回写（刷新用）；
/// - `allow_switch=true`：允许把 storage 切成另一个账号（换号用）。
///
/// 只改 `iCubeAuthInfo://icube.cloudide` 和 `iCubeAuthInfo://usertag`，保留设备
/// 密钥、会话和其余字段；写前用 `client_backup` 按代备份，写完再解一次校验。
pub fn write_local_credential(
    cred: &Value,
    allow_switch: bool,
    reason: &str,
) -> Result<LocalCredentialWrite> {
    let local = read_local_auth().ok_or_else(|| anyhow!("没有找到本机 Trae CN 登录态"))?;
    let local_uid = local.auth["userId"].as_str().unwrap_or("");
    let uid = cred["user_id"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("Trae 凭据缺少 user_id"))?;
    if !local_uid.is_empty() && local_uid != uid && !allow_switch {
        bail!("目标账号不是客户端当前登录账号，拒绝回写");
    }

    let storage_path = local
        .app_dir
        .join("User")
        .join("globalStorage")
        .join("storage.json");
    let backup = crate::core::client_backup::backup_client_login(
        "trae",
        &[(storage_path.clone(), "storage.json".to_string())],
        reason,
    )
    .map_err(|e| anyhow!("备份 Trae 登录态失败：{e}"))?;

    let raw = std::fs::read_to_string(&storage_path).context("读取 Trae storage.json 失败")?;
    let mut storage: Value =
        serde_json::from_str(&raw).context("解析 Trae storage.json 失败")?;

    let mut auth = local.auth.clone();
    if let Some(token) = cred["access_token"].as_str() {
        auth["token"] = json!(token);
    }
    if let Some(token) = cred["refresh_token"].as_str() {
        auth["refreshToken"] = json!(token);
    }
    if let Some(ms) = cred["expires_at_ms"].as_i64().filter(|v| *v > 0) {
        auth["expiredAt"] = json!(iso_from_ms(ms));
    }
    if let Some(ms) = cred["refresh_expires_at_ms"].as_i64().filter(|v| *v > 0) {
        auth["refreshExpiredAt"] = json!(iso_from_ms(ms));
    }
    auth["tokenReleaseAt"] = json!(iso_from_ms(now_ms()));

    if allow_switch {
        auth["userId"] = json!(uid);
        auth["host"] = json!(api_host(cred));
        auth["userRegion"] = cred
            .get("user_region_json")
            .filter(|value| value.is_object())
            .cloned()
            .unwrap_or_else(|| json!({"region": "CN", "_aiRegion": "CN"}));

        // 导入/捕获时保留的账号对象原样写回；旧账号没有该字段时再按目标重建，
        // 并清掉上一个账号的手机号/邮箱/组织，避免串号。
        if let Some(account) = cred.get("account_json").filter(|value| value.is_object()) {
            auth["account"] = account.clone();
        } else {
            if !auth["account"].is_object() {
                auth["account"] = json!({});
            }
            if let Some(account) = auth["account"].as_object_mut() {
                account.insert(
                    "username".to_string(),
                    json!(cred["nickname"].as_str().unwrap_or("")),
                );
                account.insert(
                    "avatar_url".to_string(),
                    json!(cred["avatar"].as_str().unwrap_or("")),
                );
                account.insert(
                    "email".to_string(),
                    json!(cred["email"].as_str().unwrap_or("")),
                );
                account.insert("nonPlainTextEmail".to_string(), json!(""));
                account.insert("nonPlainTextMobile".to_string(), json!(""));
                account.insert("description".to_string(), json!(""));
                account.insert("organization".to_string(), json!(""));
                account.insert("work_country".to_string(), json!(""));
                account.insert("storeCountryCode".to_string(), json!(""));
                account.insert("storeCountrySrc".to_string(), json!(""));
                account.insert("userTag".to_string(), json!("cn"));
                account.insert("migrateToSG".to_string(), json!(false));
            }
        }
    }

    storage["iCubeAuthInfo://icube.cloudide"] = json!(tc_encrypt_json(&auth)?);

    let mut tags = read_usertag_map(&storage);
    tags.insert(uid.to_string(), json!("cn"));
    storage["iCubeAuthInfo://usertag"] = json!(tc_encrypt_json(&Value::Object(tags))?);

    let bytes = serde_json::to_vec_pretty(&storage)?;
    crate::core::paths::atomic_write(&storage_path, &bytes)?;

    // 写完再解一次，确认目标 userId 真的生效（避免半截写入被当成成功）。
    let verify = read_local_auth_in(&local.app_dir)
        .ok_or_else(|| anyhow!("写入后无法重新读取 Trae 登录态"))?;
    if verify.auth["userId"].as_str() != Some(uid) {
        bail!("写入后校验失败：Trae 登录态仍不是目标账号");
    }

    Ok(LocalCredentialWrite {
        storage_path,
        backup: Some(backup.dir),
    })
}

// ---------------------------------------------------------------------------
// 请求/解析
// ---------------------------------------------------------------------------

fn api_host(cred: &Value) -> String {
    cred["host"]
        .as_str()
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_API_HOST)
        .trim_end_matches('/')
        .to_string()
}

fn req_source(cred: &Value) -> i64 {
    cred["req_source"].as_i64().unwrap_or_else(|| {
        if cred["product_line"].as_str() == Some("solo") {
            2
        } else {
            1
        }
    })
}

fn client_version(cred: &Value) -> String {
    cred["client_version"]
        .as_str()
        .filter(|s| !s.is_empty())
        .unwrap_or(FALLBACK_CLIENT_VERSION)
        .to_string()
}

fn checkin_device_id(cred: &Value) -> Option<&str> {
    cred["checkin_device_id"]
        .as_str()
        .or_else(|| cred["device_id"].as_str())
        .filter(|s| !s.is_empty())
}

fn request_headers(cred: &Value) -> reqwest::header::HeaderMap {
    let mut headers = reqwest::header::HeaderMap::new();
    if let Some(token) = cred["access_token"].as_str().filter(|s| !s.is_empty()) {
        if let Ok(value) = format!("Cloud-IDE-JWT {token}").parse() {
            headers.insert(reqwest::header::AUTHORIZATION, value);
        }
    }
    headers.insert("accept", "application/json".parse().unwrap());
    headers.insert("x-user-region", "CN".parse().unwrap());
    if let Some(device) = checkin_device_id(cred) {
        if let Ok(value) = device.parse() {
            headers.insert("x-device-id", value);
        }
    }
    if let Ok(value) = client_version(cred).parse() {
        headers.insert("x-app-version", value);
    }
    headers
}

fn classify_http(status: reqwest::StatusCode, what: &str) -> Result<()> {
    if status.as_u16() == 401 || status.as_u16() == 403 {
        bail!("Trae {what}失败：登录态已过期，请重新导入或打开 Trae 客户端刷新");
    }
    Ok(())
}

fn parse_status_body(body: &Value) -> Result<CheckinStatus> {
    let code = body["code"].as_i64().unwrap_or(0);
    if code != 0 {
        bail!(
            "Trae 签到状态返回 code={code}：{}",
            body["message"].as_str().unwrap_or("")
        );
    }
    let enable = body["enable"].as_bool().unwrap_or(true);
    let checked = body["checked_in"].as_bool().unwrap_or(false);
    let credits = body["credits"].as_i64().unwrap_or(0);

    if !enable {
        return Ok(CheckinStatus {
            state: ClaimState::NotAvailable,
            points: 0,
            streak_days: 0,
            message: "Trae 签到暂未开放".to_string(),
            day_index: None,
            bonus_points: 0,
            days: Vec::new(),
        });
    }
    if checked {
        return Ok(CheckinStatus {
            state: ClaimState::Claimed,
            points: credits,
            streak_days: 0,
            message: "今日已签到".to_string(),
            day_index: None,
            bonus_points: 0,
            days: Vec::new(),
        });
    }
    Ok(CheckinStatus {
        state: ClaimState::Claimable,
        points: credits,
        streak_days: 0,
        message: format!("今日可签 +{credits} 积分"),
        day_index: None,
        bonus_points: 0,
        days: Vec::new(),
    })
}

fn parse_claim_body(body: &Value, points: i64) -> Result<ClaimOutcome> {
    let code = body["code"].as_i64().unwrap_or(0);
    let message = body["message"].as_str().unwrap_or("").trim().to_string();
    match code {
        0 => Ok(ClaimOutcome::claimed(
            points,
            if message.is_empty() {
                "签到成功".to_string()
            } else {
                message
            },
        )),
        9095 => Ok(ClaimOutcome::failed(
            "本设备今日已签到：一台设备一天只能签一个账号",
        )),
        9074 => Ok(ClaimOutcome::failed(
            "设备号未被 Trae 认可（9074）：请重新从本机导入或检查设备号",
        )),
        _ if is_congestion(&message) => Err(anyhow!("Trae 上游繁忙：{message}")),
        _ => Ok(ClaimOutcome::failed(format!(
            "签到失败（code={code}）：{message}"
        ))),
    }
}

fn parse_quota_body(body: &Value) -> Quota {
    let summary = &body["usage_summary"];
    let has_summary =
        summary.get("total_amount").is_some() && summary.get("consumed_amount").is_some();
    let (total, consumed, remaining) = if has_summary {
        let total = as_f64(&summary["total_amount"]);
        let consumed = as_f64(&summary["consumed_amount"]);
        (total, consumed, total - consumed)
    } else {
        let balance = as_f64(&body["total_balance"]);
        (balance, 0.0, balance)
    };

    let mut items = Vec::new();
    if let Some(packs) = body["user_entitlement_pack_list"].as_array() {
        for pack in packs {
            let quota = &pack["entitlement_base_info"]["quota"];
            let granted = as_f64(&quota["credits_limit"]);
            if granted <= 0.0 {
                continue;
            }
            let used = as_f64(&pack["usage"]["credits_amount"]);
            let start = normalize_epoch_ms(as_i64(&pack["entitlement_base_info"]["start_time"]));
            let expire = normalize_epoch_ms(as_i64(&pack["expire_time"]));
            let name = pack["group_name"]
                .as_str()
                .or_else(|| pack["display_desc"].as_str())
                .unwrap_or("")
                .to_string();
            items.push(CreditItem {
                credit_type: "free".to_string(),
                granted,
                consumed: used,
                remaining: (granted - used).max(0.0),
                granted_at_ms: start,
                expire_at_ms: expire,
                package_name: name,
                refresh_at_ms: 0,
            });
        }
    }

    let now = now_ms();
    let refresh_at_ms = items
        .iter()
        .filter(|item| item.remaining > 0.0 && item.expire_at_ms > now)
        .map(|item| item.expire_at_ms)
        .min()
        .unwrap_or(0);

    Quota {
        total_granted: total,
        total_consumed: consumed,
        total_remaining: remaining,
        items,
        message: "签到积分 31 天有效；每月登录赠送 500 自然月有效".to_string(),
        refresh_at_ms,
        credit_groups: Vec::new(),
    }
}

fn parse_refresh_result(cred: &Value, body: &Value) -> Result<Option<Value>> {
    let result = body.get("Result").unwrap_or(body);
    let token = result["Token"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("Trae 刷新响应缺少 Result.Token"))?;
    let refresh_token = result["RefreshToken"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| cred["refresh_token"].as_str().map(str::to_string))
        .unwrap_or_default();

    let now = now_ms();
    let expire_raw = as_i64(&result["TokenExpireAt"]);
    let expire = if expire_raw > 0 {
        normalize_epoch_ms(expire_raw)
    } else {
        let duration = as_i64(&result["TokenExpireDuration"]);
        if duration > 0 {
            now + duration * 1000
        } else {
            now + 14 * 24 * 60 * 60 * 1000
        }
    };
    let refresh_expire = normalize_epoch_ms(as_i64(&result["RefreshExpireAt"]));

    let mut fresh = cred.clone();
    fresh["access_token"] = json!(token);
    fresh["refresh_token"] = json!(refresh_token);
    fresh["expires_at_ms"] = json!(expire);
    if refresh_expire > 0 {
        fresh["refresh_expires_at_ms"] = json!(refresh_expire);
    }
    if let Some(uid) = result["UserID"].as_str().filter(|s| !s.is_empty()) {
        fresh["user_id"] = json!(uid);
    }
    fresh["last_refresh_at"] = json!(now);
    Ok(Some(fresh))
}

fn is_congestion(message: &str) -> bool {
    let lower = message.to_lowercase();
    [
        "太多", "拥挤", "繁忙", "忙碌", "稍后", "重试", "too many", "busy", "later", "retry",
    ]
    .iter()
    .any(|hint| lower.contains(hint))
}

fn as_i64(value: &Value) -> i64 {
    if let Some(v) = value.as_i64() {
        return v;
    }
    if let Some(v) = value.as_u64() {
        return v as i64;
    }
    value
        .as_str()
        .and_then(|s| s.trim().parse::<i64>().ok())
        .unwrap_or(0)
}

fn as_f64(value: &Value) -> f64 {
    if let Some(v) = value.as_f64() {
        return v;
    }
    value
        .as_str()
        .and_then(|s| s.replace(',', "").trim().parse::<f64>().ok())
        .unwrap_or(0.0)
}

fn normalize_epoch_ms(value: i64) -> i64 {
    if value <= 0 {
        return 0;
    }
    if value < 1_000_000_000_000 {
        value * 1000
    } else {
        value
    }
}

fn parse_iso_ms(value: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|dt| dt.timestamp_millis())
        .unwrap_or_else(|_| normalize_epoch_ms(value.parse::<i64>().unwrap_or(0)))
}

fn iso_from_ms(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|dt| dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        .unwrap_or_default()
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn now_secs() -> i64 {
    chrono::Utc::now().timestamp()
}

fn random_hex(bytes: usize) -> Result<String> {
    let mut buf = vec![0u8; bytes];
    getrandom::getrandom(&mut buf).map_err(|e| anyhow!("生成随机数失败：{e}"))?;
    Ok(buf.iter().map(|b| format!("{b:02x}")).collect())
}

fn sha256_hex(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn stable_machine_id(seed: &Path) -> String {
    let mut input = String::new();
    for key in ["COMPUTERNAME", "USERNAME", "APPDATA"] {
        input.push_str(&std::env::var(key).unwrap_or_default());
        input.push('|');
    }
    input.push_str(&seed.to_string_lossy());
    sha256_hex(&input)[..32].to_string()
}

fn machine_device_id(seed: &Path) -> String {
    let mut input = String::new();
    for key in ["COMPUTERNAME", "USERNAME", "APPDATA"] {
        input.push_str(&std::env::var(key).unwrap_or_default());
        input.push('|');
    }
    input.push_str(&seed.to_string_lossy());
    let number = (fnv1a(input.as_bytes()) % 8_000_000_000_000_000) + 1_000_000_000_000_000;
    number.to_string()
}

pub fn is_valid_device_id(value: &str) -> bool {
    (12..=20).contains(&value.len()) && value.bytes().all(|b| b.is_ascii_digit())
}

fn derive_sibling(base: &str, used: &[String]) -> Option<String> {
    if !is_valid_device_id(base) {
        return None;
    }
    let (head, tail) = base.split_at(base.len() - 4);
    let tail_number: u32 = tail.parse().ok()?;
    for step in 1..=9999u32 {
        let number = (tail_number + step) % 10_000;
        let candidate = format!("{head}{number:04}");
        if candidate != base
            && is_valid_device_id(&candidate)
            && !used.iter().any(|item| item == &candidate)
        {
            return Some(candidate);
        }
    }
    None
}

/// 给账号分配签到用的设备号。
///
/// 第一个账号优先用真机号；其余账号从真机号同源衍生，避免「一台设备一天
/// 只能签一个账号」的 9095。已有合法设备号必须原样沿用，避免把绑定弄坏。
pub fn assign_checkin_device_id(
    account_id: &str,
    real_device_id: &str,
    used: &[String],
    prefer_real: bool,
) -> String {
    if prefer_real
        && is_valid_device_id(real_device_id)
        && !used.iter().any(|item| item == real_device_id)
    {
        return real_device_id.to_string();
    }
    if is_valid_device_id(real_device_id) {
        let (head, tail) = real_device_id.split_at(real_device_id.len() - 4);
        if let Ok(tail_number) = tail.parse::<u32>() {
            let start = (fnv1a(account_id.as_bytes()) % 9999) as u32;
            for step in 0..9999u32 {
                let number = (tail_number + start + step + 1) % 10_000;
                let candidate = format!("{head}{number:04}");
                if candidate != real_device_id
                    && is_valid_device_id(&candidate)
                    && !used.iter().any(|item| item == &candidate)
                {
                    return candidate;
                }
            }
        }
    }
    let base = machine_device_id(Path::new("trae"));
    if !used.iter().any(|item| item == &base) {
        return base;
    }
    derive_sibling(&base, used).unwrap_or(base)
}

// ---------------------------------------------------------------------------
// 本机登录态
// ---------------------------------------------------------------------------

/// 同步读取本机当前登录账号（给 `client_switch::detect` 展示「客户端当前登录」用）。
pub fn read_local_account() -> Option<(Account, Value)> {
    let local = read_local_auth()?;
    let cred = cred_from_local(&local);
    let account = account_from_cred(&cred);
    Some((account, cred))
}

fn read_usertag_map(storage: &Value) -> serde_json::Map<String, Value> {
    let Some(raw) = storage["iCubeAuthInfo://usertag"].as_str() else {
        return serde_json::Map::new();
    };
    if let Ok(value) = tc_decrypt_json(raw) {
        if let Some(object) = value.as_object() {
            return object.clone();
        }
    }
    if let Ok(value) = serde_json::from_str::<Value>(raw) {
        if let Some(object) = value.as_object() {
            return object.clone();
        }
    }
    serde_json::Map::new()
}

fn read_local_auth() -> Option<LocalAuth> {
    let appdata = std::env::var_os("APPDATA")?;
    for dir in APP_DIRS {
        let app_dir = PathBuf::from(&appdata).join(dir);
        if let Some(local) = read_local_auth_in(&app_dir) {
            return Some(local);
        }
    }
    None
}

fn read_local_auth_in(app_dir: &Path) -> Option<LocalAuth> {
    let storage_path = app_dir
        .join("User")
        .join("globalStorage")
        .join("storage.json");
    let storage_text = std::fs::read_to_string(&storage_path).ok()?;
    let storage: Value = serde_json::from_str(&storage_text).ok()?;
    let object = storage.as_object()?;

    let auth_key = if object.contains_key("iCubeAuthInfo://icube.cloudide") {
        "iCubeAuthInfo://icube.cloudide".to_string()
    } else {
        object
            .keys()
            .filter(|key| key.starts_with("iCubeAuthInfo://"))
            .max_by_key(|key| {
                object
                    .get(*key)
                    .and_then(Value::as_str)
                    .map(str::len)
                    .unwrap_or(0)
            })?
            .clone()
    };
    let auth = tc_decrypt_json(object.get(&auth_key)?.as_str()?).ok()?;
    let token = auth["token"].as_str().unwrap_or("");
    let uid = auth["userId"].as_str().unwrap_or("");
    if token.is_empty() || uid.is_empty() {
        return None;
    }

    let device_key = object
        .keys()
        .find(|key| key.starts_with("iCubeAuthInfo://icube-dc:"))?;
    let device_id = device_key
        .strip_prefix("iCubeAuthInfo://icube-dc:")?
        .trim()
        .to_string();
    let mut device = object
        .get(device_key)
        .and_then(Value::as_str)
        .and_then(|raw| tc_decrypt_json(raw).ok())
        .unwrap_or(Value::Null);

    if !is_valid_device_id(&device_id)
        || device["privateKeyPEM"].as_str().unwrap_or("").is_empty()
        || device["publicKeyPEM"].as_str().unwrap_or("").is_empty()
    {
        let (private_pem, public_pem) = generate_keypair()?;
        device = json!({
            "privateKeyPEM": private_pem,
            "publicKeyPEM": public_pem,
        });
    }

    let fallback_device_id = if is_valid_device_id(&device_id) {
        device_id
    } else {
        machine_device_id(app_dir)
    };

    Some(LocalAuth {
        app_dir: app_dir.to_path_buf(),
        auth,
        device_id: fallback_device_id,
        device,
        meta: client_meta(Some(app_dir)),
    })
}

fn client_meta(fallback_dir: Option<&Path>) -> ClientMeta {
    let product = install_product_json();
    let package_type = product
        .as_ref()
        .and_then(|p| p["packageType"].as_str())
        .unwrap_or("");
    let dir_is_solo = fallback_dir
        .map(|dir| dir.to_string_lossy().to_uppercase().contains("SOLO"))
        .unwrap_or(false);
    let is_solo = package_type.to_uppercase().contains("SOLO") || dir_is_solo;
    let app_version = product
        .as_ref()
        .and_then(|p| p["appVersion"].as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(FALLBACK_CLIENT_VERSION)
        .to_string();
    let tron_version = product
        .as_ref()
        .and_then(|p| p["tronBuildVersion"].as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(FALLBACK_TRON_VERSION)
        .to_string();
    ClientMeta {
        product_line: if is_solo { "solo" } else { "ide" },
        client_id: if is_solo {
            CLIENT_ID_SOLO
        } else {
            CLIENT_ID_IDE
        },
        req_source: if is_solo { 2 } else { 1 },
        client_version: app_version,
        tron_version,
    }
}

fn install_product_json() -> Option<Value> {
    let exe = crate::core::launcher::client_exe("trae")?;
    let path = exe
        .parent()?
        .join("resources")
        .join("app")
        .join("product.json");
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn cred_from_local(local: &LocalAuth) -> Value {
    let auth = &local.auth;
    let account = &auth["account"];
    let nickname = account["username"]
        .as_str()
        .or_else(|| account["screenName"].as_str())
        .or_else(|| account["name"].as_str())
        .unwrap_or("");
    let avatar = account["avatar_url"]
        .as_str()
        .or_else(|| account["avatarUrl"].as_str())
        .unwrap_or("");
    let email = account["email"].as_str().unwrap_or("");
    let host = auth["host"]
        .as_str()
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_API_HOST);

    json!({
        "access_token": auth["token"],
        "refresh_token": auth["refreshToken"],
        "expires_at_ms": parse_iso_ms(auth["expiredAt"].as_str().unwrap_or("")),
        "refresh_expires_at_ms": parse_iso_ms(auth["refreshExpiredAt"].as_str().unwrap_or("")),
        "user_id": auth["userId"],
        "nickname": nickname,
        "avatar": avatar,
        "email": email,
        // 原文保留账号对象/区域，换号时原样写回，避免重建字段导致客户端判无效。
        "account_json": auth["account"].clone(),
        "user_region_json": auth["userRegion"].clone(),
        "host": host,
        "region": "CN",
        "product_line": local.meta.product_line,
        "client_id": local.meta.client_id,
        "req_source": local.meta.req_source,
        "client_version": local.meta.client_version,
        "tron_version": local.meta.tron_version,
        "login_device_id": local.device_id,
        "checkin_device_id": local.device_id,
        "machine_id": stable_machine_id(&local.app_dir),
        "device_private_key_pem": local.device["privateKeyPEM"],
        "device_public_key_pem": local.device["publicKeyPEM"],
        "device_source": "client_import",
        "cred_source": "client_import",
        "device_rotated_at": 0,
        "last_refresh_at": 0,
    })
}

fn account_from_cred(cred: &Value) -> Account {
    let uid = cred["user_id"].as_str().unwrap_or("");
    let nickname = cred["nickname"].as_str().unwrap_or("").trim();
    let id = sha256_hex(uid);
    let label = if !nickname.is_empty() {
        nickname.to_string()
    } else if !uid.is_empty() {
        format!("Trae CN {}", &uid[..uid.len().min(8)])
    } else {
        "Trae CN 账号".to_string()
    };
    Account {
        id: id[..id.len().min(16)].to_string(),
        provider: "trae".to_string(),
        label,
        created_at: now_secs(),
        last_used: None,
        origin: "本机 Trae CN".to_string(),
    }
}

fn generate_keypair() -> Option<(String, String)> {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes).ok()?;
    let key = SigningKey::from_slice(&bytes).ok()?;
    let private_pem = key.to_pkcs8_pem(LineEnding::LF).ok()?.to_string();
    let public_pem = key.verifying_key().to_public_key_pem(LineEnding::LF).ok()?;
    Some((private_pem, public_pem))
}

fn sign_der_base64(private_pem: &str, data: &[u8]) -> Result<String> {
    let key = SigningKey::from_pkcs8_pem(private_pem)
        .map_err(|e| anyhow!("解析 Trae 设备私钥失败：{e}"))?;
    let signature: Signature = key.sign(data);
    Ok(B64.encode(signature.to_der().as_bytes()))
}

fn build_login_url(
    meta: &ClientMeta,
    device_id: &str,
    machine_id: &str,
    login_trace: &str,
    callback: &str,
    challenge: &str,
) -> String {
    let auth_from = if meta.product_line == "solo" {
        "solo"
    } else {
        "trae"
    };
    let mut url = format!(
        "https://www.trae.cn/authorization?login_version=1&auth_from={auth_from}\
&login_channel=native_ide&plugin_version={}&auth_type=local&client_id={}&redirect=0\
&login_trace_id={}&auth_callback_url={}&machine_id={}&device_id={}&x_device_id={}\
&x_machine_id={}&x_device_brand=PC&x_device_type=PC&x_os_version=10.0\
&x_app_version={}&x_app_type=stable&code_challenge={}&code_challenge_method=S256",
        url_encode(&meta.tron_version),
        meta.client_id,
        login_trace,
        url_encode(callback),
        machine_id,
        device_id,
        device_id,
        machine_id,
        url_encode(&meta.client_version),
        challenge
    );
    if auth_from == "solo" {
        url.push_str("&hide_saas_login=true");
    }
    url
}

async fn run_callback_server(
    listener: TcpListener,
    slot: Arc<Mutex<Option<OAuthCallback>>>,
) {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(300);
    loop {
        let accept = tokio::time::timeout_at(deadline, listener.accept()).await;
        let Ok(Ok((mut stream, _))) = accept else {
            return;
        };
        let mut buf = [0u8; 8192];
        let read = match stream.read(&mut buf).await {
            Ok(read) => read,
            Err(_) => continue,
        };
        let request = String::from_utf8_lossy(&buf[..read]).to_string();
        if let Some((login_trace, query)) = parse_request_query(&request) {
            *slot.lock().unwrap() = Some(OAuthCallback { login_trace, query });
            let body = "Trae 授权完成，可以关闭此页面。";
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.as_bytes().len(),
                body
            );
            let _ = stream.write_all(response.as_bytes()).await;
            return;
        }
        let _ = stream
            .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await;
    }
}

fn parse_request_query(
    request: &str,
) -> Option<(String, serde_json::Map<String, Value>)> {
    let line = request.lines().next()?;
    let target = line.split_whitespace().nth(1)?;
    let (path, query) = target.split_once('?')?;
    if path != "/authorize" {
        return None;
    }
    let mut map = serde_json::Map::new();
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        map.insert(url_decode(key), json!(url_decode(value)));
    }
    let login_trace = map
        .get("loginTraceID")
        .or_else(|| map.get("login_trace_id"))?
        .as_str()?
        .to_string();
    Some((login_trace, map))
}

fn callback_json(query: &serde_json::Map<String, Value>, key: &str) -> Option<Value> {
    let raw = query.get(key)?.as_str()?;
    serde_json::from_str(raw).ok()
}

fn user_field(user_info: &Value, keys: &[&str]) -> String {
    let roots = [
        Some(user_info),
        user_info.get("Result"),
        user_info.get("data"),
    ];
    for root in roots.into_iter().flatten() {
        for key in keys {
            if let Some(value) = root
                .get(*key)
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
            {
                return value.to_string();
            }
        }
    }
    String::new()
}

fn oauth_account_json(user_info: &Value) -> Value {
    json!({
        "username": user_field(user_info, &["ScreenName", "screenName", "username", "name"]),
        "avatar_url": user_field(user_info, &["AvatarUrl", "avatarUrl", "avatar_url", "avatar"]),
        "email": user_field(user_info, &["NonPlainTextEmail", "email", "Email"]),
        "nonPlainTextEmail": "",
        "nonPlainTextMobile": user_field(user_info, &["NonPlainTextMobile", "mobile", "phone"]),
        "description": user_field(user_info, &["Description", "description"]),
        "organization": "",
        "work_country": "",
        "storeCountryCode": "",
        "storeCountrySrc": user_field(user_info, &["StoreCountrySrc", "storeCountrySrc"]),
        "userTag": "cn",
        "migrateToSG": false,
        "scope": "marscode",
        "loginScope": "trae",
        "iss": "",
        "iat": 0,
    })
}

fn oauth_device_info(context: &OAuthContext) -> Value {
    json!({
        "DeviceID": context.device_id.clone(),
        "MachineID": context.machine_id.clone(),
        "PlatformCode": if context.product_line == "solo" { "SOLO_PC" } else { "IDE_PC" },
        "DeviceType": "PC",
        "DeviceName": std::env::var("COMPUTERNAME").unwrap_or_else(|_| "PC".to_string()),
        "DeviceModel": "",
        "ClientVersion": context.client_version.clone(),
        "DevicePublicKey": context.public_key_pem.clone(),
        "DeviceBrand": "",
        "DeviceCPU": "",
        "OSInfo": "Windows",
        "OSVersion": "10.0",
    })
}

fn random_base64url(bytes: usize) -> Result<String> {
    let mut buf = vec![0u8; bytes];
    getrandom::getrandom(&mut buf).map_err(|e| anyhow!("生成随机数失败：{e}"))?;
    Ok(B64URL.encode(buf))
}

fn code_challenge(verifier: &str) -> String {
    B64URL.encode(Sha256::digest(verifier.as_bytes()))
}

fn url_encode(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

fn url_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' if index + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).ok();
                if let Some(byte) = hex.and_then(|hex| u8::from_str_radix(hex, 16).ok()) {
                    out.push(byte);
                    index += 3;
                } else {
                    out.push(b'%');
                    index += 1;
                }
            }
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn device_info(cred: &Value) -> Value {
    let device_id = cred["login_device_id"]
        .as_str()
        .or_else(|| cred["checkin_device_id"].as_str())
        .unwrap_or("");
    let platform = if cred["product_line"].as_str() == Some("solo") {
        "SOLO_PC"
    } else {
        "IDE_PC"
    };
    json!({
        "DeviceID": device_id,
        "MachineID": cred["machine_id"].as_str().unwrap_or(""),
        "PlatformCode": platform,
        "DeviceType": "PC",
        "DeviceName": std::env::var("COMPUTERNAME").unwrap_or_else(|_| "PC".to_string()),
        "DeviceModel": "",
        "ClientVersion": client_version(cred),
        "DevicePublicKey": cred["device_public_key_pem"].as_str().unwrap_or(""),
        "DeviceBrand": "",
        "DeviceCPU": "",
        "OSInfo": "Windows",
        "OSVersion": "10.0",
    })
}

/// 读取本机当前登录的账号，判断是不是池里的这个账号。
pub fn client_login_state(_account: &Account, cred: &Value) -> ClientLoginState {
    let Some(local) = read_local_auth() else {
        return ClientLoginState::Unknown;
    };
    let local_uid = local.auth["userId"].as_str().unwrap_or("");
    let uid = cred["user_id"].as_str().unwrap_or("");
    if local_uid.is_empty() || uid.is_empty() {
        return ClientLoginState::Unknown;
    }
    if local_uid != uid {
        return ClientLoginState::Other;
    }

    let mut fresh = cred_from_local(&local);
    if let Some(checkin) = cred["checkin_device_id"].as_str() {
        fresh["checkin_device_id"] = json!(checkin);
    }
    if let Some(source) = cred["device_source"].as_str() {
        fresh["device_source"] = json!(source);
    }
    if let Some(source) = cred["cred_source"].as_str() {
        fresh["cred_source"] = json!(source);
    }
    ClientLoginState::Same(fresh)
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use p256::ecdsa::signature::Verifier;
    use p256::pkcs8::DecodePublicKey;

    fn sample_auth() -> Value {
        json!({
            "token": "at-1",
            "refreshToken": "rt-1",
            "expiredAt": "2026-12-01T02:01:54.400Z",
            "refreshExpiredAt": "2027-05-16T02:01:54.400Z",
            "userId": "u-1",
            "host": "https://api.trae.cn",
            "account": {"username": "测试用户"},
        })
    }

    #[test]
    fn tc_roundtrip() {
        let plain = serde_json::to_vec(&sample_auth()).unwrap();
        let blob = tc_encrypt_bytes(&plain).unwrap();
        let decoded = tc_decrypt(&blob).unwrap();
        assert_eq!(decoded, plain);
    }

    #[test]
    fn tc_rejects_bad_magic() {
        let mut blob = tc_encrypt_bytes(b"{}").unwrap();
        blob[0] = b'x';
        assert!(tc_decrypt(&blob).is_err());
    }

    #[test]
    fn tc_rejects_tampered() {
        let mut blob = tc_encrypt_bytes(b"{\"a\":1}").unwrap();
        let last = blob.len() - 1;
        blob[last] ^= 0x01;
        assert!(tc_decrypt(&blob).is_err());
    }

    #[test]
    fn device_id_shape_and_sibling() {
        assert!(is_valid_device_id("3812345678901234"));
        assert!(!is_valid_device_id("1942097ad2664caeb07cd7a4a6446a57"));
        let base = "3812345678901234";
        let used = vec![base.to_string()];
        let sibling = derive_sibling(base, &used).unwrap();
        assert_ne!(sibling, base);
        assert!(is_valid_device_id(&sibling));
        assert_eq!(&sibling[..12], &base[..12]);
    }

    #[test]
    fn assign_prefers_real_then_derives() {
        let real = "3812345678901234";
        let first = assign_checkin_device_id("a", real, &[], true);
        assert_eq!(first, real);
        let second = assign_checkin_device_id("b", real, &[first.clone()], false);
        assert_ne!(second, first);
        assert!(is_valid_device_id(&second));
    }

    #[test]
    fn status_parse_claimable_and_claimed() {
        let claimable = parse_status_body(&json!({
            "checked_in": false, "enable": true, "credits": 100, "code": 0
        }))
        .unwrap();
        assert_eq!(claimable.state, ClaimState::Claimable);
        assert_eq!(claimable.points, 100);

        let claimed = parse_status_body(&json!({
            "checked_in": true, "enable": true, "credits": 100, "code": 0
        }))
        .unwrap();
        assert_eq!(claimed.state, ClaimState::Claimed);
    }

    #[test]
    fn claim_errors_are_not_success() {
        let device = parse_claim_body(
            &json!({"code": 9095, "message": "当前设备今日已经签到"}),
            100,
        )
        .unwrap();
        assert_eq!(device.state, ClaimState::Unknown);
        assert!(device.message.contains("9095") || device.message.contains("设备"));

        let rejected = parse_claim_body(
            &json!({"code": 9074, "message": "当前参与用户太多，请稍后再试"}),
            100,
        )
        .unwrap();
        assert_eq!(rejected.state, ClaimState::Unknown);
        assert!(rejected.message.contains("9074"));

        let busy = parse_claim_body(
            &json!({"code": 5001, "message": "当前参与用户太多，请稍后再试"}),
            100,
        );
        assert!(busy.is_err());
    }

    #[test]
    fn quota_uses_usage_summary_and_skips_free_pack() {
        let body = json!({
            "total_balance": 600,
            "usage_summary": {"total_amount": 600, "consumed_amount": 100},
            "user_entitlement_pack_list": [
                {"display_desc": "免费", "entitlement_base_info": {"quota": {}}, "expire_time": 1796054399},
                {"group_name": "每月登录积分", "display_desc": "每月登录赠送",
                 "entitlement_base_info": {"quota": {"credits_limit": 500}, "start_time": 1793462400},
                 "usage": {}, "expire_time": 1796054399},
                {"group_name": "每日签到", "display_desc": "签到奖励",
                 "entitlement_base_info": {"quota": {"credits_limit": 100}, "start_time": 1794898893},
                 "usage": {}, "expire_time": 1797577293}
            ]
        });
        let quota = parse_quota_body(&body);
        assert_eq!(quota.total_remaining, 500.0);
        assert_eq!(quota.items.len(), 2);
        assert_eq!(quota.items[0].package_name, "每月登录积分");
        assert_eq!(quota.items[1].package_name, "每日签到");
    }

    /// 真机验证：本机 Trae CN 登录态能解出账号与设备密钥（只读）。
    #[test]
    #[ignore = "依赖本机 Trae CN 登录态，手动跑：cargo test -- --ignored 本机Trae登录态能解出"]
    fn 本机Trae登录态能解出() {
        let (account, cred) = read_local_account().expect("本机没有可解的 Trae 登录态");
        assert_eq!(account.provider, "trae");
        assert!(!account.id.is_empty());
        assert!(cred["access_token"].as_str().is_some_and(|s| !s.is_empty()));
        assert!(is_valid_device_id(
            cred["login_device_id"].as_str().unwrap_or("")
        ));
        assert!(cred["device_private_key_pem"]
            .as_str()
            .is_some_and(|s| s.contains("PRIVATE KEY")));
    }

    /// 真机验证：用本机 Trae 登录态查询签到状态（只读，不领取）。
    #[test]
    #[ignore = "会真实查询 Trae 签到状态，手动跑：cargo test -- --ignored 本机Trae签到状态能查"]
    fn 本机Trae签到状态能查() {
        let (account, cred) = read_local_account().expect("本机没有可用的 Trae 登录态");
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let status = runtime
            .block_on(TraeProvider::new().status(&account, &cred))
            .expect("查询签到状态失败");
        assert!(status.points > 0 || status.state == ClaimState::Claimed);
    }

    /// 真机验证：强制刷新账号池里的老 Trae 账号，成功后回写账号池。
    /// 会轮换 refresh token；只在需要救活老号时手动跑。
    #[test]
    #[ignore = "会强制刷新账号池里的第一个 Trae 账号，手动跑：cargo test -- --ignored 本机Trae账号强制刷新"]
    fn 本机Trae账号强制刷新() {
        let mut store = crate::core::store::AccountStore::load().expect("加载账号池失败");
        let account = store
            .list_of("trae")
            .into_iter()
            .next()
            .expect("账号池里没有 Trae 账号");
        let cred = store
            .credential("trae", &account.id)
            .expect("读取老号凭据失败");
        let runtime = tokio::runtime::Runtime::new().unwrap();
        match runtime.block_on(TraeProvider::new().refresh(&account, &cred)) {
            Ok(Some(fresh)) => {
                store.upsert(account, &fresh).expect("保存刷新后的凭据失败");
                println!("refresh ok, new expiry={}", fresh["expires_at_ms"]);
            }
            Ok(None) => panic!("刷新没有返回新凭据"),
            Err(e) => panic!("刷新失败：{e}"),
        }
    }

    #[test]
    fn login_url_contains_required_params() {
        let meta = ClientMeta {
            product_line: "ide",
            client_id: CLIENT_ID_IDE,
            req_source: 1,
            client_version: "3.4.20".to_string(),
            tron_version: "2.3.103559".to_string(),
        };
        let url = build_login_url(
            &meta,
            "3812345678901234",
            "0123456789abcdef0123456789abcdef",
            "trace123",
            "http://127.0.0.1:18080/authorize",
            "challenge",
        );
        assert!(url.starts_with("https://www.trae.cn/authorization?"));
        assert!(url.contains("auth_from=trae"));
        assert!(url.contains("client_id=ono9krqynydwx5"));
        assert!(url.contains("plugin_version=2.3.103559"));
        assert!(url.contains("code_challenge=challenge"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains(
            "auth_callback_url=http%3A%2F%2F127.0.0.1%3A18080%2Fauthorize"
        ));
        assert!(url.contains("device_id=3812345678901234"));
        assert!(url.contains("x_device_id=3812345678901234"));
    }

    #[test]
    fn callback_query_parses_encoded_json() {
        let auth_info = json!({"AuthCode": "ac-1"}).to_string();
        let user_info =
            json!({"UserID": "u-1", "ScreenName": "测试"}).to_string();
        let request = format!(
            "GET /authorize?loginTraceID=trace&authCodeInfo={}&userInfo={} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
            url_encode(&auth_info),
            url_encode(&user_info)
        );
        let (trace, query) = parse_request_query(&request).unwrap();
        assert_eq!(trace, "trace");
        assert_eq!(
            callback_json(&query, "authCodeInfo").unwrap()["AuthCode"],
            "ac-1"
        );
        assert_eq!(
            callback_json(&query, "userInfo").unwrap()["ScreenName"],
            "测试"
        );
    }

    #[test]
    fn oauth_account_json_maps_fields() {
        let info = json!({
            "ScreenName": "甲",
            "AvatarUrl": "https://example.com/a.png",
            "NonPlainTextEmail": "a@example.com",
            "NonPlainTextMobile": "13800000000",
        });
        let account = oauth_account_json(&info);
        assert_eq!(account["username"], "甲");
        assert_eq!(account["avatar_url"], "https://example.com/a.png");
        assert_eq!(account["email"], "a@example.com");
        assert_eq!(account["nonPlainTextMobile"], "13800000000");
        assert_eq!(account["scope"], "marscode");
        assert_eq!(account["loginScope"], "trae");
    }

    #[test]
    fn url_codec_roundtrip() {
        let raw = "http://127.0.0.1:18080/authorize?a=1&b=中 文";
        let encoded = url_encode(raw);
        assert_eq!(url_decode(&encoded), raw);
        assert_eq!(url_decode("a+b"), "a b");
    }

    #[test]
    fn signature_is_der_and_verifiable() {
        let (private_pem, public_pem) = generate_keypair().unwrap();
        let data = b"POST\n/trae/api/v3/oauth/ExchangeToken\nid\nrt\n1\nnonce";
        let encoded = sign_der_base64(&private_pem, data).unwrap();
        let der = B64.decode(encoded).unwrap();
        let signature = Signature::from_der(&der).unwrap();
        let public = p256::ecdsa::VerifyingKey::from_public_key_pem(&public_pem).unwrap();
        assert!(public.verify(data, &signature).is_ok());
    }
}
