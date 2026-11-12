# D｜配额数据源对齐上游会员接口

> 编号 D · 状态：**暂缓（2026-11-12 决定不接入）** · 风险等级：低 · 归属：`docs/minimax-hardening/`

## 一句话结论

本地现在读的是 `credit/details`（**分页积分流水**），上游 main 后端已经换成
**会员余额**接口 —— 但上游**只改了后端、前端没跟上**（前后端口径已经不一致）。
所以这件事**不能照抄上游**，必须自己实测响应结构、两端一起改，并保留回落。

## 1. 现状

### 1.1 本地怎么取配额

`src-tauri/src/providers/minimax.rs:677-744`：

```
GET https://agent.minimax.cn/minimax-cloud/api/v1/credit/details?timezone_id=<iana>
   ↓
body["details"]        ← 顶层（不是 data.details，这个结论是对的）
   ↓ 每条 → CreditItem { credit_type, granted, consumed, remaining, granted_at_ms, expire_at_ms }
Quota { total_granted, total_consumed, total_remaining, items, message, credit_groups: [] }
```

`credit_type`：`1 = 购买（purchased）`、`2 = 签到赠送（free）`（`minimax.rs:685-691`）。

界面（`src/components/QuotaCard.tsx:98-146`）拿的是：
`totalGranted / totalConsumed / totalRemaining / items / message / creditGroups`。
`credit_groups` 恒空 → 走「无官方分组」分支：
剩余大字 + 用量条 + `message`（"剩余 N 积分，最近一批 M 天后到期"）+ 明细展开表。

### 1.2 这套口径的固有问题

`credit/details` 是**积分流水的分页明细**，不是「账户余额」：

- 明细是**批次**（哪一批购买、哪一批签到赠送、哪一批过期），
  客户端界面展示的其实是**会员套餐余额**，两者口径不同；
- 分页意味着明细可能被截断，「累计发放 / 累计使用」不一定是全量；
- 批次到期时间（`expire_at_ms`）在会员语义下并不存在
  —— 会员是「一个周期内的额度」，不是「一批批到期的积分」。

### 1.3 上游改成什么样了（核查结果）

上游 main 的后端（`minimax_code_oauth.rs`）新增了 `fetch_usage_snapshot`，
并让账号刷新走它：

```
POST {origin}/matrix/api/v1/user/get_user_extra_info      body {}
     → data.workspaces[] 里找 workspace_type == 0 → workspace_id

POST {origin}/matrix/api/v1/commerce/get_membership_info   body { workspace_id }
     → subscription_type / op_credit_summary / opcredit_balance
     （三个字段全缺 → 上游直接判定「套餐响应缺少套餐或积分字段」并报错）
```

结果存进账号的 `quota_raw`。

**但是**（2026-11-12 核查）：上游 main 的前端 `src/utils/minimaxCode/parser.ts`
仍然按 `credit/details` 的行来解析（`root.details` / `root.data.details`），
`src/pages/MinimaxCodeAccountsPage.tsx` 仍调用 `getMiniMaxCodeQuotaGroups`。
会员快照里既没有 `details` 也没有 `data.details`
→ `hasMiniMaxCodeQuotaData` 恒为 false → **上游的配额显示很可能已经空了**。

> 结论：上游这次改造**只做了一半**。我们如果跟进，必须两端一起做，
> 而且不能假设「上游这么做就对了」。

## 2. 目标行为

| 场景 | 现在 | 目标 |
|---|---|---|
| 套餐 / 余额 | 只有积分批次汇总 | 显示**会员套餐余额**（与客户端一致） |
| 批次明细 | 有（分页，可能不完整） | 保留但降级为「展开明细」；接口失败时回落 |
| 到期文案 | "最近一批 N 天后到期" | 按会员语义改成"套餐到期/本期剩余"，**不臆造批次到期** |
| 接口不可用 | —— | 回落 `credit/details`，界面标注口径，**不空显示** |
| 前后端一致性 | 一致 | **必须两端同时改**，不能只改一边（上游的教训） |

## 3. 前置动作：先实测（这一步不能跳）

会员接口的字段名与语义**不能靠猜**，必须用本机真实账号打一次接口，把响应原样记下来。

### 3.1 取一个可用 access_token

从账号池里取（`账号池 JSON` → 对应账号的 `access_token`），
或走已有的 `ensure_fresh_cred` 逻辑拿一份新鲜的。

### 3.2 打两个接口

```powershell
$token = "<access_token>"
$headers = @{ Authorization = "Bearer $token"; "accept" = "application/json" }

# ① 用户 / 工作区信息
$r1 = Invoke-RestMethod -Uri "https://agent.minimax.cn/matrix/api/v1/user/get_user_extra_info" `
      -Method Post -Headers $headers -ContentType "application/json" -Body "{}"
$r1 | ConvertTo-Json -Depth 8

# ② 会员信息（body 用 ① 里 workspace_type==0 的 workspace_id）
$ws = ($r1.workspaces | Where-Object { $_.workspace_type -eq 0 } | Select-Object -First 1).workspace_id
$body = if ($ws) { @{ workspace_id = $ws } | ConvertTo-Json } else { "{}" }
$r2 = Invoke-RestMethod -Uri "https://agent.minimax.cn/matrix/api/v1/commerce/get_membership_info" `
      -Method Post -Headers $headers -ContentType "application/json" -Body $body
$r2 | ConvertTo-Json -Depth 8
```

### 3.3 记录字段映射表（**待实测填**）

| 我们要展示的 | 期望来源字段 | 实测值 | 状态 |
|---|---|---|---|
| 套餐名 | `subscription_type` / `membership.plan_name` | 待填 | ⬜ |
| 剩余额度 | `opcredit_balance` / `op_credit_summary.*` | 待填 | ⬜ |
| 已用额度 | `op_credit_summary.*` | 待填 | ⬜ |
| 总额度 | `op_credit_summary.*` | 待填 | ⬜ |
| 本期到期时间 | ？ | 待填 | ⬜ |
| 额度刷新时间 | `op_credit_summary.*` / `refresh_time` | 待填 | ⬜ |
| 积分明细（回落用） | `credit/details` 的 `details[]` | 已有 | ✅ |

**如果 `op_credit_summary` 里没有到期时间**，那么「最近一批 N 天后到期」这条文案
在会员口径下就没有依据 —— 此时应当：
- 去掉到期倒计时，只显示「本期剩余 X」；
- 或继续用 `credit/details` 的批次信息作为**补充**（明确标注是「积分批次」）。

**不做的事**：没有实测到的字段，绝不猜一个字段名去读。

## 4. 改动清单

### 4.1 后端：`providers/minimax.rs`

新增（与现有 `quota()` 并列，互为回落）：

```rust
/// 会员套餐快照（上游同款接口）。
/// 拿不到就返回 None，由调用方回落 credit/details。
async fn membership_snapshot(&self, account: &Account, cred: &Value) -> Result<Option<Value>, String>;
```

- 两个 POST 请求，字段映射按 §3.3 的实测结果填；
- `workspace_id` 取不到时，第二个接口用空 body 再试一次（上游的做法）；
- 三个关键字段（套餐 / 积分摘要 / 余额）全缺 → 判定为无效响应，返回 `None`；
- 请求失败**不报错**，返回 `None` 走回落。

`quota()` 改为：

```
1. membership_snapshot() → Some → 按映射填 Quota { total_granted, total_consumed,
   total_remaining, refresh_at_ms, credit_groups: [会员组], items: [] }
   message = 「本期剩余 X（Y 后刷新）」之类，**不编造批次到期**
2. None → 现有 credit/details 逻辑原样保留（items 明细 + "最近一批 N 天后到期"）
3. 两者都失败 → 报错（不显示 0 冒充成功）
```

### 4.2 前端：`src/components/QuotaCard.tsx`

- 新增一条「会员额度」展示分支（`credit_groups` 非空时）；
- 保留现有明细展开表，但会员口径下 `items` 为空 → 展开区不显示（不要显示「0 个发放批次」这种空态）；
- 文案分两套：**会员口径**（本期剩余 / 下次刷新）与**批次口径**（剩余总积分 / 最近一批到期），
  不要混在一句话里。

### 4.3 兼容与回落

- 账号池里已存的 `quota_raw` 是旧的 `credit/details` 结构 → **不改存量**，读时按新结构解析失败即回落；
- 新老两种口径都要能在界面上区分（加一个小标签或在 `message` 里说明），避免用户以为数据变了。

## 5. 测试计划

| 类型 | 用例 | 断言 |
|---|---|---|
| 单元 | 会员接口正常响应 | 映射出的 `total_remaining` 与响应一致 |
| 单元 | 会员接口 401 / 500 | 回落到 `credit/details`，不报错 |
| 单元 | 会员响应缺关键字段 | 判定无效 → 回落 |
| 单元 | 两个接口都失败 | 返回错误，界面显示「读取失败」，**不显示 0** |
| 回归 | 真实账号跑一次 | 界面显示的余额与 MiniMax 客户端里看到的**人工核对一致** |
| 回归 | 存量账号（老 `quota_raw`） | 仍能正常显示（走回落） |

## 6. 验收标准

- 配额卡显示的是**会员余额**，与客户端一致（人工核对一次）；
- 接口不可用时自动回落，界面不空、不显示假的 0；
- 两种口径在界面上可区分。

## 7. 风险与回滚

| 风险 | 处置 |
|---|---|
| 会员接口字段与预期不符 | §3.3 的实测表是唯一依据；未实测不写代码 |
| 只改后端不改前端 → 配额显示为空（上游踩过） | 前后端**同一个 PR** 改，验收里包含「界面不为空」 |
| 会员口径没有到期时间 | 不编造：改文案或用批次数据补充，并标明口径 |
| 上游随后把前端也改了、口径再次不同 | 以客户端界面为准；接口层做成可切换（会员 / 批次） |

## 8. 与上游的关系

- 这条**不适合现在提 PR**：上游自己正在改这块，且当前 main 的前后端状态是**不一致**的，
  提 PR 反而要跟他们的半成品对齐。
- 等上游把前端补齐、口径稳定后，再对比一次；
  如果我们的映射更完整（尤其是「回落 + 口径标注」），可以作为一条改进 PR。

## 0. 暂缓说明（2026-11-12）

**本项暂缓，不接入。**

原因：手上**没有订阅了会员套餐的账号**，会员接口返回什么、字段长什么样都没实测过；
在这种前提下按文档猜字段去改，只会把「口径不对」的问题从「批次流水」换成「空白」。

定下来的原则：

- **只对现有实现打补丁**，不做未经实测的接口替换；
- 会员接口等**有可用订阅账号时再实测**，届时按本文 §3 的步骤把字段映射表填出来，
  再决定接不接（接口结构、套餐/额度字段、到期口径都确认后再动）；
- 在那之前，`credit/details` 的现有实现**保持不变**（`details` 在响应顶层这个结论是对的）。
