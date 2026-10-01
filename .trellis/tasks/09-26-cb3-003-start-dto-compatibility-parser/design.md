# CB3-003 Design

## Boundary

只修改 MCP transport DTO / parser / generated schema contract 与对应 contract tests。Role Routing Authority、Execution creation、Provider admission、dispatch 属于 CB3-004，不在本卡。

## Typed Start Routing Intent

DTO 层应形成不可产生 half-pair 的类型语义：

- `LegacyGeneral`：请求中 taskRole/providerId 均缺失。
- `Explicit { task_role: AgentTaskRole, provider_id: ProviderId }`：两字段均存在且领域类型合法。

公开 JSON 仍保持一个 `action=start` 契约，但 schema 必须表达两个合法形态：legacy 与 explicit。half-pair 在 schema 和 serde/parser 两层均拒绝。

## Compatibility

旧客户端：
`action + workRunId + workspaceId + requestKey + prompt + context?`
继续解析为 legacy，不在 parser 中解析 current general provider。

新客户端：
必须成对发送 `taskRole + providerId`。本卡仅冻结 identity intent；CB3-004 再依据 Local Human Policy 做精确校验。

## Error Semantics

routing pair 的歧义、非法 enum/ID、额外字段统一映射至 `INVALID_PARAMS`。既有 workspaceId 缺失/空值的专用校验语义不得被 routing parser 覆盖。

## Schema / Fingerprint

- agent_query schema/hash 保持 CB3-002。
- agent_execute input schema 发生有意变化，因此 descriptor hash 必须显式更新。
- continue/cancel/resume_pending schema 不得出现 taskRole/providerId。
- descriptor hash 继续使用既有 canonical key-order-independent 算法。

## Side-effect Rule

解析与 schema validation 必须是纯 transport 逻辑：不读取配置、Registry、Product、StateStore；不创建 Execution/Claim/Runtime；不调用 Provider。
