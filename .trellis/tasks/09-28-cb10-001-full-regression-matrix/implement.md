# Execution Plan

1. 冻结 branch、HEAD、worktree、authority hashes、Windows host 与工具版本。
2. 盘点仓库 package manager、frontend scripts、Rust/MCP/migration/safety test inventory。
3. 执行 Git/format/static checks。
4. 串行执行 Rust full lib tests，并按要求用模块矩阵补足/审计覆盖。
5. 执行 frontend unit/lint/build 与完整 script tests。
6. 盘点并执行 MCP schema/contract 官方入口。
7. 汇总实际测试数、ignored/smoke、失败分类、19 条安全不变量证据。
8. 执行 release hygiene、authority hash 和 final worktree 检查，给出 PASS/FAIL；停在 CB10-001。
9. Host 批准后，在同一 Work 修复 CB6-005 provider selection ordering，新增双向 skip、自身分类、双 scoped dangling fail-closed 测试。
10. 重跑原失败、focused recovery groups、完整 Rust、frontend、release scripts、MCP 与 static gates。
11. 冻结完整 CB10 delivery target，执行独立只读 FULL_SCOPE review；P0/P1/P2 修复后重审。
