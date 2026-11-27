//! 豆包（Doubao / 豆包工作）适配器。
//!
//! 产品标识：`doubao`，客户端 `Doubao.exe`，数据目录 `%LOCALAPPDATA%\Doubao\User Data`。
//!
//! 协议说明（综合开源方案 `L0NE-6/DoubaoWork-Daily`、`smart-open/TraeWorkAssistant` 与本机实测）：
//!   * **本机导入**：豆包桌面端（Chromium 147）对 `Cookies` SQLite 做了客户端级二次加密，
//!     但把当前所有已登录账号的明文 `uid:sessionid` 与昵称缓存在
//!     `%LOCALAPPDATA%\Doubao\User Data\Local State` → `saman.local_storage_app_for_web.enterprise`
//!     的 `x-tt-multi-sids` 与 `samantha-web-multi-user-profile-store.multiUserProfiles` 中。
//!     读取 `Local State` 不受客户端运行时的文件锁影响。
//!   * **内置登录窗导入**：在 CheckinBox 内打开独立隔离数据目录的 Tauri WebView 登录 `doubao.com`，
//!     登录后直接读取 `sessionid` / `sessionid_ss` 等认证 Cookie，并调用
//!     `POST /passport/account/info/v2/` 取权威 `user_id_str` 与真实昵称 `screen_name`。
//!   * **签到链路**：
//!     1. `POST /alice/user/launch` 取当前账号实际投放的活动组 `data.ug_campaign_info.campaign_group_code`；
//!     2. `POST /alice/ug/campaign/v3/detail/`（非 v3 组走 `/alice/ug/campaign/list/`）查询每日签到任务与状态；
//!     3. 未签时优先走 `POST /alice/ug/campaign/landing/checkin` (`{"scene": group}`)，旧口径
//!        `task_status == 2` 走 `POST /alice/ug/campaign/item/reward/claim/`，完成后回查详情确认。
//!   * **额度查询**：
//!     `POST /alice/commerce/sale/subscription/quota/summary/`（`{"product_line":"membership"}`）
//!     注意带 `agw-js-conv: str` 时上游会把 int64 序列化成字符串（如 `"200"`），需兼容字符串和数字。
//!     完整解析 `quota_remaining_amount`、`incentive_quota_section`（含活动赠送额度与到期时间）、
//!     `quota_blance_usage` 以及 `window_limit_section`。

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use crate::core::paths;
use crate::core::provider::{
    Account, AccountProfile, CheckinStatus, ClaimOutcome, ClaimState, CreditGroup, CreditItem,
    Provider, Quota,
};

const BASE_URL: &str = "https://www.doubao.com";
const CHECKIN_PAGE_URL: &str = "https://www.doubao.com/pc/agent-explore";
const DEFAULT_SCENE: &str = "pc_incentive_task_mvp_v3";
const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/147.0.0.0 Safari/537.36 SamanthaDoubao/2.33.5";
const REFERER: &str = "https://www.doubao.com/pc/agent-explore";
const LAUNCH_QUERY: &str = "aid=1044603&real_aid=1044603&device_platform=web&version_code=20800&samantha_web=1&web_platform=desktop&doubao_device_platform=desktop&client_platform=pc_client&pkg_type=release_version&runtime=web&runtime_version=3.42.2&pc_version=2.33.5&doubao_pc_version=2.33.5&use-olympus-account=1&language=zh&region=CN&sys_region=CN";
const DESKTOP_QUERY: &str = "version_code=20800&language=zh&device_platform=web&aid=497858&real_aid=497858&pc_version=2.33.5&samantha_web=1&web_platform=desktop&use-olympus-account=1";

/// 认证失效业务码（会话已过期或被吊销）
const CODE_AUTH_EXPIRED: i64 = 710012001;
/// 风控验证码业务码
const CODE_RATE_LIMIT: i64 = 710022004;
/// 官方活动页 SDK 参数校验拒绝码
const CODE_SDK_REQUIRED: i64 = 710010702;

pub const LOGIN_WINDOW_URL: &str = "https://www.doubao.com/chat/";

const TITLE_RESULT_PREFIX: &str = "__CB_DOUBAO_RES__:";

static APP_HANDLE: OnceLock<tauri::AppHandle> = OnceLock::new();
static WEBVIEW_GATE: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

/// 启动阶段登记 `AppHandle`，供豆包签到时拉起隐藏 WebView2 执行官方活动页 SDK 签到。
pub fn set_app_handle(app: tauri::AppHandle) {
    let _ = APP_HANDLE.set(app);
}

/// 注入到 `https://www.doubao.com/pc/agent-explore` 的自动化脚本：
/// 1. 提前包一层 fetch / XHR 拦截 `/alice/ug/campaign/landing/checkin` 真实回执；
/// 2. 定位 `[data-reward-code="daily_signin"]` 卡片，已签直接回报，未签点击「立即签到」
///    由官方前端 SDK 自动携带 `a_bogus`/`msToken`/`AgwCommonParam`/`ttwid` 完成提交；
/// 3. 结果通过 `document.title` 回传给 Rust，不依赖额外 IPC 权限。
const WEBVIEW_CHECKIN_SCRIPT: &str = r#"
(() => {
  if (window !== window.top) return;
  if (!String(location.pathname || '').includes('/pc/agent-explore')) return;
  if (window.__cbDoubaoInstalled) return;
  window.__cbDoubaoInstalled = true;

  const PREFIX = '__CB_DOUBAO_RES__:';
  let reported = false;
  let clicked = false;
  let apiOutcome = null;
  let detailPoints = 0;

  function report(payload) {
    if (reported) return;
    reported = true;
    try {
      document.title = PREFIX + JSON.stringify(payload);
    } catch (_) {}
  }

  function num(v) {
    const n = Number(v);
    return Number.isFinite(n) ? Math.round(n) : 0;
  }

  function inspectApiJson(url, status, rawText) {
    let json = null;
    try { json = JSON.parse(rawText); } catch (_) { return; }
    if (!json || typeof json !== 'object') return;

    if (url.includes('/alice/ug/campaign/v3/detail') || url.includes('/alice/ug/campaign/list')) {
      const tasks = json?.data?.tasks;
      if (Array.isArray(tasks)) {
        const daily = tasks.find((t) => t?.task_kind === 'daily_signin' || t?.daily_checkin);
        if (daily) {
          const pts = num(daily?.daily_checkin?.reward?.reward_amount ?? daily?.reward_summaries?.[0]?.reward_amount);
          if (pts > 0) detailPoints = pts;
        }
      }
      return;
    }

    if (url.includes('/alice/ug/campaign/landing/checkin') || url.includes('/alice/ug/campaign/item/reward/claim')) {
      const code = json.code ?? json.status_code ?? 0;
      const msg = String(json.message || json.msg || json.status_msg || '');
      const data = json.data || {};
      if (status === 401 || Number(code) === 710012001) {
        apiOutcome = { status: 'auth_expired', message: '豆包登录态已失效，请重新导入或登录' };
        return;
      }
      if (Number(code) !== 0 && Number(code) !== 200) {
        apiOutcome = { status: 'failed', code: Number(code), message: msg || ('code=' + code) };
        return;
      }
      if (data.biz_error && Number(data.biz_error.code) !== 0) {
        apiOutcome = {
          status: 'failed',
          code: Number(data.biz_error.code),
          message: String(data.biz_error.message || data.biz_error.msg || '领取确认中，请稍后刷新查看'),
        };
        return;
      }
      const reward = num(data.reward_amount) || detailPoints;
      if (data.checked_in === true) {
        apiOutcome = { status: 'claimed', points: reward, message: reward > 0 ? ('签到成功 +' + reward) : '签到成功' };
      } else if (data.idempotent === true) {
        apiOutcome = { status: 'already', points: reward, message: '今日已签到' };
      }
    }
  }

  // 1) 拦截 fetch
  const origFetch = window.fetch;
  if (typeof origFetch === 'function') {
    window.fetch = function(...args) {
      const url = typeof args[0] === 'string' ? args[0] : (args[0]?.url || '');
      const p = origFetch.apply(this, args);
      if (url.includes('/alice/ug/campaign/')) {
        p.then((res) => {
          try {
            res.clone().text().then((txt) => inspectApiJson(url, res.status, txt)).catch(() => {});
          } catch (_) {}
        }).catch(() => {});
      }
      return p;
    };
  }

  // 2) 拦截 XHR
  const origOpen = XMLHttpRequest.prototype.open;
  const origSend = XMLHttpRequest.prototype.send;
  XMLHttpRequest.prototype.open = function(method, url, ...rest) {
    this.__cbUrl = String(url || '');
    return origOpen.call(this, method, url, ...rest);
  };
  XMLHttpRequest.prototype.send = function(...args) {
    if (this.__cbUrl && this.__cbUrl.includes('/alice/ug/campaign/')) {
      this.addEventListener('load', () => {
        try { inspectApiJson(this.__cbUrl, this.status, this.responseText || ''); } catch (_) {}
      });
    }
    return origSend.apply(this, args);
  };

  function findSigninCard() {
    const byAttr = document.querySelector('[data-reward-code="daily_signin"]');
    if (byAttr) return byAttr;
    const btns = [...document.querySelectorAll('button')];
    for (const b of btns) {
      const txt = (b.textContent || '').trim();
      if (txt === '立即签到' || txt === '去签到') {
        return b.closest('[class*="card"], [class*="item"], [class*="task"], div') || b.parentElement;
      }
    }
    return null;
  }

  const startedAt = Date.now();
  const timer = setInterval(() => {
    if (reported) { clearInterval(timer); return; }
    const elapsed = Date.now() - startedAt;

    if (apiOutcome) {
      clearInterval(timer);
      setTimeout(() => report(apiOutcome), 350);
      return;
    }

    const card = findSigninCard();
    if (card) {
      const btn = card.querySelector('button');
      const btnText = btn ? (btn.textContent || '').trim() : '';
      const rewarded = !!card.querySelector('[class*="rewarded-"]') ||
        (!btn && /已签到|今日已签|明日再来/.test(card.innerText || '')) ||
        (btn && /已签到|已领取/.test(btnText));

      if (rewarded) {
        clearInterval(timer);
        if (clicked) {
          report({
            status: 'claimed',
            points: detailPoints,
            message: detailPoints > 0 ? ('签到成功 +' + detailPoints) : '签到成功',
          });
        } else {
          report({ status: 'already', points: detailPoints, message: '今日已签到' });
        }
        return;
      }

      if (btn && !btn.disabled && /立即签到|去签到|签到/.test(btnText) && !clicked) {
        clicked = true;
        try { btn.scrollIntoView({ block: 'center', inline: 'center' }); } catch (_) {}
        btn.click();
        return;
      }
    } else if (elapsed > 6000) {
      const bodyText = (document.body && document.body.innerText) || '';
      if (/扫码登录|手机号登录|请登录|登录豆包/.test(bodyText)) {
        clearInterval(timer);
        report({ status: 'auth_expired', message: '豆包登录态已失效，请重新导入或登录' });
        return;
      }
    }

    if (elapsed > 24000) {
      clearInterval(timer);
      const hasCard = !!findSigninCard();
      report({
        status: 'timeout',
        clicked,
        hasCard,
        message: clicked
          ? '豆包页面已点击签到但未收到确认结果，请稍后刷新余额查看'
          : (hasCard ? '豆包签到按钮当前不可点击，请在豆包客户端查看' : '未加载出豆包每日签到卡片，可能当前账号未开放活动或网络较慢'),
      });
    }
  }, 400);
})();
"#;

// ---------------------------------------------------------------------------
// 数值解析（兼容 agw-js-conv: str 的字符串数值与常规 JSON 数字）
// ---------------------------------------------------------------------------

fn json_f64(v: &Value) -> Option<f64> {
    if let Some(n) = v.as_f64() {
        return n.is_finite().then_some(n);
    }
    if let Some(i) = v.as_i64() {
        return Some(i as f64);
    }
    if let Some(u) = v.as_u64() {
        return Some(u as f64);
    }
    v.as_str()
        .and_then(|s| s.trim().parse::<f64>().ok())
        .filter(|n| n.is_finite())
}

fn json_i64(v: &Value) -> Option<i64> {
    if let Some(i) = v.as_i64() {
        return Some(i);
    }
    if let Some(u) = v.as_u64() {
        return i64::try_from(u).ok();
    }
    if let Some(s) = v.as_str() {
        if let Ok(i) = s.trim().parse::<i64>() {
            return Some(i);
        }
    }
    json_f64(v).map(|n| n.round() as i64)
}

fn normalize_ms(raw: i64) -> i64 {
    if raw <= 0 {
        0
    } else if raw < 1_000_000_000_000 {
        raw * 1000
    } else {
        raw
    }
}

// ---------------------------------------------------------------------------
// Local State 读取（本机导入：多账号 sessionid + 真实昵称）
// ---------------------------------------------------------------------------

pub fn user_data_dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join("Doubao").join("User Data"))
}

fn local_state_path() -> Option<PathBuf> {
    user_data_dir().map(|d| d.join("Local State"))
}

/// URL 百分比解码（处理 `%XX`，零额外依赖）。
fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let h1 = (bytes[i + 1] as char).to_digit(16);
            let h2 = (bytes[i + 2] as char).to_digit(16);
            if let (Some(a), Some(b)) = (h1, h2) {
                out.push(((a << 4) | b) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// 从 `Local State` JSON 中提取 `x-tt-multi-sids`（`uid -> sessionid` 映射）。
pub fn parse_multi_sids_from_local_state(raw: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let Ok(data) = serde_json::from_str::<Value>(raw) else {
        return out;
    };
    let Some(ent) = data
        .get("saman")
        .and_then(|s| s.get("local_storage_app_for_web"))
        .and_then(|s| s.get("enterprise"))
        .and_then(Value::as_str)
    else {
        return out;
    };
    let encoded_owned;
    let encoded = if let Ok(ent_json) = serde_json::from_str::<Value>(ent) {
        if let Some(v) = ent_json.get("x-tt-multi-sids").and_then(Value::as_str) {
            encoded_owned = v.to_string();
            encoded_owned.as_str()
        } else {
            return out;
        }
    } else {
        let marker = "x-tt-multi-sids\":\"";
        let Some(idx) = ent.find(marker) else {
            return out;
        };
        let after = &ent[(idx + marker.len()).min(ent.len())..];
        after.split('"').next().unwrap_or("")
    };
    let decoded = percent_decode(encoded);
    for pair in decoded.split('|') {
        if let Some((uid, sid)) = pair.split_once(':') {
            let uid = uid.trim();
            let sid = sid.trim();
            if !uid.is_empty() && uid.bytes().all(|b| b.is_ascii_digit()) && !sid.is_empty() {
                out.insert(uid.to_string(), sid.to_string());
            }
        }
    }
    out
}

/// 从 `Local State` 的 `samantha-web-multi-user-profile-store` 读取 `entity_id -> 昵称`。
pub fn parse_local_nicknames_from_local_state(raw: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let Ok(data) = serde_json::from_str::<Value>(raw) else {
        return out;
    };
    let Some(ent) = data
        .get("saman")
        .and_then(|s| s.get("local_storage_app_for_web"))
        .and_then(|s| s.get("enterprise"))
        .and_then(Value::as_str)
    else {
        return out;
    };
    let Ok(ent_json) = serde_json::from_str::<Value>(ent) else {
        return out;
    };
    let Some(profiles) = ent_json
        .get("samantha-web-multi-user-profile-store")
        .and_then(|s| s.get("multiUserProfiles"))
        .and_then(Value::as_array)
    else {
        return out;
    };
    for p in profiles {
        let uid = p
            .get("entity_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let nick = p
            .get("nickname")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .or_else(|| {
                p.get("user_name")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
            });
        if let (Some(u), Some(n)) = (uid, nick) {
            out.insert(u.to_string(), n.to_string());
        }
    }
    out
}

fn read_local_state(path: &Path) -> (BTreeMap<String, String>, BTreeMap<String, String>) {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return (BTreeMap::new(), BTreeMap::new());
    };
    (
        parse_multi_sids_from_local_state(&raw),
        parse_local_nicknames_from_local_state(&raw),
    )
}

fn format_account_label(uid: &str) -> String {
    let tail: String = uid
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    if tail.is_empty() {
        "豆包账号".to_string()
    } else {
        format!("豆包 {tail}")
    }
}

/// 从凭据 JSON 组装 Cookie 头。
fn cookie_header_from_cred(cred: &Value) -> Result<String> {
    if let Some(cookie) = cred.get("cookie").and_then(Value::as_str) {
        let trimmed = cookie.trim();
        if !trimmed.is_empty()
            && (trimmed.contains("sessionid=") || trimmed.contains("sessionid_ss="))
        {
            return Ok(trimmed.to_string());
        }
    }
    if let Some(sid) = cred.get("sessionid").and_then(Value::as_str) {
        let sid = sid.trim();
        if !sid.is_empty() {
            return Ok(format!("sessionid={sid}; sessionid_ss={sid}"));
        }
    }
    Err(anyhow!("豆包凭据缺少 sessionid，请重新导入或登录"))
}

fn parse_cookie_pairs(cookie: &str) -> Vec<(String, String)> {
    cookie
        .split(';')
        .filter_map(|part| {
            let (k, v) = part.trim().split_once('=')?;
            let k = k.trim();
            let v = v.trim();
            (!k.is_empty() && !v.is_empty()).then(|| (k.to_string(), v.to_string()))
        })
        .collect()
}

fn webview_profile_dir(account_id: &str) -> PathBuf {
    let safe: String = account_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    let folder = if safe.is_empty() { "default".to_string() } else { safe };
    paths::data_dir().join("doubao_webviews").join(folder)
}

/// 在内置隐藏 WebView2 窗口中打开官方活动页 `https://www.doubao.com/pc/agent-explore`，
/// 注入本账号的 `sessionid` 等 Cookie，由豆包官方前端 SDK 自动携带 `a_bogus` / `msToken` /
/// `AgwCommonParam` / `ttwid` 完成签到（绕过纯 HTTP 必遇的 `710010702` 风控校验）。
async fn webview_checkin(
    app: &tauri::AppHandle,
    account: &Account,
    cred: &Value,
) -> Result<ClaimOutcome> {
    let gate = WEBVIEW_GATE.get_or_init(|| tokio::sync::Mutex::new(()));
    let _guard = gate.lock().await;

    let cookie_str = cookie_header_from_cred(cred)?;
    let pairs = parse_cookie_pairs(&cookie_str);
    if pairs.is_empty() {
        return Err(anyhow!("豆包凭据缺少有效 Cookie"));
    }

    let (tx, rx) = tokio::sync::oneshot::channel::<Value>();
    let tx_holder = Arc::new(Mutex::new(Some(tx)));
    let tx_cb = tx_holder.clone();

    let win_label = format!(
        "doubao-checkin-{}",
        &uuid::Uuid::new_v4().to_string()[..8]
    );
    let blank_url = tauri::Url::parse("about:blank")
        .map_err(|e| anyhow!("构造空白地址失败：{e}"))?;
    let target_url = tauri::Url::parse(CHECKIN_PAGE_URL)
        .map_err(|e| anyhow!("构造豆包签到页地址失败：{e}"))?;

    let mut builder = tauri::WebviewWindowBuilder::new(
        app,
        &win_label,
        tauri::WebviewUrl::External(blank_url),
    )
    .title("CheckinBox 豆包签到")
    .inner_size(1100.0, 760.0)
    .visible(false)
    .skip_taskbar(true)
    .user_agent(UA)
    .initialization_script(WEBVIEW_CHECKIN_SCRIPT)
    .on_document_title_changed(move |_win, title| {
        if let Some(payload) = title.strip_prefix(TITLE_RESULT_PREFIX) {
            if let Ok(val) = serde_json::from_str::<Value>(payload) {
                if let Some(sender) = tx_cb.lock().unwrap().take() {
                    let _ = sender.send(val);
                }
            }
        }
    });

    #[cfg(windows)]
    {
        let profile_dir = webview_profile_dir(&account.id);
        let _ = std::fs::create_dir_all(&profile_dir);
        builder = builder.data_directory(profile_dir);
    }

    let window = builder
        .build()
        .map_err(|e| anyhow!("创建豆包内置签到窗口失败：{e}"))?;

    // 注入本账号的登录态 Cookie（覆盖该账号独立 WebView2 profile 中的旧会话）
    for (k, v) in &pairs {
        let is_session = k == "sessionid" || k == "sessionid_ss";
        for domain in [".doubao.com", "www.doubao.com"] {
            let ck = tauri::webview::cookie::Cookie::build((k.clone(), v.clone()))
                .domain(domain)
                .path("/")
                .secure(true)
                .http_only(is_session)
                .build();
            let _ = window.set_cookie(ck);
        }
    }

    let nav_res = window.navigate(target_url);
    if let Err(e) = nav_res {
        let _ = window.close();
        return Err(anyhow!("打开豆包签到页失败：{e}"));
    }

    let waited = tokio::time::timeout(Duration::from_secs(32), rx).await;
    let _ = window.close();

    let payload = match waited {
        Ok(Ok(val)) => val,
        Ok(Err(_)) => return Err(anyhow!("豆包内置签到窗口提前关闭，未完成签到")),
        Err(_) => {
            return Err(anyhow!(
                "豆包内置签到页加载超时（32s），请检查网络连接或稍后重试"
            ));
        }
    };

    parse_webview_checkin_result(&payload)
}

fn parse_webview_checkin_result(payload: &Value) -> Result<ClaimOutcome> {
    let status = payload
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("failed");
    let points = payload.get("points").and_then(json_i64).unwrap_or(0);
    let message = payload
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();

    match status {
        "claimed" => {
            let msg = if !message.is_empty() {
                message.to_string()
            } else if points > 0 {
                format!("签到成功 +{points}")
            } else {
                "签到成功".to_string()
            };
            Ok(ClaimOutcome::claimed(points, msg))
        }
        "already" => Ok(ClaimOutcome::already(
            points,
            if message.is_empty() { "今日已签到" } else { message },
        )),
        "auth_expired" => Err(anyhow!(
            "{}",
            if message.is_empty() {
                "豆包登录态已失效，请重新导入或登录"
            } else {
                message
            }
        )),
        _ => {
            let code = payload.get("code").and_then(json_i64);
            let msg = if !message.is_empty() {
                message.to_string()
            } else if let Some(c) = code {
                format!("豆包签到失败（code={c}）")
            } else {
                "豆包签到未完成，请稍后重试".to_string()
            };
            Err(anyhow!("{msg}"))
        }
    }
}

/// 由一组 Cookie（如 WebView 登录窗读取结果）构造 `(Account, Value)`。
pub fn build_account_from_cookies(
    cookies: &[(String, String)],
    uid_hint: Option<&str>,
    label_hint: Option<&str>,
    origin: &str,
) -> Result<(Account, Value)> {
    let mut map: BTreeMap<String, String> = BTreeMap::new();
    for (k, v) in cookies {
        let k = k.trim();
        let v = v.trim();
        if !k.is_empty() && !v.is_empty() {
            map.insert(k.to_string(), v.to_string());
        }
    }
    let sid = map
        .get("sessionid")
        .or_else(|| map.get("sessionid_ss"))
        .cloned()
        .ok_or_else(|| anyhow!("未读取到豆包登录态 Cookie（sessionid）"))?;
    map.entry("sessionid".to_string())
        .or_insert_with(|| sid.clone());
    map.entry("sessionid_ss".to_string())
        .or_insert_with(|| sid.clone());

    let cookie_str = map
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("; ");

    let id = uid_hint
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| map.get("uid_tt").cloned())
        .or_else(|| map.get("uid_tt_ss").cloned())
        .unwrap_or_else(|| {
            use sha2::{Digest, Sha256};
            let digest = Sha256::digest(sid.as_bytes());
            format!("db_{:x}", digest)[..15].to_string()
        });

    let label = label_hint
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format_account_label(&id));

    let now = chrono::Utc::now().timestamp();
    let account = Account {
        id: id.clone(),
        provider: "doubao".to_string(),
        label,
        created_at: now,
        last_used: None,
        origin: origin.to_string(),
    };
    let cred = json!({
        "uid": id,
        "sessionid": sid,
        "cookie": cookie_str,
    });
    Ok((account, cred))
}

// ---------------------------------------------------------------------------
// 响应解析辅助
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct DailyTaskInfo {
    group: String,
    item_code: String,
    task_id: String,
    task_status: i64,
    checked_in_today: bool,
    points: i64,
    streak_days: i32,
    total_amount: Option<i64>,
}

fn find_daily_task(detail: &Value, default_group: &str) -> Option<DailyTaskInfo> {
    let data = detail.get("data").filter(|d| d.is_object())?;
    let total_amount = data
        .get("progress")
        .and_then(|p| p.get("total_amount"))
        .and_then(json_i64);

    let tasks = data.get("tasks").and_then(Value::as_array)?;
    for t in tasks {
        let is_daily = t.get("task_kind").and_then(Value::as_str) == Some("daily_signin")
            || t.get("daily_checkin").and_then(Value::as_object).is_some();
        if !is_daily {
            continue;
        }
        let dc = t.get("daily_checkin").and_then(Value::as_object);
        let checked = dc
            .and_then(|d| d.get("checked_in_today"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let streak = dc
            .and_then(|d| d.get("continuous_days"))
            .and_then(json_i64)
            .unwrap_or(0) as i32;
        let points = dc
            .and_then(|d| d.get("reward"))
            .and_then(|r| r.get("reward_amount"))
            .and_then(json_i64)
            .or_else(|| {
                t.get("reward_summaries")
                    .and_then(Value::as_array)
                    .and_then(|arr| arr.first())
                    .and_then(|r| r.get("reward_amount"))
                    .and_then(json_i64)
            })
            .unwrap_or(0);
        let group = t
            .get("campaign_group_code")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .unwrap_or(default_group)
            .to_string();
        let item_code = t
            .get("campaign_code")
            .or_else(|| t.get("task_code"))
            .or_else(|| t.get("code"))
            .and_then(Value::as_str)
            .unwrap_or("daily_signin")
            .to_string();
        let task_id = t
            .get("task_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let task_status = t.get("task_status").and_then(json_i64).unwrap_or(0);

        return Some(DailyTaskInfo {
            group,
            item_code,
            task_id,
            task_status,
            checked_in_today: checked,
            points,
            streak_days: streak,
            total_amount,
        });
    }
    None
}

fn parse_quota_summary(summary: &Value, campaign_total: Option<i64>) -> Quota {
    let data = summary.get("data").filter(|d| d.is_object());
    let mut remaining = data
        .and_then(|d| d.get("quota_remaining_amount"))
        .and_then(json_f64)
        .unwrap_or(0.0);

    let sub_name = data
        .and_then(|d| d.get("current_subscription"))
        .and_then(|s| s.get("display"))
        .and_then(|disp| {
            disp.get("short_name")
                .or_else(|| disp.get("product_name"))
                .and_then(Value::as_str)
        })
        .or_else(|| {
            data.and_then(|d| d.get("membership_display_name"))
                .and_then(Value::as_str)
        })
        .unwrap_or("");

    let mut credit_groups = Vec::new();
    let mut items = Vec::new();
    let mut granted_sum = 0.0_f64;
    let mut used_sum = 0.0_f64;

    // 1) 活动赠送额度（incentive_quota_section：带明确 total_amount / used_amount / end_time）
    if let Some(groups) = data
        .and_then(|d| d.get("incentive_quota_section"))
        .and_then(|w| w.get("window_limit_groups"))
        .and_then(Value::as_array)
    {
        for group in groups {
            let gname = group
                .get("feature_group_name")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
                .unwrap_or("活动赠送额度");
            let gkind = group
                .get("feature_group")
                .and_then(Value::as_str)
                .unwrap_or("incentive");
            for (idx, w) in group
                .get("window_limits")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .enumerate()
            {
                let total = w.get("total_amount").and_then(json_f64).unwrap_or(0.0);
                let used = w.get("used_amount").and_then(json_f64).unwrap_or(0.0);
                let rem = (total - used).max(0.0);
                if total <= 0.0 && rem <= 0.0 {
                    continue;
                }
                let start_ms = w.get("start_time").and_then(json_i64).map(normalize_ms).unwrap_or(0);
                let end_ms = w.get("end_time").and_then(json_i64).map(normalize_ms).unwrap_or(0);
                granted_sum += total;
                used_sum += used;
                credit_groups.push(CreditGroup {
                    kind: format!("{gkind}_{idx}"),
                    name: gname.to_string(),
                    total,
                    used,
                    remaining: rem,
                    count: 1,
                    time_ms: end_ms,
                    uses_expiry: end_ms > 0,
                });
                items.push(CreditItem {
                    credit_type: "free".to_string(),
                    granted: total,
                    consumed: used,
                    remaining: rem,
                    granted_at_ms: start_ms,
                    expire_at_ms: end_ms,
                    package_name: gname.to_string(),
                    refresh_at_ms: 0,
                });
            }
        }
    }

    // 2) 汇总余额明细（quota_blance_usage，上游字段拼写就是 blance）
    if let Some(usages) = data
        .and_then(|d| d.get("quota_blance_usage"))
        .or_else(|| data.and_then(|d| d.get("quota_balance_usage")))
        .and_then(Value::as_array)
    {
        let mut usage_total = 0.0_f64;
        let mut usage_used = 0.0_f64;
        for row in usages {
            usage_total += row.get("total_amount").and_then(json_f64).unwrap_or(0.0);
            usage_used += row.get("used_amount").and_then(json_f64).unwrap_or(0.0);
        }
        if usage_total > granted_sum {
            granted_sum = usage_total;
        }
        if usage_used > used_sum {
            used_sum = usage_used;
        }
        if remaining <= 0.0 && usage_total > usage_used {
            remaining = (usage_total - usage_used).max(0.0);
        }
    }

    // 3) 订阅周期窗口（window_limit_section：按使用百分比展示）
    if let Some(groups) = data
        .and_then(|d| d.get("window_limit_section"))
        .and_then(|w| w.get("window_limit_groups"))
        .and_then(Value::as_array)
    {
        for group in groups {
            let gname = group
                .get("feature_group_name")
                .and_then(Value::as_str)
                .unwrap_or("");
            for w in group
                .get("window_limits")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let Some(used_pct) = w.get("used_percent").and_then(json_f64) else {
                    continue;
                };
                let wt = w.get("window_type").and_then(json_i64).unwrap_or(0);
                let label = match wt {
                    1 => "当前时段".to_string(),
                    2 => "近 7 天".to_string(),
                    _ if !gname.is_empty() => format!("{gname}·窗口{wt}"),
                    _ => format!("窗口 {wt}"),
                };
                let end_ms = w.get("end_time").and_then(json_i64).map(normalize_ms).unwrap_or(0);
                let clamped_used = used_pct.clamp(0.0, 100.0);
                credit_groups.push(CreditGroup {
                    kind: format!("window_{wt}"),
                    name: label,
                    total: 100.0,
                    used: clamped_used,
                    remaining: (100.0 - clamped_used).max(0.0),
                    count: 1,
                    time_ms: end_ms,
                    uses_expiry: false,
                });
            }
        }
    }

    let granted = granted_sum
        .max(campaign_total.map(|v| v as f64).unwrap_or(0.0))
        .max(remaining + used_sum)
        .max(remaining);
    let consumed = if used_sum > 0.0 {
        used_sum
    } else {
        (granted - remaining).max(0.0)
    };
    let message = if !sub_name.is_empty() {
        format!("当前套餐：{sub_name}")
    } else {
        String::new()
    };

    Quota {
        total_granted: granted,
        total_consumed: consumed,
        total_remaining: remaining,
        items,
        message,
        refresh_at_ms: 0,
        credit_groups,
    }
}

/// 从 `POST /passport/account/info/v2/` 响应中提取 `(uid, nickname, avatar)`。
fn parse_passport_info(raw: &Value) -> (Option<String>, Option<String>, Option<String>) {
    let Some(data) = raw.get("data").filter(|d| d.is_object()) else {
        return (None, None, None);
    };
    let uid = data
        .get("user_id_str")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| data.get("user_id").and_then(json_i64).filter(|&i| i > 0).map(|i| i.to_string()));
    let name = data
        .get("screen_name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            data.get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
        })
        .map(str::to_string);
    let avatar = data
        .get("avatar_url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    (uid, name, avatar)
}

// ---------------------------------------------------------------------------
// Provider 实现
// ---------------------------------------------------------------------------

pub struct DoubaoProvider {
    http: reqwest::Client,
}

impl DoubaoProvider {
    pub fn new() -> Self {
        // 国内域名直连，避免系统代理导致 TLS 握手超时（与 DoubaoWork-Daily trust_env=False 一致）
        let http = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(20))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self { http }
    }

    async fn post_json(
        &self,
        path: &str,
        query: &str,
        cookie: &str,
        body: &Value,
    ) -> Result<Value> {
        let url = format!("{BASE_URL}{path}?{query}");
        let resp = self
            .http
            .post(&url)
            .header("Cookie", cookie)
            .header("User-Agent", UA)
            .header("Referer", REFERER)
            .header("Origin", BASE_URL)
            .header("Accept", "application/json, text/plain, */*")
            .header("Content-Type", "application/json")
            .header("agw-js-conv", "str")
            .json(body)
            .send()
            .await
            .map_err(|e| anyhow!("请求豆包接口失败：{e}"))?;

        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| anyhow!("读取豆包响应失败：{e}"))?;
        let raw: Value = serde_json::from_str(&text)
            .map_err(|_| anyhow!("豆包接口未返回有效 JSON（HTTP {}）", status.as_u16()))?;

        let code = raw.get("code").and_then(json_i64);
        let msg = raw
            .get("message")
            .or_else(|| raw.get("msg"))
            .or_else(|| raw.get("error_detail").and_then(|d| d.get("message")))
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();

        let ok_code = matches!(code, None | Some(0))
            || (code == Some(200) && path.starts_with("/alice/ug/"));

        if !status.is_success() || !ok_code {
            if status.as_u16() == 401
                || code == Some(CODE_AUTH_EXPIRED)
                || msg.to_ascii_lowercase().contains("not logged in")
            {
                return Err(anyhow!("豆包登录态已失效（HTTP 401），请重新导入或登录"));
            }
            if status.as_u16() == 429 || code == Some(CODE_RATE_LIMIT) {
                return Err(anyhow!("豆包触发风控或限流，请在豆包客户端验证后再试"));
            }
            if code == Some(CODE_SDK_REQUIRED) {
                return Err(anyhow!(
                    "豆包签到受活动风控校验拦截（710010702），请先在豆包客户端点一次「立即签到」"
                ));
            }
            let detail = if msg.is_empty() {
                format!("HTTP {} / code={}", status.as_u16(), code.unwrap_or(-1))
            } else {
                format!("{msg}（code={}）", code.unwrap_or(-1))
            };
            return Err(anyhow!("豆包接口错误：{detail}"));
        }

        Ok(raw)
    }

    /// 调 `/passport/account/info/v2/` 取权威 `user_id_str`、昵称与头像。
    async fn fetch_passport_profile(
        &self,
        cookie: &str,
    ) -> Result<(Option<String>, Option<String>, Option<String>)> {
        let raw = self
            .post_json("/passport/account/info/v2/", DESKTOP_QUERY, cookie, &json!({}))
            .await?;
        Ok(parse_passport_info(&raw))
    }

    /// 调 `/alice/user/launch` 取投放的活动组代码与 `sec_user_id`。
    async fn fetch_launch(&self, cookie: &str) -> Result<(Option<String>, Option<String>)> {
        let raw = self
            .post_json("/alice/user/launch", LAUNCH_QUERY, cookie, &json!({}))
            .await?;
        let data = raw.get("data");
        let group = data
            .and_then(|d| d.get("ug_campaign_info"))
            .and_then(|u| u.get("campaign_group_code"))
            .and_then(Value::as_str)
            .map(|s| s.trim().to_string());
        let sec_uid = data
            .and_then(|d| d.get("sec_user_id"))
            .and_then(Value::as_str)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        Ok((group, sec_uid))
    }

    async fn fetch_campaign_detail(&self, cookie: &str, group: &str) -> Result<Value> {
        let path = if group == DEFAULT_SCENE {
            "/alice/ug/campaign/v3/detail/"
        } else {
            "/alice/ug/campaign/list/"
        };
        self.post_json(
            path,
            DESKTOP_QUERY,
            cookie,
            &json!({ "campaign_group_code": group }),
        )
        .await
    }

    /// 校验内置登录窗拿到的 Cookie 是否有效，并返回带权威 uid 与真实昵称的账号。
    pub async fn verify_cookies(&self, cookies: &[(String, String)]) -> Result<(Account, Value)> {
        let (_, temp_cred) = build_account_from_cookies(cookies, None, None, "内置登录窗")?;
        let cookie = cookie_header_from_cred(&temp_cred)?;

        // 优先用 passport/account/info/v2 拿权威 user_id_str 和真实昵称（与本机导入同一 id，不重复入池）
        let (passport_uid, passport_name, _) = self
            .fetch_passport_profile(&cookie)
            .await
            .unwrap_or((None, None, None));

        // 顺带验证业务接口可访问
        let (_, sec_uid) = self.fetch_launch(&cookie).await?;

        let fallback_uid = sec_uid.map(|sec| {
            let short: String = sec
                .chars()
                .rev()
                .take(12)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            format!("db_{short}")
        });
        let uid_hint = passport_uid.as_deref().or(fallback_uid.as_deref());
        build_account_from_cookies(cookies, uid_hint, passport_name.as_deref(), "内置登录窗")
    }
}

#[async_trait]
impl Provider for DoubaoProvider {
    fn id(&self) -> &'static str {
        "doubao"
    }

    fn display_name(&self) -> &'static str {
        "豆包"
    }

    fn accent(&self) -> &'static str {
        "#1664ff"
    }

    fn docs_url(&self) -> &'static str {
        LOGIN_WINDOW_URL
    }

    async fn discover(&self) -> Result<Vec<(Account, Value)>> {
        let Some(path) = local_state_path() else {
            return Ok(Vec::new());
        };
        if !path.is_file() {
            return Ok(Vec::new());
        }
        let (sids, nicknames) = read_local_state(&path);
        let mut out = Vec::new();
        for (uid, sid) in sids {
            let pairs = vec![
                ("sessionid".to_string(), sid.clone()),
                ("sessionid_ss".to_string(), sid),
            ];
            let nick = nicknames.get(&uid).map(String::as_str);
            if let Ok(entry) =
                build_account_from_cookies(&pairs, Some(&uid), nick, "本机豆包登录态")
            {
                out.push(entry);
            }
        }
        Ok(out)
    }

    async fn status(&self, _account: &Account, cred: &Value) -> Result<CheckinStatus> {
        let cookie = cookie_header_from_cred(cred)?;
        let (launch_group, _) = self.fetch_launch(&cookie).await?;

        let group = match launch_group.as_deref() {
            Some("") => {
                // 服务端显式返回空活动组：再试一次默认 v3 组确认；若返回 campaign not found 则为未投放
                match self.fetch_campaign_detail(&cookie, DEFAULT_SCENE).await {
                    Ok(detail) => {
                        return Ok(status_from_detail(&detail, DEFAULT_SCENE));
                    }
                    Err(_) => {
                        return Ok(CheckinStatus {
                            state: ClaimState::NotAvailable,
                            points: 0,
                            streak_days: 0,
                            message: "当前账号未投放每日签到活动（可在豆包客户端「免费用豆包」查看）"
                                .to_string(),
                            ..Default::default()
                        });
                    }
                }
            }
            Some(g) => g.to_string(),
            None => DEFAULT_SCENE.to_string(),
        };

        let detail = self.fetch_campaign_detail(&cookie, &group).await?;
        Ok(status_from_detail(&detail, &group))
    }

    async fn claim(&self, account: &Account, cred: &Value) -> Result<ClaimOutcome> {
        // 桌面应用运行时优先走内置静默 WebView2 官方活动页签到：
        // 豆包 `/alice/ug/campaign/landing/checkin` 强校验前端 SDK 生成的 `a_bogus`/`msToken`/`AgwCommonParam`，
        // 纯 HTTP 直调会触发 `710010702`；内置 WebView2 运行官方页面脚本点击「立即签到」，
        // 不安装代理证书、不打断用户正在运行的豆包客户端，且天然支持多账号隔离。
        if let Some(app) = APP_HANDLE.get() {
            return webview_checkin(app, account, cred).await;
        }

        let cookie = cookie_header_from_cred(cred)?;
        let (launch_group, _) = self.fetch_launch(&cookie).await?;
        let group = match launch_group.as_deref() {
            Some("") | None => DEFAULT_SCENE.to_string(),
            Some(g) => g.to_string(),
        };

        let detail = self.fetch_campaign_detail(&cookie, &group).await?;
        let Some(task) = find_daily_task(&detail, &group) else {
            return Ok(ClaimOutcome {
                state: ClaimState::NotAvailable,
                points: 0,
                message: "当前没有可领取的豆包每日签到任务".to_string(),
            });
        };

        if task.checked_in_today {
            return Ok(ClaimOutcome::already(task.points, "今日已签到"));
        }

        // 双口径分派：旧口径 task_status == 2 走 item/reward/claim；v3 新口径走 landing/checkin
        let resp = if task.task_status == 2 {
            self.post_json(
                "/alice/ug/campaign/item/reward/claim/",
                DESKTOP_QUERY,
                &cookie,
                &json!({
                    "task_id": task.task_id,
                    "campaign_group_code": task.group,
                    "campaign_item_code": task.item_code,
                }),
            )
            .await?
        } else {
            self.post_json(
                "/alice/ug/campaign/landing/checkin",
                DESKTOP_QUERY,
                &cookie,
                &json!({ "scene": task.group }),
            )
            .await?
        };

        let data = resp.get("data");
        if let Some(biz) = data.and_then(|d| d.get("biz_error")).filter(|b| b.is_object()) {
            let bcode = biz.get("code").and_then(json_i64).unwrap_or(0);
            if bcode != 0 {
                let bmsg = biz
                    .get("message")
                    .or_else(|| biz.get("msg"))
                    .and_then(Value::as_str)
                    .unwrap_or("领取结果确认中，请稍后刷新查看");
                return Ok(ClaimOutcome::failed(format!("豆包：{bmsg}")));
            }
        }

        let checked = data
            .and_then(|d| d.get("checked_in"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let idempotent = data
            .and_then(|d| d.get("idempotent"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let reward = data
            .and_then(|d| d.get("reward_amount"))
            .and_then(json_i64)
            .unwrap_or(task.points);

        if checked {
            return Ok(ClaimOutcome::claimed(reward, format!("签到成功 +{reward}")));
        }
        if idempotent {
            return Ok(ClaimOutcome::already(reward, "今日已签到"));
        }

        // 回查一次官方详情确认是否已落账
        if let Ok(recheck) = self.fetch_campaign_detail(&cookie, &task.group).await {
            if let Some(t) = find_daily_task(&recheck, &task.group) {
                if t.checked_in_today {
                    return Ok(ClaimOutcome::claimed(
                        t.points.max(reward),
                        format!("签到成功 +{}", t.points.max(reward)),
                    ));
                }
            }
        }

        Ok(ClaimOutcome::failed(
            "豆包签到结果待确认，请稍后刷新或前往客户端查看",
        ))
    }

    async fn quota(&self, _account: &Account, cred: &Value) -> Result<Quota> {
        let cookie = cookie_header_from_cred(cred)?;
        let summary = self
            .post_json(
                "/alice/commerce/sale/subscription/quota/summary/",
                DESKTOP_QUERY,
                &cookie,
                &json!({ "product_line": "membership" }),
            )
            .await?;

        // 可选补充：若账号有投放签到活动，顺带取累计获得额度作为 granted 下界
        let campaign_total = match self.fetch_launch(&cookie).await {
            Ok((Some(g), _)) if !g.is_empty() => self
                .fetch_campaign_detail(&cookie, &g)
                .await
                .ok()
                .and_then(|d| find_daily_task(&d, &g))
                .and_then(|t| t.total_amount),
            _ => None,
        };

        Ok(parse_quota_summary(&summary, campaign_total))
    }

    async fn profile(&self, _account: &Account, cred: &Value) -> Result<Option<AccountProfile>> {
        let cookie = cookie_header_from_cred(cred)?;
        let (_, name, avatar) = self.fetch_passport_profile(&cookie).await?;
        if name.is_none() && avatar.is_none() {
            return Ok(None);
        }
        Ok(Some(AccountProfile {
            name,
            email: None,
            avatar,
        }))
    }
}

fn status_from_detail(detail: &Value, group: &str) -> CheckinStatus {
    let Some(task) = find_daily_task(detail, group) else {
        return CheckinStatus {
            state: ClaimState::NotAvailable,
            points: 0,
            streak_days: 0,
            message: "当前活动未提供每日签到任务".to_string(),
            ..Default::default()
        };
    };
    if task.checked_in_today {
        CheckinStatus {
            state: ClaimState::Claimed,
            points: task.points,
            streak_days: task.streak_days,
            message: if task.streak_days > 0 {
                format!("今日已签（连签 {} 天）", task.streak_days)
            } else {
                "今日已签".to_string()
            },
            ..Default::default()
        }
    } else {
        CheckinStatus {
            state: ClaimState::Claimable,
            points: task.points,
            streak_days: task.streak_days,
            message: if task.points > 0 {
                format!("可签到 +{}", task.points)
            } else {
                "今日可签到".to_string()
            },
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 解析local_state里的multi_sids与真实昵称() {
        let raw = r#"{
            "saman": {
                "local_storage_app_for_web": {
                    "enterprise": "{\"x-tt-multi-sids\":\"59804565116907%3Asid_demo_1%7C123456%3Asid_demo_2\",\"samantha-web-multi-user-profile-store\":{\"multiUserProfiles\":[{\"entity_id\":\"59804565116907\",\"nickname\":\"Skyblueee\",\"user_name\":\"Skyblueee_1024\"}]}}"
                }
            }
        }"#;
        let map = parse_multi_sids_from_local_state(raw);
        assert_eq!(map.len(), 2);
        assert_eq!(map.get("59804565116907").map(String::as_str), Some("sid_demo_1"));
        assert_eq!(map.get("123456").map(String::as_str), Some("sid_demo_2"));

        let nicks = parse_local_nicknames_from_local_state(raw);
        assert_eq!(nicks.get("59804565116907").map(String::as_str), Some("Skyblueee"));
    }

    #[test]
    fn 解析签到详情_未签与已签() {
        let raw = json!({
            "code": 0,
            "data": {
                "progress": { "total_amount": "1200" },
                "tasks": [{
                    "task_kind": "daily_signin",
                    "campaign_code": "daily_signin",
                    "task_status": 1,
                    "daily_checkin": {
                        "checked_in_today": false,
                        "continuous_days": 3,
                        "reward": { "reward_amount": "200" }
                    }
                }]
            }
        });
        let st = status_from_detail(&raw, DEFAULT_SCENE);
        assert_eq!(st.state, ClaimState::Claimable);
        assert_eq!(st.points, 200);
        assert_eq!(st.streak_days, 3);
    }

    #[test]
    fn 解析额度汇总_兼容字符串数值与活动赠送额度() {
        // 真实抓包结构（agw-js-conv: str 下数字字段为字符串）
        let summary = json!({
            "code": 0,
            "data": {
                "quota_remaining_amount": "200",
                "current_subscription": {
                    "display": { "short_name": "免费体验", "product_name": "个人订阅" }
                },
                "incentive_quota_section": {
                    "entitlement_count": 1,
                    "window_limit_groups": [{
                        "feature_group": "doubao_activity_reward_quota",
                        "feature_group_name": "活动赠送额度",
                        "window_limits": [{
                            "end_time": 1798298067170i64,
                            "start_time": 0,
                            "total_amount": "200",
                            "used_amount": "0",
                            "used_percent": 0,
                            "window_type": 3
                        }]
                    }]
                },
                "quota_blance_usage": [{
                    "category": 3,
                    "title": "赠送",
                    "total_amount": "200",
                    "used_amount": "0",
                    "used_percent": 0
                }],
                "window_limit_section": {}
            }
        });
        let q = parse_quota_summary(&summary, None);
        assert_eq!(q.total_remaining, 200.0);
        assert_eq!(q.total_granted, 200.0);
        assert_eq!(q.total_consumed, 0.0);
        assert_eq!(q.message, "当前套餐：免费体验");
        assert_eq!(q.credit_groups.len(), 1);
        assert_eq!(q.credit_groups[0].name, "活动赠送额度");
        assert_eq!(q.credit_groups[0].remaining, 200.0);
        assert_eq!(q.credit_groups[0].time_ms, 1798298067170);
        assert!(q.credit_groups[0].uses_expiry);
        assert_eq!(q.items.len(), 1);
        assert_eq!(q.items[0].expire_at_ms, 1798298067170);
    }

    #[test]
    fn 解析passport资料_取真实昵称与uid() {
        let raw = json!({
            "data": {
                "user_id": 59804565116907i64,
                "user_id_str": "59804565116907",
                "screen_name": "Skyblueee",
                "name": "Skyblueee",
                "avatar_url": "https://example.com/a.png"
            }
        });
        let (uid, name, avatar) = parse_passport_info(&raw);
        assert_eq!(uid.as_deref(), Some("59804565116907"));
        assert_eq!(name.as_deref(), Some("Skyblueee"));
        assert_eq!(avatar.as_deref(), Some("https://example.com/a.png"));
    }

    #[test]
    fn 解析webview签到回执() {
        let ok = parse_webview_checkin_result(&json!({
            "status": "claimed",
            "points": 200,
            "message": "签到成功 +200"
        }))
        .unwrap();
        assert_eq!(ok.state, ClaimState::Claimed);
        assert_eq!(ok.points, 200);

        let already = parse_webview_checkin_result(&json!({
            "status": "already",
            "points": 0,
            "message": "今日已签到"
        }))
        .unwrap();
        assert_eq!(already.state, ClaimState::Claimed);

        let err = parse_webview_checkin_result(&json!({
            "status": "auth_expired",
            "message": "豆包登录态已失效"
        }));
        assert!(err.is_err());
    }
}
