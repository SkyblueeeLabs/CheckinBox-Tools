# CheckinBox —— 项目现状与交接

> 这份文档面向**接手这个项目的人或 AI**。目标是：读完它 + `README.md`，就能在没有
> 原作者在场的情况下继续开发、排查、发布。
>
> 最后更新：2026-10-29 · 版本 `0.1.0` · 分支 `master`

---

## 1. TL;DR

- **是什么**：Windows 桌面工具（Tauri 2 + React 18 + Rust），把本机已登录的 AI 编程客户端
  接进来，统一看积分 + 按平台自己的活动刷新时刻自动签到 + 看流水和趋势。
- **现在能跑**：MiniMax Code、Qoder CN、WorkBuddy、妙手 四个平台的「账号接入 / 签到 /
  积分 / 趋势 / 流水」全部打通；能出独立 exe 和 NSIS/MSI 安装包。
  （WorkBuddy 为后加：积分已真机验证，换号和聊天同步的端到端待复验，见 §8.3）
- **最大不确定性**：所有接口都是**客户端内部非公开接口**，没有文档、没有契约、随时可能变。
  代码里每处协议都写了实测结论，改之前先读注释。
- **最需要注意的约束**：这是**纯 Windows** 项目（凭据用 DPAPI 加密），不要指望在 mac/linux 上跑。
- **数据目录**：2026-10-29 起为 `%APPDATA%\CheckinBox\`，旧目录 `AgentCheckinTools`
  首次启动时自动复制迁移（见 §5 / §7）。

---

## 2. 定位与边界

**做**：本机客户端的登录态复用、每日领取、积分/趋势展示、自动调度、签到流水、
客户端换号、系统托盘常驻。

**不做**（刻意的）：

- 不做 Trae —— 已有 Cockpit Tools 等工具覆盖，不重复造。
  （WorkBuddy 原也在此列，2026-10-28 已接入，见 §3。）
- 不做「每个账号一份隔离的客户端 userData」—— 那是更大的架构改动（见 `core/launcher.rs` 顶部注释）。
- 不做云端同步 / 多机共享 —— 凭据 DPAPI 加密天然绑定本机本用户。
- 不做自动更新。

---

## 3. 完成度清单

| 功能 | 状态 | 入口 |
|---|---|---|
| 平台适配器抽象（新增平台不动其他层） | ✅ | `src-tauri/src/core/provider.rs` |
| MiniMax Code：本机导入 / 状态 / 领取 / 积分 | ✅ 真机验证 | `providers/minimax.rs` |
| Qoder CN：设备授权 / 活动领取 / 积分 / 真实昵称 | ✅ 真机验证 | `providers/qoder.rs` |
| 妙手：本机导入 / 积分（**无每日签到**） | ✅ 真机验证 | `providers/miaoshou.rs` |
| WorkBuddy（CodeBuddy）：Cockpit 账号库多账号导入 / 本机客户端导入 / 积分 / 签到（当前账号域未开活动时如实显示） | ✅ 真机验证（4 账号积分正常） | `providers/workbuddy.rs` |
| 凭据 DPAPI 加密落盘 | ✅ | `core/dpapi.rs` + `core/store.rs` |
| Electron safeStorage（OSCrypt v10）加解密 | ✅ 有单测 + selftest roundtrip | `core/os_crypt.rs` |
| 账号池（多账号 / 当前账号 / 移除） | ✅ | `core/store.rs`、`core/config.rs` |
| 按 agent 独立的自动签到（开关 + 时间窗） | ✅ | `core/scheduler.rs`、`commands.rs::scheduler_loop` |
| 确定性随机错峰 + 窗内补跑 + 时间窗变更重排 | ✅ 有单测 | `core/scheduler.rs` |
| 自动同步（每 agent 间隔 + 跨天强制） | ✅ | `commands.rs::sync_one` / `scheduler_loop` |
| 签到流水（按月 JSONL，按 agent 过滤） | ✅ | `core/logging.rs`、`components/LogPanel.tsx` |
| 积分趋势（快照 + 迷你图 + 悬浮数值） | ✅ | `core/quota_history.rs`、`components/TrendLine.tsx` |
| 积分预警阈值（低于阈值标红） | ✅ | `config.rs::alert_threshold_of` |
| 客户端换号（备份→写入→启动） | ✅ **四个平台全支持**（Qoder 走 safeStorage 解密重加密；WorkBuddy 走 state.vscdb secret 键重加密，端到端待真机复验） | `core/client_switch.rs` |
| 换号同步聊天记录（WorkBuddy 合并 / Qoder 换库，agent 面板顶部开关，落盘持久化） | ✅ 有 10 个单测（借鉴 Cockpit `share_sessions_on_switch`；MiniMax 服务端自动同步不走此开关） | `core/chat_sync.rs` |
| 历史备份管理（设置页 tab：按平台列出每次换号备份，逐条/按天批量删除） | ✅ 有前端测试（只列 `_backups`，账号归档不可删） | `components/BackupPanel.tsx` |
| 换号前检测客户端是否在运行（运行则拦截） | ✅ 已实测（Toolhelp32 快照） | `core/launcher.rs::is_running` |
| 客户端 exe 动态定位（App Paths 注册表 + 目录扫描 + 兜底） | ✅ 已实测 | `core/launcher.rs` |
| 客户端当前登录账号检测（只读） | ✅ 四个平台（Qoder 优先解密 auth.v1.dat；WorkBuddy 读 state.vscdb） | `core/client_switch.rs::detect` |
| 设置页（卡片默认收起，点标题展开；同步 / 调度 / 关闭行为 / 数据 / 说明） | ✅ 有前端测试 + 真机验证 | `SettingsPanel.tsx` |
| 签到记录（默认收起，点标题展开；标题旁显示条数） | ✅ 有前端测试 | `LogPanel.tsx` |
| 系统托盘（显示 / 立即全部签到 / 退出） | ✅ | `tray.rs` |
| 关闭窗口行为（每次询问 / 最小化到托盘 / 直接退出，可记住选择） | ✅ 真机验证 | `lib.rs` + `CloseConfirmDialog.tsx`、`SettingsPanel.tsx` |
| 前端事件通道（签到进度 toast / 自动同步刷新 / 关闭确认） | ✅ 真机验证（此前缺少 capabilities，事件被 ACL 静默拒绝） | `src-tauri/capabilities/default.json` |
| 首页总览（全局指标卡 + 各 agent 积分卡，默认落地页） | ✅ | `DashboardPanel.tsx` |
| 侧栏 agent 拖拽调序（pointer 自实现；顺序落盘 config.agentOrder） | ✅ 有单测 | `config.rs::sort_provider_ids`、`Sidebar.tsx` |
| 浮动卡跟手 DOM 直改（绕过 setState 帧延迟，漂移实测 ±1px） | ✅ | `Sidebar.tsx` |
| 账号小卡片（收起关键信息 + 点击展开详情） | ✅ 有前端测试 | `AccountCard.tsx` |
| 签到记录 id 副名字 + 每 agent 打码开关 + 按 id 解析当前名字 | ✅ 有前端测试 | `LogPanel.tsx`、`lib/mask.ts` |
| 命令行入口（`--selftest` / `--import-local` / `--device-auth` / `--switch-client`） | ✅ | `main.rs` |
| 独立 exe / NSIS / MSI 打包 | ✅ | 见 README「快速开始」 |
| 单元测试 | ✅ Rust 40 个（调度 3 + Qoder 活动周期 6 + OSCrypt 4 + 定位/进程 2 + 聊天同步/备份 10 + 数据目录迁移/降级 4 + 配置/刷新/并发 11） | `cargo test` |
| 前端自动化测试 | ✅ 40 个（分组聚合 + 账号卡/签到记录渲染/折叠 + 聊天同步开关 + 设置页 tab/折叠/关闭行为 + 关闭确认框 + 备份管理） | `npm test` |
| CI | ❌ 无 | — |

---

## 4. 架构地图

### 4.1 分层

```
┌─ 前端 React（src/）─────────────────────────────────────┐
│  App.tsx  全局状态；调 invoke；45s 轮询；订阅事件        │
│  components/  纯展示 + 回调，不含业务逻辑                │
│  lib/api.ts   invoke 封装 + 事件订阅（唯一出口）          │
└──────────────────────┬──────────────────────────────────┘
                       │ Tauri IPC（34 个命令）
┌──────────────────────▼──────────────────────────────────┐
│  commands.rs  命令层 + 调度器后台循环（30s 一轮）         │
│    · 组装 Overview（前端一次拿全）                        │
│    · run_checkin：查状态 → 需要才领取                     │
│    · sync_one：刷新状态 + 积分快照 + 账号资料             │
└──────────────────────┬──────────────────────────────────┘
┌──────────────────────▼──────────────────────────────────┐
│  core/   领域层：provider 抽象 / store / config /         │
│          scheduler / logging / quota_history /            │
│          client_switch / chat_sync / launcher /           │
│          dpapi / tz / paths                               │
└──────────────────────┬──────────────────────────────────┘
┌──────────────────────▼──────────────────────────────────┐
│  providers/   平台实现：minimax.rs / qoder.rs / miaoshou.rs│
│  （只依赖 core::provider 的 trait，互不感知）              │
└──────────────────────────────────────────────────────────┘
```

**关键约定**：上层只依赖 `Provider` trait（`core/provider.rs`）。
新增平台 = 在 `providers/` 加一个实现 + 在 `providers/mod.rs::all()` 登记，
`core/`、`commands.rs`、前端都不用改（图标除外，见 §11）。

### 4.2 数据流（一次签到）

```
scheduler_loop（30s）
  → 跨天？roll_over 重排计划（按各 agent 的时间窗）
  → 该账号今天没处理 && now >= 计划分钟 && now <= 窗口结束？
      → run_checkin(provider.status → 需要则 provider.claim)
      → record_result：落流水 + 广播 checkin-progress 事件给前端
  → 按 agent 间隔到点？sync_one（状态 + 积分快照 + 资料）
      → 广播 accounts-synced 事件 → 前端 refresh()
```

### 4.3 前端状态

`App.tsx` 一个 `overview` 对象装下所有东西（providers / accounts / agents / 汇总），
由后端 `overview` 命令一次组装。前端不做数据合并，只做展示和乐观更新
（改阈值/时间窗时先改本地再落盘）。

---

## 5. 关键设计决策

| 决策 | 原因 | 影响 |
|---|---|---|
| **凭据用 DPAPI 加密** | token 不能明文落盘 | 数据绑本机本用户；换机器要重新导入 |
| **生产构建必须带 `custom-protocol` 特性** | 不带则界面不嵌入 exe，运行时连 devUrl（localhost:5173）→「localhost 拒绝连接」错误页 | lib.rs 有 compile_error 护栏（release 且无特性 = 直接编译失败）；tauri-cli 2.12+ 不再自动带，必须显式：`npx tauri build --no-bundle --features custom-protocol` 或 `cargo build --release --features custom-protocol` |
| **MiniMax 凭据双通道保鲜** | 当前登录账号由客户端自己续期（工具只捕获，不动它的 RT）；非当前登录账号只能靠自己存着的 RT 续 | 见 `commands.rs::ensure_fresh_cred`：按账号加锁 → 重读池 → 刷新 → 落盘 → 解锁 |
| **401 时立即强制 RT 刷新抢救** | 上游会提前吊销 token（客户端切号/重新登录作废旧会话）：`expires_at_ms` 未到也会 401，等到期刷新是等不来的（2026-10-27 实测） | `providers/minimax.rs::request_with_retry`：非当前登录 401 → 先采纳池内新凭据（防重复轮换）→ 否则强制刷新并落盘 → 新 token 重试一次；仍 401 才报「需重新导入」 |
| **调度配置按 agent 独立，无全局时间窗** | Qoder 每天 10:00 才刷新活动，其他平台是自然日 | 每个平台可以配不同窗口；UI 里配置嵌在各 agent 面板顶部 |
| **确定性随机分钟偏移**（FNV-1a of `date:key`） | 多账号同时打同一上游容易被风控 | 同账号同一天重启后计划不变，可复现 |
| **时间窗内补跑 + 窗变更重排** | 晚启动不该整天漏签；改窗口后旧计划会失效 | `scheduler.rs::roll_over` 会清掉「已处理」标记 |
| **流水只记「成功（当天每账号一条）」和「失败（10 分钟节流）」** | 「今日已签」反复落没有信息量，只会刷屏（2026-10-26 用户反馈）；失败不节流会被调度器 30 秒重试刷成灾难（2026-10-27 实测 16 分钟 32 条） | `record_result` 按状态判定（`should_record_status`），成功用 `logging::has_success_today` 去重、失败用 `has_recent_failure` 节流；`recent()` 读取时过滤历史 already 存量（磁盘原始数据保留） |
| **跨天强制同步，不看间隔** | 否则界面会停在昨天的「今日已签」 | 每天第一次同步一定发生 |
| **活动周期用「今天 10:00 起的窗口」判定** | 活动行是跨天窗口（10:00 → 次日 09:59） | 修掉了「10:00 前显示今日已签」的假阳性，有 6 个单测 |
| **客户端换号 = 改写客户端凭据文件** | 客户端单账号，没有官方多账号机制 | 必须先完全退出客户端，否则内存里的旧登录态会覆盖；**运行中会被自动拦截**（Toolhelp32 进程检测）；会留 `.bak` 备份；换号成功后「当前账号」自动同步 |
| **换号同步聊天记录 = 换号前把本地会话带给目标账号**（借鉴 Cockpit 的 `share_sessions_on_switch`） | WorkBuddy 的会话按 uid 分目录存本地、Qoder 的聊天库是本机全局的，换号后历史不跟人走；MiniMax 存服务端自动同步，**不需要也不提供**此开关 | WorkBuddy 走目录级**合并**（同 id 比 lastMessageAt 新者胜，目标已有的不丢）；Qoder 走**整库归档/回填**（main.sqlite 无账号字段，重映射不可行）；动手前先备份到 `chat_history\<平台>\_backups\`；sqlite 主库 + wal/shm 成组搬；同步失败不阻塞换号；开关默认**关**（`config.chatSyncEnabled`，落盘重启保持），关闭时行为与旧版完全一致 |
| **Qoder 换号 = safeStorage 解密重加密** | token 是 `v10`+AES-256-GCM，密钥在 `Local State` 里由 DPAPI 保护，同用户可解可写 | 依赖 `core/os_crypt.rs`；roundtrip 在 `--selftest` 常驻验证；上游改加密方案时 selftest 会立刻暴露 |
| **客户端 exe 动态定位**（App Paths 注册表 + 标准目录两级扫描 + 旧路径兜底） | 换机器/换安装位置不用改代码 | `core/launcher.rs`；Qoder 的 `.qoder-versions\<版本>\` 布局单独处理 |
| **数据目录已改为 `CheckinBox`**（2026-10-29） | 旧名 `AgentCheckinTools` 与产品名不一致，用户也找不到数据 | `paths.rs::data_dir` 首次访问时把旧目录**整目录复制**（先写临时目录再 rename）到新目录；新目录已存在则不动（绝不覆盖），旧目录保留作备份；迁移失败退回旧目录；有 4 个单测 |
| **关闭窗口默认「每次询问」**（2026-10-29） | 既要能后台常驻（自动签到依赖进程活着），也要能彻底退出；一刀切最小化会让想退出的人困惑 | `lib.rs` 拦 `CloseRequested`：tray = `prevent_close + hide`、exit = 放行、ask = prevent + 广播 `close-requested`；前端确认框可「记住选择」，设置页可随时改三选一 |
| **事件必须用 `app.emit` 广播 + 声明 capabilities**（2026-10-29） | ① `window.emit` 的目标是 Window，而 JS `listen` 默认监听 `{kind:"Any"}`，收不到；② 没有 capabilities 文件时 `plugin:event|listen` 被 ACL 拒绝（`Command plugin:event|listen not allowed by ACL`），所有事件监听静默失效 | `src-tauri/capabilities/default.json` 声明 `core:event:default`；后端统一用 `app.emit`；新增事件监听前先确认这一层通 |
| **产品名用纯 ASCII** | WiX 的 `LGHT0311`（代码页 1252 装不下中文） | 安装包能直接打；中文只在窗口标题/界面上出现 |
| **release profile: `panic = "abort"` + LTO + strip** | 体积（约 4 MB） | panic 不能 unwind，别依赖 catch_unwind |

---

## 6. 上游协议要点与坑

> 三份协议的**实测结论**都写在各自文件顶部注释里，这是最有价值的资产，改动前必读。

### MiniMax Code（`providers/minimax.rs`）

- 登录态：`~/.minimax/auth/<buildEnv>/<region>/<clientId>/auth.json`，**明文**。
  层级随版本变 → 递归查找（深度 ≤ 4），不写死层数。
- ⚠️ **记录键的 hash 是设备级的（2026-10-24 实测）**：`<clientId>\0<设备hash>`，
  在客户端里换号登录后**键不变、只有 token 换**。三大后果（都已按此实现）：
  1. 账号 id 必须用客户端配置 `%APPDATA%\MiniMax\minimax-agent-cn-config.json` 里的
     `sharedUser.userID`（账号级）。用记录键 hash 当 id 会造成「导入新号覆盖旧号」
     （启动时有 `migrate_device_key_ids` 一次性迁移旧池条目）。
  2. 「凭据属于谁」的判定（`client_login_is`）= `sharedUser.userID == 账号 id`，
     不能比对记录键 —— 否则会拿当前登录号的 token 给池里所有号用（数据串号）。
  3. 客户端换号（`switch_minimax`）必须**沿用文件里已有的记录键**，不能自己拼；
     客户端从未登录过（没有键）时如实报错。
- ⚠️ **客户端配置里的 `sharedUser.userID` 不可信**（2026-10-25 实测踩坑）：它只在
  客户端自己走登录流程时更新，外部切换（我们写文件）后一直是陈旧的 —— 信它会
  把 A 账号的凭据捕获进 B 账号的池条目（污染账号池）。**身份判定一律走 token 实测**：
  `GET agent.minimax.cn/v1/api/user/info?device_platform=mcode&biz_id=3&app_id=3001…`
  → `data.userInfo.{userID, subUserName}`（`minimax.rs::client_session`，按 token
  前缀缓存，轮换自动重探）。另：**refresh token 轮换即作废**（invalid_grant），
  切换前的捕获必须在写文件**之前**做，否则旧会话的 RT 就永久丢了。
- ⚠️ **客户端有带防篡改围栏的鉴权状态机**（`auth-state.json`，与 auth.json 同目录）：
  登录成功态是 `status: "authenticated"` 且 **generation 必须与凭据文件严格一致**。
  只写 auth.json 不写状态日记，客户端启动会判会话不可信、直接弹重新登录
  （2026-10-24 实测踩坑）。`switch_minimax` 现在**双文件一起写**（凭据 + 配套的
  authenticated 状态，generation 取两者最大值 +1），两个文件都会先备份。
- 签到接口**必须带 `?timezone_id=<IANA>`**，否则 `1406010011 invalid timezone_id`。
  IANA 名由 `core/tz.rs` 从注册表 `TimeZoneKeyName` 映射得到（表里没有则退回 `Asia/Shanghai`）。
- 响应包一层 `base_resp.status_code`（0 成功）。
- 面板语义：`days[].status` 1=Upcoming 2=Claimable 3=Claimed 4=Disabled；
  `claim_result` 1=Claimed 2=AlreadyClaimed。
- 连签加成由上游下发（`bonus_points` / `day_no`），不要在前端硬编码天数规则。
- 401 处理：客户端没在运行 → **快速失败**（不退避白等，加载中卡半分钟的主因）；
  登录的是别的账号 → 快速失败并提示「重新导入 / 切换客户端账号」；
  登录的是本账号且客户端在运行 → 退避重试（等客户端续期 token）。
- **OAuth 刷新（2026-10-24 从 app.asar 逆向 + 实测）**：
  `POST https://account.minimax.cn/oauth2/token`（form 编码），
  `grant_type=refresh_token & refresh_token=<mmort_…> & client_id=mcode-public
  & scope=agent.default & audience=agent-backend` →
  `{access_token(1h), refresh_token, expires_in:3600}`。
  **refresh token 每次轮换（旧作废）** → 刷新后必须回写池；且绝不能刷
  「客户端当前登录」的账号（会踩坏客户端手里的 RT）。凭据保鲜的完整策略见
  `commands.rs::ensure_fresh_cred`（捕获当前登录 + 自刷新非当前 + 切换前捕获，
  方案借鉴 Cockpit Tools 的 `trae_account_core_refresh` / 运行中账号保护）。

### Qoder CN（`providers/qoder.rs`）

三个坑，全部已在代码里处理：

1. **设备风控头必须带**：`Cosy-MachineToken` / `Cosy-MachineCode` / `Cosy-MachineType`，
   来源是本机 `~/.qoder-cn/umid-cache.json`。少了活动根本不下发。
2. **活动周期按「10:00 刷新」算**：活动行 `[startAt, endAt]` 是跨天窗口。
   「今天能不能签」= 今天 10:00 起的那个窗口。直接找 `CLAIMED` 行会把上一个周期的
   记录当成今天已签 → 10:00 前一直显示「今日已签」。
3. **只认 `benefit.kind == CREDITS` 的活动**：列表里混着 `REDEMPTION_CODE` 类
   （如「发布站点领奶茶兑换卡」），不筛会领错东西。

另外：`expires_at` 实测是 ISO 字符串不是数字；账号 id = `sha256(uid)` 前 16 位十六进制；
真实昵称走 `GET /api/v1/userinfo`（授权只给 uid，界面默认只能显示 `Qoder 01a1348c-3fc`）。

**客户端登录态（换号基础，2026-10-24 破解）**：`%APPDATA%\com.qodercn.app.stable\auth.v1.dat`
是 Electron safeStorage（Chromium OSCrypt）加密的 JSON：
`"v10" || nonce(12B) || AES-256-GCM 密文 || tag(16B)`；AES 密钥（32 字节）在旁边
`Local State` 的 `os_crypt.encrypted_key`（base64，`DPAPI` 前缀 + CryptProtectData 输出）。
同一 Windows 用户可以用 DPAPI 解出密钥 → 解密登录态，也可以重新加密写回，
因此 **Qoder 支持换号**（`core/os_crypt.rs` + `client_switch.rs::switch_qoder`）。
解密出的 JSON 结构：`{schemaVersion, token(dt-…), refreshToken(drt-…), expiresAt,
refreshTokenExpiresAt, user:{id,name,email,phone,avatarUrl}, authenticationContext}`，
其中 `authenticationContext` 是账号无关的受众列表，切换时从现有文件抄或用实测默认值。

### WorkBuddy（`providers/workbuddy.rs`）

- 凭据两处来源（discover 按「Cockpit 库 → 本机客户端」顺序）：
  1. **Cockpit Tools 账号库**（多账号）：`~/.cockpit_tools/workbuddy_accounts/<id>.json`
     是 AES-256-GCM 信封（`nonce`/`ciphertext` 字段），密钥是
     `~/.cockpit_tools/secure-account-storage.key`（base64 的 32 字节），无 AAD，已实测解密。
  2. **本机 CodeBuddy 客户端**（单账号）：`%APPDATA%\CodeBuddy\User\globalStorage\state.vscdb`
     （SQLite **TEXT** 列）键 `secret://{"extensionId":"tencent-cloud.coding-copilot","key":"planning-genie.new.accessToken"}`，
     值是 safeStorage（v10）Buffer——和 Qoder 同一套，复用 `core/os_crypt.rs`。
- **协议（参考 jlcodes99/cockpit-tools 的 codebuddy_cn 实现 + 本机实测）**：
  签到状态 `POST /v2/billing/meter/checkin-activity-status`（退回 `/checkin-status`）、
  签到 `POST /v2/billing/meter/daily-checkin`、积分 `POST /v2/billing/meter/get-user-resource`
  （body 带 `PageNumber/PageSize/ProductCode:"p_tcaca"/Status:[0,3]` + 时间范围）、
  刷新 `POST /v2/plugin/auth/token/refresh`（`X-Refresh-Token` 头）。
- **两个必须踩对的点**：
  1. **基址跟随账号 domain**（`www.workbuddy.cn` / `www.codebuddy.cn` / 国际版 `.ai`）——
     写死 `.cn` 会让国际版 401；
  2. **网关要求浏览器 UA + `X-User-Id`(uid) + `X-Domain` 头**——少一个就 401
     （HTML 页的 401 Authorization Required 是 nginx 层拦的）。
- 字段兼容 snake_case（`today_checked_in`）和 camelCase（`todayCheckedIn`）两种响应。
- 国际版/部分账号「签到活动未开启」（`active:false`）时如实显示「暂无可签」，不报错。

### 妙手（`providers/miaoshou.rs`）

- 鉴权要**同时**带 `X-Auth-Token` 和 `access-token`（同值），少一个 `4011`。
- **没有每日签到**：`credit/register` 只是「注册到积分中心 + 新用户奖励」，老用户返回
  「用户已注册」。积分来自套餐。别去实现「每日领取」。

---

## 7. 数据与凭据

数据目录 `%APPDATA%\CheckinBox\`（布局见 README「数据目录」；旧名 `AgentCheckinTools`
首次访问时自动复制迁移，旧目录保留作备份）。

- `accounts.json`：凭据字段是 DPAPI 密文的 base64，账号元信息（昵称/来源/时间）明文。
- 账号键：`provider:id`。各平台 id 来源不同：
  - `minimax`：客户端配置里的 `sharedUser.userID`（身份一律用 token 实测确认；
    `auth.json` 的 records 键 hash 是**设备级**的，不能当账号 id，见 §6）
  - `qoder`：`sha256(uid)` 前 16 位十六进制
  - `miaoshou`：`account.uid`
  - `workbuddy`：登录态里的 uid（随账号 domain 走）
- 落盘一律走 `paths::atomic_write`（写 `.tmp` 再 rename）。
- **日志和 UI 里不出现任何 token**。加日志时注意别把 `cred` 整体打出来。

---

## 8. 已知问题与限制

1. **MiniMax token 约 1 小时**，工具已能靠 refresh token 自动续期（`ensure_fresh_cred`
   按账号加锁 + 重读池 + 刷新落盘，见 §7「已清掉的坑」最后一条）。仍然存在的边界情况：
   账号**从未**进过客户端（纯设备授权导入）且长期不刷新，RT 到期后只能重新导入。
2. **Qoder 换号的端到端链路**（写入 auth.v1.dat 后客户端真实读回）只做到了
   加解密 roundtrip 实测（selftest 常驻），没有做过「真切换一次再看客户端登录」
   的全链路验证 —— 有 `.bak` 备份兜底，首次真机切换时留意一下。
3. **WorkBuddy 换号是 2026-10-28 新补的能力**（写 state.vscdb 的 secret 键），
   和 Qoder 一样只做了单测级验证，**没有真机换过一次号**：CodeBuddy 客户端的
   登录态除了 secret 键可能还有别的缓存（memento / storage.json），首次真机
   切换时确认客户端真的登成了目标账号；失败可回滚 `.bak`。
4. **换号同步聊天记录的边界**：Qoder 的聊天库版本升级加新表/新文件时，同步
   范围只覆盖 `chat_sync.rs::QODER_CHAT_FILES` 里列出的三个库，新文件不会被
   同步（不丢数据，只是那部分不带过去）；CodeBuddy 的 `genie-history`
   （`%APPDATA%\CodeBuddy\User\globalStorage\tencent-cloud.coding-copilot\`，
   插件自己的对话历史，不按 uid 分目录）目前**不在**合并范围内，只合并
   `CodeBuddyExtension\Data` 下按 uid 分目录的会话。
5. **上游接口随时可能变更或下线**，且没有版本协商。出问题时第一件事是
   用 `--selftest` 看是哪一个平台挂了，再去抓对应客户端的真实请求对比。
   Qoder 若改了加密方案，selftest 的 OSCrypt roundtrip 也会立刻报出来。
6. **窗口尺寸**固定初值 1080×720（可缩放，最小 900×620）。
7. **数据目录迁移只做一次**：仅当 `%APPDATA%\CheckinBox\` 不存在且旧目录存在时复制；
   迁移后又运行旧版本往旧目录写，新目录不会二次同步（数据不丢，手动复制即可）。
   确认新目录数据正常后，旧目录 `AgentCheckinTools` 可手动删除。
8. 没有 CI。Rust 40 个单测 + 前端 40 个测试都只能本地跑。

---

## 9. 待办 / 待决策

**待用户决策**

- （暂无。数据目录迁移已于 2026-10-29 按「复制 + 保留旧目录」落地。）

**明确待做**

- [ ] Qoder 换号的真机端到端验证：找一台机器真的切一次，确认客户端启动后
      登录的是目标账号（roundtrip 已过，只差这一步，见 §8.2）。
- [ ] WorkBuddy 换号 + 换号同步聊天记录的真机端到端验证（同上，见 §8.3）。
- [ ] CI（GitHub Actions：`cargo test` + `npm test` + `npm run build`）。

**2026-10-29 已完成**

- [x] 项目名统一为 CheckinBox：crate/lib 名 `agent-checkin-tools` →
      `checkinbox` / `checkinbox_lib`；数据目录迁到 `%APPDATA%\CheckinBox\`
      （首次启动整目录复制，旧目录保留作备份，迁移失败退回旧目录，4 个单测覆盖）；
      文档同步刷新。
- [x] 关闭窗口行为：默认每次询问（最小化到系统托盘 / 直接退出，可勾选「记住我的
      选择」），设置页可随时改三选一；选择落盘 `config.closeAction`。托盘驻留时
      调度器和自动同步继续运行。
- [x] 补 `src-tauri/capabilities/default.json`（`core:event:default`）：此前没有
      capabilities，前端 `listen()` 全部被 ACL 拒绝，签到进度 toast / 自动同步刷新
      / 关闭确认都收不到事件。顺带把后端事件统一为 `app.emit` 广播
      （`window.emit` 的目标是 Window，JS 默认监听 Any，收不到）。
- [x] 设置页视觉：五张卡片（自动同步 / 自动签到调度 / 关闭窗口 / 数据 / 说明）
      默认全部收起，点标题展开（`aria-expanded` + 旋转箭头），可多张同时展开；
      「关闭窗口」收起时在标题旁显示当前选择。
- [x] 视觉细节：签到记录默认收起；侧栏「设置」换成齿轮图标、删除左下角
      「积分仅用于平台内 AI 任务消耗」提示，设置按钮固定在侧栏最底部；
      设置页标题前的空白方块也补上齿轮。

**2026-10-24 已清掉的坑**

- [x] ~~客户端换号前检测客户端是否在运行，在运行就拦住并提示~~
      → `launcher.rs::is_running`（Toolhelp32 快照）+ 前端预检 + 后端兜底拦截，已实测。
- [x] ~~通用空态类 `.empty` 改名成 `.empty-state`，拆掉类名冲突的雷~~
- [x] ~~`core/launcher.rs` 的客户端路径改成「注册表/开始菜单搜索 + 候选列表」~~
      → App Paths 注册表（`winreg`）+ 标准安装根两级扫描 + 旧路径兜底，MiniMax / Qoder / 妙手三个平台真机定位成功（WorkBuddy 候选路径已接入，待真机确认）。
- [x] ~~前端加测试~~ → vitest + testing-library，14 个（`src/lib/__tests__`、`src/components/__tests__`）。
- [x] ~~Qoder 客户端换号~~ → safeStorage（OSCrypt v10）破解：DPAPI 解密钥 + AES-256-GCM，
      可解可写，roundtrip 在 selftest 常驻验证（见 §6 Qoder 节）。
- [x] ~~MiniMax 图标满幅视觉偏大~~ → 满幅图标自动补白边（`agentIcons.ts::iconNeedsPad`）。
- [x] ~~趋势图无悬浮数值~~ → 每个数据点带 `<title>` 提示（时间 + 剩余积分）。
- [x] 多账号展示：客户端登录中的账号排最前、头像平台色描边 + 「客户端登录」徽标，
      与「当前账号」徽标明确区分；换号成功后「当前账号」自动同步。
- [x] ~~MiniMax 换号后「从本机导入」覆盖旧号、只剩一个账号~~ → 根因：登录态记录键的
      hash 是**设备级**的（换号不变），旧代码拿它当账号 id。已改为用 `sharedUser.userID`
      （账号级），detect/switch/token 判定三处联动修正，旧池条目启动时自动迁移
      （见 §6 MiniMax 节）。被覆盖丢失的旧号凭据救不回来：在客户端登录它 → 重新导入即可。
- [x] **MiniMax 多账号状态保存**（借鉴 Cockpit Tools）：当前登录的账号每次同步
      捕获最新凭据回池；非当前账号临过期自动 OAuth 刷新（RT 轮换回写）；
      切换客户端账号前先捕获当前登录（Qoder 也支持，解密 auth.v1.dat），
      池里的账号不再「切走就死」。自动签到前也会刷新，非当前账号的 morning 签到不再漏。
- [x] **切换一条龙（2026-10-25）**：借鉴 Cockpit 的 `launch_on_switch` —— 点击
      「切换并打开客户端」自动 **关闭客户端（优雅→强制→稳定退出确认）→ 捕获当前
      登录 → 备份 → 写入目标凭据+状态日记 → 以目标账号重启**，不再要求手动退出。
      稳定退出确认是必须的：客户端退出时的收尾写盘晚于主进程，等得不干净写入会被覆盖。
- [x] ~~Qoder 换号后刷新 token 时长信息不完整~~ → 设备授权轮询现在会捕获
      `refreshTokenExpiresAt` 存进凭据（`providers/qoder.rs`）。
- [x] **MiniMax 账号约 1 小时就「凭据已失效（HTTP 401）」** → 根因：MiniMax 的
      refresh token 每次刷新都轮换，而 `overview`（前端每 45 秒轮询）走的是
      **纯凭据层** `ensure_fresh_cred_minimax_only`，**刷新后没有把新 RT 回写账号池**。
      于是池里留的是已作废的旧 RT，下次刷新必然 `invalid_grant`，账号永久失效，
      只能重新导入或切客户端账号（2026-10-26 实测：`Skyblueee` 失效 5.7 小时、
      `Skyblueee-a` 复活后 54 分钟再次失效）。
      修法分两步，最终对齐了官方 MiniMax CLI 与 Cockpit Tools 的做法：
      1. 刷新结果必须落盘（RT 轮换与回写成对发生）；
      2. `commands.rs::ensure_fresh_cred` 改成**账号池感知**，做三道防线：
         **按账号加锁**（同一账号串行、不同账号并行）→ **拿到锁后重读账号池**
         （别人刚刷新过就直接采纳，不拿旧 RT 去撞 `invalid_grant`）→
         **先落盘再解锁**（后面的调用方重读到的就是新 RT）。
      因为要读写账号池，凭据保鲜从 `overview` 的并发任务挪到了主循环
      （网络请求仍并行，不受影响）。
      其余平台不受影响——它们的 token 有效期是「天」级，`ensure_fresh_cred`
      第一行就直接返回，压根不走刷新。
      参考：`MiniMax-AI/cli` 的 `src/auth/refresh.ts`（single-flight + 落盘在共享
      promise 内）、`jlcodes99/cockpit-tools` 的 `prepare_account_tokens_before_exchange`。
- [x] **重编 exe 后打开是「localhost 拒绝连接」（ERR_CONNECTION_REFUSED）错误页**
      → 裸 `cargo build --release` 不带 `custom-protocol` 特性，界面不嵌入 exe，
      运行时去连 devUrl（http://localhost:5173）。README「打包独立 exe」一节
      其实早就写了这条，但只靠文档拦不住——本次三重防护：
      ① `Cargo.toml` 补上标准的 `[features] custom-protocol = ["tauri/custom-protocol"]`；
      ② `lib.rs` 加 compile_error 护栏（release 且无特性 = 编译期直接报错，
      不再产出能跑但白屏的 exe）；
      ③ README 补上可搜索的症状描述 + 托盘常驻导致的 WebView2 目录锁（0x800700AA）。
      连带教训：**排查"打不开"别只看进程活没活，要看窗口实际加载的 URL**
      （`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222`
      启动后 curl `http://127.0.0.1:9222/json` 可看到页面真实地址）。

**已废弃 / 不要再加回来**

- ❌ **千问办公（QwenWork）**：曾实现过，后来整个移除。原因有二：
  上游 `cn_daily_check_in_legacy` 已经是 `DISABLED`（活动停用），
  且它和 Qoder 共用同一个授权账号体系，接进来只会重复。**不要重新加。**

**可以做的体验打磨（优先级低）**

- 趋势图目前是迷你折线，没有坐标轴。

---

## 10. 接手须知

### 10.1 构建与验证

```bash
npm install
npm run tauri:dev                      # 开发
npx tauri build --no-bundle --features custom-protocol   # 独立 exe（2.12+ 必须显式带特性）
npx tauri build --features custom-protocol               # NSIS + MSI
cd src-tauri && cargo test             # 单测
.\src-tauri\target\release\CheckinBox.exe --selftest   # 不启 GUI 的自检
```

### 10.2 三个必踩的坑

1. **`cargo build --release` 单独跑会白屏** —— 它不走 Tauri 的前端嵌入流程，
   产物去加载 `devUrl`（localhost）。必须用 `tauri build`。
2. **`npm run tauri build -- --no-bundle` 里的 `--no-bundle` 会被 npm 吃掉**，用 `npx`。
3. **shell 里没有 `cargo` 时**，`tauri build` 报
   `failed to run 'cargo metadata' ... program not found`；
   把 `%USERPROFILE%\.cargo\bin` 加进 PATH。

### 10.3 改代码时的注意点

- 前端只通过 `src/lib/api.ts` 调后端；新增命令要同时改 `lib.rs` 的 `invoke_handler` 列表。
- **前端事件监听要确认 capabilities**：没有 `src-tauri/capabilities/default.json`
  里的 `core:event:default` 时，`listen()` 会被 ACL 拒绝且**静默失效**；
  后端发事件统一用 `app.emit`（`window.emit` 到不了 JS 的 `{kind:"Any"}` 监听器）。
- 后端返回给前端的所有结构体都 `#[serde(rename_all = "camelCase")]`，前端类型定义在
  `src/lib/types.ts`，改字段两边一起改。
- 涉及「今日」的判断一律用 `core::scheduler::today()`（本地日期），不要用 UTC。
- 任何会写文件的地方都用 `paths::atomic_write`。
- 平台相关的魔法值/协议注释写在对应 `providers/*.rs` 顶部，不要散落在 `commands.rs`。

### 10.4 验证一次改动的清单

- [ ] `cd src-tauri && cargo test` 全绿
- [ ] `npm run build`（含 `tsc --noEmit`）无错
- [ ] `--selftest` 里四个平台都能读到状态和积分
- [ ] 界面上：侧栏三行等高、积分/状态/趋势正常、切 agent 后日志跟着过滤
- [ ] 改了调度相关：跨天和「改时间窗」两种情况下行为正确
- [ ] 改关闭相关：三种行为（每次询问 / 最小化到托盘 / 直接退出）与「记住选择」都正确
- [ ] 改事件相关：签到进度 toast / 自动同步刷新 / 关闭确认都能收到（capabilities 生效）

---

## 11. 如何新增一个平台

1. `src-tauri/src/providers/<name>.rs`：实现 `Provider` trait。
   至少实现 `id` / `display_name` / `accent` / `docs_url` / `status` / `claim`；
   能自动发现就实现 `discover`，有积分接口就实现 `quota`，有昵称接口就实现 `profile`。
2. `providers/mod.rs`：`pub mod <name>;` + 在 `all()` 里 `Arc::new(...)`。
3. `core/config.rs::AgentScheduleConfig::default_for`：如果这个平台的活动不是自然日刷新，
   在这里给它一个合适的默认时间窗（参考 Qoder 的 10:00–11:00）。
4. `core/launcher.rs`：如果它有客户端 exe，加进 `candidates()`。
5. `core/client_switch.rs`：如果登录态是明文文件，可以加 `detect_*` / `switch_*`
   并更新 `is_switchable`。
6. `src/lib/agentIcons.ts` + `src/assets/agents/<name>.png`：放官方图标
   （从客户端安装目录里取原图，不要自己画）。取色板参考 `Provider::accent`。
7. 跑一遍 `--selftest`（`selftest.rs` 里也可以顺手加一段），再开界面验证。

---

## 12. 开发历史（commit 线索）

```
084496a feat: 聚合签到工具初始实现（Tauri 2 + React）
03e8b39 chore: 零告警清理 + 首次运行落盘默认配置
6cc7925 feat: 积分展示 + cockpit 风格分组 UI
0a400e9 docs: README 更新积分与分组 UI 说明
e79613e fix: dev 端口 1420 -> 5173
2651b9e fix: 设备授权会话持久化，重启/热更新后不丢
0d763f1 fix: Qoder 账号入库 + 积分解析 + 日志去重
d49829a feat: 按 agent 独立调度 + 多账号切换/打开客户端 + 仪表盘风格重设计
7a519a1 docs: README 更新界面与调度说明
624d5a4 feat: 接入妙手 + 千问办公，修两个 bug
760df59 feat: 借鉴 Cockpit/Tokdash —— 积分预警阈值 + 积分趋势
1cc3e39 docs: 补充 release exe 打包说明（tauri build --no-bundle）
ecf7915 feat: 日志按 agent 分开 + Qoder 真实昵称 + 自动同步设置页
b275202 移除千问办公：活动上游已停用，且与 Qoder 共用授权账号
c98ef71 视觉重构：照 Neo Financial 的浅色 iOS 风重做界面
d565674 界面细节：侧边栏/面板换官方图标，修账号行积分区对齐
283da65 feat: 切换客户端实际登录的账号（借鉴 Cockpit 的换凭据做法）
9050cd3 改名 CheckinBox + 换新图标
9fe569d 图标换成生图模型出的版本
391d5d6 图标换成生图的第二批（先放 c2 占位，等选终版）
52fe5fa 图标定稿：cb1（黑底 + 几何 CB + CheckinBox）
```

**参考对象**：Cockpit Tools（调度模型、自动刷新间隔、换凭据换号）、
Tokdash / syrtis（积分快照做趋势）、Neo Financial（界面视觉）。

**最初的设计规格**：`docs/superpowers/specs/2026-10-22-agent-checkin-tools-design.md`
（写于动手前，部分内容已被实现超越，以代码为准）。
- [x] **CLI `--device-auth` 只支持 qoder** → 补上 minimax 的 OAuth 设备授权
      （字段映射与 `begin_device_auth` 命令一致：nonce 存 poll_code、extra 存
      poll_param/interval）。2026-10-27 首次实战：恢复了 RT 被上游吊销的
      1w4g 账号——浏览器确认授权即可，不依赖客户端重新登录。
