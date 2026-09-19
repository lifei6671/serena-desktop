# 实施计划

1. 同步 TypeScript `ExecutionView` 并定位所有前端 fixture，补齐 DTO required 字段。
2. 在 `agentPresentation.ts` 提供 Provider、Activity、silence、Token formatter 的 display-only helpers 与单元测试。
3. 将 `ExecutionDetails` 连接动态 provider、活动和 Token 用量，并最小化扩展局部样式。
4. 扩展已有 jsdom 组件测试，执行要求的全量验证与独立只读 review。
