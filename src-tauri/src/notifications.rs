//! 本地提醒收件箱。后台提醒即使在窗口隐藏或应用重启后也可回看。

use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::commands::AppState;
use crate::core::paths::{atomic_write, data_dir};

const MAX_NOTICES: usize = 100;
// 保留去重键的时间覆盖 7 天到期提醒窗口，并独立于列表保留条数。
const DEDUP_RETENTION_MS: i64 = 8 * 24 * 60 * 60 * 1000;

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum NoticeKind {
    Ok,
    Warn,
    Err,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Notice {
    id: String,
    title: String,
    body: String,
    kind: NoticeKind,
    created_at: i64,
    read: bool,
}

#[derive(Default, Deserialize, Serialize)]
struct InboxData {
    notices: Vec<Notice>,
    seen: BTreeMap<String, i64>,
}

pub struct Inbox {
    path: PathBuf,
    access: Mutex<()>,
}

impl Inbox {
    pub fn new() -> Self {
        Self {
            path: data_dir().join("notifications.json"),
            access: Mutex::new(()),
        }
    }

    fn load(&self) -> Result<InboxData> {
        match std::fs::read(&self.path) {
            Ok(bytes) => serde_json::from_slice(&bytes).context("提醒记录格式无效"),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(InboxData::default()),
            Err(err) => Err(err).context("读取提醒记录失败"),
        }
    }

    fn save(&self, data: &InboxData) -> Result<()> {
        atomic_write(&self.path, &serde_json::to_vec(data)?).context("保存提醒记录失败")
    }

    pub fn list(&self) -> Result<Vec<Notice>> {
        let _access = self.access.lock().unwrap();
        Ok(self.load()?.notices)
    }

    pub fn unread_count(&self) -> Result<usize> {
        Ok(self.list()?.iter().filter(|notice| !notice.read).count())
    }

    fn push(&self, notice: Notice, dedup_key: Option<String>) -> Result<bool> {
        let _access = self.access.lock().unwrap();
        let mut data = self.load()?;
        if let Some(key) = dedup_key {
            let cutoff = notice.created_at - DEDUP_RETENTION_MS;
            data.seen.retain(|_, timestamp| *timestamp >= cutoff);
            if data.seen.contains_key(&key) {
                return Ok(false);
            }
            data.seen.insert(key, notice.created_at);
        }
        data.notices.insert(0, notice);
        data.notices.truncate(MAX_NOTICES);
        self.save(&data)?;
        Ok(true)
    }

    fn mark_read(&self, ids: &[String]) -> Result<()> {
        let _access = self.access.lock().unwrap();
        let mut data = self.load()?;
        let ids: HashSet<_> = ids.iter().collect();
        for notice in &mut data.notices {
            if ids.contains(&notice.id) {
                notice.read = true;
            }
        }
        self.save(&data)
    }
}

#[tauri::command]
pub async fn list_notices(state: State<'_, AppState>) -> Result<Vec<Notice>, String> {
    state.inbox.list().map_err(|err| format!("{err:#}"))
}

#[tauri::command]
pub async fn mark_notices_read(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Vec<String>,
) -> Result<(), String> {
    state
        .inbox
        .mark_read(&ids)
        .map_err(|err| format!("{err:#}"))?;
    changed(&app, None);
    Ok(())
}

#[tauri::command]
pub async fn send_test_notification(app: AppHandle) -> Result<(), String> {
    publish(
        &app,
        NoticeKind::Ok,
        "应用内提醒测试",
        "提醒已送达。后台提醒也会保存在这里，供你打开 CheckinBox 后查看。",
        None,
    )
}

pub fn publish(
    app: &AppHandle,
    kind: NoticeKind,
    title: &str,
    body: &str,
    dedup_key: Option<String>,
) -> Result<(), String> {
    let notice = Notice {
        id: uuid::Uuid::new_v4().to_string(),
        title: title.into(),
        body: body.into(),
        kind,
        created_at: chrono::Utc::now().timestamp_millis(),
        read: false,
    };
    if app
        .state::<AppState>()
        .inbox
        .push(notice.clone(), dedup_key)
        .map_err(|err| format!("{err:#}"))?
    {
        changed(app, Some(notice));
    }
    Ok(())
}

fn changed(app: &AppHandle, notice: Option<Notice>) {
    let _ = app.emit("notices-changed", notice);
    crate::tray::update_unread(app);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unread_and_dedup_survive_reopening_the_inbox() {
        let dir = tempfile::tempdir().unwrap();
        let reopen = || Inbox {
            path: dir.path().join("notifications.json"),
            access: Mutex::new(()),
        };
        let notice = Notice {
            id: "first".into(),
            title: "提醒".into(),
            body: "内容".into(),
            kind: NoticeKind::Warn,
            created_at: 0,
            read: false,
        };
        let key = || Some("same-account-and-day".into());
        assert!(reopen().push(notice.clone(), key()).unwrap());
        assert!(!reopen().push(notice, key()).unwrap());
        assert_eq!(reopen().unread_count().unwrap(), 1);
        reopen().mark_read(&["first".into()]).unwrap();
        assert_eq!(reopen().unread_count().unwrap(), 0);
        assert_eq!(reopen().list().unwrap().len(), 1);
    }
}
