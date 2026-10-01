# CB3-004 Start Routing Authority

## Goal

在 Execution 创建前，依据当前 Local Human Policy 对 Start 的 taskRole + providerId 做权威解析和校验，并将最终 provider / taskRole 冻结进 Execution。保持旧 Start 的 bounded compatibility，不做自动 Role 变更或 Provider fallback。

## Requirements

- 显式 Start 使用 CB3-003 的 Explicit routing intent，按技术设计 §8 的稳定错误优先级重新校验当前 Authority。
- Legacy Start 使用 CB3-003 的 LegacyGeneral intent，固定 taskRole=general，并在创建前从当前 general Routing Policy 解析 providerId。
- role 未配置时返回 AGENT_ROLE_NOT_CONFIGURED。
- 显式 providerId 与当前 role route 不一致时返回 AGENT_ROLE_PROVIDER_MISMATCH。
- Provider registered / enabled / health / canExecute 校验保持各自稳定错误，不合并为 routing 错误。
- 显式请求必须遵循 §8 冻结校验顺序；legacy 因请求没有 providerId，先解析 general route，再对解析出的 Provider 执行现有 admission。
- 禁止自动 fallback、自动换 Role、修改 Local Human 配置、解析 Prompt 分类任务。
- 最终 provider + taskRole 必须在 Execution 创建事务中冻结；后续 policy 变化不得改写已有 Execution。
- 同 requestKey retry 必须绑定 canonical request identity：
  - 新请求不同 provider 或不同 taskRole => EXECUTION_REQUEST_KEY_CONFLICT；
  - approved legacy v2/v1 compatibility 只用于历史持久化行，不得弱化新 v3 identity。
- creation/retry race 必须在真正创建前重新读取必要 Authority，避免使用陈旧 query/provider policy 快照。
- policy uncertainty / mismatch / provider 不可接收新执行时，不创建 Execution/Claim/Runtime。
- 本任务不实现 CodeBuddy Provider，不修改 Continue/Resume 行为（属于 CB3-005）。

## Acceptance Criteria

- [x] explicit role/provider 与当前 routing 精确匹配时可创建。
- [x] legacy Start 解析为 general + 当前 general route。
- [x] general/任意 role 未配置 => AGENT_ROLE_NOT_CONFIGURED，且无 Execution/Claim。
- [x] explicit provider 与 role route 不一致 => AGENT_ROLE_PROVIDER_MISMATCH，且无 Execution/Claim。
- [x] disabled / unavailable / canExecute=false 分别返回各自稳定 Provider 错误。
- [x] 显式 Start 错误优先级与设计 §8 一致；legacy 解析 provider 后复用现有 Provider admission 语义。
- [x] 创建后的 Execution provider/taskRole 与最终 Authority 一致，并在 policy 改变后保持不变。
- [x] same requestKey + same payload retry 幂等；不同 provider 或 taskRole => EXECUTION_REQUEST_KEY_CONFLICT。
- [x] query 后 policy 改变、创建前 provider 被禁用等 race 被重新校验并 fail closed。
- [x] SerenaDesktop 不依据 Prompt 内容分类 Role。
- [x] focused routing/creation transaction tests、相关 MCP/agent regression、cargo check、fmt、git diff --check 通过。
- [x] 不进入 CB3-005，不提交 Git。

## Notes

Start Routing 的 Authority 是 Local Human Policy。ChatGPT 负责选择 taskRole，SerenaDesktop 只验证当前本地策略和 Provider 可用性。

## Implementation verification

本轮实现者自验已完成，详见 delivery.md；Host Gate / 独立最终评审仍单独判定。Clippy 仅剩已知 usage_tests.rs await_holding_lock，未修改；没有 Linux 验证证据。
