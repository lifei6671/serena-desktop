# CB3-004 Design

## Authority Flow

Start 创建 Execution 前必须重新读取当前 Local Human Policy 和 Provider facts，不能依赖较早的 agent_query(providers) 结果。

### Explicit Start

请求已经冻结 typed `taskRole + providerId`。按技术设计 §8 的错误优先级校验：

1. agentEnabled == true
2. provider registered
3. provider enabled
4. role configured
5. role.preferredProviderId == request.providerId
6. provider health == available
7. provider.canExecute == true
8. 使用该 provider + taskRole 创建 Execution / Claim

因此 unregistered、disabled、role unconfigured/mismatch、unavailable、capability 必须保持可区分，不能统一映射。

### Legacy Start

请求没有 providerId，必须先：

1. agentEnabled == true
2. taskRole = general
3. 读取当前 roleRouting.general；缺失 => AGENT_ROLE_NOT_CONFIGURED
4. 得到 providerId 后，对该 Provider 执行现有 registered → enabled → health → capability admission
5. 使用解析出的 provider + general 创建 Execution / Claim

legacy 不允许默认 Codex，也不允许 fallback。

## Race / Retry

Product / work_adapter 不得使用较早的 Provider Catalog 快照作为 Authority。真正 create/retry 前读取当前 Supervisor config / Provider facts。

requestKey identity 使用已冻结的 execution-request-v3：
- provider 与 taskRole 都参与新请求 hash。
- 新请求相同 requestKey 但 provider/taskRole 不同 => EXECUTION_REQUEST_KEY_CONFLICT。
- 历史 v2/v1 compatibility 继续只允许 Store 已批准的 legacy 条件，不重写历史 hash。

必须复用现有 operation/configuration/workspace/creation 边界，避免新增第二套 Authority。若在校验与 create 之间无法证明 Authority 一致性，fail closed，不允许“先创建再修正”。

## Persistence

Execution 创建时显式写入：
- workspace identity
- provider
- task_role
- execution_profile

生产插入不得依赖数据库 default 来填 task_role。创建后 policy/Provider 开关变化不修改已有 Execution identity。

## Error Separation

Routing:
- AGENT_ROLE_NOT_CONFIGURED
- AGENT_ROLE_PROVIDER_MISMATCH

Provider:
- AGENT_PROVIDER_NOT_FOUND
- AGENT_PROVIDER_DISABLED
- AGENT_PROVIDER_UNAVAILABLE
- AGENT_PROVIDER_CAPABILITY_UNSUPPORTED
- 既有 contract/operation errors

这些错误保持独立。

## Scope Boundary

Allowed:
- Product work_adapter
- TaskManager admission/start creation glue
- Store product creation transaction 及 focused tests

Excluded:
- CodeBuddy implementation
- Continue / Resume routing
- UI
- Provider policy mutation
- 自动 fallback
- Prompt NLP classification
