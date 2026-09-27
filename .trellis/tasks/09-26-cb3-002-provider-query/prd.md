# CB3-002
仅实现 agent_query(action=providers)，只接受 action 字段，无 workspace 依赖。直接复用 CB3-001 ProviderCatalogSnapshot；只读查询 Provider 与 Role Routing。成功/失败均不创建 Runtime/ACP/Session/Execution/Claim、不刷新 health、不修改 policy/Binding。保留所有既有未提交改动。不改 Start DTO/routing、UI、CodeBuddy runtime，不提交。Host 独立验收。

验收：DTO/schema strict matrix、descriptor/hash gate、router dispatch、真实 MCP call_tool 返回 Product JSON、额外字段 INVALID_PARAMS、完整无副作用断言、Remote registry 无 Provider mutation、get/list/observe 回归。记录 input/output JSON、schema/hash、命令/测试名/数量。执行 focused tests、相关 MCP tests、cargo check/fmt/diff check；已知 usage_tests.rs Clippy blocker 不修复。
