# 远程访问 UI 与状态契约

侧边栏「远程访问」与首页「连接 ChatGPT」打开同一页面。三张卡片均已实现：快捷隧道、自建接入、仅 MCP。首次默认仅 MCP，后续按持久化模式显示；「当前使用」跟随后端配置事实，不由当前是否有 Tunnel 推断。

## 选择、应用与停止

选择卡片只阅读说明，点击应用才修改配置。操作进行中禁用卡片和重复提交；运行中的自建地址不可修改。应用任一不同连接方式时，单次操作会先停止现有远程运行时并撤销旧授权，再保存并启动目标方式；无需用户先手动停止。应用仅 MCP 后解除本机 OAuth，并按所选声明恢复 Passthrough。

快捷隧道显示组件检查/安装、URL 发现、公网验证、Ready、失败、断连或停止状态。Ready 之前不提供可复制公网 MCP 地址。Ready 表示公网 metadata、认证挑战和使用短期内部凭据的真实 `initialize`/`tools/list` 成功，不代表 ChatGPT 已完成授权。后台 Serena 不可用会令 Probe 失败。

断线清除 Public Context 与 OAuth，保留 `mode=quick_tunnel`、`status=disconnected`、`active=false`，不自动重新创建 Tunnel。用户可主动重开并更新客户端中的临时地址。停止后保留配置模式和 OAuth 拒绝边界；显式应用仅 MCP 才解除保护。

远程启动不会永久开启本地 Broker，不改变端口或 allowLan。停止后恢复用户持久化偏好：原 disabled 则停止临时 Broker，原 enabled 则保持运行。快捷/自建模式的 OAuth 也作用于同一 Broker 的本地和 LAN 请求；Transport 在三种模式下均为统一 JSON Streamable HTTP。

## 自建接入

输入框恢复本地保存的 Public Origin。只接受 HTTPS 域名和可选端口，拒绝路径、用户凭据、query、fragment。提示用户转发 `/mcp`、`/.well-known/*` 和 `/oauth/*`；可以保留公网 Host。

启动失败仍保持 OAuth，允许重试连接测试；只有授权 MCP Probe 成功才展示可复制地址。用户的代理进程始终由用户管理。停止撤销当前授权，但不会把仍可达的 MCP 变成匿名服务。

应用重启按持久化自建模式先恢复 OAuth 保护与有效授权，再启动 Broker/探测；相同公网地址的未过期授权无需重新确认，退出应用不等于撤销。Client、Grant 和 Token digest 保存在专用授权状态文件中，明文 Token 不落盘。用户点击停止或切换方式仍撤销旧授权。无效配置、授权文件损坏或 issuer/resource 不匹配时保持 Error 和拒绝访问，不回退 Passthrough。切换到快捷隧道会自动停止自建 Runtime，再启用新的 Quick Tunnel OAuth；自建公网代理进程仍由用户自行关闭或保护，不能把连接方式切换误认为外部代理已停止。

自建授权默认使用 72 小时滑动期限，每次成功刷新 Token 后续期 72 小时。普通 MCP 请求和应用重启不会续期；超过期限未刷新则需要重新授权。Access Token 单次有效期仍为 1 小时。

## 仅 MCP 安全声明

单选项实际提交并保存 `mcpOnly.securityDeclaration`：

- 「我的网关已经负责认证」→ `external_auth`，文字明确这只是用户声明，SerenaDesktop 不验证外部网关认证配置，不显示「认证已验证」「OAuth 已启用」或「安全」。Cloudflare Access / MCP Portal 均归此类。
- 「不使用认证」→ `none`，显示未默认勾选的风险确认：「我理解如果该 MCP 被暴露到公网，任何能访问 Endpoint 的客户端都可能调用公开工具，包括 Agent。」没有主动勾选不能应用；后端同样拒绝 `none + riskAccepted=false/缺失`。

风险确认作为本次应用命令的授权参数，不保存 Token 或所谓认证验证结果。保存 `none` 后重启恢复这一明确选择；选择卡片、刷新页面都不会静默应用变更。服务开关/端口/LAN 仍由 MCP 设置控制。

`external_auth` 显示可选「网关公网 Origin」，恢复独立保存的 `mcpOnly.publicOrigin`，与自建 OAuth 地址分开。代理保留公网 Host 时填写完整 HTTPS Origin（可带端口）；留空时说明代理必须改写为 Broker 本机 Host。应用前编辑不会改配置；保存后显示对应 `/mcp` 地址及「未验证外部认证」。只扩展 Host/Origin allowlist，不启动 OAuth 或 Tunnel，不展示 OAuth Public Context/授权客户端。切换 `none` 不提交该 Origin，风险确认仍须主动勾选。

连接失败展示后端脱敏 stage/category/host/elapsed/status，区分网络层和 OAuth/MCP 阶段；不展示响应正文或任何凭据。Ready Gate 与重试期限保持不变。

## 本机授权与隐私

授权对话框挂载应用根级，通过本机 IPC 获取 Pending 与允许/拒绝。显示客户端声明名称、回调地址、确认码、权限和剩余时间。默认焦点拒绝；关闭视为拒绝；过期请求禁用允许。保留焦点约束和关闭后的焦点恢复。

浏览器只能等待/轮询，不可自行批准。Probe 使用内部短期凭据，不弹真实用户授权对话框，也不计入已授权客户端数量。

OAuth Code、Token、PKCE verifier、完整 MCP 参数/结果、Prompt、源码不写 MCP Tool 日志。认证策略不再决定日志隐私等级。

## 验收边界

前端自动化覆盖实际声明参数、未确认禁止应用、保存 Origin 恢复、模式切换、断连旧地址清除、操作禁用、复制反馈、过期授权和请求 ID 绑定。DOM/mock IPC 仅证明前端行为，不能替代原生焦点或真实 ChatGPT OAuth。

后端的重建 Broker、严格 Host/Origin、统一 JSON、完整授权 Probe、临时 Broker 偏好恢复、原子 Job 所有权、Host 强杀分别验证。Windows 隔离 Host 脚本运行生产进程管理路径并强杀自己的测试 Host；不强杀用户已有 SerenaDesktop。真实 SelfHosted HTTPS 反代和 ChatGPT discovery/tools/list/tools/call 仍需分别记录实际结果，未运行一律 NOT_RUN。

本轮收尾：MCP Only 独立 Origin UI 与后端恢复/Host 测试通过；真实 ngrok 保留公网 Host 的 initialize/tools/list 通过。Quick Tunnel 既有完整成功样本，也有 Ready 后 metadata TLS 复测失败，不能显示为已证明稳定兼容。Cloudflare MCP Portal 仍缺已登录客户端的验收，真实 ChatGPT 三项均 NOT_RUN。

## MCP Only 认证区域展示

认证方式使用两项完整选项行，只有选中项展开配置。网关选项显示独立地址输入框及简短格式提示，Host/Origin 技术说明默认折叠在「什么时候需要填写？」中；用户声明未验证的说明始终可见。不使用认证时显示独立风险确认区，确认框未选中不能应用。应用按钮位于配置之后，并显示当前已应用或待应用状态；所有后端认证、配置和风险确认契约保持不变。

## ngrok 授权页轮询

授权页的同源 `/oauth/approval/{id}` 轮询显式请求 JSON，并携带 ngrok 官方支持的 `ngrok-skip-browser-warning: 1` API 请求头，避免免费入口把轮询响应替换成提示页。该请求仍不发送 Cookie；本机原生授权、OAuth 校验及跳转逻辑保持不变。如果网关仍返回非 JSON 页面，页面明确提示检查网关配置并停止轮询；临时网络错误仍在原有 120 秒授权期限内重试。


## 授权操作与状态刷新

本机 Approval 独立于开启、停止、测试连接的 busy 锁；长时间 Probe 期间授权事件仍刷新 Pending，允许/拒绝仍可响应。授权对话框显示实际 scope，并根据注册能力 refreshAllowed 明确提示是否签发刷新令牌；持续授权提示 Access Token 一小时、Grant 72 小时滑动续期、固定地址跨重启恢复及停止即撤销。即使 scope 只有 serena:mcp，也会展示持续授权说明。默认焦点拒绝和关闭即拒绝不变。

操作后的刷新保留当前 Snapshot，以新 Snapshot 原位替换；读取失败保留最后状态并显示读取错误。Quick 与 SelfHosted 使用同一测试连接处理方式，失败后展示后端 Error，Quick Error 仍可重新测试连接。
