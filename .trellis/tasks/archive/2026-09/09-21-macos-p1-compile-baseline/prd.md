# Phase 1：macOS 可编译基线

## Goal

解除跨平台产品逻辑与 Windows Runtime 的错误耦合，使 macOS arm64 通过相关前端和 Rust 质量 Gate，但不声称 Agent Runtime 可用。

## Requirements

- 依赖：父任务 Phase 0 决策已完成；无其他实施子任务依赖。
- 将 `product`、`work`、`task_manager` 保持为跨平台业务模块，将 Codex 发现、launcher、runtime 和 recovery observation 收敛到明确的平台边界。
- 保留 Windows 现有实现、错误码和测试边界，不在本任务中实现 macOS Agent Runtime。
- 修正 Unix 构建所需的 `ErrorKind` 导入，并为尚未实现的 macOS Runtime 能力提供显式、稳定的 unavailable 结果。
- 增加平台选择的编译覆盖，将纯业务逻辑测试移出不必要的 Windows 条件编译。

## Acceptance Criteria

- [x] macOS arm64 完成前端 lint/build/test 和 Rust fmt/check/clippy/test Gate。
- [x] Rust 构建不再因 Windows 模块整体裁剪或 Windows 专属符号无条件导入而失败。
- [x] Windows 专属实现及其有效测试仍保留原有语义。
- [x] macOS 尚未实现的 Runtime 能力明确返回 unavailable，不静默 fallback。
- [x] 任务结论明确说明“可编译基线”不等于 Agent Runtime 已可用。

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.
