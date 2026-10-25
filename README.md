# CheckinBox

> 把散落在各个 AI 编程工具里的「每日领取积分」，收进一个窗口。

![stack](https://img.shields.io/badge/Tauri_2-6366f1) ![rust](https://img.shields.io/badge/Rust-stable-orange) ![react](https://img.shields.io/badge/React-18-61dafb) ![platform](https://img.shields.io/badge/Windows-10%2F11-0078d4)

一个 Windows 桌面小工具：把本机已登录的 AI 编程客户端（MiniMax Code / Qoder CN / 妙手）
接进来，统一查看积分、按平台自己的活动刷新时刻自动签到、看签到流水和积分趋势。

> 接手这个项目（人或 AI）请先读 **[docs/PROJECT-STATUS.md](docs/PROJECT-STATUS.md)** ——
> 那里有现状、关键决策、已知坑和待办。

## 它解决什么

| 平台 | 客户端登录态 | 每日签到 | 积分查询 | 客户端换号 | 本项目 |
|---|---|---|---|---|---|
| **MiniMax Code** | `~/.minimax/auth/**/auth.json`（明文） | ✅ 每日领取，连签有额外加成（上游下发 `bonus_points`） | ✅ | ✅ 可写 | ✅ |
| **Qoder CN** | Electron safeStorage 加密（`auth.v1.dat`，可解可写） | ✅ 活动领取制，**每天 10:00 刷新** | ✅ | ✅ 可写 | ✅ |
| **妙手**（美团 CatPaw） | `~/.meituan-catpaw/auth.json`（明文） | ❌ 积分中心没有每日签到 | ✅ | ✅ 可写 | ✅ 仅接入 + 积分 |
| WorkBuddy / CodeBuddy CN、Trae 等 | — | — | — | — | 不重复造（已有 Cockpit Tools 等） |

**核心原则：失败就如实报错。** 上游接口是非公开的，随时可能变。本工具宁可显示
「签到失败：xxx」，也不会谎报「已签到」。

## 界面

参考 Neo Financial（Mobbin）的浅色 iOS 金融风：**浅灰底 + 白色大圆角卡片 + 极淡阴影 +
近黑主按钮 + 等宽数字**。刻意避开「AI 生成感」：不用渐变、不用发光、不用 emoji 图标。

- 左侧每个 agent 一行：**官方图标**（从本机客户端安装目录里取的原始图标）、名称、账号数、待签/已签角标、剩余积分
- 左侧列表**支持拖拽调序**：按住任意 agent 上下拖动，蓝色指示线提示插入位置，
  松手即保存（顺序持久化到 `config.json` 的 `agentOrder`，重启保持）
- 右侧是该 agent 的面板：积分汇总（剩余 / 累计发放 / 今日已签）+ 账号列表
- 账号是**小卡片网格**：收起时只显示关键信息（头像、名称、徽标、剩余积分、状态摘要），
  点击展开成完整详情（积分明细 / 用量条 / 趋势 / 签到 / 操作），展开的卡片占满一行；
  可签的卡片带橙色描边
- 多账号排序：**客户端登录中的账号最靠前**（头像带平台色描边 + 「客户端登录」徽标），
  然后是当前账号、可签、已签；「当前账号」与「客户端登录」是两个不同的状态，卡片上都有徽标
- 自动签到配置属于每个 agent 自己（开关 + 时间窗 + 预警阈值），嵌在面板顶部
- 账号详情里：剩余积分大字 + 用量条 + `已用 / 发放` + 趋势迷你图（**悬浮显示时间和积分**）+ 批次明细 + 签到状态
- 面板标题下标出**客户端当前登录的是哪个账号**；和工具里选中的不一致时会提示
- 底部：签到记录，**只显示当前选中 agent 的记录**（切到「全部」才合并，并多一列平台）
- 左侧底部「设置」页：自动同步开关与间隔、调度总览、数据目录、说明
- 系统托盘：左键显示主窗口，右键菜单「显示主窗口 / 立即全部签到 / 退出」

## 快速开始

### 环境要求

- Windows 10/11（本项目只做了 Windows：凭据用 DPAPI 加密）
- Node 18+、Rust stable（`rust-version = 1.77`）
- WebView2 运行时（Win11 自带；Win10 可能需装 Evergreen Runtime）

```bash
npm install                 # 前端依赖
cd src-tauri && cargo build # 先编一次 Rust 后端（可选，tauri dev 也会编）
npm run tauri:dev           # 开发模式运行
```

> 端口固定 5173：本机 1420/1421/1425/1500 落在 Windows 保留端口段（vite 会报 EACCES）。
> 换机器若冲突，改 `vite.config.ts` 里的 `server.port`。

### 打包独立 exe（免装环境，约 4 MB）

```bash
npx tauri build --no-bundle
```

产物：`src-tauri/target/release/CheckinBox.exe`。

> ⚠️ 两个坑：
> 1. 直接 `cargo build --release` **不会**走 Tauri 的前端嵌入流程，产物会去加载
>    `devUrl`（localhost）而白屏 —— 必须用 `tauri build`。
> 2. `npm run tauri build -- --no-bundle` 里的 `--no-bundle` 会被 npm 吃掉，
>    用 `npx tauri build --no-bundle`。
> 3. 如果 shell 里找不到 `cargo`，`tauri build` 会报
>    `failed to run 'cargo metadata' ... program not found`；把
>    `%USERPROFILE%\.cargo\bin` 加进 PATH 即可。

### 打包安装程序（NSIS / MSI）

```bash
npx tauri build
```

产物在 `src-tauri/target/release/bundle/` 下：

| 文件 | 大小 | 用途 |
|---|---|---|
| `nsis/CheckinBox_0.1.0_x64-setup.exe` | 约 1.5 MB | 在线安装器 |
| `msi/CheckinBox_0.1.0_x64_en-US.msi` | 约 2.2 MB | 企业部署 |

> 产品名改成纯 ASCII（`CheckinBox`）之后，WiX 的 `LGHT0311`（代码页 1252 装不下中文）
> 就消失了，所以安装包可以直接打。

### 命令行入口（不启 GUI）

```bash
# 自检：DPAPI 加解密 → 时区 → 各平台发现/状态/积分 → 账号池读写（只读状态，不领取）
.\src-tauri\target\release\CheckinBox.exe --selftest

# 把本机客户端登录态的账号全部导入（MiniMax / 妙手）
.\src-tauri\target\release\CheckinBox.exe --import-local

# 命令行版设备授权（发起会话并落盘，轮询交给应用）
.\src-tauri\target\release\CheckinBox.exe --device-auth qoder

# 命令行版切换客户端账号（会备份原登录态并启动客户端）
.\src-tauri\target\release\CheckinBox.exe --switch-client minimax <account_id>
```

### 测试

```bash
cd src-tauri && cargo test     # 15 个单测：调度重排/跨天（3）+ Qoder 活动周期（6）+ OSCrypt 加解密（4）+ 客户端定位/进程检测（2）
npm test                       # 14 个前端测试：分组聚合逻辑 + 账号卡/签到记录渲染（vitest + testing-library）
```

## 用法

1. **MiniMax Code / 妙手**：点「从本机导入」——读本机客户端登录态（明文 JSON）。
   客户端会自动续期 token，本工具每次请求前**重读该文件**，所以不用手动刷新。
2. **Qoder CN**：点「添加账号」——浏览器里用目标账号确认一次授权（设备授权，无验证码）。
3. **一键全部签到**：遍历所有账号，先查状态，需要才领取。
4. **自动签到**：每个 agent 自己的开关 + 时间窗。窗内每个账号有一个**确定性随机**
   分钟偏移（同账号同一天重启后计划不变），避免多账号同时打同一个上游；
   **窗内还能补跑**，晚启动不漏签。
5. **自动同步**（设置页）：按 agent 各自的间隔刷新积分 / 签到状态 / 账号资料。
   **跨天时不看间隔，强制同步一次** —— 新的一天开始后「今日已签 / 今日可签」
   会立刻变成当天的真实状态，而不是停在昨天。
6. **切换客户端账号**：客户端同一时刻只能登一个账号，工具里选的「当前账号」不影响客户端。
   要换客户端实际登录的账号，点账号行里的「切换客户端账号」——
   **检测客户端是否在运行（运行中会拦下）→ 备份客户端当前登录态 → 写入目标账号的凭据
   → 启动客户端 → 工具里的「当前账号」同步成它**（借鉴 Cockpit Tools）。
   MiniMax / 妙手的登录态是明文文件，直接写；Qoder 的 token 是 Electron
   safeStorage 加密（`v10` + AES-256-GCM，密钥由 DPAPI 保护），工具可以解密
   也可以重新加密写回，所以三个平台都支持换号。

## 实现要点

### MiniMax Code

```
登录态  ~/.minimax/auth/<buildEnv>/<region>/<clientId>/auth.json   ← 明文，客户端自动续期
账号 id  %APPDATA%\MiniMax\minimax-agent-cn-config.json → sharedUser.userID
查状态  GET  agent.minimax.cn/minimax-cloud/api/v1/signin/status
领取    POST agent.minimax.cn/minimax-cloud/api/v1/signin/claim
积分    GET  agent.minimax.cn/minimax-cloud/api/v1/credit/details
```

- ⚠️ **登录态文件里记录键的 hash 是设备级的**（`<clientId>\0<设备hash>`）：在客户端里
  换号登录后**键不变，只有 token 换成新号的**。所以账号 id 不能用记录键的 hash
  （会导致「导入新号」覆盖池里的旧号条目），必须用客户端配置里的
  `sharedUser.userID`（账号级，换号会变）。同理，「客户端换号」写入时必须**沿用
  文件里已有的记录键**，不能自己拼。
- **多账号状态保存**（借鉴 Cockpit Tools）：客户端同一时刻只能登一个号，但池里的
  每个账号都能一直活着 ——
  - 当前登录的账号：每次同步把客户端续期后的最新凭据**捕获**回账号池；
  - 不在客户端里的账号：token 临过期（1 小时寿命）时自动走 OAuth 刷新
    （`POST account.minimax.cn/oauth2/token`，refresh token **每次轮换**，新值回写池）；
  - 切换客户端账号前，先把客户端当前登录的账号捕获进池，谁都不丢。
  - ⚠️ 绝不对「客户端当前登录的账号」做 OAuth 刷新 —— RT 轮换会让客户端手里的
    旧 RT 作废、下次续期时掉登录（Cockpit 同款保护）。
- 签到接口**必须带 `?timezone_id=<IANA>`**，否则报 `1406010011 invalid timezone_id`。
- token 生命周期很短（实测约 1 小时），所以每次请求前重读本机 `auth.json`；
  401 时若客户端没在运行则快速失败（不再退避白等）。
- 面板语义：`days[].status` 1=Upcoming 2=Claimable 3=Claimed 4=Disabled；
  `claim_result` 1=Claimed 2=AlreadyClaimed。
- 登录态目录层级随版本变（`auth/prod/cn/mcode-public/auth.json`），所以按深度递归找，不写死。

### Qoder CN

```
设备授权（无验证码）
  GET  qoder.com.cn/device/selectAccounts?challenge=..&challenge_method=S256&machine_id=..&nonce=..
  轮询 openapi.qoder.com.cn/api/v1/deviceToken/poll
签到  GET  openapi.qoder.com.cn/sash/api/v1/me/campaigns?forceRefresh=true
      POST openapi.qoder.com.cn/sash/api/v1/me/campaigns/{campaignId}/claim
积分  GET  openapi.qoder.com.cn/sash/api/v2/me/usage
昵称  GET  openapi.qoder.com.cn/api/v1/userinfo          ← 真实昵称，不是 id 前缀
```

三个坑（都已在代码里处理，改这块前务必读 `providers/qoder.rs` 顶部注释）：

1. **请求必须带设备风控头** `Cosy-MachineToken` / `Cosy-MachineCode` / `Cosy-MachineType`
   （本机 `~/.qoder-cn/umid-cache.json` 里就有），否则活动根本不下发。
2. **活动周期按「10:00 刷新」算，不是自然日**。活动行的 `startAt`/`endAt` 是跨天窗口
   （如 10-22 10:00 → 10-23 09:59）。判断「今天能不能签」必须用**今天 10:00 起的那个窗口**；
   直接拿「有没有 CLAIMED 行」会把上一个周期的已领记录当成今天已签，10:00 前一直显示「今日已签」。
3. 活动列表里混着别的 `CLAIM_BENEFIT` 活动（比如「发布站点领奶茶兑换卡」，
   `benefit.kind == REDEMPTION_CODE`）。代码只认 `benefit.kind == CREDITS` 的那条，否则会领错东西。

**客户端登录态（换号用）**：Electron safeStorage 加密，存于
`%APPDATA%\com.qodercn.app.stable\auth.v1.dat`。格式（实测）：`"v10" || nonce(12B) || AES-256-GCM 密文 || tag(16B)`，
AES 密钥（32 字节）在旁边 `Local State` 的 `os_crypt.encrypted_key` 里，由 DPAPI 保护。
同一 Windows 用户可以解出密钥，也就能重新加密写回 —— 这就是 Qoder 换号的实现
（`core/os_crypt.rs`），加解密 roundtrip 已在 `--selftest` 里常驻验证。

### 妙手（美团 CatPaw）

```
登录态  ~/.meituan-catpaw/auth.json          ← 明文 JSON
积分    GET  catx.nocode.cn/api/gateway/credit/balance
订阅    GET  catx.nocode.cn/api/gateway/credit/subscription/current
注册    POST catx.nocode.cn/api/gateway/credit/register   ← 只对新用户生效
```

- 鉴权要**同时**带 `X-Auth-Token` 和 `access-token`（同值），少一个就 `4011 Authentication failed`。
- **妙手没有每日签到**：`credit/register` 是「向积分中心注册并发放注册奖励」，只对新用户生效。
  积分来自套餐。所以这个适配器只做「账号接入 + 积分展示」。

### 凭据保护

Windows DPAPI（`CryptProtectData`）加密后 base64 存进 `accounts.json`，换用户/换机器读不到；
明文只存在于内存。日志和界面里不出现任何 token。

### 调度模型

借鉴 Cockpit Tools 的「时间窗 + 每账号随机分钟偏移」，并补上三点：

- **配置按 agent 各自独立**（每个 agent 自己管开关和时间窗），没有全局时间窗 ——
  这样 Qoder 这种 10:00 才刷新活动的平台可以单独配窗口。
- **默认时间窗按平台的活动刷新时刻来配**：Qoder 默认 10:00–11:00，其余默认 06:00–08:00。
  窗口配在刷新之前的话，到点只会拿到「未开始」，然后被标记成「今天已处理」，一整天不再重试。
- **时间窗内补跑** + **时间窗变化时重排**：计划分钟已过但今天还没处理过就现在补上；
  改了时间窗导致旧计划落在窗外，也会重排并清掉「已处理」标记（否则新窗口永远等不到它）。

### 自动同步

借鉴 Cockpit Tools 的 `*_auto_refresh_minutes`（默认 10 分钟）：

- 每个 agent 一个开关 + 一个间隔（设置页里配）；
- 到点刷新积分（落快照给趋势图用）、签到状态、账号资料（把
  `Qoder 01a1348c-3fc` 这种 id 前缀换成真实昵称）；
- **跨天强制同步**，不看间隔；
- 同步跑完给前端发 `accounts-synced` 事件，界面立刻跟着刷新。

## 数据目录

```
%APPDATA%\AgentCheckinTools\
  ├── config.json              应用配置（每个 agent 的调度/间隔、当前账号、预警阈值）
  ├── accounts.json            账号池（凭据 DPAPI 加密后 base64）
  ├── scheduler_state.json     当日调度计划（每账号随机分钟偏移 + 今日已处理标记）
  ├── auth_pending.json        未完成的设备授权会话（重启后恢复轮询）
  ├── quota_history.jsonl      积分快照（趋势图数据源，最多留 4000 条）
  └── logs\
      └── checkin-2026-10.jsonl  签到流水，按月分文件
```

> ⚠️ 目录名仍是 `AgentCheckinTools`（旧的内部名），**不是** `CheckinBox`。
> 这是改名时**故意保留**的：换名字会让已有账号池读不到，需要重新导入。
> 想改成 `CheckinBox` 得写一次迁移（见 `core/paths.rs::data_dir`）。

## 目录

```
src/                        React 前端
  App.tsx                   全局状态 + 命令调用 + 轮询
  components/               Sidebar / AgentGroupPanel / AccountCard / QuotaCard
                            TrendLine / LogPanel / SettingsPanel
  lib/api.ts                invoke 封装 + 事件订阅
  lib/agentIcons.ts         各平台官方图标（assets/agents/*.png）
  styles.css                全部样式（单文件，按区块分节注释）
src-tauri/src/
├── core/
│   ├── provider.rs         Provider trait + Account/Quota/CheckinStatus 等领域模型
│   ├── store.rs            账号池持久化（DPAPI 加密凭据）
│   ├── dpapi.rs            CryptProtectData / CryptUnprotectData 绑定
│   ├── os_crypt.rs         Electron safeStorage（OSCrypt v10）加解密
│   ├── config.rs           应用配置（按 agent 的调度/间隔/阈值）
│   ├── scheduler.rs        时间窗 + 确定性随机分钟偏移 + 跨天重排
│   ├── logging.rs          签到流水（按月 JSONL）
│   ├── quota_history.rs    积分快照
│   ├── paths.rs            数据目录布局 + 原子写
│   ├── launcher.rs         客户端 exe 动态定位（注册表+目录扫描）+ 进程检测 + 启动
│   ├── client_switch.rs    改写客户端登录态实现换号（三个平台全支持）
│   └── tz.rs               Windows 时区名 → IANA 名
├── providers/              minimax.rs / qoder.rs / miaoshou.rs（+ mod.rs 注册表）
├── commands.rs             Tauri 命令层 + 调度器后台循环
├── tray.rs                 系统托盘
├── selftest.rs             --selftest 自检
└── lib.rs / main.rs        入口 + 命令行分发
docs/
├── PROJECT-STATUS.md       项目现状与交接（先读这个）
└── superpowers/specs/      最初的设计规格
```

## 免责声明

调用的是各客户端的**非公开内部接口**，归各厂商所有，可能随时变更或下线。
本项目与各厂商无任何隶属、授权或合作关系，仅供个人学习自用，禁止商业用途。
使用前请自行评估并遵守相关服务条款与法律法规，风险由使用者承担。
