# P4-002 设计

## 迁移边界

沿用 `StateStore::migrate` 的 `Immediate` transaction 与逐版本
`apply_migration`。v9 是仅扩展型 migration：执行 `schema_v9.sql` 后才把
`user_version` 写为 9；任一 SQL 失败会由未提交 transaction 回滚。

不会回填历史 Execution，也不会创建 Provider-private epoch/state。没有
`execution_usage` 行本身是公共 unknown 语义。

## Store 边界

`ExecutionUsageRecord` 是公共表的存储投影，字段直接对应 P4-001
`UsageSnapshot`，并在读取时安全转换 SQLite `INTEGER` 至 domain 的无符号
counter/revision 表示。三个 Codex identity/baseline/telemetry 记录仅作为
store-private persistence read model；不会从 `agent::store` 导出到 Product
或任何公共 DTO。

本任务没有 write API、snapshot/delta 计算、revision 自增、parser，亦没有
terminal grace/freeze 状态迁移。

## 测试策略

在既有 `agent::store::tests` 模式内以真实 v8 SQL fixture 打开数据库，分别
检查升级、约束和重启。故障注入复用 migration helper 的可控 SQL，断言 v9
表与 `user_version` 均不会部分提交。
