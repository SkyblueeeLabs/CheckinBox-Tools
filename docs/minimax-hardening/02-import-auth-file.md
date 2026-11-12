# C｜设备授权导入时记录客户端登录态路径

> 编号 C · 状态：**已执行（2026-11-12）** · 风险等级：中 · 归属：`docs/minimax-hardening/`

## 一句话结论

只有**浏览器设备授权**导入的账号没有 `auth_file`（客户端登录态文件路径），
导致换号时只能全盘搜索、可能选中别的环境的 `auth.json`。
补上这个字段即可 —— 只记路径，不记凭据。

## 1. 现状

### 1.1 三种取号路径，只有一种缺 `auth_file`

| 取号路径 | 代码 | 凭据里有没有 `auth_file` |
|---|---|---|
| 本机登录态导入 | `providers/minimax.rs:581-604` → `client_session()` | ✅ 有（`minimax.rs:268` / `:286`） |
| 调度器 / 换号时捕获客户端登录态 | `client_session()` | ✅ 有 |
| **浏览器设备授权** | `providers/minimax.rs:885-963` `poll_device_authorization` | ❌ **没有** |

设备授权产出的凭据只有三个字段（`minimax.rs:957-961`）：

```rust
let cred = json!({
    "access_token": access,
    "refresh_token": refresh,
    "expires_at_ms": ...,
});
```

落盘发生在 `commands.rs:1147` 的 `store.upsert(account.clone(), &cred)`。

### 1.2 缺了它会怎样

换号写凭据时（`core/client_switch.rs:330-336`）：

```rust
let path = cred.get("auth_file").and_then(|v| v.as_str()).map(PathBuf::from)
    .filter(|p| p.is_file())
    .or_else(minimax_auth_file)          // ← 兜底：全盘搜
    .ok_or_else(|| anyhow!("没找到 MiniMax Code 的登录态文件"))?;
```

兜底的 `minimax_auth_file()`（`client_switch.rs:193-199`）是
在 `~/.minimax/auth` 下递归 4 层、**取第一个** `auth.json` —— `read_dir` 顺序未定义。

由此产生三个问题：

1. **可能写错文件**：机器上存在多套命名空间（如 `prod/cn` 与 `prod/en`、或不同 buildEnv）时，
   选中的不一定是目标环境 → 凭据写进另一个命名空间 → 客户端读不到（静默失效）。
2. **找回客户端真实记录键的能力被削弱**：`minimax_known_record_keys`（`client_switch.rs:257-290`）
   需要拿到那个 `auth.json` 路径才能扫备份里的键；路径选错 → 找不到客户端真正用过的键 →
   退化成按公式推导 → 客户端做过路径归一化时静默失效（2026-11-02 踩过的坑）。
3. **「当前登录账号」判定也会取错文件**：`detect_minimax`（`client_switch.rs:293-321`）
   同样用 `minimax_auth_file()` → 界面上「客户端登录」徽标可能挂到错的账号上。

### 1.3 上游的对应做法

上游 cockpit-tools 的账号模型里 `MinimaxCodeAccount.auth_file` 是**一等字段**，
本机导入（`payload_from_local_record`）时写入，设备授权路径则由
`bound_client_auth_files`（main 版 `minimax_code_account.rs`）按实例绑定反推补齐。
上游是「有字段 + 反推」，我们是「设备授权这条路直接缺字段」。

## 2. 目标行为

| 场景 | 现在 | 目标 |
|---|---|---|
| 设备授权导入账号 | 凭据无 `auth_file` | 凭据带 `auth_file`（探测到的客户端登录态路径） |
| 机器上有多个 `auth.json` | 换号时随机挑一个 | 用凭据里记的确定路径 |
| 客户端没装 / 从没登录过 | —— | 探测不到就不写该字段，行为与现在一致（换号会提示「没找到登录态文件」） |
| 探测到的文件属于**别的账号** | —— | **只记路径，绝不取里面的凭据** |

## 3. 改动清单

### 3.1 `providers/minimax.rs`：加一个「只取路径」的探测

现有 `local_auth_with_path()`（`minimax.rs:126-131`）已经能返回 `(PathBuf, Value)`，
但它**读的是第一个** `auth.json`，且顺带把内容也读了出来。新增一个只要路径的版本：

```rust
/// 客户端登录态文件的路径（只取路径，不读内容）。
///
/// 设备授权导入的账号没有登录态文件归属，用它把凭据锚定到正确的命名空间，
/// 避免换号时全盘搜索选中别的环境。
fn local_auth_path() -> Option<PathBuf> {
    auth_files().into_iter().next()
}
```

### 3.2 `poll_device_authorization`：凭据里补上 `auth_file`

在 `minimax.rs:957-961` 构造 `cred` 时追加：

```rust
let cred = json!({
    "access_token": access,
    "refresh_token": refresh,
    "expires_at_ms": ...,
});
// 锚定客户端登录态文件（探测不到就不写，不影响设备授权本身）
if let Some(path) = local_auth_path() {
    cred["auth_file"] = json!(path.to_string_lossy());
}
```

**只记路径。** 文件里的记录属于客户端当前登录的那个账号，与本次设备授权的账号无关，
取它的凭据就是串号（2026-10-25 踩过：「两个池条目被污染成同一个会话」）。

### 3.3 `auth_files()` 的搜索顺序要确定化

`providers/minimax.rs:59-85` 的 `collect` 依赖 `read_dir` 顺序。
建议在 `auth_files()` 返回前排序，优先级：

1. `prod/cn/mcode-public/auth.json`（官方客户端默认布局）
2. 其余按路径字典序

这样即使没写 `auth_file`，兜底路径也是确定的。

### 3.4 老账号的兼容

已经入库的设备授权账号没有 `auth_file`，**不用做数据迁移**：
换号时兜底搜索仍然可用（只是确定性差一点）。若要迁移，可在
`providers/minimax.rs:979-1043` 的 `migrate_device_key_ids` 里顺带补
（该函数本来就在做设备授权账号的键迁移），但优先级低，可不做。

## 4. 测试计划

| 类型 | 用例 | 断言 |
|---|---|---|
| 单元 | 设备授权成功后检查凭据 | 含 `auth_file` 字段，值指向探测到的文件 |
| 单元 | 探测不到登录态文件 | 凭据**不含** `auth_file`，授权仍成功 |
| 单元 | `auth_files()` 在多环境下 | 返回顺序确定（默认布局优先） |
| 回归 | 设备授权导入 → 换号 | 写入的是**目标命名空间**的 `auth.json`，客户端登录成功 |
| 回归 | 机器上有 `prod/cn` 与 `prod/en` 两套 | 换号写 `cn` 时不动 `en` |
| 回归 | 客户端清空 records 后换号 | 仍能找回记录键（2026-11-02 场景） |

## 5. 验收标准

- 设备授权导入的账号，凭据里有 `auth_file`；
- 多环境机器上换号，写入的命名空间正确；
- 探测失败时导入流程不受影响（不报错、不阻断）。

## 6. 风险与回滚

| 风险 | 处置 |
|---|---|
| 记错路径反而比不记更糟 | 只在**能唯一确定**时写；多候选时按 §3.3 的确定性顺序取第一个，仍不确定就不写 |
| 路径指向的文件后来被删 | 换号时 `filter(\|p\| p.is_file())` 已做存在性检查，失效会自动回退到兜底搜索 |
| 误取文件里的凭据 | 实现上**只读路径**，不读 `records`；单测断言 cred 里没有来自文件的 token |

## 7. 与上游的关系

上游同类字段是有的，这一条主要是**我们自己的自洽性欠账**，先补自己。
若要提 PR，价值点不在「加字段」本身，而在配套的
**「按路径反推命名空间 + 记录键优先用客户端真实用过的」** 这套做法
（上游是「按实例绑定反推」，遇到自定义 data root 的客户端同样会选错）。

## 8. 实施记录（2026-11-12）

已按本方案落地，`cargo test --lib` **86 passed / 0 failed**。

### 8.1 `providers/minimax.rs`

- 新增 `local_auth_path()`：**只取路径、不读内容**（`auth_files()` 的第一个结果）；
- `poll_device_authorization` 拿到 token 后，把探测到的路径写进凭据的 `auth_file`；
  探测不到就不写该字段，**不影响设备授权本身**（换号时会照常提示「没找到登录态文件」）；
- `auth_files()` 增加**排序确定化**：官方默认布局（路径含 `prod` 且含 `cn`）优先，
  其余按路径字典序 —— `read_dir` 顺序未定义，机器上有多套命名空间时「取第一个」
  会选到不同文件；
- **只记路径，不取文件里的凭据**（文件里的记录属于客户端当前登录的那个账号，
  取它的凭据就是串号 —— 2026-10-25 实测踩过）。

### 8.2 尚未覆盖（留待后续）

- 老账号（此前用设备授权导入、凭据里没有 `auth_file` 的）**未做数据迁移**：
  换号时仍走兜底搜索，只是确定性差一点；迁移可放进 `migrate_device_key_ids` 一起做；
- 设备授权**之后**才首次登录客户端的场景：导入时探不到登录态文件，
  `auth_file` 仍为空（此时兜底搜索能正常工作）。
