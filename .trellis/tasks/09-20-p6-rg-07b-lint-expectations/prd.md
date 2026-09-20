# P6-RG-07B 局部 lint expectation

## Goal

仅以局部带 reason 的 Rust expect 保留四项已冻结契约 lint，完成聚焦验证与只读交付复审。

## Requirements

- 仅处理 `ProviderErrorCode` 的 `clippy::enum_variant_names`、`SourceWriteTool` 及其 `ALL`/`code` 的 `dead_code`、以及 `SupervisorState::replace_workspaces` 的 `dead_code`。
- 四项均使用最小局部 `#[expect(..., reason = "...")]`；reason 说明冻结 wire/domain 契约或测试回归保留原因。
- 不改全局 lint 配置，不重命名或删除既有项，不注册或路由 Source Write，不改调用方，不处理其他 lint。
- 不 commit、不 push；保留现有工作树中所有非本任务改动。

## Acceptance Criteria

- [ ] 四个指定 lint 不再阻断严格 Clippy，且仅有局部带 reason 的 expectation 改动。
- [ ] Provider 五个稳定错误码、六个未公开 Source Write identity 与 Source Write 不 advertise/unroutable 回归保持不变。
- [ ] `replace_workspaces` 的四个 Source Write workspace-drift 回归调用保持可编译和可执行。
- [ ] 指定 focused/contract tests、fmt、check、strict Clippy 与 `git diff --check` 已如实记录。
- [ ] 已完成 P0/P1/P2 只读交付复审；不包含提交或推送。

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.
