//! 账号池持久化。
//!
//! 凭据（access token / refresh token 等）用 Windows DPAPI 加密后 base64 存盘，
//! 明文只存在于内存。账号索引信息（昵称、来源、时间）明文存，方便 UI 直接读。

use anyhow::{anyhow, Result};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

use super::dpapi;
use super::paths::{accounts_file, atomic_write};
use super::provider::Account;

/// 单条账号记录。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredAccount {
    pub account: Account,
    /// DPAPI 密文的 base64
    pub credential: String,
}

/// 账号池文件结构。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AccountStore {
    #[serde(default)]
    pub accounts: BTreeMap<String, StoredAccount>,
}

impl AccountStore {
    /// 账号键：`provider:id`
    pub fn key(provider: &str, id: &str) -> String {
        format!("{provider}:{id}")
    }

    /// 从磁盘加载；文件不存在时返回空账号池。
    pub fn load() -> Result<Self> {
        let path = accounts_file();
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = std::fs::read_to_string(&path)?;
        if raw.trim().is_empty() {
            return Ok(Self::default());
        }
        serde_json::from_str(&raw).map_err(|e| anyhow!("账号池文件解析失败：{e}"))
    }

    /// 原子写盘。
    pub fn save(&self) -> Result<()> {
        let path = accounts_file();
        let text = serde_json::to_string_pretty(self)?;
        atomic_write(&path, text.as_bytes())
    }

    /// 列出全部账号（按 provider、创建时间排序）。
    pub fn list(&self) -> Vec<Account> {
        let mut list: Vec<Account> = self.accounts.values().map(|s| s.account.clone()).collect();
        list.sort_by(|a, b| {
            a.provider
                .cmp(&b.provider)
                .then(a.created_at.cmp(&b.created_at))
        });
        list
    }

    /// 列出某平台的账号。
    pub fn list_of(&self, provider: &str) -> Vec<Account> {
        self.list()
            .into_iter()
            .filter(|a| a.provider == provider)
            .collect()
    }

    /// 取某账号的凭据明文。
    pub fn credential(&self, provider: &str, id: &str) -> Result<Value> {
        let key = Self::key(provider, id);
        let entry = self
            .accounts
            .get(&key)
            .ok_or_else(|| anyhow!("账号不存在：{key}"))?;
        let cipher = BASE64
            .decode(entry.credential.as_bytes())
            .map_err(|e| anyhow!("凭据密文格式错误：{e}"))?;
        let plain = dpapi::unprotect(&cipher)?;
        serde_json::from_slice(&plain).map_err(|e| anyhow!("凭据解析失败：{e}"))
    }

    /// 写入（或覆盖）一个账号。
    pub fn upsert(&mut self, account: Account, cred: &Value) -> Result<()> {
        let plain = serde_json::to_vec(cred)?;
        let cipher = dpapi::protect(&plain)?;
        let key = Self::key(&account.provider, &account.id);
        self.accounts.insert(
            key,
            StoredAccount {
                account,
                credential: BASE64.encode(cipher),
            },
        );
        self.save()
    }

    /// 仅更新账号元信息（不碰凭据）。
    pub fn touch(&mut self, account: Account) -> Result<()> {
        let key = Self::key(&account.provider, &account.id);
        if let Some(entry) = self.accounts.get_mut(&key) {
            entry.account = account;
            self.save()?;
        }
        Ok(())
    }

    /// 删除账号。
    pub fn remove(&mut self, provider: &str, id: &str) -> Result<bool> {
        let key = Self::key(provider, id);
        let removed = self.accounts.remove(&key).is_some();
        if removed {
            self.save()?;
        }
        Ok(removed)
    }

    pub fn contains(&self, provider: &str, id: &str) -> bool {
        self.accounts.contains_key(&Self::key(provider, id))
    }
}
