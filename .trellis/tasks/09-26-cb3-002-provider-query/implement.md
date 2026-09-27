# Implementation
1. 读取 Product DTO/provider_catalog、MCP DTO/router/schema/descriptor/hash 与 Remote registry/transport tests，按当前实际代码完成最小接线。
2. 增加严格 schema/DTO、路由、真实 call_tool、mutation registry 和成功/失败状态不变测试及 JSON evidence。
3. Native Windows PowerShell 执行相关 Rust tests、cargo check、fmt --check、clippy 和 git diff --check；Linux 仅可用项目 Docker runner，禁止 WSL。无需 Linux gate。
4. 冻结本卡代码 hash 与 evidence，独立 check 审阅，仅报告本卡 diff，Host Gate pending。

授权：用户已明确要求实现并允许必要 Trellis artifacts；不再询问计划批准。已有改动的 hash 记录在 baseline-hashes.json，必须完整保留。
