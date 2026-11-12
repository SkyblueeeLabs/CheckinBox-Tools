//! Electron safeStorage（Chromium OSCrypt）的加解密。
//!
//! Qoder CN 客户端（Electron）把登录态存在
//! `%APPDATA%\com.qodercn.app.stable\auth.v1.dat`，格式已在本机实测：
//!
//! ```text
//! 文件布局：  "v10" || nonce(12B) || ciphertext || tag(16B)
//! 密钥来源：  <userData>\Local State → os_crypt.encrypted_key
//!             = base64( "DPAPI" || DPAPI(CryptProtectData) 的 32 字节 AES-256-GCM 密钥 )
//! ```
//!
//! 也就是说：**同一 Windows 用户**可以用 DPAPI 解出 AES 密钥，进而解密 /
//! 重新加密客户端的登录态 —— 这就是 Qoder「客户端换号」的实现基础
//! （MiniMax / 妙手是明文 JSON，直接读写；Qoder 走这里）。
//!
//! AES-GCM 用 RustCrypto 的 `aes-gcm` crate，DPAPI 复用 `core/dpapi.rs`。

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use anyhow::{anyhow, Result};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use serde_json::Value;

use super::dpapi;

/// OSCrypt 的版本前缀（Windows 上新版 Chromium 的密钥型方案用 `v10`）。
const PREFIX: &[u8] = b"v10";
/// GCM nonce 长度。
const NONCE_LEN: usize = 12;
/// AES-256-GCM 密钥长度。
pub const KEY_LEN: usize = 32;

/// 从 Electron 应用的 userData 目录读出 AES 密钥。
///
/// `local_state` 是 `<userData>\Local State`（JSON）。
pub fn read_key(local_state: &std::path::Path) -> Result<[u8; KEY_LEN]> {
    let raw = std::fs::read_to_string(local_state)
        .map_err(|e| anyhow!("读 Local State 失败（{e}）：客户端可能从未运行过"))?;
    let parsed: Value =
        serde_json::from_str(&raw).map_err(|e| anyhow!("Local State 解析失败：{e}"))?;

    let encoded = parsed["os_crypt"]["encrypted_key"]
        .as_str()
        .ok_or_else(|| anyhow!("Local State 里没有 os_crypt.encrypted_key"))?;
    let blob = BASE64
        .decode(encoded)
        .map_err(|e| anyhow!("encrypted_key 不是合法 base64：{e}"))?;

    // 前 5 字节是 ASCII "DPAPI" 标记，其后是 CryptProtectData 的输出
    const DPAPI_MARK: &[u8] = b"DPAPI";
    if blob.len() < DPAPI_MARK.len() || &blob[..DPAPI_MARK.len()] != DPAPI_MARK {
        return Err(anyhow!("encrypted_key 没有 DPAPI 前缀，格式不认识"));
    }
    let plain = dpapi::unprotect(&blob[DPAPI_MARK.len()..])?;
    let key: [u8; KEY_LEN] = plain
        .as_slice()
        .try_into()
        .map_err(|_| anyhow!("密钥长度不是 {KEY_LEN} 字节（上游可能已改格式）"))?;
    Ok(key)
}

/// 解密一个 OSCrypt blob（`v10` 前缀的 AES-256-GCM）。
pub fn decrypt(key: &[u8; KEY_LEN], data: &[u8]) -> Result<Vec<u8>> {
    if data.len() < PREFIX.len() + NONCE_LEN + 16 || &data[..PREFIX.len()] != PREFIX {
        return Err(anyhow!("不是 v10 格式的 OSCrypt 数据"));
    }
    let nonce = Nonce::from_slice(&data[PREFIX.len()..PREFIX.len() + NONCE_LEN]);
    let cipher_text = &data[PREFIX.len() + NONCE_LEN..];
    let cipher = Aes256Gcm::new(key.into());
    cipher
        .decrypt(nonce, Payload::from(cipher_text))
        .map_err(|_| anyhow!("AES-GCM 解密失败（密钥不匹配或数据已损坏）"))
}

/// 加密成 OSCrypt blob：`v10` || 随机 nonce || ciphertext+tag。
///
/// 用的是**同一把** Local State 密钥，所以客户端能正常读回。
pub fn encrypt(key: &[u8; KEY_LEN], plain: &[u8]) -> Result<Vec<u8>> {
    let mut nonce_bytes = [0u8; NONCE_LEN];
    getrandom::getrandom(&mut nonce_bytes).map_err(|e| anyhow!("取随机数失败：{e}"))?;

    let cipher = Aes256Gcm::new(key.into());
    let sealed = cipher
        .encrypt(Nonce::from_slice(&nonce_bytes), Payload::from(plain))
        .map_err(|_| anyhow!("AES-GCM 加密失败"))?;

    let mut out = Vec::with_capacity(PREFIX.len() + NONCE_LEN + sealed.len());
    out.extend_from_slice(PREFIX);
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&sealed);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 加密后能解回原文() {
        let key = [42u8; KEY_LEN];
        let plain = br#"{"token":"dt-demo","user":{"id":"u1"}}"#;
        let blob = encrypt(&key, plain).unwrap();
        assert_eq!(&blob[..3], b"v10");
        assert_eq!(blob.len(), 3 + NONCE_LEN + plain.len() + 16);
        let back = decrypt(&key, &blob).unwrap();
        assert_eq!(back, plain);
    }

    #[test]
    fn 随机nonce让两次加密的密文不同() {
        let key = [7u8; KEY_LEN];
        let a = encrypt(&key, b"same").unwrap();
        let b = encrypt(&key, b"same").unwrap();
        assert_ne!(a, b, "nonce 不同，密文应当不同");
    }

    #[test]
    fn 非v10数据报错不panic() {
        let key = [1u8; KEY_LEN];
        assert!(decrypt(&key, b"garbage").is_err());
        assert!(decrypt(&key, &[]).is_err());
    }

    #[test]
    fn 密钥错误时解密失败() {
        let key = [1u8; KEY_LEN];
        let blob = encrypt(&key, b"secret").unwrap();
        let wrong = [2u8; KEY_LEN];
        assert!(decrypt(&wrong, &blob).is_err());
    }
}
