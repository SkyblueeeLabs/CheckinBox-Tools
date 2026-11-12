# A｜客户端运行态守卫：进程枚举失败时不得自刷新

> 编号 A · 状态：**已执行（2026-11-12）** · 风险等级：**高** · 归属：`docs/minimax-hardening/`

## 一句话结论

我们是**唯一**知道「客户端在跑、当前登录的是谁」的那一方，但还有一个残余窗口：
**进程枚举失败时会被当成「客户端没在跑」**，从而在身份探测也失败的情况下，
把客户端正在用的 refresh token 轮换掉 → 客户端下次续期 `invalid_grant` → **掉线**。
把「确认没在跑」和「不知道」分开，未知一律不刷。

## 1. 背景：上游掉线复现（2026-11-11）

上游 cockpit-tools 存的 MiniMax Code 账号（两个），客户端登录了其中一个，
另一个账号掉线。定位到的上游根因：

```
上游刷新时先抄客户端 auth.json 里的 refreshToken（minimax_code_account.rs:785-828）
  → 用这个 RT 刷新（:831-834）→ RT 轮换
  → 客户端内存里那份 RT 作废 → 客户端下次续期 invalid_grant
  → delete_credentials → 客户端退回登录页 = 掉线
```

上游 main 的 5 个 minimax 模块里 grep `is_running|client_running|process_running|is_client|single_session`
**零命中** —— 上游根本没有「客户端运行态」这一层。

我们本地早就有这一层（`providers/minimax.rs:296-326` 的 `ClientLoginState` 三态 +
`allow_self_refresh`），但**还剩最后一个窗口**，就是本文要补的。

## 2. 现状与残余风险

### 2.1 现有三态判定

`providers/minimax.rs:292-326`：

```rust
pub enum ClientLoginState {
    Same(Value),   // 客户端当前登录的就是这个账号（带客户端最新凭据）
    Other,         // 客户端登录的是别的账号 → 可以安全刷新本账号
    Unknown,       // 探测失败 / 客户端没登录
}

impl ClientLoginState {
    pub fn allow_self_refresh(&self, client_running: bool) -> bool {
        match self {
            Self::Same(_) => false,
            Self::Other => true,
            Self::Unknown => !client_running,   // ← 依赖这个布尔值
        }
    }
}
```

### 2.2 残余窗口就出在那个布尔值上

`core/launcher.rs:358-374`：

```rust
pub fn is_running(provider: &str) -> bool {
    let Some(spec) = spec_of(provider) else { return false; };
    let targets: Vec<String> = spec.exe_names.iter().map(|n| n.to_lowercase()).collect();
    for name in winproc::process_names() {
        if targets.iter().any(|t| name == *t) { return true; }
    }
    false
}
```

而 `core/launcher.rs:506-532` 的进程枚举：

```rust
/// 全部进程名（小写）。枚举失败返回空（宁可放行也不误报客户端在运行）。
pub fn process_names() -> Vec<String> {
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE || snap.is_null() {
            return Vec::new();          // ← 枚举失败 == 空列表 == 「客户端没在跑」
        }
        ...
```

**「枚举失败」和「客户端确实没运行」被压成了同一个 `false`。**

于是这条路径成立：

```
进程枚举失败（快照拿不到 / 权限不足 / 被安全软件拦）
  → is_running() = false
  → 同时身份探测也失败（客户端 token 已过期，probe_identity 调不通）
  → ClientLoginState::Unknown + client_running=false
  → allow_self_refresh() = true → 允许自刷新
  → 轮换掉客户端正在使用的 RT
  → 客户端下次续期 invalid_grant → 掉线
```

> 注：非 Windows 平台更彻底 —— `launcher.rs:535-540` 的 `process_names()` 恒返回空，
> 也就是 `is_running()` 恒为 false，这层守卫在非 Windows 上等于没有。

### 2.3 两个调用点

| 位置 | 场景 | 现有写法 |
|---|---|---|
| `providers/minimax.rs:396-400` | 401 抢救路径 | `let client_running = launcher::is_running("minimax"); if login_state.allow_self_refresh(client_running) { … 强制刷新 … }` |
| `commands.rs:1868-1873` | 主动保鲜（`ensure_fresh_cred`） | `let client_running = launcher::is_running("minimax"); if !login_state.allow_self_refresh(client_running) { return latest; }` |

## 3. 目标行为

把「运行态」从二值改成三值，**未知一律按最保守处理**：

| 进程状态 | `Same`（客户端登录的就是它） | `Other`（客户端登录的是别人） | `Unknown`（探测不出） |
|---|---|---|---|
| `Running`（确认在跑） | ❌ 不刷新 | ✅ 可刷新 | ❌ **不刷新** |
| `Absent`（确认没跑） | ❌ 不刷新 | ✅ 可刷新 | ✅ 可刷新 |
| `Unknown`（枚举失败 / 平台不支持） | ❌ 不刷新 | ✅ 可刷新 | ❌ **不刷新** |

关键变化只有一格：**`Unknown` + 枚举失败 → 不刷新**（现在会刷新）。

并且**不刷新时要把原因说出来**，不能静默：

- 界面/日志提示：「客户端状态未知，未刷新该账号凭据（避免顶掉客户端登录），请在客户端关闭后重试」。

## 4. 改动清单

### 4.1 `core/launcher.rs`：进程枚举要能表达失败

```rust
// winproc 模块
/// 全部进程名（小写）。枚举失败返回 Err —— 调用方需要区分
/// 「枚举失败」和「确实没有客户端进程」，两者对刷新策略的含义相反。
pub fn process_names_result() -> Result<Vec<String>, String> { … }
```

- Windows：`CreateToolhelp32Snapshot` 失败 → `Err("进程快照失败")`；`Process32FirstW` 失败 → 同样 `Err`。
- 非 Windows：直接 `Err("当前平台不支持进程枚举")`（= 未知，不是「没在跑」）。

### 4.2 新增三态类型与探测函数

```rust
/// 客户端进程的三种状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientProcessState {
    /// 确认客户端进程存在
    Running,
    /// 确认没有客户端进程
    Absent,
    /// 枚举失败 / 平台不支持 —— 既不能确认在跑，也不能确认没跑
    Unknown,
}

pub fn client_process_state(provider: &str) -> ClientProcessState {
    let Some(spec) = spec_of(provider) else { return ClientProcessState::Unknown; };
    let targets: Vec<String> = spec.exe_names.iter().map(|n| n.to_lowercase()).collect();
    match winproc::process_names_result() {
        Ok(names) => {
            if names.iter().any(|n| targets.iter().any(|t| n == t)) {
                ClientProcessState::Running
            } else {
                ClientProcessState::Absent
            }
        }
        Err(_) => ClientProcessState::Unknown,
    }
}

/// 兼容旧调用点的布尔包装（Unknown/Absent 都当作「没在跑」）。
/// ⚠️ **只用于「要不要关客户端」这类无害判断**；
/// 涉及刷新/改凭据的地方必须用 `client_process_state`。
pub fn is_running(provider: &str) -> bool {
    client_process_state(provider) == ClientProcessState::Running
}
```

### 4.3 `providers/minimax.rs`：许可规则改为三态

```rust
impl ClientLoginState {
    /// 是否允许我们自己刷新这个账号的 RT。
    ///
    /// 只有「确认客户端登录的是别的账号」，或「确认客户端没在运行」才允许。
    /// 客户端状态未知（进程枚举失败 / 平台不支持）时一律不允许 ——
    /// 宁可不刷新，也不能轮换掉客户端手里的 RT（那会让客户端下次续期
    /// 直接 invalid_grant 掉登录，2026-11-11 上游复现过）。
    pub fn allow_self_refresh(&self, process: ClientProcessState) -> bool {
        match self {
            Self::Same(_) => false,
            Self::Other => true,
            Self::Unknown => process == ClientProcessState::Absent,
        }
    }
}
```

### 4.4 两个调用点跟着改

`providers/minimax.rs:396-400`：

```rust
let process = crate::core::launcher::client_process_state("minimax");
if login_state.allow_self_refresh(process) {
    … 强制刷新 …
}
// 不允许刷新时，给出可执行的原因
```

`commands.rs:1868-1873` 同理。

> **注意**：`Unknown` 且不允许刷新时，`ensure_fresh_cred` 返回 `latest`（旧凭据），
> 后续请求会 401 → 由 401 路径给出错误文案。错误文案里要区分这两种 401：
> - 「客户端状态未知，未刷新以免顶掉客户端登录」；
> - 「自动刷新也没能恢复 —— refresh token 可能已被吊销」。

### 4.5 可选加固：客户端活跃度兜底信号

进程枚举不可靠时，可以用**客户端登录态文件的活跃度**做第二判据：

- 读客户端 `auth.json` 的 `expiresAtMs`：若在未来（说明刚续期过）且文件 mtime 在最近几分钟内
  → 判定为 `Running`（客户端刚活动过）。

这条只用于**把 `Unknown` 升级成 `Running`**（更保守），不用于把 `Unknown` 降级成 `Absent`。

## 5. 测试计划

| 类型 | 用例 | 断言 |
|---|---|---|
| 单元（真值表） | `allow_self_refresh` × {Same, Other, Unknown} × {Running, Absent, Unknown} | 与 §3 的表**逐格一致** |
| 单元 | `client_process_state` 在枚举失败时 | 返回 `Unknown`（不是 `Absent`） |
| 单元 | 非 Windows 平台 | 返回 `Unknown` |
| 单元 | 枚举失败 + 身份探测失败 | **不触发**自刷新；错误文案提示「客户端状态未知」 |
| 单元 | 枚举失败 + 身份是 `Other` | 仍允许刷新（客户端明确登录的是别人） |
| 回归 | 客户端在跑且 token 过期 | 账号池里的凭据不被改动，客户端不掉线 |
| 回归 | 客户端没跑、账号临期 | 正常刷新并落盘 |

> 测试可注入枚举失败：把 `winproc::process_names_result` 换成可注入的版本
> （或抽一个 `process_names_fn: fn() -> Result<Vec<String>, String>` 的测试钩子）。

## 6. 验收标准

- 进程枚举失败时，**任何路径都不会自刷新**（日志里能看到「客户端状态未知」）；
- 客户端在跑且身份探测失败时，客户端不掉线；
- 客户端没跑时，账号照常保鲜（功能不退化）；
- `cargo test --lib` 全绿。

## 7. 风险与回滚

| 风险 | 处置 |
|---|---|
| 枚举失败时账号不再自动保鲜，可能显得"不活跃" | 这是**有意的保守取舍**：账号在客户端关闭后会重新变得可刷新（`Absent`）；若长期枚举失败，需要在日志里提示用户 |
| 影响其它平台的 `is_running` 调用点 | `is_running` 保留布尔语义（`Unknown`→false），行为与现在一致，**不改**其它平台 |
| 非 Windows 平台上守卫形同虚设 | 改成 `Unknown` 后，非 Windows 变成「永不自动刷新」→ 需要在界面提示；本项目当前主要面向 Windows（`launcher.rs` 的实现是 Windows 专有），但要留好接口 |
| 回滚 | 改动集中在 3 个文件，回滚成本低；`allow_self_refresh` 的签名变化是编译期可见的 |

## 8. 与上游的关系 —— 这是提 PR 的头号素材

上游**完全没有这一层**。我们做完之后可以直接提 **PR-A**：

> **feat(minimaxcode): 刷新前感知客户端运行态，避免轮换客户端持有的 refresh token**

补丁范围（上游侧）：
1. 在 `minimax_code_account.rs` 的 `refresh_account_token_once` 里，
   刷新前判断「客户端当前登录的是不是这个账号」：
   - 是 → 只回填客户端最新凭据，**不刷新**；
   - 不是但客户端在跑 → 刷新**别的**账号可以，但这个账号要看情况；
   - 身份探测不出来且客户端在跑 → **不刷新**。
2. 身份判定必须用 **token 实测**（上游已有 `fetch_user_info`，直接复用），
   不要用客户端配置里的 `sharedUser`（外部切换后是陈旧的，会把 A 的 token 挂到 B 上）。
3. 补一条「客户端进程状态探测」，至少区分「确认没在跑」与「不知道」。

证据：我们这边有 2026-11-02（刷错账号把客户端顶下线）与 2026-11-11（上游两个账号掉线）
两次实测记录，可以直接写进 PR 描述。

## 9. 实施记录（2026-11-12）

已按本方案落地，改动 3 个文件，`cargo test --lib` **82 passed / 0 failed**（新增 2 个用例）。

### 9.1 `core/launcher.rs`

| 改动 | 说明 |
|---|---|
| 新增 `ClientProcessState` 三态枚举 | `Running` / `Absent` / `Unknown` |
| 新增 `client_process_state(provider)` | 统一入口，返回三态 |
| 新增 `process_state_from_names(provider, names)` | **纯函数**：把「枚举结果 → 三态」的判定抽出来，便于单测注入失败 |
| `is_running(provider)` 改为包装 | 保留布尔语义（`Unknown` → `false`），其它调用点行为不变 |
| `winproc::process_names()` → `process_names_result()` | 枚举失败改为返回 `Err`；非 Windows 一律 `Err`（= 不知道，不是没在跑） |

### 9.2 `providers/minimax.rs`

- `ClientLoginState::allow_self_refresh(&self, process: ClientProcessState)`：
  `Unknown` 身份**只在 `process == Absent`（确认没在跑）时**才允许自刷新；
  进程状态 `Unknown`（枚举失败）一律不刷。
- 401 抢救路径改用三态；不能自己刷时把**原因**记下来，最终错误信息形如：
  `HTTP 401：客户端当前登录的就是这个账号，刷新会轮换掉它手里的 refresh token（会导致客户端掉线），所以没有自动刷新；请在客户端里重新登录该账号，或退出客户端后重试`
- 「客户端未在运行」仍走原来的快速失败（不空等 17 秒）。

### 9.3 `commands.rs`

- `ensure_fresh_cred` 改用 `client_process_state("minimax")`；
  进程状态未知时**不刷新**，返回池里的旧凭据（由后续请求如实报 401）。

### 9.4 新增单测

| 用例 | 覆盖 |
|---|---|
| `core::launcher::tests::进程状态三态_枚举失败算未知而不是没在跑` | 枚举失败 → `Unknown`；枚举成功无客户端 → `Absent`；命中 → `Running`；未知平台 → `Unknown` |
| `providers::minimax::tests::客户端登录态_刷新许可规则` | 身份三态 × 进程三态 = 9 格真值表逐格断言 |

### 9.5 尚未覆盖的部分（留待后续）

- 非 Windows 平台现在一律判 `Unknown` → 不自动刷新（保守），需要在界面提示；
- 「客户端活跃度兜底信号」（§4.5）本次**未实现**，作为可选加固；
- `selftest.rs` / `chat_sync.rs` 仍用布尔 `is_running`（无害判断，行为不变）。
