# Remote Access 实现契约

本文描述当前三种接入方式的实际实现；真实客户端兼容性与自动化测试分别记录，不将本地 fixture 视为 ChatGPT 集成证据。

| 模式 | 认证 | 公网入口 |
| --- | --- | --- |
| `quick_tunnel` | SerenaDesktop Embedded OAuth | SerenaDesktop 管理的 cloudflared Quick Tunnel |
| `self_hosted_oauth` | SerenaDesktop Embedded OAuth | 用户提供公网 HTTPS Origin，自行管理代理 |
| `mcp_only` | 用户声明第三方认证，或明确接受无认证风险 | 本地、可信网络或用户网关 |

三者共享现有 Broker、Tool Registry 和 Dispatcher；不增加 Provider trait、插件框架、第二套 MCP Broker 或 OAuth Token 数据库。Cloudflare Access / MCP Portal 属于 `mcp_only + external_auth`。

## 配置与启动

默认配置：

```json
{
  "remoteAccess": {
    "mode": "mcp_only",
    "selfHosted": { "publicOrigin": null },
    "mcpOnly": { "securityDeclaration": "external_auth", "publicOrigin": null }
  }
}
```

Rust `RemoteAccessMode::default()`、`Remote::default()`、ManagerConfig 与前端初始配置均为 `mcp_only`。旧配置缺少 `remoteAccess` 时使用该默认值；旧版从未保存的 OAuth 选择无法从 Broker enabled 单独推断。

配置持久化包含模式、自建 Origin、MCP Only 声明及其可选公网 Origin。自建 OAuth 另在应用数据目录的 `runtime/oauth-state.json` 保存 Client、Grant、Access/Refresh Token digest、Refresh 使用状态和绝对到期时间，绑定 issuer/resource。不会保存明文 Token、Pending、Code、PKCE verifier、Probe credential、Quick Tunnel URL/PID 或 Public Context instanceId。

`SupervisorState::new` 读取配置后，`Broker::new` 同步调用 `Remote::from_config`，先确定策略：两个 OAuth 模式均为 EmbeddedOAuth。自建模式验证 HTTPS Origin 并恢复相同 issuer/resource 的有效授权，再由 `Broker::startup` 建立监听和执行公网探测。退出或异常进程终止后，已成功签发且未过期、未撤销的凭据仍可使用；重启不延长原到期时间。无效 Origin、损坏或不匹配的授权文件保持 Error 和拒绝访问，不回退 Passthrough，不自动重置授权文件。

自建模式在启动时恢复服务和探测；Quick Tunnel 只恢复配置及 fail-closed 策略，不自动创建隧道，也不显示旧地址。普通设置保存不能修改 Remote Access 配置，模式切换必须通过专用本机 IPC。

## Broker 的持久偏好与运行时需求

远程启动不写 `broker.enabled=true`，不修改 `port` 或 `allowLan`。两个 OAuth 模式都先安装 EmbeddedOAuth protection，再启动唯一 Broker；缺少 Runtime 时 `/mcp` 仍返回 401。监听失败时回滚保存的配置与原策略；回滚失败保持更严格策略。后续启动失败清理运行时资源，并保留已配置 OAuth 模式的拒绝边界。

用户显式 Remote stop 撤销持久化授权并等待受管进程退出：persistent enabled=false 时停止临时 Broker；enabled=true 时保持 Broker 运行，但 OAuth 模式保持拒绝匿名访问，直到用户显式应用 MCP Only。应用 shutdown 停止 Remote 和 Broker、保留有效自建授权。Quick Tunnel 地址是临时地址，不会把旧 Origin 的授权转移到新 Tunnel。运行时 Broker 端口与 Serena 端口也必须不同，普通设置保存会在停止 Serena 前拒绝冲突。

## Authentication 与 Transport

所有模式的 `/mcp` 使用同一个 rmcp 3.2.0 `StreamableHttpService`：

```rust
legacy_session_mode = false;
json_response = true;
```

正常 JSON-RPC POST 请求返回 `application/json`，不创建 session ID；独立 GET SSE 不可用。服务声明协议版本 2024-11-05、2025-03-26、2025-06-18、2025-11-25。认证 middleware 只验证凭据，不选择另一套 Transport，也不将 SSE 错误转换作为认证职责。不会因切换 Remote Mode 而切换本地 MCP 的 Transport。

认证策略只决定是否要求 SerenaDesktop OAuth；Remote 模式决定是否管理 Quick Tunnel。已有 session/SSE 客户端需要重新连接并使用这一统一 HTTP 契约，真实 Cloudflare MCP Portal / ChatGPT 兼容性仍须单独验收。

## Host 与 Origin

Host 基础 allowlist 为 rmcp loopback 主机；`allowLan=true` 时加入监听启动时的本机 IPv4 地址。请求副本可从当前受控 OAuth Public Context，或本地配置的 `mcp_only + external_auth + mcpOnly.publicOrigin`，加入 publicOrigin 的 host[:port]。Quick Tunnel 仍传 `--http-host-header 127.0.0.1:<port>`；自建反代可以保留公网 Host。

`mcpOnly.publicOrigin` 只用于 Host/Origin allowlist、端点诊断和 UI 显示，绝不建立 OAuth Runtime、Token、issuer 或 Public Context。AuthPolicy 始终为 Passthrough；重建 Broker 后保持相同契约。字段只接受完整 HTTPS Origin，可带端口，拒绝路径、用户凭据、query 和 fragment；通过本机应用命令及配置加载校验。本机应用命令在 `none` 模式拒绝提交公网 Origin；只有 `external_auth` 才将该字段加入 allowlist。留空时，上游代理必须将 Host 改写为允许的 Broker 本机 Host；不要求所有网关都改写 Host。

本轮真实 ngrok 请求在本机 19120 收到 `Host: immersion-lagging-levitator.ngrok-free.dev`、同值 `X-Forwarded-Host`，Origin 与 Forwarded 均缺失。因此支持本地显式公网 Origin，而不是猜测第三方网关会改写 Host。Cloudflare MCP Portal 的 Host 行为须通过其认证入口单独验收；ngrok 的结果不代表 Cloudflare。

`Forwarded` / `X-Forwarded-Host` 不扩展 allowlist。Host、Forwarded、X-Forwarded-Host、Origin、Referer 都不参与 issuer、authorization_endpoint、token_endpoint 或 resource 的生成。

Origin 默认只接受准确的本机 loopback HTTP Origin（Broker 端口）及上述模式对应的受控公网 Origin。没有 Origin 的普通 MCP 客户端正常访问；恶意跨站 Origin、null Origin、重复 Origin 和相同域名的其他端口被拒绝。额外严格校验弥补 rmcp 3.2.0 对未指定 Origin 端口的通配匹配与只读取首个 Origin 的行为；rmcp 自身 Host/Origin 检查继续开启。

## OAuth 与 Ready Probe

保持 Authorization Code、PKCE S256、DCR、opaque Access/Refresh Token、refresh rotation 和本机授权。授权码与 Pending 有效期 120 秒且仅进程内有效；Access Token 1 小时，Grant 默认 72 小时。每次成功使用 Refresh Token 时，Grant 从刷新时刻重新计算 72 小时，并持久化新的到期时间；普通 MCP 调用、失败刷新及应用重启不续期。超过期限不能通过刷新恢复，需要重新授权。仅自建模式跨重启恢复这些状态。没有 JWT/JWKS、RBAC、账号体系或 per-tool OAuth scope。

DCR Client 注册后有效期为 24 小时；现有请求驱动的 prune 会回收过期且没有有效 Pending、Code 或 Grant 引用的 Client，注册容量检查之前先清理。有有效 Grant 的 Client 不会因注册期限到期而失效。注册支持 `refresh_token` 的 Client 经本机持续授权确认后签发 Refresh Token，包括仅请求 `serena:mcp` 的客户端；scope 保持原值。显式仅注册 `authorization_code` 的客户端不签发 Refresh Token，且不能请求 `offline_access`。省略 grant_types 时保持既有注册响应，同时支持两种流程。HTTP 回调仅允许 loopback，IPv6 使用解析后的 `::1` 地址判断。

Refresh Token 及其使用记录有各自的绝对到期时间（新签发为 72 小时），不会随后续刷新无限延长。有效期内重放已使用 Token 仍撤销整个 family；过期记录会被清理，过期 Token 始终拒绝，但不会撤销后续续期的授权。这避免滑动授权的历史记录永久累积。授权文件 v3 保存 Client 的刷新流程能力和 Token 期限；兼容读取 v1/v2，沿用原到期时间，不延长旧 Token，也不为旧授权凭空生成 Refresh Token。旧客户端之前未获得 Refresh Token 时，需要在更新后重新连接并允许一次，之后可自动刷新。读取 v1 时旧 Refresh Token 沿用原 Grant 到期时间。

SelfHosted 在 Broker 开始监听前安装完整 OAuth Runtime，首个请求即可读取 metadata 和完整 401 challenge；公网 Probe 仍异步决定 Ready。Authorization Endpoint 对已验证 Client 和回调地址后的 `invalid_scope`、`unsupported_response_type` 使用 302 返回错误及原 state；无效 Client 或回调地址不重定向。

对外路由：

- `/mcp`
- `/.well-known/oauth-authorization-server`
- `/.well-known/oauth-protected-resource` 与 `/.well-known/oauth-protected-resource/mcp`
- `/oauth/register`、`/oauth/authorize`、`/oauth/token`、`/oauth/approval/{id}`

无效或缺少 Bearer Token 的 OAuth MCP 请求返回 401；Runtime 存在时带 resource_metadata challenge。撤销的 Desktop Token 在 Passthrough 下也不能退化成匿名请求。MCP Only 不创建 OAuth Runtime，discovery/authorization 返回不可用。Tauri IPC、设置 API、日志和 Dashboard 不经 Tunnel 暴露。

Quick Tunnel 与 SelfHosted 的 Ready 都要求同一公网 Probe 完成：

1. OAuth Metadata 的 issuer/token_endpoint 与本地 Public Context 一致。
2. 无 Token `/mcp` 返回准确的 401 challenge。
3. Protected Resource Metadata 的 resource/authorization_servers 一致。
4. 短期内部凭据经正常 OAuth protect → rmcp → 现有 Handler 完成 `initialize`。
5. 同一公网入口的 `tools/list` 返回真正工具数组，不能是 JSON-RPC error。

Probe 失败只返回 `REMOTE_PUBLIC_PROBE_FAILED: stage=... category=... host=... elapsed_ms=... status=...`。阶段区分 oauth_metadata、unauthorized_mcp、resource_metadata、initialize、tools_list；类别区分 DNS、connect、TLS、proxy、timeout、HTTP status、invalid_json、metadata_mismatch、Host/Origin/OAuth reject 及 MCP result/transport。网络类别从 reqwest 错误链提取固定标签，不输出原始错误文本；无法识别的连接失败保留 connect，不猜测 DNS/代理原因。403 只识别有限长度的固定 Host/Origin 拒绝正文，其余按 HTTP status 报告，不显示 response body、credential 或工具内容。

保持 reqwest 0.13.4 默认系统代理发现行为：环境代理变量及 Windows 启用的系统代理参与选择；不自动尝试直连、不禁用 TLS、不覆盖 DNS。当前 Windows hyper-util 实现读取 ProxyEnable/ProxyServer/ProxyOverride；残留 ProxyServer 不等于代理已启用，也不代表覆盖 VPN、透明代理或 PAC 的全部行为。

Quick Tunnel 仍在同一个 Tunnel 中使用固定 45 秒总验证 deadline、每请求 10 秒 timeout、2 秒间隔及最后诊断；进程退出立即中断等待，绝不自动创建新 Tunnel。Ready 仍要求本机通过公网 URL 完成全部 Probe；不因本机 DNS/代理/回环问题降级为 PID 或 401 就绪。

内部凭据通过本机内存创建，仅保存散列，60 秒失效；不创建 Client、Pending 或用户 Grant，不弹授权框，不增加公开 probe/免认证 endpoint。Probe 串行，正常结束、错误、取消或 Runtime 更换都会撤销凭据。HTTPS 不跟随重定向；每次请求 10 秒，metadata 上限 16 KiB，MCP 响应上限 4 MiB。真实 Serena 不可用时不会假 Ready；本地测试的 upstream fixture 明确不代表真实 Serena 或 ChatGPT。

本机授权对话框与浏览器显示同一确认码。浏览器只能等待/轮询；允许或拒绝来自本机 IPC。OAuth 响应保留 no-store、no-referrer、CSP、DENY 等保护。

## cloudflared 所有权

优先复用 PATH / 用户 .local/bin 中可运行的 cloudflared，不覆盖用户安装。已发现但版本检查失败时报错；未找到时下载固定官方 2026.9.0 组件。受管下载的 filename、SHA-256、版本与 URL 由同一官方 Release 固定，macOS `.tgz` 先校验压缩包再提取可执行文件。每次使用受管缓存均校验。

官方来源及全量资产 fixture：`https://api.github.com/repos/cloudflare/cloudflared/releases/tags/2026.9.0`，`src-tauri/src/remote/fixtures/cloudflared-2026.9.0.json`。本轮重新核验五个受支持 artifact，包含 darwin-amd64/arm64；现有 digest 与官方相符，静态测试绑定平台、文件名、digest 和下载 URL。

Windows Quick Tunnel 通过简单原生封装，在 `CreateProcessW` 时用 `PROC_THREAD_ATTRIBUTE_JOB_LIST` 进入 unnamed Job，启用 `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`，不允许 breakaway。Job handle 不可继承且不复制给 Child；受限 HANDLE_LIST 只传 NUL 与 stderr。没有 spawn→assign 裸跑窗口，不依赖 `kill_on_drop` 来保证 Host 强杀回收；创建失败不回退到裸进程。Host 崩溃或 TerminateProcess 关闭 Job handle，由内核回收 cloudflared 及后代。

正常 stop 先撤销 OAuth，终止 Job，再等待确认退出；退出证据不足时保留受管 Child 供重试，不恢复匿名访问。非 Windows 使用既有 Tokio child/kill-on-drop 与明确 wait，未声明 Windows 等价的 Host crash 保证。没有复制 Agent Runtime 的 SQLite、Claim 或恢复状态机。

使用空临时 cloudflared config、`--no-autoupdate`，并移除继承的 TUNNEL_* 设置。仅本次启动允许等待 DNS/edge 传播；意外退出不创建新 Tunnel。

## 状态与日志

`Snapshot.mode` 是配置事实，独立于 `status`、`active` 和 `publicContext`；后者仅 Ready 时提供。断连为 `mode=quick_tunnel,status=disconnected,active=false,publicContext=null`。Stop 不把模式改成 MCP Only；应用 MCP Only 才改变模式和策略。

所有 MCP Tool 使用统一日志策略，记录 request id、tool name、duration、success/failure 与参数校验错误码，不记录 arguments、Prompt、源码、完整结果或错误内容。OAuth Token、Code、PKCE verifier 不写日志；HTTP 只记录路径，不记录 query/body/Authorization。

## 验证命令与证据边界

从仓库根执行用户要求的 lint/build、前端测试、cargo fmt/check/clippy/test。额外显式公网测试：

```powershell
cargo test --manifest-path src-tauri/Cargo.toml official_quick_tunnel_start_probe_stop -- --ignored --nocapture
cargo test --manifest-path src-tauri/Cargo.toml official_managed_cloudflared_install_and_reuse -- --ignored --nocapture
```

Windows 强杀测试先构建 lib test binary，再将 Cargo 输出的实际 `.exe` 路径传给：

```powershell
./scripts/test-quick-tunnel-host-crash.ps1 -TestExecutable <test-binary.exe>
```

脚本创建隔离原生 Host，执行生产 Remote/Broker/受管 cloudflared，等 cloudflared 已受管、生成公网 Origin 且持有 socket 后只强杀 Host；检查原 cloudflared 进程句柄已退出、TCP/UDP sockets 归零。使用本地 upstream fixture，不操作已有桌面实例。该结果是生产进程所有权的原生运行证据，不是 Tauri UI 或真实 ChatGPT 验收。

Quick Tunnel local/runtime、SelfHosted local/runtime、MCP Only、Public Quick Tunnel、真实 SelfHosted 反代、ChatGPT discovery/tools/list/tools/call、Windows Host Crash 必须分别报告。ignored 不计 PASS；未运行标记 NOT_RUN，环境不可用标记 UNAVAILABLE。不能由本地 HTTP/401 Probe 推断真实客户端兼容性。

### 有界收尾验证记录（2026-09-10）

| 项目 | 实际结果 |
| --- | --- |
| MCP Only local/runtime | PASS：external_auth 公网 Host 的 initialize/tools/list 进入真实 Handler；未知 Host、恶意 Origin、无效 Origin 拒绝；save/reload/重建 Broker 后保持 Passthrough、地址保留、OAuth 不启用 |
| Probe diagnostics | PASS：分阶段 HTTP/JSON/metadata/Host/Origin/OAuth/MCP 错误及 connect/timeout 脱敏测试；DNS/TLS/proxy 错误链固定分类；URL 中的 proxy 字样不会影响错误分类 |
| Rust | cargo check、clippy --all-targets -- -D warnings PASS；cargo test：272 passed、0 failed、19 ignored（ignored 不计 PASS） |
| Frontend | npm run lint、npm run build PASS；node --test src/*.test.mjs：55 passed |
| 格式 | changed files rustfmt PASS；whole repository cargo fmt -- --check FAIL：未修改的 agent/* 与 mcp/registry.rs 已有格式差异；git diff --check PASS |
| Real Quick Tunnel public smoke | 首次 RUN/PASS；后续 RUN/FAIL。后续运行的启动 Gate 已完成 metadata、401 challenge、resource metadata、授权 initialize/tools/list 并 Ready，但紧接的连接复测在 oauth_metadata 阶段 TLS 失败（564 ms、无 HTTP status）。两次均停止、撤销 OAuth、回收 cloudflared。不能报告稳定 PASS |
| Real ngrok MCP | RUN/PASS：公网请求→用户既有 ngrok→本机 19120 的生产 Broker→initialize/tools/list JSON；Broker 日志证实公网 Host 被保留。上游使用本地测试 fixture，不代表用户真实 Serena 数据、外部认证验证或 ChatGPT |
| Cloudflare MCP Portal | 当前未登录请求返回外层 403，未到本机 9120；authenticated initialize/tools/list 及实际转发 Host NOT_RUN，等待可用的已登录客户端 |
| Windows Host Crash | 本轮未改所有权实现；完整 Host Crash/socket 脚本本轮 NOT_RUN，上一轮隔离原生测试 PASS。本轮网络诊断 Host 的清理仍确认其受管 cloudflared 退出，不将其替代完整脚本或 Tauri UI 验收 |
| Real ChatGPT OAuth discovery / tools/list / tools/call | 均 NOT_RUN |

本机观察：ProxyEnable=0、无环境代理变量；Clash/Mihomo 进程在运行，Karing TUN 网卡启用并配置 DNS 10.20.0.2。默认 reqwest 与仅测试使用的 no_proxy 对 ngrok 均能完成 TLS（无后端时 HTTP 502）；一个存活的 Quick Tunnel 上两者均曾在 metadata TLS 阶段失败；另一个保持存活、发现地址 15 秒后测试的 Tunnel 上，两者又都获得 metadata HTTP 200。因此不能把禁用 reqwest 代理等同于绕过系统 TUN，也不能把问题直接归因为系统显式代理。未改网络配置、DNS、TLS 或 Ready Gate。尚无证据证明“外部稳定可达而 Host 稳定不能回环”；不作该 Material Contract Difference 判断。

本机临时证据：`serena-remote-closure-public.log`（首次 PASS）、`serena-remote-closure-public-final.log`（后续 TLS FAIL）、`serena-remote-closure-ngrok-final.log`（真实 ngrok PASS）、`serena-remote-closure-network.log`（默认/测试 no_proxy 诊断），以及 `serena-remote-closure-{test,check,clippy,fmt,frontend}.log`。另一次存活 Tunnel 的双客户端 HTTP 200 证据位于临时目录 `serena-live-network-83bb2e4841904643913c9660f1d91719/network.log`。诊断测试正常结束不等于 TLS 或公网 MCP 验收通过。

额外网络诊断均需显式执行：

```powershell
$env:REMOTE_GATEWAY_TEST_ORIGIN = 'https://your-gateway.example'
$env:REMOTE_GATEWAY_TEST_PORT = '19120'
cargo test --manifest-path src-tauri/Cargo.toml real_mcp_only_gateway_initialize_and_tools_list -- --ignored --nocapture

$env:REMOTE_NETWORK_DIAGNOSTIC_ORIGIN = 'https://your-gateway.example'
cargo test --manifest-path src-tauri/Cargo.toml public_network_proxy_and_tls_diagnostic -- --ignored --nocapture
```

前者要求已有网关映射到空闲本机端口，仅短时启动隔离测试 Broker/上游 fixture，不修改用户网关或真实配置。后者只 GET metadata 对比网络行为，不启用生产 no_proxy，也不发送 Token。


## 有界并发与 OAuth 修复

OAuth 模式转换先安装内存认证边界，再持久化 Remote 配置和启动 Listener；Quick 尚无 Public Context 时也返回 401。Remote 专用保存入口不执行 Serena discovery。应用启动等待 Serena 检测/自动启动任务结束后再恢复 Remote，Startup 与 Broker 配置修改、Remote 模式修改共用 management 锁。启动失败且配置回滚也失败时保留两项诊断并进入受保护的 Error。

有效 Authorization Code 在凭据容量不足时可稍后重试；错误 Client、Redirect 或 PKCE 仍一次性消费。重定向前限制 state 长度与控制字符。相同 Client 的相同请求复用 Pending；每个内存 OAuth Runtime 每 10 秒最多创建 4 个新 Pending，窗口唤醒至少间隔 5 秒，仍保留 32 个 Pending 的容量上限，不信任转发 IP 头。

显式 Quick/SelfHosted Probe 均更新 Ready/Error，失败时隐藏可复制 Public Context，保留原进程所有权与认证保护；重新 Probe 不创建 Tunnel。以上验证使用本地 fixture，不代表真实公网或 ChatGPT OAuth 验收。
