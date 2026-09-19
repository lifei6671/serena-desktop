# P2D-009 执行计划

1. 记录 Git baseline，并用两个 focused lib tests 复现和分类既有失败。
2. 审计 P2D CodeGraph Provider、MCP registry/server/adapter 和现有 P2D tests，确认 Remote guard、schema、Lease routing 与 compatibility mapper 的最小改动。
3. 实现 Remote cutover，并以 focused tests 验证 tools/list、missing/unknown/valid `workspaceId`、旧 global route negative、Source Write disabled。
4. 补强/新增 P2D integration gate，覆盖 A/B、readiness、capacity/LRU/BUSY/idle/crash、health/remove/shutdown 与 third-provider/Core no-branch。
5. 依次执行 focused Rust、workspace/codegraph/registry/MCP suites、前端 test/lint/build、Rust check/fmt/diff，最后完整 lib suite 并重新分类已知失败。
6. 冻结 delivery-owned diff，进行只读 full-scope self-review（本会话无可用的独立 reviewer），修复发现后重新验证并复审；不 commit/push，不进入 Phase 3。
