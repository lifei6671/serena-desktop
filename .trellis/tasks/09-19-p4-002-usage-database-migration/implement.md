# P4-002 实施计划

1. 核对 P4-001 `UsageSnapshot`、现有 v1–v8 migration 与 store 测试夹具。
2. 新建 `schema_v9.sql`，仅定义三张已批准的 Usage 表和约束。
3. 在 `store.rs` 注册 v9、扩展版本允许范围，并增加无写入副作用的读模型。
4. 添加/扩展 store migration tests 覆盖 task card 的 14 项矩阵。
5. 执行指定 focused tests 与静态验证，记录命令和结果到 `evidence.md`。
