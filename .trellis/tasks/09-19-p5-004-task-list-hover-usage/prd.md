# P5-004 Task List Hover Usage

## Goal

在任务列表及 Hover 中仅使用 summary DTO 的 providerId、usageTotalTokens 和 usageCompleteness 展示 Provider 与 Total Token，禁止详情请求和 N+1 查询。

## Requirements

- 任务列表与任务 Hover 展示 Provider 和 Total Token；数据只可直接来自列表/summary DTO 的 `providerId`、`usageTotalTokens`、`usageCompleteness`。
- Provider 优先复用 Phase 5 已有的公共展示逻辑；列表仅有 `providerId` 时可使用该逻辑的安全 fallback，不得引入 Codex 私有语义。
- Total 只读取 `usageTotalTokens`；不得从任何 usage breakdown 推导。
- `usageTotalTokens === null` 或 `usageCompleteness === "unknown"` 显示统一未知态 `—`；真实 `0` 显示 `0`；`partial` 显示 Total 并加“统计不完整”；`complete` 正常显示。
- 历史任务缺 Usage 时保持兼容，不报错、不伪造零。
- 禁止为列表/Hover 请求 Execution Detail，禁止新增 N+1；不得修改 history API、后端 Usage、Provider Runtime/Activity/Workspace/Claim 契约，也不得进入 P5-005。

## Acceptance Criteria

- [ ] 列表与 Hover 均从既有 summary row 呈现 Provider 和 Total Token。
- [ ] unknown、partial、complete、真实 0 与历史无 Usage 均有前端测试。
- [ ] 前端请求测试证明 Hover/分页不会触发 Execution Detail 或 N+1 请求。
- [ ] `npm test`、`npm run lint`、`npm run build`、`git diff --check` 通过。
- [ ] 已完成独立只读 Review；P0/P1 均已修复并复审。

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.
