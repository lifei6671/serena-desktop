# Agent terminal notification title

## Goal

Use persisted task title semantics in Agent terminal desktop notifications.

## Requirements

- Agent 终态桌面通知正文必须使用任务标题，不再显示 Execution 短 ID。
- 标题语义与前端 `taskTitle` 一致：优先持久化 `thread_name` 的 trim 值；否则折叠 prompt 空白、trim、按 Unicode 字符截断为 100 个字符并追加省略号；仍为空则为“未命名任务”。
- 任务管理层从既有 `ProductSnapshot` 取标题数据并传入 `AgentTerminalNotifier`；Desktop 实现不得自行查询数据库，也不得新增持久化字段。
- 保持 Completed、Failed、Interrupted、Cancelled、能力开关、提示音、Host 生命周期去重，以及通知失败不改变 Agent 终态的既有语义。

## Acceptance Criteria

- [ ] 持久化 thread_name 优先且已 trim 后进入通知正文。
- [ ] prompt fallback 的空白折叠、100 Unicode 字符截断、空值 fallback 均有单元测试。
- [ ] 终态策略、去重与能力开关行为不回归。
- [ ] 聚焦 Rust 测试和 `git diff --check` 通过。

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.
