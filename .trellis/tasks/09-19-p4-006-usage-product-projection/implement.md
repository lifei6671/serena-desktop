# P4-006 执行计划

1. 审计 `UsageSnapshot`、Product DTO/projection、`product_read` 和相关测试/descriptor hash。
2. 最小新增 typed Usage Product DTO；确保 enum schema 严格、无行 shape 稳定且 revision hash 隔离。
3. 在 `product_read` 首 query LEFT JOIN `execution_usage`，先 validation 再 provider identity check，将 `Option<UsageSnapshot>` 写入 ProductSnapshot。
4. 让 detail/list/history 通过既有 views 路径投影 `usage`，添加聚焦 regression tests 与必要 schema hash 更新。
5. 运行规定的 focused tests、受影响既有 tests、Cargo/fmt/diff checks；记录真实 totals。
6. 冻结最终 target，进行独立只读 review；只在 P0/P1=0 后写 `evidence.md`。不 commit/push，且不进入 P4-007。
