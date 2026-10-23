# CheckinBox 设计规格

- 日期：2026-10-22
- 版本：0.1（demo）
- 状态：已实现，待联调验收

---

## 1. 背景与目标

本机装了多款 AI 编程/Agent 工具（MiniMax Code、Qoder CN、通义灵码、CodeBuddy/WorkBuddy、Cursor、Codex、Claude Code 等），
其中带「每日积分」体系的工具需要每天手动打开客户端、找到签到入口、点一下领取。

已装��的 **Cockpit Tools**（`github.com/jlcodes99/cockpit-tools`）已经聚合了 WorkBuddy / CodeBuddy CN / Trae 的账号管理与每日签到，
本项目**不重复造轮子**，而是补齐它没覆盖的两块：

| 平台 | Cockpit Tools | 本项目 |
|---|---|---|
| WorkBuddy / CodeBuddy CN | ✅ 完整支持 | 不重复实现 |
| Trae | ✅ 支持 | 不重复实现 |
| **MiniMax Code** | ❌ 无 | ✅ 第一版 |
| **Qoder CN** | ❌ 只做 OAuth/额度，无签到 | ✅ 第一版 |

**目标**：一个专注「签到」一件事的桌面工具，账号池 + 自动调度 + 状态总览，新增平台只需实现一个适配器。

**非目标（第一版）**：账号切换/托管、额度监控、模型代理、实例管理、多平台账号同步、exe 打包分发。

---

## 2. 关键调研结论

### 2.1 MiniMax Code —— 已在本机实测打通

登录态**明文 JSON**，无需解密：

```
~/.minimax/auth/<region>/mcode-public/auth.json
  records["com.minimax.mcode.oauth.prod.cn\0<hash>"] = {
    accessToken: "mmoat_…", refreshToken: "mmort_…",
    tokenType: "Bearer", clientId: "mcode-public",
    audience: "agent-backend", expiresAtMs: 1792664659088
  }
```

签到接口：

```
GET  https://agent.minimax.cn/minimax-cloud/api/v1/signin/status?timezone_id=Asia/Shanghai
POST https://agent.minimax.cn/minimax-cloud/api/v1/signin/claim?timezone_id=Asia/Shanghai
Authorization: Bearer <accessToken>
```

- **必须带 `timezone_id`**，否则 `base_resp.status_code = 1406010011 / "invalid timezone_id"`。
- 响应包一层：`{ base_resp: { status_code, status_msg }, data: … }`，`status_code != 0` 即错误。
- 面板：`data.days[7] = { day_no, points, bonus_points, status, is_today }`，
  `status` 枚举 `1=Upcoming 2=Claimable 3=Claimed 4=Disabled`。
- 领取结果：`data.claim_result`，`1=Claimed 2=AlreadyClaimed`。
- 实测：当天面板为 `400/400/400/1000/400/400/1000`（连签第 4、7 天加码 1000），积分 30 天有效，仅限 MiniMax Code 使用。

### 2.2 Qoder CN —— 协议确认，签到是「活动领取制」

签到不是固定接口，而是先拉活动列表再领取：

```
GET  https://openapi.qoder.com.cn/sash/api/v1/me/campaigns?forceRefresh=true
POST https://openapi.qoder.com.cn/sash/api/v1/me/campaigns/{campaignId}/claim
```

- 在列表里找 `actionType == "CLAIM_BENEFIT"`：
  - `claimStatus == "CLAIMABLE"` → 可领，`campaignId` 来自 `campaignId` 或 `campaignKey`；
  - `claimStatus == "CLAIMED"` → 今日已领。
- 领取结果判定：HTTP `409` / `result == "ALREADY_CLAIMED"` / `replayed == true` → 今日已领（不算失败）。
- 成功时 `body.status == "CLAIMED"`，积分在 `body.benefit.amount`，有效期 `body.benefit.validity.days`。

**设备授权登录**（无验证码、无需抓包）：

```
GET https://qoder.com.cn/device/selectAccounts?challenge=<b64url(SHA256(verifier))>
    &challenge_method=S256&machine_id=<uuid>&nonce=<hex>
轮询 GET https://openapi.qoder.com.cn/api/v1/deviceToken/poll
    ?nonce=…&verifier=…&challenge_method=S256
头：Cosy-Version: 1.0.1, Cosy-ClientType: 5, User-Agent: qoder-local-proxy
返回：data.token / refresh_token / user_id / expires_at
```

**设备风控头**（缺了活动就不下发）：

```
Cosy-Version, Cosy-MachineOS(x86_64_win32), Cosy-MachineId,
Cosy-MachineHostname, Cosy-MachineToken, Cosy-MachineCode, Cosy-MachineType
```

本机来源：`~/.qoder-cn/umid-cache.json`（含 machineToken/Code/Type）、`~/.qoder-cn/.auth/machine_id`。

**约束**：官方活动窗口每天 **10:00（UTC+8）** 开启；没到点时如实提示「上游暂未下发」，不谎报失败。

### 2.3 Cockpit Tools 借鉴的调度模型

Cockpit 的自动签到不是「每天固定点全打一遍」，而是**时间窗 + 每账号随机分钟偏移**：

```json
{ "enabled": true, "startTime": "06:00", "endTime": "08:00",
  "accountSchedules": { "<accountId>": { "scheduledDate": "…", "scheduledMinute": 473 } } }
```

这样多账号不会在同一秒打同一个上游。本项目沿用该模型，并把偏移做成**确定性随机**（同账号同一天重启后计划不变）。

---

## 3. 架构

```
┌──────────────────────────────────────────────────────────┐
│  React 前端（Vite + TS）                                  │
│  顶栏/调度条 / 平台面板 / 账号卡 / 签到记录 / Toast        │
└───────────────┬──────────────────────────────────────────┘
                │ tauri::command（serde camelCase）
┌───────────────▼──────────────────────────────────────────┐
│  commands.rs —— 命令层                                    │
│  overview / checkin_all / checkin_account / discover      │
│  begin_qoder_auth / poll_qoder_auth / config / logs       │
├──────────────────────────────────────────────────────────┤
│  core/                                                    │
│  provider.rs  平台抽象 trait + 共享类型                    │
│  store.rs     账号池（凭据 DPAPI 加密）                    │
│  config.rs    应用配置（调度时间窗）                        │
│  scheduler.rs 时间窗 + 确定性随机偏移 + 调度状态             │
│  logging.rs   签到流水（按月 JSONL）                       │
│  dpapi.rs     Windows DPAPI 封装                           │
│  tz.rs        Windows 时区名 → IANA                        │
├──────────────────────────────────────────────────────────┤
│  providers/                                               │
│  minimax.rs  MiniMax Code 适配器                          │
│  qoder.rs    Qoder CN 适配器                              │
└──────────────────────────────────────────────────────────┘
```

### 3.1 平台抽象

```rust
#[async_trait]
pub trait Provider: Send + Sync {
    fn id(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    fn accent(&self) -> &'static str;
    fn docs_url(&self) -> &'static str;

    async fn discover(&self) -> Result<Vec<(Account, Value)>> { Ok(vec![]) }
    async fn status(&self, account: &Account, cred: &Value) -> Result<CheckinStatus>;
    async fn claim(&self, account: &Account, cred: &Value) -> Result<ClaimOutcome>;
    async fn refresh(&self, account: &Account, cred: &Value) -> Result<Option<Value>> { Ok(None) }
}
```

`Credential` 用 `serde_json::Value` 而非强类型：各平台需要存的字段不同
（MiniMax 要 access/refresh token，Qoder 还要 machine_id、region）。
凭据本身由 `store` 层用 DPAPI 加密落盘，明文只在内存。

**新增一个平台** = 实现这个 trait + 在 `providers::all()` 登记 + 在 `AppState` 里建实例，其余各层零改动。

### 3.2 凭据安全

- 落盘格式：DPAPI（`CryptProtectData`）加密 → base64 → 存 `accounts.json`。
- DPAPI 密钥绑定当前 Windows 用户：换用户/换机器读不到，程序卸载重装主密钥后失效。
- 直接声明 `extern "system"` 调 `crypt32.dll`，不依赖 `windows` crate，避免 API 版本漂移。
- 日志与 UI **不出现任何 token 内容**；`doctor` 式信息只报格式、过期时间、是否需要人工介入。
- 凭据只发往对应官方域名：MiniMax → `agent.minimax.cn`，Qoder → `openapi.qoder.com.cn` / `qoder.com.cn`。

### 3.3 调度

- 后台 tokio 任务每 30 秒 tick 一次。
- 跨天时重排计划：给每个账号在 `[start, end)` 窗内分配一个**确定性**分钟偏移
  （FNV-1a(日期+账号键)），同一天内重启不变。
- 到点后执行一次；成功/已签则标记 `last_checked_date = today`，当天不再重复。
- 失败不标记，下一个 tick 会重试（不会因为一次网络抖动就丢掉当天签到）。
- 时间窗默认 `06:00–08:00`，可在 UI 改；可按平台单独开关。

### 3.4 托盘

- 左键点击托盘图标 → 显示/聚焦主窗口；右键 → 菜单（显示主窗口 / 立即全部签到 / 退出）。
- 托盘菜单的「立即全部签到」复用 `checkin_all_inner`，与 UI 按钮走同一条路径。

---

## 4. 关键设计决策

| 决策 | 选择 | 理由 |
|---|---|---|
| 技术栈 | Tauri 2 + React + TS | 与 Cockpit 同构；托盘/自动启动原生支持；产物小 |
| 打包 | 第一版不打包 exe | 需求里明确「先做 demo，稳定后再打包」 |
| 触发方式 | 托盘常驻 + 应用内定时器 | 需求指定「软件功能触发」 |
| 凭据保护 | Windows DPAPI | 比自造 AES+密钥文件更安全，零密钥管理 |
| 调度模型 | 时间窗 + 确定性随机偏移 | 借鉴 Cockpit，避免多账号同时打上游 |
| MiniMax 凭据 | 读本机明文登录态 | 免解密，开箱即用 |
| Qoder 凭据 | 设备授权（浏览器确认一次） | 客户端 token 是加密信封，逆向成本高；设备授权无验证码无抓包 |
| Qoder 风控身份 | 读 `~/.qoder-cn/umid-cache.json` | 客户端已生成，无需跑 UMID 组件 |

---

## 5. 数据布局

```
%APPDATA%\AgentCheckinTools\
├── config.json            调度配置
├── accounts.json          账号池（凭据 DPAPI 加密后 base64）
├── scheduler_state.json   当日计划与已签标记
└── logs\
    └── checkin-2026-10.jsonl
```

所有配置类文件用「写临时文件再 rename」原子写，避免掉电写坏。

---

## 6. 错误处理原则

- **不谎报成功**：拿不到状态就报「状态未知」+ 原因，而不是当成已签。
- **区分「今日已领」与「失败」**：Qoder 的 409/ALREADY_CLAIMED/replayed 一律记为「今日已领」。
- **区分「暂不可签」与「失败」**：Qoder 活动未下发（未到 10:00 / 已在客户端领过）记为 skipped，不是失败。
- **失败不写「已签」标记**，调度器下个 tick 自动重试。
- 凭据过期时明确提示「请在客户端重新登录」，而不是反复重试。

---

## 7. 验收标准

1. `cargo build` 与 `tsc --noEmit` 均无错误。
2. 应用启动后，MiniMax Code 账号能从本机登录态导入并显示今日状态（实机应显示「今日已签 +400」）。
3. 点「一键全部签到」，MiniMax 账号返回「今日已签到」而不是失败。
4. Qoder 点「添加账号」能拉起浏览器授权页，授权后账号入库并能读到活动列表状态。
5. 调度器在时间窗内按每账号的随机分钟执行，日志落 JSONL。
6. 托盘图标可交互，右键菜单的「立即全部签到」可用。
7. `accounts.json` 中不出现任何明文 token。

---

## 8. 风险与免责声明

- 调用的是各客户端的**非公开内部接口**，归各厂商所有，可能随时变更或下线。
- 上游改版时，适配器会如实报错（`doctor` 报 `DECRYPT_FAILED` / `KEY_UNAVAILABLE` / `SUITE_UNSUPPORTED` 等），**不会谎报成功**。
- 本项目与各厂商无任何隶属、授权或合作关系；仅供个人学习自用，禁止商业用途。
- 接口使用遵循各平台服务条款，风险由使用者自行承担。

---

## 9. 后续可扩展方向

- 接入通义灵码 / QoderWork CN、Trae、百度搭子等更多平台（只需新增适配器）。
- 借鉴现有 `WorkBuddy-Daily-Checkin` 的接口兜底思路，为每个平台补一层 GUI 自动化兜底。
- 签到日历视图（哪些天断签了、连签奖励差多少）。
- 打包 exe + 开机自启 + 通知（今日到账提醒）。
- 与 Cockpit Tools 联动：直接读取它的账号池，避免重复维护账号。
