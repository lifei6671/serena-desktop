# P4-006 Usage Product Projection

## Goal

Project persisted public execution_usage into Product ExecutionView usage for detail, list, and history with stable no-row defaults, fail-closed identity validation, and no added Usage N+1 queries.

## Requirements

- 在 `ExecutionView` 增加稳定存在的公共 `usage` 对象；字段使用 camelCase，包含七个 nullable token/context 数值、严格 enum completeness、`usageRevision` 与 nullable `updatedAt`。
- `execution_usage` 无行时，返回全 nullable 数值、`unknown`、revision `0` 与 `updatedAt: null`；不省略对象且不伪造零值。
- 有行时逐字段投影既有已验证 `UsageSnapshot`：保留 null/零区别、总量只用 persisted `total_tokens`、时间与 revision 只用 Usage 行。
- provider identity 必须与 Execution provider 相同；不相同或 Usage 行损坏即 Product read fail-closed。不得读取任何 Codex private usage state。
- detail、`Action::List` 与 `history_page` 使用相同 Product usage 投影；Usage 更新不得影响 control/revision legacy alias 或 activity revision。
- `product_read` 通过初始 execution list SQL 的 LEFT JOIN 取得 Usage，不在 Product `views()` 循环做每 execution Usage 查询。本任务不重构其他既存查询。
- 允许为 `ExecutionView` schema 变化更新受影响 MCP descriptor/hash fixtures；不添加 MCP action/parameter/行为，不做 UI、Usage 写入/重算或 P4-007 Gate。

## Acceptance Criteria

- [ ] DTO 能 schema/serde 表达 `unknown|partial|complete`，无 private usage identity。
- [ ] 无行、有行、null/zero、persisted complete、total 不求和、完整字段映射均由 Product tests 覆盖。
- [ ] provider mismatch 与 raw SQL 损坏值 fail-closed；list 至少多行无错配，history 正确。
- [ ] Usage 变动的 revision 隔离、camelCase 序列化和 Usage 无新增 N+1 有证据。
- [ ] 聚焦 Product/store/MCP schema（若受影响）测试、`cargo check --locked`、fmt check、diff check 完成；独立只读 review P0/P1=0。

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.
