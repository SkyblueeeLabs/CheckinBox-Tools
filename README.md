# 聚合签到（Agent Checkin Tools）

> 把散落在各个 AI 编程工具里的「每日领取积分」，收进一个窗口。

![stack](https://img.shields.io/badge/Tauri_2-6366f1) ![rust](https://img.shields.io/badge/Rust-stable-orange) ![react](https://img.shields.io/badge/React-18-61dafb)

## 它解决什么

| 平台 | 每日奖励 | Cockpit Tools 是否支持 | 本项目 |
|---|---|---|---|
| MiniMax Code | 400 分，连签第 4/7 天加码 1000 | ❌ | ✅ |
| Qoder CN | 100 Credits，30 天有效 | ❌ | ✅ |
| WorkBuddy / CodeBuddy CN | 100 分，第 7 天 1000 | ✅ | 不重复造 |
| Trae | 100 通用积分 | ✅ | 不重复造 |

> 上游接口随时可能变。本工具失败时**如实报错**，不会谎报「已签到」。

## 界面

参考 Neo Financial（Mobbin）的浅色 iOS 金融风：**浅灰底 + 白色大圆角卡片 + 极淡阴影 +
近黑主按钮 + 等宽数字**。刻意避开「AI 生成感」：不用渐变、不用发光、不用 emoji 图标。

- 左侧每个 agent 一行：**官方图标**（从本机客户端里取的原始图标）、名称、账号数、待签/已签角标、剩余积分
- 右侧是该 agent 的面板：积分汇总（剩余 / 累计发放 / 今日已签）+ 账号列表
- 账号是**分区卡里的行**（行间发丝线分隔），不是套娃卡片；可签的账号带左侧色条提示
- 自动签到配置属于每个 agent 自己（开关 + 时间窗），嵌在面板顶部
- 账号行内：剩余积分大字 + 用量条 + `已用 / 发放`（标签和值同一行，右侧对齐）+ 趋势迷你图 + 批次明细 + 签到状态
- 底部：签到记录，**只显示当前选中 agent 的记录**（切到「全部」才合并）
- 左侧底部「设置」页：自动同步开关与间隔、调度总览、数据目录、说明

## 快速开始

### 开发模式

```bash
npm install                              # 前端依赖
cd src-tauri && cargo build              # Rust 后端
npm run tauri:dev                        # 开发模式运行
```

> 端口用 5173：本机 1420/1421/1425/1500 落在 Windows 保留端口段（vite 会报 EACCES）。

### 打包独立 exe（免装环境，4 MB）

```bash
npm run tauri build -- --no-bundle
```

产物：`src-tauri/target/release/agent-checkin-tools.exe`。

> `--no-bundle` 是跳过安装包打包（跳过 NSIS / WiX）。**注意**：直接
> `cargo build --release` 不会走 Tauri 的前端嵌入流程，产物会去加载 `devUrl`
> （localhost）而白屏 —— 必须用 `tauri build`。
>
> 中文产品名会让 WiX 报 `LGHT0311`（代码页 1252 装不下中文），所以默认跳过打包；
> 需要安装包时先把产品名改成纯 ASCII 再去掉 `--no-bundle`。

命令行自检（不启 GUI）：

```bash
.\src-tauri\target\release\agent-checkin-tools.exe --selftest
```

## 用法

1. **MiniMax Code**：点「从本机客户端导入」——读取
   `~/.minimax/auth/<env>/<region>/<clientId>/auth.json` 的登录态（明文）。
   客户端会自动续期 token，本工具每次请求前重读该文件，所以**不用手动刷新**。
2. **Qoder CN**：点「添加账号」——浏览器里用目标账号确认一次授权（设备授权，无验证码）。
3. **一键全部签到**：遍历所有账号，先查状态，需要才领取。
4. **自动签到**：每个 agent 自己的开关 + 时间窗。窗内每个账号有一个确定性随机
   分钟偏移，避免多账号同时打同一个上游；**时间窗内还能补跑**，晚启动不漏签。
5. **自动同步**（设置页）：按 agent 各自的间隔刷新积分 / 签到状态 / 账号资料。
   **跨天时不看间隔，强制同步一次** —— 新的一天开始后，「今日已签 / 今日可签」
   会立刻变成当天的真实状态，而不是停在昨天。

## 实现要点

### MiniMax Code

```
读  ~/.minimax/auth/<env>/<region>/<clientId>/auth.json   ← 明文，客户端自动续期
签到 GET/POST  agent.minimax.cn/minimax-cloud/api/v1/signin/{status,claim}
积分 GET       agent.minimax.cn/minimax-cloud/api/v1/credit/details
```

两个签到接口都**必须带 `?timezone_id=Asia/Shanghai`**，否则报 `1406010011 invalid timezone_id`。
token 生命周期很短（实测约 1 小时），因此每次请求前重读本机 `auth.json`。

### Qoder CN

```
设备授权（无验证码）
  GET qoder.com.cn/device/selectAccounts?challenge=S256&machine_id&nonce
  轮询 openapi.qoder.com.cn/api/v1/deviceToken/poll
签到 GET  openapi.qoder.com.cn/sash/api/v1/me/campaigns?forceRefresh=true
     POST openapi.qoder.com.cn/sash/api/v1/me/campaigns/{campaignId}/claim
积分 GET  openapi.qoder.com.cn/sash/api/v2/me/usage
昵称 GET  openapi.qoder.com.cn/api/v1/userinfo          ← 拿真实昵称，不是 id 前缀
头必须带 Cosy-MachineToken/Code/Type（本机 umid-cache.json 已有）
```

**活动周期按「10:00 刷新」算，不是自然日**。活动行的 `startAt`/`endAt` 是一个跨天
窗口（如 10-22 10:00 → 10-23 09:59）。判断「今天能不能签」用的是今天 10:00 起的那个
窗口 —— 直接拿「有没有 CLAIMED 行」会把**上一个周期**的已领记录当成今天已签，
10:00 前一直显示「今日已签」。

活动列表里还混着别的 `CLAIM_BENEFIT` 活动（比如「发布站点领奶茶兑换卡」，
`benefit.kind == REDEMPTION_CODE`），代码只认 `benefit.kind == CREDITS` 的那条，
否则会领错东西。

### 凭据保护

Windows DPAPI（`CryptProtectData`）加密后落盘，换用户/换机器读不到；
日志和界面里不出现任何 token。

### 调度模型

借鉴 Cockpit Tools 的「时间窗 + 每账号随机分钟偏移」，并补上两点：

- **默认时间窗按平台各自的活动刷新时刻来配**：Qoder 默认 10:00–11:00，
  其余默认 06:00–08:00。窗口配在刷新之前的话，到点只会拿到「未开始」，
  然后被标记成「今天已处理」，一整天都不再重试。
- **时间窗内补跑**：计划分钟已过但今天还没处理过，就现在补上，晚启动不漏签。

### 自动同步

借鉴 Cockpit Tools 的 `*_auto_refresh_minutes`（默认 10 分钟）：

- 每个 agent 一个开关 + 一个间隔（设置页里配）；
- 到点刷新积分（落快照给趋势图用）、签到状态、账号资料（把
  `Qoder 01a1348c-3fc` 这种 id 前缀换成真实昵称）；
- **跨天强制同步**，不看间隔；
- 同步跑完给前端发 `accounts-synced` 事件，界面立刻跟着刷新。

## 目录

```
src/                    React 前端
  components/           Sidebar / AgentGroupPanel / AccountCard / QuotaCard
                        TrendLine / LogPanel / SettingsPanel
src-tauri/src/
├── core/               provider 抽象 / 账号池 / 调度 / 日志 / DPAPI / 时区
├── providers/          minimax.rs、qoder.rs、miaoshou.rs
├── commands.rs         Tauri 命令层
├── tray.rs             系统托盘
└── selftest.rs         --selftest 自检
docs/superpowers/specs/ 设计规格
```

## 免责声明

调用的是各客户端的**非公开内部接口**，归各厂商所有，可能随时变更或下线。
本项目与各厂商无任何隶属、授权或合作关系，仅供个人学习自用，禁止商业用途。
使用前请自行评估并遵守相关服务条款与法律法规，风险由使用者承担。

