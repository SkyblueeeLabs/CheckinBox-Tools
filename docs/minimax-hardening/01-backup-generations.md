# B｜换号备份改成多代可回滚

> 编号 B · 状态：**已执行（2026-11-12）** · 风险等级：中 · 归属：`docs/minimax-hardening/`

## 一句话结论

现在的备份**只保留第一次**：第二次换号之后，客户端原来的登录态就再也回滚不了了。
改成「每次换号生成一代、成组可回滚、保底留 N 代」。

## 1. 现状

### 1.1 备份只做一次

`src-tauri/src/core/client_switch.rs:1042-1048`：

```rust
fn backup_file(path: &std::path::Path) -> Result<Option<String>> {
    let bak = path.with_extension("json.bak");
    if bak.exists() {
        return Ok(Some(bak.to_string_lossy().to_string()));   // ← 已存在就直接返回，不更新
    }
    std::fs::copy(path, &bak)?;
    Ok(Some(bak.to_string_lossy().to_string()))
}
```

即 `<file>.json` → `<file>.json.bak`，**只在目标不存在时复制**。
第一次换号后 `.bak` 就固化了，之后无论客户端换到哪个账号、写入什么凭据，
`.bak` 里永远是「第一次换号之前」的那份。

### 1.2 调用点

| 位置 | 平台 | 备份对象 |
|---|---|---|
| `client_switch.rs:474-475` | minimax | `auth.json` + `auth-state.json`（两个独立文件，各自备份） |
| `client_switch.rs:567` | miaoshou | 登录态文件 |
| `client_switch.rs:782-783` | qoder / workbuddy | 登录态文件 |

### 1.3 用户其实看不到备份在哪

- 界面只显示「（原登录态已备份）」这个布尔提示（`src/App.tsx:398`）；
- **备份路径只在命令行打印**（`commands.rs:2444-2446`）；
- 设置页的 `BackupPanel` 列出的是**聊天记录**备份（`chat_history/<平台>/_backups/<操作id>/`），
  与客户端登录态备份**不是一回事**，也没法从这里回滚登录态。

结果：出问题时用户既不知道备份在哪，也没有回滚入口 → 只能重新授权。

### 1.4 上游同款问题

cockpit-tools 合并版 `minimax_code_account.rs:1526-1534` 也是 `.cockpit.bak` **只在不存在时复制**，
所以「备份永远只保留最初那一次」是两边共有缺陷（上游风险清单 R7）。我们修好后可以作为 PR 素材。

### 1.5 一个必须注意的联动点

`client_switch.rs:257-290` 的 `minimax_known_record_keys` 靠扫备份文件找回
**客户端自己真正用过的记录键**（客户端中途清空 `records` 时全靠它）：

```rust
for suffix in ["bak", "cockpit.bak"] {
    files.push(path.with_file_name(format!("auth.json.{suffix}")));
}
```

它只认 `auth.json.bak` 和 `auth.json.cockpit.bak` 两个固定名字。
**改了备份命名而不同步改这里，客户端清空 records 后的换号会再次卡死**（这正是 2026-11-02 踩过的坑）。

## 2. 目标行为

| 场景 | 现在 | 目标 |
|---|---|---|
| 第 1 次换号 | 生成 `.bak` | 生成第 1 代备份（`auth.json` + `auth-state.json` 成组） |
| 第 2 次换号 | `.bak` 不变，**丢掉最新现场** | 生成第 2 代，第 1 代仍在 |
| 第 N 次换号 | 同上 | 生成第 N 代，保留最近 N 代，超出删最旧 |
| 出问题要回滚 | 无入口，只能手工找文件 | 界面可列出备份 → 一键回滚（成组） |
| 客户端清空过 records | 靠 `.bak` / `.cockpit.bak` 找回键 | 同样能找回（新命名也要被扫到） |

## 3. 改动清单

### 3.1 备份落点：应用数据目录，不放客户端目录

**不要**把备份目录建在 `~/.minimax/auth/` 下面 —— 我们的 `collect_auth_files`
（`providers/minimax.rs:59-85`）会递归 4 层、收集**任何名为 `auth.json` 的文件**，
备份目录里若放同名的 `auth.json`，就会被当成候选登录态文件 → 选错文件 → 串号。

沿用聊天备份的既有约定（`chat_sync.rs:624-626`）：

```
<应用数据目录>/
└── client_login_backups/
    └── minimax/
        └── <操作id>/          ← 一次换号 = 一个操作id = 一组文件
            ├── auth.json
            ├── auth-state.json
            └── meta.json       ← 记录来源路径、平台、账号、时间、原始文件名
```

操作 id 沿用 `chat_sync::operation_id()` 的思路（时间戳 + 随机后缀），保证可排序且不撞。

### 3.2 新增 `core/client_backup.rs`

对外接口（先只给后端，UI 下一轮接）：

```rust
pub struct ClientLoginBackup {
    pub provider: String,
    pub op_id: String,
    pub created_at_ms: i64,
    pub source_files: Vec<String>,   // 原始文件路径（回滚时按它写回）
    pub bytes: u64,
    pub reason: String,              // "switch" / "manual"
}

pub fn backup_client_login(provider: &str, files: &[(PathBuf, String)]) -> Result<ClientLoginBackup, String>;
pub fn list_client_login_backups(provider: &str) -> Vec<ClientLoginBackup>;
pub fn restore_client_login_backup(provider: &str, op_id: &str) -> Result<(), String>;
pub fn prune_client_login_backups(provider: &str, keep: usize) -> Result<usize, String>;
```

- `backup_client_login`：**成组**复制，一次换号的 `auth.json` 与 `auth-state.json` 必须同组；
  任一文件缺失也照常备份（记录进 `source_files`），但不报错中断换号。
- `restore_client_login_backup`：**按 `source_files` 原样写回**（文件名保持原样），
  写回前先给当前现场也做一次备份（备份套备份，允许）。
- `prune_client_login_backups`：按 `created_at_ms` 倒序保留最近 `keep` 代（默认 5），其余删除。
  删除只允许发生在 `client_login_backups/<provider>/` 之下（路径校验，防 `..`）。

### 3.3 换号流程接入

`client_switch.rs:474-475` 改成：

```rust
let op = crate::core::client_backup::backup_client_login(
    "minimax",
    &[
        (path.clone(),        "auth.json".to_string()),
        (state_path.clone(),  "auth-state.json".to_string()),
    ],
)?;
```

`SwitchOutcome` 增加一个字段：

```rust
pub struct SwitchOutcome {
    pub backup: Option<String>,          // 保留：兼容现有 UI 布尔提示，改为备份目录路径
    pub backup_op_id: Option<String>,     // 新增：回滚用
    ...
}
```

### 3.4 `minimax_known_record_keys` 改成通配扫描

`client_switch.rs:257-290` 的文件列表从固定两个名字改成：

```rust
// 备份文件按通配找：新命名 `<name>.<ts>.bak`、旧命名 `<name>.bak`、
// 上游 cockpit 写的 `<name>.cockpit.bak` 都要认。
```

注意：只扫 `auth.json*` 前缀且必须是 `.bak` 结尾，避免把别的文件当凭据。

### 3.5 UI（可放第二轮）

设置页「历史备份」旁边加一个「客户端登录态备份」区域：
列出「时间 / 平台 / 来源文件 / 大小」，提供「回滚」与「删除」按钮，
回滚前二次确认并提示「回滚后需要重启客户端」。

## 4. 测试计划

| 类型 | 用例 | 断言 |
|---|---|---|
| 单元 | 连续两次 `backup_client_login` | 生成 2 个不同 op_id 的目录，各含完整文件 |
| 单元 | `prune_client_login_backups(keep=2)` 写入 5 代 | 只剩最新 2 代，删掉 3 代 |
| 单元 | `restore_client_login_backup` | 目标文件内容与备份时逐字节一致（`auth.json` + `auth-state.json` 成组） |
| 单元 | 备份目录在 `client_login_backups/minimax/` 下 | 不会被 `providers::minimax::auth_files()` 扫到（**关键回归**） |
| 单元 | `minimax_known_record_keys` 扫新命名备份 | 能从 `<name>.<ts>.bak` 里找回记录键 |
| 回归 | 手动换号 3 次 | 3 代备份齐全；把第 2 代回滚后，客户端用第 2 代的账号登录成功 |
| 回归 | 客户端清空 records 后换号 | 仍能找回记录键并成功写入（2026-11-02 场景） |

## 5. 验收标准

- 连续 3 次换号后，3 代备份都在，且第 2 代可一键回滚成功；
- 回滚后客户端显示的是回滚那一代的账号；
- 备份目录不会被当成候选登录态文件；
- 客户端清空 records 的场景不再复发。

## 6. 风险与回滚

| 风险 | 处置 |
|---|---|
| 新备份逻辑写坏导致换号失败 | 备份失败**不阻断换号**（与现状一致：换号主流程不依赖备份成功），但要在日志里留痕 |
| 保留代数过多占空间 | 登录态文件很小（几 KB），5 代可忽略；仍做上限裁剪 |
| 与上游 `.cockpit.bak` 命名冲突 | 我们不写 `.cockpit.bak`；读取时两种都认 |
| 改动波及聊天备份 | 两套备份目录完全独立（`client_login_backups` vs `chat_history`），不改 `chat_sync` |

## 7. 与上游的关系

- **上游同款缺陷**（R7），我们修好后可作为 PR：把 `.cockpit.bak` 改成多代 + 提供回滚入口。
- 提 PR 时注意切口要小：**只做「多代 + 成组 + 可回滚」**，不要连带改 UI 大改。

## 8. 实施记录（2026-11-12）

已按本方案落地，`cargo test --lib` **86 passed / 0 failed**（新增 4 个用例），
`cargo check --all-targets` 无错误。

### 8.1 新增模块 `core/client_backup.rs`

| 对外接口 | 说明 |
|---|---|
| `backup_client_login(provider, files, reason)` | 备份一代（一次换号 = 一代），`files` = [(原始路径, 备份内文件名)] |
| `list_client_login_backups(provider)` | 列出全部代（新→旧） |
| `restore_client_login_backup(provider, op_id)` | 回滚到某一代；**回滚前先把当前现场再存一代** |
| `prune_client_login_backups(provider, keep)` | 只保留最近 N 代（默认 5），返回删除数 |
| `delete_client_login_backup(provider, op_id)` | 删除某一代 |
| `stored_login_state_files(provider)` | 供找回客户端真实用过的记录键 |

- 所有 `*_in(root, ...)` 变体可注入根目录 —— **单测不写真实数据目录**；
- 落点：`<应用数据目录>/client_login_backups/<平台>/<操作id>/`；
  刻意**不放客户端目录** —— 客户端目录扫描是「递归找任何名为 auth.json 的文件」，
  备份里放同名文件会被当成候选登录态 → 选错文件 → 串号；
- `op_id` / 平台名按外部输入校验（拒 `..`、分隔符、路径穿越）。

### 8.2 `core/client_switch.rs`

- MiniMax：备份改成**成组**（`auth.json` + `auth-state.json` 同一代）；
- 妙手 / Qoder：同样切到新机制（原来也只有「第一次」的备份）；
- 删除旧的 `backup_file()`（只留第一次的那套），换成 `backup_name_of()`；
- `minimax_known_record_keys` 增加扫描**多代备份里的登录态文件** ——
  否则客户端清空 records 后换号会再次卡死（2026-11-02 的坑）；
- 备份失败**不阻断换号**（与原行为一致），只在 stderr 留一行原因。

### 8.3 命令与命令行

| 入口 | 作用 |
|---|---|
| `list_client_login_backups` | Tauri 命令：列出某平台的备份代 |
| `restore_client_login_backup` | Tauri 命令：回滚到某一代 |
| `delete_client_login_backup` | Tauri 命令：删除某一代 |
| `prune_client_login_backups` | Tauri 命令：只保留最近 N 代 |
| `CheckinBox --list-client-backups <平台>` | 命令行：列出备份（含原始文件路径） |
| `CheckinBox --restore-client-backup <平台> <操作id>` | 命令行：回滚 |

### 8.4 新增单测

| 用例 | 覆盖 |
|---|---|
| `连续备份生成多代且回滚能回到那一代` | 两代备份独立、回滚后内容回到第 1 代 |
| `裁剪只保留最近若干代` | 存 4 代裁到 2 代 |
| `操作id含分隔符会被拒绝` | `..` / 分隔符 / 路径穿越被拒 |
| `备份里的登录态文件能被找出来` | `stored_login_state_files` 能扫到备份里的 auth.json |

### 8.5 尚未覆盖（留待后续）

- 设置页 UI 还没接（目前只能走命令行 / Tauri 命令）；
- 旧的单文件 `.bak` 仍会被扫描（兼容历史备份），但不再新建；
- WorkBuddy 的登录态库备份走的是它自己的 `backup_sqlite`（sqlite 三件套），
  本次**未**并入多代机制（它的备份是「已有就不覆盖」的另一套语义）。
