# P4-001 Public Usage Domain 与 Validation

## Goal

实现公共 UsageSnapshot 与 UsageCompleteness 严格 JSON validation、单元测试、agent/mod.rs 导出和任务证据；禁止 provider parser、DB、epoch/baseline/store、Product/MCP/UI。

## Requirements

- 以 `docs/technical-design-agent-platform-v0.2.md` §30～§31.1 与
  `docs/implementation-task-breakdown-agent-platform-v0.2-revision003.md` P4-001 为 authority。
- 新增公共 `agent::usage` domain：`UsageSnapshot`、`UsageCompleteness` 与稳定
  `USAGE_EVENT_INVALID` validation entry；不包含 runtime/thread/turn/baseline identity。
- token 和 modelContextWindow 仅接受 `0..=i64::MAX` 整数；revision 仅接受完整 `u64`；
  updatedAt 仅接受完整 `i64`。null/absent 与零必须保持可区分。
- totalTokens 只接收输入 Provider 值，禁止从 breakdown 计算。
- 禁止 Provider parser、DB migration、epoch/baseline/store、Product/MCP/UI 和 commit/push。

## Acceptance Criteria

- [ ] `UsageSnapshot` 以 camelCase 且拒绝未知字段序列化/反序列化。
- [ ] 非法 wire 输入通过公共入口稳定映射为 `USAGE_EVENT_INVALID`。
- [ ] unknown/partial/complete 均可 round-trip，domain 不硬编码 Codex capability。
- [ ] 覆盖 P4-001 指定的 null/zero、total、数值边界、identity、未知字段与 enum 测试矩阵。

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.
