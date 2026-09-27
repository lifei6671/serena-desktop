# Design
Registry 提供注册、descriptor/version、capabilities、health；当前 Supervisor Local Human config 快照提供 agentEnabled、providers enabled 与 roleRouting。优先 Product 方法接受现有 Supervisor 引用读取 current config，复用 manager.registry()，避免扩展 policy authority。只枚举 Registry，未配置 enabled 为 false 与 admission 一致；未知 route 原样保留。
新 Product DTO 使用 camelCase；不接 Action/MCP，不修改 Execution ProviderProduct。能力逐项映射，无 provider-specific 分支。若范围内接口无法满足返回 DESIGN_BLOCKER。
所有新函数与核心逻辑中文注释。复用现有测试工具与 Store/Runtime 观察方法，测试实际 Local Policy 更新后查询可见。
