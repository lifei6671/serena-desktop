# CB4-001 Agent 管理 / Provider Cards

## Goal

把左侧“Agent”和当前 Agent 任务页升级为“Agent 管理”，在保持现有任务 Composer / 最近任务 / 详情能力的同时，新增动态 Provider Cards。所有 Provider 展示必须来自统一 Product Catalog/Execution 数据，不允许按 providerId 写布局或状态特判。

## Requirements

- 左侧导航“Agent”改为“Agent 管理”。
- 页面新增“Agent 接入”区域，按注册 Provider 动态渲染卡片。
- 卡片展示 displayName/id fallback、enabled、health、version、protocol、runtime presentation、active executions。
- enabled / health / runtime / activeExecutions 是独立维度。
- Idle：enabled + available + activeExecutions=0 时 stopped 是正常状态。
- Draining：enabled=false 且 activeExecutions>0 显示“正在停用”，runtime 仍显示 running。
- Provider metadata 缺失安全降级；未知 Provider 不崩溃。
- 不实现 Role Routing Editor（CB4-002）。
- 不修改 Runtime / Routing 生产行为；不按 providerId 写布局特判。

## Data prerequisite

当前前端没有本地 Provider Catalog 读取入口，而 Provider Settings 只含 enabled / roleRouting，无法满足动态 descriptor/version/health 展示。允许一个最小只读 prerequisite：
- 新增 local Tauri IPC agent_provider_catalog_get（名称可按项目命名规范调整）；
- 直接返回现有 CB3-001 AgentProductService::provider_catalog(supervisor)；
- 只读，无 probe、无 health refresh、无 Runtime/Session/Execution/Claim 副作用；
- 不形成第二 Authority；Remote MCP 契约不变。

## Runtime / active execution projection

CB4-001 不新增 Runtime backend contract。页面用当前已加载 ExecutionView rows 计算 provider activeExecutions；首版 runtime presentation：activeExecutions > 0 => running，否则 stopped。它只是 UI presentation，不声称 OS process evidence。

## Acceptance Criteria

- [ ] 导航和页面语义升级为 Agent 管理。
- [ ] Codex-only 与 Codex+CodeBuddy descriptor fixture 均动态渲染。
- [ ] disabled / unavailable / idle / draining DOM 状态清楚且独立。
- [ ] unknown provider/version/metadata 安全 fallback。
- [ ] active execution 按 persisted Execution.provider.id 统计。
- [ ] 现有 Composer/列表/详情/Cancel/Resume 不回归。
- [ ] local catalog bridge 只读且无副作用。
- [ ] frontend tests、build、eslint/typecheck（若项目脚本存在）通过。
- [ ] backend bridge focused tests、cargo check/fmt/diff-check 通过。
- [ ] 不进入 CB4-002，不提交 Git。