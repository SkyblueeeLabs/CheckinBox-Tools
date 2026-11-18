# 自动更新（Windows / NSIS）

## 1. 方案

使用 Tauri 官方 updater：

- `tauri-plugin-updater` + `tauri-plugin-process`
- `bundle.createUpdaterArtifacts: true`
- 单一 `latest.json`，由 GitHub Release 提供：
  `https://github.com/SkyblueeeLabs/CheckinBox-Tools/releases/latest/download/latest.json`
- NSIS 安装版自动下载、校验签名、安装并重启。
- 便携版和 MSI 版不自动安装，只提示下载新版。

更新签名与 Windows Authenticode 是两套东西：前者保证更新包没被篡改，后者才影响
SmartScreen 的「未知发布者」。

## 2. 配置

`src-tauri/tauri.conf.json`：

```json
{
  "bundle": {
    "createUpdaterArtifacts": true,
    "windows": {
      "nsis": {
        "installMode": "currentUser",
        "installerHooks": "windows/nsis/installer-hooks.nsh"
      }
    }
  },
  "plugins": {
    "updater": {
      "pubkey": "<minisign 公钥>",
      "endpoints": [
        "https://github.com/SkyblueeeLabs/CheckinBox-Tools/releases/latest/download/latest.json"
      ]
    }
  }
}
```

`src-tauri/windows/nsis/installer-hooks.nsh` 在升级时读取旧卸载项的
`InstallLocation`，复用旧安装目录，避免装出第二份。

## 3. 发布流程

1. 更新 `package.json` 版本。
2. 更新 `CHANGELOG.md` 对应版本段落（缺失会导致 workflow 失败）。
3. `npm run sync-version` 同步 `tauri.conf.json`、`Cargo.toml`、lock 文件。
4. 打 `v<version>` tag 并推送，触发 `.github/workflows/release.yml`。
5. workflow 构建 NSIS/MSI、生成 `.sig` 和 `latest.json`，创建**草稿** Release。
6. 校验草稿资产后手动发布；`releases/latest` 只读取已发布的 Release。
7. 发布后可再发一个 `v0.2.1` 验证真实更新链路。

本地生成安装包 + updater 签名：

```bash
export TAURI_SIGNING_PRIVATE_KEY="<私钥路径或内容>"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="<密码>"
npm run tauri:build:installer
```

GitHub Secrets 需要：

- `TAURI_SIGNING_PRIVATE_KEY`
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`

私钥和密码只保存在本机受控目录 / GitHub Secrets，不进入仓库。丢失后只能换新公钥，
已安装用户必须手动重装一次。

## 4. 应用内行为

- 默认开启自动检查；启动后检查一次，之后每 6 小时检查。
- 发现新版本弹窗显示版本、更新内容和下载进度，支持「立即更新 / 稍后 / 跳过此版本」。
- 下载完成显示「重启」；默认不静默安装，避免打断正在跑的签到。
- 设置 → 软件更新：自动检查开关、当前版本、上次检查时间、手动检查、取消跳过。
- 便携版 / MSI 版：检查到更新后只提供「下载新版」。

## 5. 旧版本迁移

v0.1.0 没有 updater，无法远程推更新策略。需要用户手动安装一次 v0.2.0：

- NSIS 版：直接覆盖安装，复用旧目录；账号/配置在 `%APPDATA%\CheckinBox`，不丢。
- MSI 版：建议手动卸载旧 MSI 后装新版。
- 便携版：下载新版 exe，旧文件可手动删除。

新版首次启动会检测旧安装目录 / 便携形态，显示一次性迁移提示；不会自动卸载或删除文件。

## 6. 已验证 / 待验证

已验证：

- v0.1.0 NSIS → v0.2.0 NSIS 覆盖安装：版本更新到 0.2.0，只保留一个安装目录和卸载项，
  `%APPDATA%\CheckinBox` 数据完整。
- 安装版被识别为 `nsis`，`oldInstalls` 为空，不误报迁移。
- 设置页显示 v0.2.0、自动检查已开启、上次检查时间。
- `latest.json` 生成：`windows-x86_64` / `windows-x86_64-nsis`、签名长度正常。
- 手动检查能发起网络请求并正常结束（线上 v0.1.0 Release 还没有 `latest.json`，
  所以当前失败属预期）。

待验证：

- 发布 v0.2.0（带 latest.json）后，从 v0.2.0 → v0.2.1 的完整下载/安装/重启链路。
- 签名不匹配、断网、404 等异常的用户提示。
- GitHub 在国内网络下的可用性；必要时后续加镜像/手动下载兜底。
