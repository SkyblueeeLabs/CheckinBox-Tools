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

## 快速开始

```bash
npm install          # 前端依赖
cd src-tauri && cargo build   # Rust 后端
npm run tauri:dev   # 开发模式运行
```

需要 Rust stable（`rustup`）与 VS Build Tools（C++ 工具链）。本项目第一版**不做 exe 打包**，
稳定后再补 `npm run tauri build`。

## 用法

1. **MiniMax Code**：点「从本机客户端导入」——直接读取
   `~/.minimax/auth/<region>/mcode-public/auth.json` 的登录态（明文，无需解密）。
2. **Qoder CN**：点「添加账号」——浏览器里用目标账号确认一次授权（设备授权，无验证码）。
3. **一键全部签到**：遍历所有账号，先查状态，需要才领取。
4. **自动签到**：顶部调度条开关 + 时间窗（默认 06:00–08:00）。窗内每个账号有一个
   确定性随机分钟偏移，避免多账号同时打同一个上游。

## 实现要点

- **MiniMax 签到接口**：`GET/POST https://agent.minimax.cn/minimax-cloud/api/v1/signin/{status,claim}`，
  必须带 `?timezone_id=Asia/Shanghai`（否则报 `1406010011 invalid timezone_id`）。
- **Qoder 签到接口**：`GET /sash/api/v1/me/campaigns` 拉活动 →
  `POST /sash/api/v1/me/campaigns/{campaignId}/claim` 领取；必须带 `Cosy-Machine*` 设备风控头。
  官方活动窗口每天 10:00（UTC+8）开启。
- **凭据保护**：Windows DPAPI（`CryptProtectData`）加密后落盘，换用户/换机器读不到；
  日志和界面里不出现任何 token。
- **调度模型**：借鉴 Cockpit Tools 的「时间窗 + 每账号随机分钟偏移」。

## 目录

```
src/                    React 前端
src-tauri/src/
├── core/               provider 抽象 / 账号池 / 调度 / 日志 / DPAPI / 时区
├── providers/          minimax.rs、qoder.rs
├── commands.rs         Tauri 命令层
└── tray.rs             系统托盘
docs/superpowers/specs/ 设计规格
```

## 免责声明

调用的是各客户端的**非公开内部接口**，归各厂商所有，可能随时变更或下线。
本项目与各厂商无任何隶属、授权或合作关系，仅供个人学习自用，禁止商业用途。
使用前请自行评估并遵守相关服务条款与法律法规，风险由使用者承担。
