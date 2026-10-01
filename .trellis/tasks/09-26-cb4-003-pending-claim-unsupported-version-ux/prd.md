# CB4-003 Pending Claim / Unsupported Version UX Gate

## Goal

关闭 Phase 4 Agent 管理 UX Gate：Provider 停用后若存在 pending/resumable Execution 占用 Workspace Claim，页面必须给出安全且可行动的恢复路径；未知 CodeBuddy 版本必须使用稳定 diagnostic code 显示“尚未经过当前 SerenaDesktop 兼容性验证”，且不存在 Force Unlock 或版本强制放行。

## Pending / draining UX

- pending blocker 只从当前已加载 ExecutionView 的持久化事实识别：row.provider.id + attention=pending_explicit_resume / canResumePending；不新增 Claim Authority。
- disabled Provider 若存在 blocker，在 Provider Card 内显示阻塞提示：待恢复任务仍占用 Workspace Claim。
- 对每个可见 blocker 提供：查看任务、取消任务。
- Provider 级提供：重新启用 Provider。
- 查看任务复用现有 openDetails；取消复用现有 agent cancel；重新启用复用 local-only agent_provider_set_enabled。
- 禁止 Force Unlock；不得自动取消 pending Execution；不得自动释放 Claim。
- Provider disable 不杀 running Execution；已有 active rows 继续展示 draining。

## Provider enable control

为闭合 CB-004 Gate，Provider Card 增加本地启用/停用控制，复用既有 agent_provider_set_enabled。
- disable 只修改 Local Human Policy；运行任务继续，UI 转 draining。
- re-enable 后只更新 policy，不自动 ResumePending；用户仍显式点击恢复任务。
- mutation 失败回滚 UI；不影响 Role Routing。
- 不新增 Remote mutation。

## Unsupported-version UX

- UI 只接受稳定 diagnostic code `CODEBUDDY_VERSION_UNSUPPORTED`，禁止解析 errorMessage，禁止由 providerId、version 字符串或 health=unavailable 猜测。
- 命中时显示：`<displayName> 版本 <version> 尚未经过当前 SerenaDesktop 的兼容性验证。当前未启用该版本的 Agent 执行。请使用受支持版本，或升级 SerenaDesktop 后重新检测。`
- UI 不提供“忽略版本检查”“强制放行”“仍然运行”等 override。
- supported-version table 属于 release-owned data；CB4-003 不实现表、不安装/降级 CodeBuddy。
- 当前 Rust ProviderCatalog 尚无稳定 diagnostic source；本卡不得为了造数据修改 Remote MCP schema或 Core Registry。前端以可选 diagnosticCode 消费 seam + DOM fixture 锁定 UX；真实 `CODEBUDDY_VERSION_UNSUPPORTED` 生产接线由后续 CodeBuddy discovery/admission 阶段完成。

## Acceptance Criteria

- [ ] disabled + pending/resumable Claim 显示阻塞提示。
- [ ] 每个 blocker 可查看任务、可取消；Provider 可重新启用。
- [ ] 重新启用不自动 resume；取消仍走既有 Provider registration-only Authority。
- [ ] 无 Force Unlock action。
- [ ] running task 在 disable 后仍存在并显示 draining。
- [ ] Provider enabled/disabled toggle 使用现有 local-only IPC，失败安全回滚。
- [ ] disabled Role binding 继续保留“已停用”，不自动改绑。
- [ ] unsupported-version fixture 显示固定“尚未经过当前 SerenaDesktop 的兼容性验证”文案。
- [ ] generic unavailable 不显示 unsupported-version 文案。
- [ ] unsupported UI 无 override。
- [ ] existing Agent cards / Role editor / Composer / task list/detail 不回归。
- [ ] Remote MCP mutation surface/hash 不变。
- [ ] 不进入 Phase 5，不提交 Git。