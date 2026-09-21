# 修复 Source Replace Content 参数契约

## Goal

修复 source_replace_content 的 mode=first 对可选 maxReplacements=1 的运行时校验，使其与公开 Schema 一致并保持既有替换、OCC 与 newline 契约。

## Requirements

- 仅修复 `source_replace_content` 的 `mode=first` 参数校验：省略 `maxReplacements` 或显式传入 `1` 均合法，显式大于 `1` 必须返回 `SOURCE_INVALID_ARGUMENT`。
- `mode=first` 始终最多替换一处；`expectedMatches` 如提供，继续表示修改前全文 literal 匹配数而非替换数量。
- 保持 `mode=all` 的必填上限、全文匹配数和超过上限时 `SOURCE_CONTENT_AMBIGUOUS` 的现有语义。
- 不修改 literal 匹配、OCC/`expectedSha256`、newline、原子替换或 Workspace 边界。

## Acceptance Criteria

- [ ] first 无 `maxReplacements`、first `maxReplacements=1`、first 大于 `1` 参数拒绝均有聚焦测试。
- [ ] first 多匹配且 `expectedMatches` 大于 `1` 时只替换首处；all 的既有行为回归测试保持通过。
- [ ] 聚焦测试、格式检查、`cargo check` 和 `git diff --check` 的执行结果已如实记录。

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.
