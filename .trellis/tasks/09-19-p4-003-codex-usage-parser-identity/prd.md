# P4-003 Codex Usage Event Parser 与 Identity Binding

## Goal

Implement pinned Codex 0.153.4 thread/tokenUsage/updated parsing, strict validation, exact execution/runtime/thread/turn binding, and provider-agnostic Usage telemetry publication. Excludes schema/store/delta/lifecycle/Product/MCP/UI, commits, and push.

## Requirements

- 仅识别 pinned Codex 0.153.4 `thread/tokenUsage/updated` notification。其固定 JSON 形状为 `threadId`、`turnId` 与包含 `total`、`last`、`modelContextWindow` 的 `tokenUsage`。
- `total.totalTokens` 为 required provider-supplied cumulative total；所有 token 数字与 `modelContextWindow` 只接受 `0..=i64::MAX` JSON integer。`cacheWriteInputTokens` 缺失保持 `None`，显式 `0` 保持 `Some(0)`；nullable `modelContextWindow` 映射为 `None`。
- `last` 必须按同一 fixed schema 严格验证，但不得进入 Provider-Agnostic event 或任何 Public Usage accounting；`rawResponse/completed` 不进入生产路径。
- 已识别但 schema/numeric 无效的 Usage notification 必须返回稳定 `USAGE_EVENT_INVALID`，不得降级为 `Notification::Other`。不相关的未知 notification 保持 `Other`。
- Usage event 只能在 Codex adapter 以现有 Activity 模式完成 execution/runtime/root-thread/row-thread/row-turn 精确绑定后发布；身份不匹配 fail-closed 且沿用现有 observability drop 行为。
- adapter 无状态：允许 duplicate 与 identity 合法但数值倒退的 cumulative notification 原样重复发布；不做 Store 写入、baseline、delta、epoch、lifecycle、terminal 或 Public Usage 投影。
- 不修改 schema_v9、Store、Product、MCP、UI；不 commit、不 push。

## Acceptance Criteria

- [ ] protocol 将合法 P0-008 fixed fixture 映射为 typed Usage notification，保留 private `last` 并正确保留 total 与 `modelContextWindow=258400`。
- [ ] protocol 覆盖 absent/zero cache-write、null context window、缺失 total、坏 object、空 identity、float/negative/string/overflow，且无效输入统一为 `USAGE_EVENT_INVALID`。
- [ ] Provider-Agnostic `UsageEvent` 无 thread、turn、runtime、last 或 raw payload，仅暴露安全 total/breakdown/context/observed-at 与 execution/provider identity。
- [ ] exact execution/runtime/thread/turn 绑定才发布 Usage；wrong thread/turn/runtime/unbound 不发布，duplicate 和 out-of-order total 仍原样发布。
- [ ] publish 失败仅作为 observability failure，不改变 provider Execution result、Claim 或 terminal。
- [ ] 运行约定的 focused protocol、adapter、telemetry、cargo check、fmt、diff-check，并把实际结果写入 `evidence.md`。

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.
