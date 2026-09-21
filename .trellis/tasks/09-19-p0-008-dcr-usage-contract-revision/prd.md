# P0-008-DCR Usage Contract Revision

## Goal

基于已验收的 P0-008 Host Evidence 修订 Codex Usage Contract、Phase 4 设计和任务拆分；仅文档与 DCR，不修改生产代码或测试。

## Requirements

- 只修订 `docs/technical-design-agent-platform-v0.2.md`、`docs/implementation-task-breakdown-agent-platform-v0.2-revision003.md`、P0-008 research DCR 与本 Task 文档；禁止生产、测试、schema、runtime、UI 代码修改，以及 commit/push。
- 以已验收 P0-008 Host Evidence 为唯一 Codex v0.153.4 事实来源：同 Runtime/Thread 累计、restart/resume reset、`last` 的 response-level 候选语义、Provider-supplied total、`account/usage/read` 的 `threadUsage=null`、terminal/grace 的非 finality，以及 P0-008B multi-response 的 INCONCLUSIVE。
- 将 Codex cumulative authority 收窄为 `CodexUsageEpoch=(provider_id='codex', runtime_instance_id, thread_id)`；禁止跨 epoch subtract，Codex v0.153.4 不提供 sync usage checkpoint，也不能生成 `complete`。
- 选择证据最强的最小参与计数合同：仅 Provider-supplied `totalTokens` 参与 Codex Execution delta；breakdown 均为独立 optional，`cacheWriteInputTokens` 缺失保持 null，`modelContextWindow` 仅 metadata/display。
- 同步 P4-001..P4-007 的依赖、blocked-by、实现边界和 Gate 矩阵；仅解除 DCR evidence blocker，不虚构 Codex `complete` 或 response aggregation 证据。

## Acceptance Criteria

- [ ] 技术设计 §30～§38、Phase 4 Gate 与任务拆分不再声称 Codex sync checkpoint、source-complete fallback、跨 Runtime subtraction 或 grace => complete。
- [ ] 新增 append-only `research/usage-contract-dcr.md`，记录 Evidence、Decision、Rejected Alternatives、Migration/Task impact，并保留对冻结输入 SHA 的引用。
- [ ] P4-001..P4-007 的新链路可由 DCR 直接追溯，且明确哪些 Codex 语义仍为 DEFERRED。
- [ ] 工作区变更仅限获准的文档与单一 Trellis DCR Task；未执行 production/test/schema/runtime/UI 修改、commit 或 push。

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.
