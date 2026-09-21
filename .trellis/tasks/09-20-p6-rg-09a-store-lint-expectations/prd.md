# P6-RG-09A 两项稳定存储边界 lint expectation

## Goal

仅为 Work 创建持久化边界与 continuation 原子事务添加局部 clippy 参数数量 expectation。

## Requirements

- 仅处理 `src-tauri/src/agent/store/work_runs.rs` 中 `StateStore::create_work_run` 与 `src-tauri/src/agent/store/transactions/product.rs` 中 `product_create_continuation_with_work` 的 `clippy::too_many_arguments`。
- 两处仅添加局部 `#[expect(clippy::too_many_arguments, reason = "...")]`；reason 必须说明其稳定事务/API 边界，以及避免只为单项 lint 而改动大量调用面。
- 不修改参数、调用点、事务顺序、SQL、状态机语义、全局 lint 配置或其他 lint；不重新设计 DTO。
- 保留所有既有非本任务工作区改动；不 commit、不 push。
- 验证相关 Store/Work/Continuation focused tests、`cargo fmt --all -- --check`、`cargo check --locked`、`cargo clippy --locked --all-targets -- -D warnings`、`git diff --check`；记录严格 Clippy 的新首批剩余诊断，并做独立只读 P0/P1/P2 评审。

## Acceptance Criteria

- [ ] 两个指定函数各有且仅有一个带准确 reason 的局部 `clippy::too_many_arguments` expectation。
- [ ] 参数、调用点、事务顺序、SQL 与状态机语义未变，且不存在全局 lint 配置或其他 lint 改动。
- [ ] 相关 focused tests、格式、locked check、严格 Clippy 和 diff 检查已如实记录；两条目标诊断不再出现，且已记录新首批剩余诊断。
- [ ] 已完成独立只读 P0/P1/P2 交付评审；不包含提交或推送。

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.
