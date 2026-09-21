# P2A2-008 Continue Workspace Inheritance

## Goal

Continue creates a child Execution that inherits only the persisted parent Workspace snapshot and rejects alternate Workspace authorities.

## Requirements

- Continue 必须创建新的 child Execution，terminal parent 保持 absorbing；child 的 `workspace_id`、`canonical_workspace_root` 与 `workspace_generation` 只能继承 persisted parent Execution snapshot。
- Remote `agent_execute` Continue 与 Local compatibility Continue 均拒绝任何附带的 workspace authority 字段；query/cancel/continue 不重新解析当前 Registry、Desktop selection 或 Global ActiveWorkspace。
- 父快照不完整或现有 continuation eligibility 不满足时，创建 child、Claim、Runtime attempt 和 Provider dispatch 前以既有 `AGENT_CONTINUE_NOT_ALLOWED` fail closed。
- WorkRun membership 和 Work Workspace identity 继续约束 Continue；相同 requestKey 必须返回既有 child，不重复 dispatch。
- 本任务不改 Thread/historyMode、Usage baseline、Runtime Evidence、Claim release、Recovery、Cancellation 或后续 P2A2 阶段；不提交 Git。

## Acceptance Criteria

- [ ] 父 A 在 Desktop/Global 选中 B 后 Continue 仍从 parent A snapshot 创建 child，并保持 parent 不变。
- [ ] Remote 和 Local Continue 的恶意 workspace 字段均被 DTO/schema 拒绝。
- [ ] 重启后 Continue 从 persisted parent snapshot 恢复同一 Workspace identity。
- [ ] 不完整 parent identity 在任何 creation/claim/dispatch 前 fail closed。
- [ ] Work membership/identity、requestKey replay 和 Claim 的既有回归均有定向测试证据。

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.
