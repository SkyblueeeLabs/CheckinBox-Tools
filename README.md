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

参考 Cockpit Tools 的布局：**左侧 agent 列表 + 右侧账号面板**。

- 左侧每个 agent 一行：强调色小方块、名称、账号数、待签/已签角标、剩余积分
- 右侧顶部是该 agent 的积分汇总（剩余 / 累计发放）
- **自动签到配置属于每个 agent 自己**（开关 + 时间窗），不再有全局时间窗 ——
  这样 Qoder 这种 10:00 才刷新活动的平台可以单独配窗口
- 账号卡：积分（**剩余**大字 + 已用/发放 + 用量条 + 批次明细）、签到状态、操作
- 账号操作：`设为当前` / `打开客户端` / `移除`，并显示本地项目数与会话数
- 底部：签到记录

视觉取向是**仪表盘 / 仪器面板**：平面表面 + 1px 细线边框 + 小圆角 + 等宽数字 +
全大写微型标签，每个 agent 只用一处强调色（左侧细条 + 小圆点）。

## 快速开始

```bash
npm install                              # 前端依赖
cd src-tauri && cargo build              # Rust 后端
npm run tauri:dev                        # 开发模式运行
```

需要 Rust stable（`rustup`）与 VS Build Tools（C++ 工具链）。本项目第一版**不做 exe 打包**，
稳定后再补 `npm run tauri build`。

命令行自检（不启 GUI）：

```bash
.\src-tauri\target\debug\agent-checkin-tools.exe --selftest
```

## 用法

1. **MiniMax Code**：点「从本机客户端导入」——读取
   `~/.minimax/auth/<env>/<region>/<clientId>/auth.json` 的登录态（明文）。
   客户端会自动续期 token，本工具每次请求前重读该文件，所以**不用手动刷新**。
2. **Qoder CN**：点「添加账号」——浏览器里用目标账号确认一次授权（设备授权，无验证码）。
3. **一键全部签到**：遍历所有账号，先查状态，需要才领取。
4. **自动签到**：底部调度条开关 + 时间窗（默认 06:00–08:00）。窗内每个账号有一个
   确定性随机分钟偏移，避免多账号同时打同一个上游。

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
头必须带 Cosy-MachineToken/Code/Type（本机 umid-cache.json 已有）
```

官方活动窗口每天 **10:00（UTC+8）** 开启。

### 凭据保护

Windows DPAPI（`CryptProtectData`）加密后落盘，换用户/换机器读不到；
日志和界面里不出现任何 token。

### 调度模型

借鉴 Cockpit Tools 的「时间窗 + 每账号随机分钟偏移」。

## 目录

```
src/                    React 前端
  components/           Sidebar / AgentGroupPanel / AccountCard / QuotaCard / LogPanel
src-tauri/src/
├── core/               provider 抽象 / 账号池 / 调度 / 日志 / DPAPI / 时区
├── providers/          minimax.rs、qoder.rs
├── commands.rs         Tauri 命令层
├── tray.rs             系统托盘
└── selftest.rs         --selftest 自检
docs/superpowers/specs/ 设计规格
```

## 免责声明

调用的是各客户端的**非公开内部接口**，归各厂商所有，可能随时变更或下线。
本项目与各厂商无任何隶属、授权或合作关系，仅供个人学习自用，禁止商业用途。
使用前请自行评估并遵守相关服务条款与法律法规，风险由使用者承担。

