# P4-005 Terminal Grace Freeze Late Events

## Goal

Implement Codex Usage telemetry terminal grace, late-event drain, and runtime teardown freeze without changing Product MCP UI or Codex completeness.

## Requirements

- 仅实现 Codex Usage telemetry `accepting -> terminal_grace -> frozen` 生命周期、terminal 后有界 late Usage drain，以及 runtime termination 同事务 freeze。
- `USAGE_TERMINAL_GRACE_MS = 2000`；Codex 0.153.4 在 terminal、grace、freeze、account/usage/read 均不得产生 `complete`。
- Provider terminal、recover、cleanup、`WorkspaceExecutionCoordinator.finish` 和 Claim release 必须先完成；仅其后在同一 runtime lease 中 drain exact runtime/thread/turn Usage 到原 deadline。
- grace 内仅 telemetry 可更新；不得改 terminal、Claim 或 release evidence。错误必须 best-effort，不能升级为 provider failure。
- 不修改 Product、MCP、UI，不进入 P4-006，不提交或推送。

## Acceptance Criteria

- [ ] Store grace/freeze API 的 identity、幂等、deadline、frozen no-op 和 public Usage revision/completeness 不变量均有确定性测试。
- [ ] runtime termination evidence 的 SQLite 事务冻结同 runtime 的 accepting/grace state，且不改 public usage row。
- [ ] provider 在 `finish` 后才 bounded-drain exact Usage；drain 中 Execution 已 terminal、workspace Claim 已释放。
- [ ] +1s exact Usage 被消费；逾期、frozen、wrong thread/turn 与非-Usage late notification 均不反向影响终态 Execution。
- [ ] focused suites、`cargo test --locked agent::store::tests --lib`、`cargo check --locked`、fmt 和 diff check 均有诚实结果；独立只读 review P0/P1=0。

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.
