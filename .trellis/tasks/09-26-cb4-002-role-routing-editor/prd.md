# CB4-002 Role Routing Editor

## Goal

在 Agent 管理页增加本地 Role Routing Editor，让用户显式配置 development / testing / review / analysis / general 的首选 Provider。只修改 Local Human Policy，不影响 running Execution，不暴露 Remote mutation，不自动 fallback。

## Requirements

- 五个固定 Role：development、testing、review、analysis、general。
- 每个 Role 最多绑定一个 Provider。
- 允许清空绑定；空值显示“未指定 Agent”。
- 可选择所有当前已注册 Provider，包括 disabled Provider。
- 当前绑定若指向 disabled Provider，必须保留并显示“已停用”；不得自动改绑。
- 当前绑定若指向未注册/未知 Provider，必须保留可见，不能静默改成 Codex 或 null；用户可以主动改绑/清空。
- 保存使用现有 local Tauri IPC agent_provider_set_role_route；不得新建第二 Authority。
- 保存成功后 UI 与后端返回的 AgentProviderSettings 对齐；随后 Catalog polling 可以刷新 current roleRouting。
- 保存失败：回滚当前 Role 的 UI draft 到提交前值，显示可理解错误；其它 Role draft 不受影响。
- running / completed Execution 的 frozen provider/taskRole 不随 Role Routing 编辑变化。
- 不实现 Provider priority list、自动 fallback、Prompt 自动分类。
- 不增加 Remote MCP mutation tool。

## UI

- 位于 Agent 接入卡片之后、任务 Composer 之前，标题“角色分工”。
- 每个 Role 一行：中文名称 + Provider Select。
- Select 使用现有非原生 shadcn Select。
- Provider option 使用动态 Catalog descriptor；disabled Provider 带“已停用”提示。
- clear option：“未指定 Agent”。
- unknown current binding 使用安全 label，例如“<id> · 未注册”，并作为当前值可见。
- 保存过程中仅禁用对应 Role 控件，避免重复提交；不阻塞现有任务 Composer。

## Acceptance Criteria

- [ ] set role 成功，local policy 返回值与 UI 一致。
- [ ] clear role 成功，显示未指定 Agent。
- [ ] disabled Provider 可被选择，已绑定 disabled Provider 不自动改绑。
- [ ] unknown/unregistered current route 可见并可清空/改绑。
- [ ] restart/persisted fixture 后 Catalog roleRouting 正确回显。
- [ ] save failure 回滚当前 draft，显示错误。
- [ ] 不修改 Execution frozen identity，不触发 Runtime/Execution/Claim。
- [ ] Remote registry 不新增 Provider mutation tool，descriptor hashes 不变。
- [ ] frontend focused/full tests、build、lint、backend policy regressions、cargo check/fmt/diff-check 通过。
- [ ] 不进入 CB4-003，不提交 Git。