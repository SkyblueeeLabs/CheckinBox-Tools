# Trae CN 协议与加固记录

测试客户端：Trae CN 3.4.20（tronBuild 2.3.103559），2026-11-17。
实现：`src-tauri/src/providers/trae.rs`。

> 所有接口都是客户端内部接口，随时可能变。改动前先跑下面的真机验证，
> 并重新确认客户端 `out/main.js` 里的端点、请求头和错误码。

## 1. 本机登录态

```text
%APPDATA%\Trae CN\User\globalStorage\storage.json
```

- `iCubeAuthInfo://icube.cloudide`：账号凭据，解密后是 JSON：
  `token` / `refreshToken` / `expiredAt` / `refreshExpiredAt` / `userId` /
  `host` / `userRegion` / `account.username` / `account.avatar_url`。
- `iCubeAuthInfo://icube-dc:<设备号>`：设备密钥，解密后是
  `privateKeyPEM` / `publicKeyPEM`（EC P-256，PKCS#8/SPKI PEM）。
- `iCubeAuthInfo://usertag`：uid → 区域标签（`cn`）映射。
- 设备号也写在键名里，不需要解密。
- 值不是明文，是 Trae 自己的 `tc` 格式：

```text
[magic 6 bytes]
[32 bytes key material]
[AES-128-CBC + PKCS7 ciphertext]

plaintext = [SHA-512(data) 64 bytes] [data]
key/iv = SHA-512( SHA-512(key material) || (A || B) ) 的前 32 字节
```

`A/B` 是客户端 `byteCrypto.js` 里两组硬编码常量；标准 magic 是
`74 63 05 10 00 00`（"tc"），少数版本用 private magic `12 39 20 20 02 03`。
解密后必须校验 SHA-512，校验不过按损坏处理。

## 2. 签到与积分

Base：`https://api.trae.cn`（账号凭据里的 `host` 优先）。

| 用途 | 请求 |
|---|---|
| 状态 | `POST /trae/api/v2/ug/checkin_credits/status` |
| 领取 | `POST /trae/api/v2/ug/checkin_credits/claim` |
| 积分 | `POST /trae/api/v2/pay/ide_user_ent_usage` |

公共请求头：

```text
Authorization: Cloud-IDE-JWT <access token>
X-User-Region: CN
x-device-id: <12~20 位纯数字 Aha 设备号>
Content-Type: application/json
```

签到 body：

```json
{"req_source": 1}
```

- IDE（Trae CN）：`req_source=1`，OAuth ClientID `ono9krqynydwx5`
- SOLO（Trae Solo CN）：`req_source=2`，OAuth ClientID `en1oxy7wnw8j9n`

实测状态响应（2026-11-17，未签）：

```json
{
  "checked_in": false,
  "code": 0,
  "credits": 100,
  "did_checked_in": false,
  "enable": true,
  "extra_credits": 100,
  "message": "success"
}
```

领取响应：`{"code":0,"message":"success"}`。
领取后状态变为 `checked_in=true`、`did_checked_in=true`，余额 500→600。

积分口径：

```text
可用积分 = usage_summary.total_amount - usage_summary.consumed_amount
```

`total_balance` 只做交叉校验。`user_entitlement_pack_list` 里：
- `group_name = "每日签到"`：`quota.credits_limit = 100`，31 天有效；
- `group_name = "每月登录积分"`：`quota.credits_limit = 500`，自然月有效；
- “免费”基础包没有 `credits_limit`，跳过。

`credits` 是免费用户当天实际可领；`extra_credits` 是客户端里的会员加成提示，
不能加进当天积分。

## 3. 错误码

| code | 含义 | 处理 |
|---|---|---|
| `0` | 成功 | 记录 |
| `9074` | 设备号不被认可 | 不重试同号；提示重新导入/检查设备号。文案「当前参与用户太多」是幌子 |
| `9095` | 本设备今日已签到 | 终局失败；一台设备一天只能签一个账号 |
| 200 + “太多/拥挤/繁忙/稍后” | 上游拥塞 | 退避后重试，不能记成功 |
| 401/403 | 登录态过期 | 先按刷新护栏处理；失败提示重新导入/授权 |

## 4. 刷新 token

当前客户端用 v3：

```text
POST https://api.trae.cn/trae/api/v3/oauth/ExchangeToken
Content-Type: application/json
x-cloudide-token: <当前 access token>
```

Body：

```json
{
  "ClientID": "ono9krqynydwx5",
  "ClientSecret": "",
  "RefreshToken": "<rt>",
  "DeviceInfo": {
    "DeviceID": "<真机号>",
    "MachineID": "<32 hex>",
    "PlatformCode": "IDE_PC",
    "DeviceType": "PC",
    "DeviceName": "<主机名>",
    "DeviceModel": "",
    "ClientVersion": "3.4.20",
    "DevicePublicKey": "<SPKI PEM>",
    "DeviceBrand": "",
    "DeviceCPU": "",
    "OSInfo": "Windows",
    "OSVersion": "10.0"
  },
  "DeviceProof": {
    "Signature": "<base64 DER>",
    "Timestamp": 1794898893,
    "Nonce": "<32 hex>"
  },
  "IDEVersion": "3.4.20"
}
```

签名原文（换行分隔）：

```text
POST
/trae/api/v3/oauth/ExchangeToken
<ClientID>
<RefreshToken>
<Timestamp>
<Nonce>
```

ECDSA P-256 / SHA-256，输出 **DER** 再 base64。Node 的 `crypto.sign` 默认就是 DER；
Rust 侧必须用 `Signature::to_der()`，不要用 raw `r||s`。

响应取 `Result.Token` / `Result.RefreshToken` / `Result.TokenExpireAt` /
`Result.RefreshExpireAt` / `Result.UserID`。时间戳兼容秒/毫秒。

旧 `/cloudide/api/v3/trae/oauth/ExchangeToken`（`ClientSecret:"-"`）只做兜底，
并且只在 v3 返回 `404/405/501` 时尝试；401、400、业务错误不回落。

## 5. 刷新护栏（与 MiniMax 同款）

Trae 的 RT 是否轮换、旧值是否立即失效尚未实测。为避免把桌面客户端顶下线：

1. 当前客户端登录的账号（`Same`）：
   - 客户端 `Running` / `Unknown`：只从 storage 捕获最新凭据，**绝不自行刷新**。
   - 客户端 `Absent`：可以刷新，刷新成功后备份并回写 `storage.json`。
   - 回写只改 `iCubeAuthInfo://icube.cloudide`，保留设备密钥、会话和其余字段。
2. 非当前账号（`Other`）：可以自刷，只更新账号池。
3. 身份不明（`Unknown`）：客户端 Running/Unknown 时不刷；只有 OAuth 账号且客户端
   确认 Absent 时才允许自刷。
4. 401 抢救：先重读账号池，再按上面的规则捕获或刷新，仍 401 就提示重新导入/授权。
5. 切换客户端账号前：先捕获当前登录，再关闭客户端，目标凭据临期则强制刷新；
   刷新失败拒绝切换（与 MiniMax 的 `switch_cred_guard` 同策略）。

## 6. 多账号设备号

一台设备一天只能签一个账号（`9095`）。账号凭据里拆成：

- `login_device_id`：真机号，OAuth 登录和刷新 DeviceInfo 用；
- `checkin_device_id`：签到 `x-device-id` 用。

第一个账号优先用真机号；后续账号从真机号同源衍生（前缀不变，末 4 位递增），
避免撞号。已有合法设备号必须沿用，不要每次导入都换。

衍生号是否会被 `9074` 拒还需第二个测试账号验证。验证不通过时，多账号只能退回
「一机一号」或独立 Windows 用户配置。

## 7. 已验证 / 未验证

已验证（2026-11-17，本机）：

- `tc` 解密能得到 token / refreshToken / userId / 昵称 / 设备密钥；
- 状态查询：未签 `checked_in=false`、`credits=100`、`enable=true`；
- 真实 claim：`code=0`，余额 500→600，状态变 `checked_in=true`；
- 积分包解析：每日 100 + 每月 500；
- ECDSA DER 签名可被本地公钥验证；
- 换号预检：目标账号 `CheckLogin=false` 时拒绝写入，当前客户端重新拉起并保持原账号登录；
- 浏览器 OAuth：PKCE + 本地回调 + v3 AuthCode 交换成功；OAuth token 绑定客户端同一把设备密钥；
- A↔B↔A 切换成功，`storage.json` 按代备份，`conversation-tips` / `ModularData` 等会话目录
  共享未清空，两个账号都可继续签到/续期；
- 多账号签到：A 用真机号、B 用同源衍生号，同机同日各 +100，未触发 9074/9095；
- v3 刷新（DeviceProof）真机成功：用 OAuth 独立凭据刷新后，新 token 已回写账号池。

未验证：

- 旧 refresh token 是否立即失效（现实现总是持久化新值，不依赖旧值）；
- 切号后 Trae 的聊天记录是否按账号隔离（当前确认是共享，不删除）。

## 8. 换号实测与结论（2026-11-17）

- 本机导入两个账号后，在客户端手动登录新号，老号的 access token 调用
  `CheckLogin` 返回 `Result.IsLogin=false`，refresh 也返回失效。
- 直接切到老号：客户端启动后清空 `iCubeAuthInfo://icube.cloudide`，停在未登录状态。
  已从切换前的备份恢复新号。
- 已加换号预检：写入前先 `CheckLogin`；失败则强制刷新一次再校验；仍失败就拒绝写入，
  并重新拉起当前客户端。复测切到老号被拒绝，新号保持登录，`storage.json` 未被覆盖。
- 结论：本机导入只能保证「当前登录账号」的会话；手动登录另一账号会作废旧会话。
  多账号 A↔B 必须用 OAuth 独立授权（每个账号有自己的 refresh token）。
- OAuth 实测：浏览器登录老号拿到独立凭据后，切到老号成功；再切回新号也成功。
  换号只改 `iCubeAuthInfo://icube.cloudide` / `usertag`，设备密钥和会话目录不动；
  两个账号的 `CheckLogin` 均通过。换号预检继续保留，失效目标仍会被拒绝。

## 9. 验证命令

```bash
# 只读：本机登录态能解出
cd src-tauri
cargo test --lib -- --ignored 本机Trae登录态能解出

# 只读：本机签到状态能查
cargo test --lib -- --ignored 本机Trae签到状态能查

# 全部 Trae 单测
cargo test --lib trae
```

日志里不要出现 token / refreshToken / 设备私钥；需要取证时只记录状态码、code、
余额和脱敏后的字段名。
