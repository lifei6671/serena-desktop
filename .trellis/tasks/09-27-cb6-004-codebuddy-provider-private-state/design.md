# 设计

- schema_v13.sql 仅新增 codebuddy_execution_state。v12 文件不动，不回填任何历史行。
- agent/codebuddy/store.rs 为 domain/private typed API；SQL 在 agent/store/codebuddy.rs，保留 StateStore raw connection/closure 封装。这是用户明确允许的必要窄 scope 扩展。
- 所有写操作 IMMEDIATE transaction：验证 expected generic execution revision/provider/runtime binding、R1/R2 provider、expected private revision；无 upsert。create 明确 conflict，expected private absence；成功初始 revision=0，后续 mutation +1。
- Host correction: create 原子预留并持久化 conversation_request_id，所有现存行必需。prepared 仅表示本地 Prompt identity 已 durable、未发送，R1/protocol/session 可暂空。MarkSent 前要求三者齐全。
- create 在 store 内生成 UUIDv7 (48-bit Unix milliseconds + getrandom，严格 version/variant/wire 校验)，不接收 prompt/text 派生 identity。
- provider_request_id 与 conversation 分列；额外 provider_request_id_source 保存 exact_provider_observation 来源，缺失保持 null。ExactProviderRequest 单独记录真实来源，不覆盖已知不同值。
- RPC ID 复用 pinned SDK RequestId 的 string/i64 范围并禁止 Null，以 JSON scalar text 保存。protocol 值按 SDK u16，未协商 null。
- Host correction: exact live terminal 支持 sent 或 uncertain predecessor，必须 exact session/conversation/R1；禁止 history/time 推断。terminal 后 identity/RPC/terminal 冻结，重复完全一致 terminal 正确 OCC 时 +1。
- recovery 仅 session/load provenance，一次 begin/finish，结果仅 partial/unknown/material_difference；不产生结果或释放 authority。
- 没有发现已验证多 account namespace 的相反契约；使用用户指定 pair partial unique，不增加猜测 namespace。

## Closeout 测试映射（保持上述设计）

- store/tests/v13_migration.rs：冻结完整 v12 dump，迁移/reopen 全历史表逐命名列值对比；v0/v9/v11、v14 拒绝、后段索引失败/FK 损坏回滚；三 FK RESTRICT、CHECK 与 pair partial unique；v12 文本 hash。原始字节 hash 由 Host Gate 单独保持。
- store/tests/codebuddy.rs：使用 CodeBuddyStore typed API；SQL 仅在 Store 私有测试设置与全表快照断言内。覆盖 reservation/restart、create conflict、双 revision/runtime/provider ownership、MarkSent readiness、exact terminal identity freeze、RPC string/i64 持久化、R2 与 generic/Claim authority 原值、missing/corrupt fail-closed。
- provider/tests.rs 与 mcp/orchestration_tests.rs：公共 Provider domain 拒收私有字段；填充真实私有行前后，Product/Work/MCP 的 get/list/observe 只读输出逐值不变，拒绝私有 key/value 泄露。
- 未扩大生产接口、capability、Usage、网络或生命周期 authority。
