# CB3-005 Continue / Resume Routing + MCP Gate

## Goal

关闭 Continue / ResumePending 的 Provider / taskRole 继承语义和公开 MCP 契约，确保 Start Authority cutover 后后续执行不会重新选择 Provider、重新解析 Role Routing 或退化为 fresh Start。

## Requirements

- Continue 的公共请求继续禁止 workspaceId / providerId / taskRole；携带这些字段必须被 strict schema / parser 拒绝。
- Continue child 必须从 source Execution 继承：
  - workspace identity
  - provider
  - taskRole
- Continue 不读取当前 Role Routing 来替换已冻结 Provider / Role。
- 创建 Continue 前必须按 source.provider 重新检查：
  - Provider registered
  - Provider enabled
  - Provider health
  - canContinue
  - provider.validate_continuation(sourceExecutionId)
- disabled Provider 阻止新的 Continue，不创建 child Execution/Claim/Runtime。
- ResumePending 是同一 Execution 的首次派发恢复：
  - 使用原 Execution provider / taskRole；
  - 不重新选择 Provider/Role；
  - 不读取 Role Routing；
  - 重新检查 enabled / health 及既有 pending/attempt/Claim 安全条件；
  - disabled 返回 AGENT_PROVIDER_DISABLED，Execution/Claim/dispatch_state/runtime bind 保持不变。
- Cancel 继续只从 persisted Execution.provider 路由，保持 registration-only 语义；disabled / unavailable 不得删除取消入口。
- Execution Product View 展示冻结的 provider + taskRole；agent_query(providers) 展示当前 policy，两者允许不同。
- Provider capabilities 的公开投影必须等于 implementation ∩ passed Evidence Gate；未实现的 CodeBuddy Continue/Usage 不得宣告 true。
- routing error 后 agent_query(providers) 仍能读取当前恢复信息。
- 不允许把 Continue 转成 fresh Start，不允许 Provider fallback，不允许 role migration。

## Acceptance Criteria

- [ ] explicit / legacy Start 与 CB3-004 行为回归通过。
- [ ] half routing field 仍 INVALID_PARAMS。
- [ ] general unconfigured 与 Start routing error 不回归。
- [ ] Continue child provider/taskRole 与 source 完全一致，即使当前 Role Routing 已变化。
- [ ] disabled source provider 阻止新 Continue，零 child side effect。
- [ ] unavailable / canContinue=false / validate_continuation reject 保持稳定语义。
- [ ] ResumePending 使用原 Execution provider/taskRole；当前 route 变化不影响它。
- [ ] disabled ResumePending 保留原 Execution/Claim/dispatch_state/runtime evidence。
- [ ] Cancel 对 disabled/unavailable historical execution 仍 registration-only 可达。
- [ ] Product view 的冻结 routing 与 providers query 当前 routing 可以同时观察。
- [ ] capability=false 时公共 Catalog/MCP 不超前宣告能力。
- [ ] CB-003 MCP contract matrix 和相关 Agent regressions 全 PASS。
- [ ] 不实现 CodeBuddy Session，不进入 Phase 4，不提交 Git。

## Notes

Continue 和 ResumePending 的唯一 Provider Authority 是 persisted Execution identity。当前 Local Human Role Routing 只影响新的 Start / Continue admission 是否允许，不迁移已有 Execution。
