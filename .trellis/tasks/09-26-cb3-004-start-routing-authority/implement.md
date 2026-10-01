# CB3-004 Implementation Plan

1. 保护当前 dirty baseline，读取 CB3-004 卡、设计 §8～§9、CB3-003 typed StartRoutingIntent、现有 Product work_adapter / TaskManager / Store create transaction。
2. 找出当前 Start 从 MCP DTO 到 Product create/execute 的唯一 Authority 路径和已有 lock/lease 边界。
3. 实现纯 routing resolver：
   - Explicit 精确校验 role route；
   - LegacyGeneral 读取 general route；
   - 返回冻结的 AgentTaskRole + ProviderId 或稳定 routing error。
4. 把 resolver 接入真正 Execution 创建前，随后复用现有 Provider admission（registered → enabled → health → capability）。
5. 确保 create input / request-v3 identity / insert transaction 使用最终 provider + taskRole，且 retry conflict 对 provider/taskRole 敏感。
6. 补 focused tests：
   - explicit happy path；
   - legacy general；
   - unconfigured；
   - mismatch；
   - disabled/unavailable/capability；
   - same requestKey retry / provider-role conflict；
   - policy changed before create；
   - provider disabled after query；
   - created execution routing frozen；
   - no Execution/Claim on rejection。
7. 运行相关 Product/TaskManager/Store/MCP regression、cargo check、fmt、git diff --check；完整 mcp:: 应保持绿色。
8. Clippy 仅记录已知 task 外 blocker，不扩大范围。
9. 冻结 diff 和 routing matrix，等待 Host review；不进入 CB3-005。

## Rollback Point

若实现需要自动 fallback、Prompt 分类、修改 Local Human policy、弱化 request identity/Claim fail-closed，立即停止并返回 DESIGN_BLOCKER。

## Execution status

- [x] 已按 HOST_RESOLUTION.md 复用 management → operation 完成最终 Authority 到创建的接线。
- [x] 已覆盖 explicit/legacy、错误优先级、retry identity、无副作用拒绝和真实锁交错。
- [x] Windows scoped regression、完整 MCP、check、fmt、diff check 通过。
- [x] Clippy 仅记录既知 usage_tests.rs await_holding_lock；未修任务外问题。
- [x] 已冻结 implementation.diff / implementation-manifest.json 并记录 delivery.md。
- [ ] Host Gate（主会话独立执行；本实现者不代为通过）。

## Host fixture follow-up

- [x] 用纯 cfg(test) fake 替换依赖真实 Codex 的拒绝 fixture；保留原三项 MCP 断言。
- [x] 新增 acceptance 未调用、Runtime/attempt 行数为零的回归。
- [x] A 三项、B routing 8、C MCP 303/0/4、D Product 132/0/5 及 E/F/G 完成本轮重新验证；H 仅旧 Clippy 问题。
- [x] 更新累计 manifest/diff 和 repair-only baseline/manifest/diff；生产 routing/锁/dispatch 不变。
- [ ] 修复后的 Host Gate（不由实现者代为通过）。
