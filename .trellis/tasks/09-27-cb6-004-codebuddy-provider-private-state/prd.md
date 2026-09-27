# CB6-004 — CodeBuddy Provider-Private State Store

用户已明确批准创建与实施本卡，不 commit/push，不进入 CB6-005。
权威：task breakdown CB6-004；technical design §15.2/15.3/16/23；CB5-005 codebuddy-private-state-dcr.md；本轮用户详细约束。
DCR 的 Stage C preamble 是历史状态，用户确认 Host Gate 已完成；当前生产 v12，v13 未占用。

只持久化 Adapter 私有 identity/provenance。禁止 session/prompt 网络行为、Usage ledger、capability/public DTO/Product/Role Routing 变化、Claim release、termination/reconcile、Codex 私有复用。
验收：14 类用户矩阵全部落实：历史迁移、restart、future reject、rollback、Codex 全字段不变、ownership/OCC/create conflict、状态转换、SDK RPC type、UUIDv7、partial unique、R2 不改 R1、缺失损坏 fail-closed、FK、公共投影隔离。
