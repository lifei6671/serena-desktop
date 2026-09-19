# P3-001 summaryCode Deterministic Mapping

## Goal

Freeze the pure summaryCode mapping and exhaustive deterministic tests without Store, Observe, MCP, UI, migration, or revision changes.

## Requirements

- 仅在 `src-tauri/src/agent/activity.rs` 实现唯一纯函数
  `derive_summary_code(progress_phase, activity_phase, tool_category)`。
- 输出只能是冻结 allowlist 中的 summaryCode、`None`，或稳定错误码
  `AGENT_ACTIVITY_CONTRACT_ERROR`；不得生成自由文本或 generic code。
- `Finalizing` 与 `Reconciling` 必须先于 Activity 合法性校验返回对应
  `execution.*` code。
- 当 `ProgressPhase` 整理到 activity domain 时，保留既有
  `agent::product::ProgressPhase` 路径与 snake_case 序列化语义。
- 禁止修改 Store、migration、Observe、MCP、UI、execution/activity revision
  或 Progress DTO 字段。
- 以 table-driven 穷举测试覆盖全部 `ProgressPhase × Option<ActivityPhase>
  × Option<ToolCategory>` 组合，并单独证明进度优先级、无 Activity 与非法
  Activity 组合。

## Acceptance Criteria

- [ ] 所有 126 个输入组合均有唯一、冻结的预期结果。
- [ ] `Finalizing`/`Reconciling` 对全部下层合法及非法 Activity 组合返回
      `execution.finalizing`/`execution.reconciling`。
- [ ] `Pending`、`Dispatching`、`Running`、`Terminal` 下无 Activity 返回 `None`。
- [ ] `Provider + category` 与 `Tool + None` 在非覆盖进度返回
      `AGENT_ACTIVITY_CONTRACT_ERROR`。
- [ ] 相关 Rust focused tests、`cargo check --locked`、格式检查与
      `git diff --check` 通过。

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.
