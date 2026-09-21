# Phase 5 Agent detail display batch (P5-001 to P5-003)

## Goal

实现 P5-001 Provider、P5-002 Activity 和 P5-003 Token Usage 的前端详情页展示，后端冻结。

## Requirements

- 仅修改前端 `types`、presentation、`ExecutionDetails`、相关测试及必要局部样式；不得修改 Runtime、DB、MCP contract 或后端 Product。
- 将 `ExecutionView` 同步为冻结 Product DTO：provider、providerSessionLabel、required usage，以及 progress.summaryCode；保留已有 wakeReason/mismatchKind 等兼容字段（如有）。
- P5-001：详情页所有 Provider 名称从 DTO 展示，包含安全 fallback、可选版本、可选 session label 与 continuation footer；不得硬编码 Codex。
- P5-002：当前活动优先映射 progress.summaryCode；silence 只作中性显示，不能推断失败、卡住或异常。
- P5-003：详情页增加 Token 用量展示；total 只读 `usage.totalTokens`，不能由 breakdown 重新求和。

## Acceptance Criteria

- [ ] P5-001：known、fallback、缺少 version、session label 与历史 task 均能安全展示动态 Provider。
- [ ] P5-002：全部已知 summaryCode、unknown/null fallback、age bucket 与 silenceLevel 均有单元/渲染覆盖，文案不包含停滞或失败推断。
- [ ] P5-003：unknown/partial/complete、0、大数、null breakdown/context 均可表达，且 total 不会由 breakdown 推导。
- [ ] 全部受影响 fixture 补齐 required 字段且没有以 `as any` 逃逸核心字段。
- [ ] `npm test`、`npm run lint`、`npm run build` 与 `git diff --check` 通过；完成独立只读 review，P0/P1 为 0。
- [ ] 不进入 P5-004，不 commit 或 push，不修改 P4 evidence。

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.
