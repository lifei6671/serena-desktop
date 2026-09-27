# CB3-005 Implementation Plan

1. 保护当前 CB3-004 后 dirty baseline，读取设计 §10、CB-003 Gate、Continue/Resume/Cancel 当前 Product/TaskManager/Store/Provider 路径。
2. 盘点当前 continuation create transaction 是否已显式继承 source provider/taskRole；若缺失，最小修正，不读取 current Role Routing。
3. 校准 Continue admission：registered / enabled / health / canContinue / validate_continuation，全失败发生在 child create 前。
4. 校准 ResumePending：只读取 persisted provider，保留原 taskRole/Claim/dispatch/runtime evidence；disabled/unavailable fail closed。
5. 确认 Cancel 继续 registration-only，防止 CB3-004 Start admission helper 泄漏到 Cancel。
6. Product/MCP contract：
   - ExecutionView frozen provider/taskRole；
   - Providers query current roleRouting；
   - Continue/Cancel/Resume schema strict；
   - capability projection 不超前。
7. 补完整 MCP contract matrix + Agent focused tests：
   - explicit/legacy Start 回归；
   - half field；
   - general unconfigured；
   - frozen route vs changed current policy；
   - Continue inheritance；
   - disabled/unavailable/capability false；
   - ResumePending immutable evidence；
   - re-enable resume；
   - Cancel registration-only；
   - providers query recovery info。
8. 运行完整 Agent contract / Product / TaskManager / Store / MCP regressions；保持 CB3-004 race tests 绿色。
9. cargo check --locked、fmt、git diff --check；Clippy 仅记录既有 usage_tests.rs blocker。
10. 冻结交付 target，等待 Host Gate；不进入 Phase 4。

## Rollback Point

如果 Continue/Resume 必须重新解析 Role Routing 或创建替代 Start 才能工作，停止并返回 DESIGN_BLOCKER；不得弱化 frozen execution identity。

## Execution record

- [x] 当前 Codex session 已 start 既有 task；保留前序 dirty baseline。
- [x] 修复 Store lineage provider 硬编码，比较已冻结请求 provider，跨 provider 仍拒绝。
- [x] 新增六项公开 MCP Continue/Resume/Cancel 契约测试；生产 Start/Resume/Cancel authority 不变。
- [x] focused 6、Start routing 14（原 8 + 新 6）、Product 132、TaskManager 47、Store 124、Provider 29、Execution 13、MCP 309 全通过；ignored 数见 delivery.md。
- [x] cargo check / fmt / git diff --check PASS；Clippy 仅记录既有 usage_tests.rs await_holding_lock。
- [x] 冻结三文件 manifest/diff、MCP contract/state matrix、精确命令及日志。
- [x] 独立 CHILD_AGENT 审查 PASSED / COMPLETE / FRESH，3/3 覆盖，无 P0–P3；审查后复核 0 hash mismatch。
- [ ] Host 独立 Gate（不由实现者代为通过；不进入 Phase 4）。
