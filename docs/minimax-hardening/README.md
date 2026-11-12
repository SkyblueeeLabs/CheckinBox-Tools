# MiniMax Code 稳定性加固

> 创建于 2026-11-12。目标：把 MiniMax Code 这条链路上的**已知致命路径**逐条堵死，
> 换号不丢凭据、刷新不抢客户端的 RT、失败可诊断、数据不串号。
>
> 背景：2026-11-11 在上游 cockpit-tools 上实测复现「两个账号存着，其中客户端登录的那个掉线」，
> 定位到上游**缺少客户端运行态感知**——刷新时先抄客户端 `auth.json` 里的 refresh token 再用它刷新，
> RT 一轮换，客户端内存里那份就作废，客户端下次续期 `invalid_grant` → `delete_credentials` → 掉线。
> 本地 CheckinBox 早就有对应防护（`ClientLoginState` / `allow_self_refresh`），但**仍有一个残余窗口**
> （见 04），另有三处与上游对齐/自洽性上的欠账。

## 本目录包含什么

| 文档 | 编号 | 主题 | 状态 | 风险等级 |
|---|---|---|---|---|
| [04-client-running-guard.md](04-client-running-guard.md) | A | 客户端运行态守卫：进程枚举失败时不得自刷新 | **已执行 ✅** | 高 |
| [01-backup-generations.md](01-backup-generations.md) | B | 换号备份改成多代可回滚 | **已执行 ✅** | 中 |
| [02-import-auth-file.md](02-import-auth-file.md) | C | 设备授权导入时记录客户端登录态路径 | **已执行 ✅** | 中 |
| [03-quota-membership-api.md](03-quota-membership-api.md) | D | 配额数据源对齐上游会员接口 | **暂缓 ⏸**（无订阅账号，不猜测实现） | 低 |

**执行顺序**：A（✅ 已完成）→ B（✅ 已完成）→ C（✅ 已完成）→ D（⏸ 暂缓）。

**回归状态（2026-11-12）**：A / B / C 的人工回归**全部通过**，
判定点明细见 [REGRESSION-CHECKLIST.md](REGRESSION-CHECKLIST.md) §6。
自动化侧：`cargo test --lib` 86 passed / 0 failed / 9 ignored（本轮新增 6 个），
`cargo check --all-targets` 无错误，`npx tsc --noEmit` 通过，
release exe 已重新构建（`src-tauri\target\release\CheckinBox.exe`）。

D 暂缓的原因：手上没有订阅了会员套餐的账号，接口返回结构未经实测；
按现有实现打补丁，不做未经实测的接口替换。等有可用订阅账号时再按 03 文档实测并决定。

## 现状总览

| 能力 | 本地 CheckinBox | 上游 cockpit-tools（当前 main） | 备注 |
|---|---|---|---|
| 账号取号 | 设备授权 / 本机导入 / 调度器捕获回写 | 设备授权 / WebView / 粘贴 token / JSON 导入 / 本机导入 | 上游多两条入口 |
| 凭据存储 | 账号池 JSON（`core/store.rs`） | 索引明文 + 详情加密（`secure_account_storage`） | 各有取舍 |
| 身份判定 | **token 实测**（调接口拿 uid） | 已加 uid 比对（main 新增） | 口径已一致 |
| 刷新时机 | 只在临期（剩余 <5min）或 401 时 | 只在临期（剩余 ≤60s） | 阈值不同，本地更保守 |
| 刷新并发 | 按账号加锁 + 重读池 + 先落盘再解锁 | per-account 锁（只锁刷新本身） | 本地更完整 |
| 401 抢救 | 强制刷新 → 落盘 → 新 token 立即重试 | 401 → 刷新 → 重试，但**只在快过期时才真刷** | 上游仍有缺口 |
| 客户端运行态感知 | ✅ 身份三态（Same / Other / Unknown）＋ 进程三态（Running / Absent / Unknown） | ❌ **完全没有** | 上游致命点 |
| 写客户端前保活 | ✅ 刷新不了就中止换号 | ❌ 直接写库里那份 | 本地更安全 |
| 换号备份 | ✅ 多代 + 成组 + 可回滚（命令行可回滚） | 只保留第一次 | 上游仍是 R7 |
| 多开隔离 | 不做（明确列为待办） | 每实例独立 `MINIMAX_DATA_DIR` | 方向不同 |
| 配额数据源 | `credit/details`（分页流水） | 后端已换会员接口，**前端没跟上** | 见 D |

## 验收标准

「非常稳定」定义成可判定的四条，达成即收手，不做无限打磨：

1. **零掉线**：非用户主动切号导致的客户端掉线，连续 30 天为 0。
2. **零串号**：账号池里的凭据与客户端登录态永不错绑（2026-10-25「两个池条目被污染成同一会话」不能再出现）。
3. **失败可定位**：任何失败在日志里都能定位到「账号 / 阶段 / 原因」，且不刷屏（同账号 10 分钟只落一条）。
4. **数据不撒谎**：拿不到就报错，不显示错数据（`current_token` 的既有原则）。

## 与上游的关系

本目录四项里，**A 是上游至今没有的一层能力**（上游 main 的 5 个 minimax 模块里 grep
`is_running|client_running|process_running|is_client|single_session` **零命中**），
它不是 bugfix 而是补能力，配套证据是本次两个账号掉线的真实复现 —— 这是给上游提 PR 时最硬的弹药。

B/C/D 里：

- **B** 上游同款问题（`.cockpit.bak` 只留首次），可以提，但优先级低于 A。
- **C** 上游同类实现（`account.auth_file`），但上游是导入时自然带上的；我们缺失属自洽性欠账，先补自己。
- **D** **暂缓，不接入**（2026-11-12 决定）：手上没有订阅了会员套餐的账号，接口结构未经实测，
  不做猜测实现。将来有订阅账号时再实测再定。另外上游只改了后端、前端仍按 `credit/details` 解析，
  口径已经不一致（详见 D 文档「上游现状核查」），所以**照抄上游也不对**。

## 回归测试

- [REGRESSION-CHECKLIST.md](REGRESSION-CHECKLIST.md) —— A / B / C 的人工回归清单（每条都有预期结果与判定点）

## 相关背景文档

- [`../MONITOR-FINDINGS-minimax.md`](../MONITOR-FINDINGS-minimax.md) —— 2026-10-27 RT 吊销与刷屏回归的实战记录
- [`../PROJECT-STATUS.md`](../PROJECT-STATUS.md) —— 项目总状态（换号、凭据、调度相关章节）
