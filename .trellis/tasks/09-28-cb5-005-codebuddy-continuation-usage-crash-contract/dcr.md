# Provider-private Persistence DCR

## Decision

`EXISTING_FIELDS_SUFFICIENT`。Continue 的 Host contract 已证明，但当前 StateStore schema 已包含实现所需的最小持久化身份；CB5-005 不新增表、字段、migration 或 schema version。

## Evidence-to-field mapping

| Contract requirement | Existing durable field | Rule |
|---|---|---|
| source CodeBuddy session S1 | `codebuddy_execution_state.session_id` | 从 parent/source execution 读取；`session/load` 只使用这一 exact S1 |
| Workspace / cwd identity | `executions.workspace_id`, `canonical_workspace_root`, `workspace_generation` | child 必须继承并验证 exact canonical root；load 的 `cwd` 来自该 authority |
| parent / child lineage | `executions.parent_execution_id` 与既有 work/execution links | Continue child 明确指向 source execution；不能只按最近 session 猜测 |
| per-execution conversation identity | `codebuddy_execution_state.conversation_request_id` | 每个 execution 在 create 事务中本地、原子预留独立 UUIDv7 Prompt identity；child 不继承 parent，也不把它描述为 Provider 返回值 |
| per-execution Provider request identity | `provider_request_id`, `provider_request_id_source` | Provider 实际返回的 request identity 仅在真实 wire 可绑定时写入，并遵守 existing exact-identity checks |
| connection-local prompt request | `prompt_rpc_id` | 只属于创建它的 Runtime/connection；跨 R1→R2 不复用 |
| recovery state | `recovery_method`, `recovery_state`, `recovery_runtime_instance_id`, recovery timestamps | 唯一 method 固定为 `session/load`；记录 R2 recovery lifecycle，不冒充 result completeness |
| target terminal/result | existing prompt terminal/result columns | 没有 exact typed PromptResponse 时保持 partial/unknown，不由 replay text 合成 |

当前 source sessionId、Workspace/cwd、parent/child lineage 和 provider/conversation request identity 均有真实现存位置。CB8-003 可能需要新增 Store query/transaction 以组合这些既有字段，但这属于实现逻辑，不是 schema 缺口。

## Fields deliberately not added

- 不持久化 Provider session storage path、history mode 或 epoch：attempt 4/5 未证明 recovery 需要它们。
- 不持久化 marker、P3 answer 或 replay 文本来模拟 lineage。
- 不建立 Usage baseline/cumulative epoch：usage event 无 exact Prompt binding，语义未冻结。
- 不为 `session/resume` 或其他猜测 API 设计字段。

## Migration

无需 migration，也不提高当前 schema version 13。历史 execution 不需要伪造 continuation、usage 或 result recovery evidence。若未来 Provider contract 引入新的真实必需 identity，必须另开 bounded Contract Gate 和 DCR。
