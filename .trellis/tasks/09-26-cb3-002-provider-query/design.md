# Design
权威：冻结任务卡 CB3-002 与设计 §7/7.1/26（已核验两份 SHA256）。MCP AgentQuery 新增空 struct variant Providers {} 确保 serde deny_unknown_fields 生效；输出 QueryData 直接包含 Product ProviderCatalogSnapshot，不复制 DTO。router 只读调用 provider_catalog(supervisor)，保留现有 query envelope 与安全 error 投影。保留既有 global agentEnabled 的 Remote MCP gate，不扩大 disabled 工具公开范围；availability 由 Product 计算。若 Product DTO 缺少 JsonSchema，仅追加必要 derive，保持现有序列化/业务实现。

不经过 workspace resolution/binding、start dispatch 或 health probe。schema/descriptor/hash 依照现有测试机制同步。真实 transport fixture 验证 strict input、成功及失败无副作用和现有 action 回归。

