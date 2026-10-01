# 已被 Host correction 取代的初稿，不作为验收依据

# 设计

- schema_v13.sql 仅新增 codebuddy_execution_state。v12 文件不动，不回填任何历史行。
- agent/codebuddy/store.rs 为 domain/private typed API；SQL 在 agent/store/codebuddy.rs，保留 StateStore raw connection/closure 封装。这是用户明确允许的必要窄 scope 扩展。
- 所有写操作 IMMEDIATE transaction：验证 expected generic execution revision/provider/runtime binding、R1/R2 provider、expected private revision；无 upsert。create 明确 conflict，expected private absence；成功初始 revision=0，后续 mutation +1。
- create 的 prepared 表示尚未发送的父记录；conversation 可空直到 prepare，只有 prepare 成功返回的 durable identity 才具备后续 mark_sent 条件。prepare 要求 R1/protocol/session 已记录。
- prepare 在 store 内生成 UUIDv7 (48-bit Unix milliseconds + getrandom，严格 version/variant/wire 校验)，不接收 prompt/text 派生 identity。
- provider_request_id 与 conversation 分列；额外 provider_request_id_source 保存 exact_provider_observation 来源，缺失保持 null。
- RPC ID 复用 pinned SDK RequestId 的 string/i64 范围并禁止 Null，以 JSON scalar text 保存。protocol 值按 SDK u16，未协商 null。
- exact terminal 要求 sent predecessor 和 exact session/conversation/R1；重复相同 terminal 可接受但仍 OCC +1；uncertain 不自动 terminal。
- recovery 仅 session/load provenance，一次 begin/finish，结果仅 partial/unknown/material_difference；不产生结果或释放 authority。
- 没有发现已验证多 account namespace 的相反契约；使用用户指定 pair partial unique，不增加猜测 namespace。
