# Cloudflare MCP 同步失败对照证据

## 结论

`WinSerena` 同步失败不是 Windows Serena MCP 协议或工具列表差异，而是 Cloudflare MCP 服务器记录缺少访问上游 `serena.disign.me` 所需的 Access Service Auth 标头。

## 现场对照

| MCP 服务器记录 | HTTP URL | 上游身份验证 | 状态 | 工具数 |
| --- | --- | --- | --- | --- |
| WinSerena | `https://serena.disign.me/mcp` | 无 | 错误 | 0 |
| Serena Upstream | `https://serena.disign.me/mcp` | 基于标头 | 就绪 | 27 |
| MacSerena | `https://mac.disign.me/mcp` | 无 | 就绪 | 17 |

Cloudflare Access 应用列表还显示：

- `serena.disign.me` 被名为 `ChatGPT Serena` 的自托管 Access 应用保护，目标路径为空，因此覆盖整个主机。
- 该应用使用 `Service Auth` 策略。
- `mac.disign.me` 没有对应的自托管 Access 应用。

未携带标头直接请求 `https://serena.disign.me/mcp` 时，Cloudflare 返回 Access HTML `403`，响应包含 `cf-access-domain` 和 `cf-access-aud`；请求在到达 Serena Desktop 前已经被拒绝。

## 影响

Cloudflare MCP Portal 将 `WinSerena` 配置为“不需要验证”后，同步器不会发送 `CF-Access-Client-Id` / `CF-Access-Client-Secret`，因此无法完成 MCP `initialize` 和工具同步。现有 `Serena Upstream` 已证明相同 URL 在携带正确标头时可以正常同步和调用。

## 最小处理方向

优先复用已经就绪的 `Serena Upstream`。如果必须保留独立的 `WinSerena` 记录，则需要把它改为与 `Serena Upstream` 相同的基于标头的上游身份验证，再重新同步能力；不需要修改 Serena Desktop 代码。
