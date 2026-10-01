# CB3-005 Design

## Continue Authority

Continue 请求只携带 sourceExecutionId / work context / request identity。创建 child 前从 Store 读取 source Execution，并继承其冻结身份：

- workspace_id / canonical_workspace_root / workspace_generation
- provider
- task_role
- execution profile

当前 roleRouting 只作为“当前政策信息”对外展示，不用于给 Continue 重新选 Provider。

Continue admission 顺序沿现有 Provider port：
1. source Execution 合法且可 continuation；
2. persisted provider registered；
3. provider enabled；
4. provider health available；
5. provider canContinue；
6. provider.validate_continuation(sourceExecutionId)；
7. 创建 child，并显式冻结 source provider + taskRole。

任何失败在 child create 前 fail closed。

## ResumePending Authority

ResumePending 不创建 replacement Execution。它对原 Execution：
- 使用 persisted provider；
- taskRole 只读保留；
- 不解析当前 role route；
- registered → enabled → health / capability 与原 pending-dispatch safety gate 结合；
- disabled/unavailable 时不改变 Execution、Claim、dispatch state、Runtime attempt/binding evidence。

重新启用 Provider 后，应允许同一个 pending Execution 在其它安全条件满足时恢复。

## Cancel

Cancel 保持 persisted provider registration-only：
`get_registered -> canCancel -> provider.cancel`。
enabled / health 不能成为取消历史任务的门禁。

## Product / MCP Projection

ExecutionView provider/taskRole 是 frozen execution facts。
ProviderCatalog roleRouting 是 current human policy。
两组数据允许不同，UI/ChatGPT 用于解释“旧 Execution 仍绑定旧 Provider”。

Catalog capabilities 只投影 Provider 已通过 Evidence Gate 的能力，不因 routing 需求而人为设 true。CodeBuddy 未实现前 canContinue 继续 false。

## Public Contract

- Continue / Cancel / ResumePending schema 不出现 providerId/taskRole/workspaceId。
- Start 保留 CB3-003/004 compatibility matrix。
- routing error 不改变 agent_query(providers) 只读可用性。
- 不增加任何 Remote policy mutation tool。

## Rollback / Safety

禁止：
- Continue fallback 为 fresh Start；
- Continue 根据当前 route 换 Provider；
- ResumePending 创建替代 Execution；
- disable 自动 cancel 或 Force Unlock；
- capability 虚假宣告。
