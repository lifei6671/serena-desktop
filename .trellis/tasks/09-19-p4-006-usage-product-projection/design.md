# P4-006 设计：Usage Product Projection

## 边界与数据流

`execution_usage`（P4-002 持久化）→ P4-001 `UsageSnapshot` validation → `ProductSnapshot.usage` → `ExecutionView.usage` → detail/list/history 的既有 DTO 和 schema。

Product 只读公共 Usage；绝不访问 `codex_thread_usage_epochs` 或 `codex_execution_usage_state`。`UsageProduct` 复用 P4-001 `UsageCompleteness` 的 typed enum，并以最小 schema adapter 维持 JSON enum 约束。`UsageProduct::unknown()` 规定无行的稳定形状，`From<&UsageSnapshot>` 只字段拷贝，绝不推导 total 或 completeness。

## Store 读取

`StateStore::product_read` 的初始 query 对 `execution_usage` 做 `LEFT JOIN`，select 同一行的 Usage 列。NULL join identity 表示无 Usage；否则通过既有 P4-001 validation 构造 `UsageSnapshot`。任何无效 Usage 数据或 provider/execution identity 不一致使整次 Product read 返回错误。随后现有 ProductSnapshot 带该 `Option<UsageSnapshot>` 进入 views；不在 views 中调用 `execution_usage()`。

## Revision 与兼容性

`usage` 不参与 control revision 或 activity revision hash；新鲜度只由 `usage.usageRevision` 表达。ExecutionView 是 MCP/Tauri 共用 response，所以保留既有 MCP action/参数，仅按现有 schema hash tests 更新 expected 值（若确有变更）。

## 测试策略

使用现有 Product/store test helpers 写入公共 Usage fixture 与 raw SQL 负例。覆盖无行、所有 completeness、null/zero、完整映射、不求和、identity/validation失败、多行 list、history、revision隔离、serde 和无 private 字段；另以 `product_read` LEFT JOIN 与 ProductSnapshot 已携带 usage 的测试作为无 Usage N+1 结构性证据。
