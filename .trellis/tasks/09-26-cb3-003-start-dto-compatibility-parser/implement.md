# CB3-003 Implementation Plan

1. 记录并保护当前未提交基线，读取 CB3-003 任务卡、设计 §8/§8.0 和当前 AgentExecute DTO/schema/hash tests。
2. 在 `mcp/orchestration/dto.rs` 内引入仅 transport 层的 Start routing intent，保持旧 Start 字段与 workspace validation。
3. 使 serde/parser 明确接受 only:
   - legacy: routing pair 都缺失；
   - explicit: routing pair 都存在且领域类型合法。
4. 调整 schemars 输出，使 schema 自身拒绝 half-pair，并保持 continue/cancel/resume_pending routing-free。
5. 补完整 parse/schema compatibility matrix 与无业务副作用断言。
6. 更新 agent_execute descriptor/hash fixture；断言 agent_query hash 仍为 CB3-002 值，hash 对 object key order 稳定。
7. 运行 focused DTO/parser tests、registry/schema/hash tests、必要 orchestration regression、cargo check、fmt、git diff --check。
8. Clippy 只记录已知 task 外 blocker；不扩大范围。
9. 冻结最终 diff/证据并进行只读 review，等待 Host Gate。

## Rollback Point

若必须读取 Role Routing、修改 Product work_adapter/TaskManager、创建 Execution 或触发 Provider 才能完成，停止并返回 DESIGN_BLOCKER。
