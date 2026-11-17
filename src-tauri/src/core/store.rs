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
use std::path::{Path, PathBuf};

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
        Self::load_from(&accounts_file())
    }

    /// 解析失败时先把原文件改名为 `accounts.json.corrupt-<时间戳>` 再报错：
    /// 调用方会以空池继续运行，后续任意一次保存都会重写 accounts.json ——
    /// 不备份就等于把用户账号静默删掉。
    fn load_from(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = std::fs::read_to_string(path)?;
        if raw.trim().is_empty() {
            return Ok(Self::default());
        }
        match serde_json::from_str(&raw) {
            Ok(store) => Ok(store),
            Err(err) => {
                let backup = backup_corrupt(path);
                Err(anyhow!(
                    "账号池文件解析失败：{err}{}",
                    match backup {
                        Some(b) => format!("；原文件已备份到 {}", b.display()),
                        None => "；备份原文件失败，请先手动复制 accounts.json".to_string(),
                    }
                ))
            }
        }
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

    #[allow(dead_code)] // 多账号 UI 的「改备注」会用到
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

/// 把解析失败的账号池文件改名留档，成功时返回备份路径。
fn backup_corrupt(path: &Path) -> Option<PathBuf> {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let base = path.display();
    let mut target = PathBuf::from(format!("{base}.corrupt-{stamp}"));
    let mut seq = 1;
    while target.exists() {
        target = PathBuf::from(format!("{base}.corrupt-{stamp}-{seq}"));
        seq += 1;
    }
    std::fs::rename(path, &target).ok().map(|_| target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_file(tag: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "checkinbox-store-{tag}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("accounts.json")
    }

    #[test]
    fn 损坏的账号池会先备份再报错() {
        let path = temp_file("corrupt");
        std::fs::write(&path, b"{ this is not json").unwrap();

        let err = AccountStore::load_from(&path).unwrap_err().to_string();

        assert!(err.contains("解析失败"), "错误文案应说明解析失败：{err}");
        assert!(!path.exists(), "原文件应被移走，避免后续保存覆盖");
        let dir = path.parent().unwrap();
        let backups: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("accounts.json.corrupt-")
            })
            .collect();
        assert_eq!(backups.len(), 1, "应留一份损坏文件备份");
        assert_eq!(
            std::fs::read(backups[0].path()).unwrap(),
            b"{ this is not json"
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn 正常账号池能原样读回() {
        let path = temp_file("roundtrip");
        let mut store = AccountStore::default();
        store.accounts.insert(
            AccountStore::key("minimax", "u1"),
            StoredAccount {
                account: Account {
                    id: "u1".into(),
                    provider: "minimax".into(),
                    label: "测试账号".into(),
                    created_at: 1,
                    last_used: None,
                    origin: "测试".into(),
                },
                credential: "AAAA".into(),
            },
        );
        std::fs::write(&path, serde_json::to_string_pretty(&store).unwrap()).unwrap();

        let loaded = AccountStore::load_from(&path).unwrap();

        assert_eq!(loaded.accounts.len(), 1);
        assert_eq!(loaded.accounts["minimax:u1"].account.label, "测试账号");
        assert!(path.exists(), "正常读取不应移动文件");
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }
}
