# 兼容 ChatGPT OAuth discovery 与授权触发

## Goal

查明 ChatGPT 连接在 macOS 失败而 Windows 正常的实际差异，在证据确认根因后完成最小 OAuth 兼容修复，并验证 DCR 到首次工具调用授权流程。

## Requirements

- 修复前必须比较 Windows 与 macOS 的应用版本、远程访问模式、公网入口、discovery 请求序列及 `/oauth/authorize` 是否到达，不能把两个平台都会返回的响应直接认定为 macOS 根因。
- 内置 OAuth 服务必须继续提供 RFC 8414 风格的 `/.well-known/oauth-authorization-server` 元数据。
- 只有在对照证据表明 ChatGPT 确实因 OIDC discovery 探测终止流程时，才增加 `/.well-known/openid-configuration` 兼容响应。
- 若增加兼容端点，不得把 Serena Desktop 扩展为完整 OpenID Connect Provider；不新增 ID Token、UserInfo、`openid` scope 或 OIDC 身份语义。
- 现有 Dynamic Client Registration、PKCE、Token、refresh token、本机人工审批及 MCP Bearer 保护行为保持不变。
- 授权弹窗的触发条件保持为客户端实际访问 `/oauth/authorize`；仅注册客户端或读取 discovery 元数据不得产生待审批记录或弹窗。
- 兼容逻辑适用于所有使用内置 OAuth 的自托管公网入口，不绑定 ngrok 域名或单一平台。
- 变更保持最小，不引入新依赖、新配置项、fallback 状态机或自动重试。

## Acceptance Criteria

- [ ] 留存 Windows 正常链路与 macOS 异常链路的关键请求序列及运行版本，明确第一个发生差异的边界。
- [ ] 根因能够解释“相同功能在 Windows 正常、macOS 异常”，或明确证明两次测试并非相同代码/配置/ChatGPT 连接状态。
- [ ] 若根因要求增加 OIDC discovery 兼容响应，则 `GET /.well-known/oauth-authorization-server` 与 `GET /.well-known/openid-configuration` 在内置 OAuth 就绪时均返回 `200`，且核心 OAuth 元数据一致；否则不新增该路由。
- [ ] `POST /oauth/register` 成功不会创建 pending authorization，也不会触发本机授权事件。
- [ ] 合法客户端随后访问 `/oauth/authorize` 时会创建 pending authorization，并保持现有 `remote-authorization` 事件与唤醒主窗口行为。
- [ ] 现有 OAuth HTTP 流程测试通过，并新增覆盖 discovery 兼容端点及“注册不弹窗、授权才弹窗”的回归断言。
- [ ] 相关 Rust 格式、定向测试与编译检查通过。

## Notes

- 现场日志显示 ChatGPT 已成功调用 `/oauth/register`，但没有后续 `/oauth/authorize`；因此当时没有本机弹窗是调用链尚未进入授权阶段，不是弹窗监听器本身失效。
- ChatGPT 会额外探测 `/.well-known/openid-configuration`，但当前该路由在 Windows 与 macOS 共用的代码中都不存在；仅凭 macOS 日志中的 `404` 不能证明它是平台差异根因。
- 修改 discovery 元数据后，需要删除并重新创建 ChatGPT 侧连接，避免继续使用旧的缓存元数据。
