# CB4-002 Design

## Authority

唯一写 Authority 继续是 Supervisor ManagerConfig.agent_providers.role_routing，通过既有 local-only IPC agent_provider_set_role_route(taskRole, providerId?).
前端不写 save_config，不维护第二份持久化配置。

## Data flow

ProviderCatalogSnapshot.roleRouting -> UI committed route
ProviderCatalogSnapshot.providers -> available registered Provider options
用户选择 -> per-role draft -> api.agentProviderSetRoleRoute -> returned AgentProviderSettings -> commit UI
失败 -> revert draft to prior committed value。

Catalog polling 与 mutation 可能交错：
- 正在保存的 Role 不应被旧 Catalog snapshot 覆盖；
- mutation 成功后立即用返回 settings 更新本地 committed route；
- 后续 Catalog 新快照可继续成为只读 current policy。
可用 per-role pending state / mutation revision 防止 stale response。

## Provider options

- Registered Providers 来自 catalog.providers，不按 providerId 特判。
- disabled option 保留 selectable，并附“已停用”。
- 当前 route 若不在 catalog.providers：插入一个仅用于展示/选择当前值的 unknown option，label `<id> · 未注册`。
- null 使用 sentinel（例如 __none__）映射为“未指定 Agent”，不得把空字符串当 ProviderId。

## Role labels

固定角色可以由 UI 常量提供中文 label，因为 Role 集合由协议固定；这不是 Provider 特判：
- development 开发
- testing 测试
- review 评审
- analysis 分析
- general 通用

## Safety

- mutation 不触发 health refresh / Runtime / Execution。
- 不改变已有 execution.provider/taskRole。
- Remote MCP 不新增 mutation。
- 禁止自动 fallback。

## Tests

- set/clear
- disabled binding
- unknown persisted binding
- failure rollback
- stale catalog vs in-flight mutation
- persisted/reload fixture
- no role select in Provider Card itself; editor is dedicated section
- existing Agent task UI regression