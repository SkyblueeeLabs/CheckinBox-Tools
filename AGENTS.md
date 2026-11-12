# CheckinBox —— 代理协作约定

## 常用命令

```bash
npm run tauri:dev                                  # 开发模式
npm run tauri:build                               # release exe（tauri build --no-bundle --features custom-protocol）
npx vitest run                                     # 前端测试
cd src-tauri && cargo test --lib                   # Rust 测试
npx tsc --noEmit                                   # 前端类型检查
```

## 构建 release：先杀进程，不用问用户

`target/release/CheckinBox.exe` 被占用时构建会**在最后一步失败**（`拒绝访问 (os error 5)`），
Rust 已经编译完了、只差替换文件。所以构建前：

```bash
taskkill //IM CheckinBox.exe //F        # Git Bash；PowerShell 用 /IM /F
tasklist //FI "IMAGENAME eq CheckinBox.exe"   # 确认杀干净
```

- **直接杀，不用问用户**（用户明确要求过）；
- 「关窗口」≠「进程结束」——关窗口默认可能只是最小化到托盘，必须按进程名确认；
- 杀掉会中断正在跑的操作（调度签到 / 切换客户端），所以别在用户明显正操作时杀；
- 构建完直接把 exe 拉起来（`explorer.exe <exe 路径>`）。

更完整的构建坑（cargo PATH、custom-protocol 特性、WebView2 数据目录占用等）
见 `README.md` 的「打包独立 exe」小节。
