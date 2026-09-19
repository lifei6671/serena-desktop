# Phase 2D Batch B CodeGraph capability readiness and actions

## Goal

P2D-004 and P2D-005 only: close P0-006 CLI evidence; implement CodeGraph readiness adapter and Local Human init/sync/rebuild actions without RuntimeSlot or Remote CodeGraph exposure.

## Requirements

- P0-006：以隔离临时用户配置和临时 workspace 重现实机 CodeGraph CLI 1.6.0 的非交互与 JSON 契约；证据不得包含真实用户配置或业务项目路径。
- P2D-004：新增只使用服务端 `WorkspaceLease` 的 CodeGraph capability adapter。readiness 唯一 authority 是有界、可取消的 `codegraph status --json <canonicalRoot>`；不得以 `.codegraph/` 目录、Desktop selection、legacy ActiveWorkspace 或 caller root 推断状态。
- P2D-005：只通过既有本地 Tauri `workspace_capability_prepare(workspaceId, providerId, actionId)` 暴露 `build_index`、`update_index`、`rebuild_index`；分别执行 `init --yes`、`sync`、`index`，并在成功后再次 status JSON 验证。
- Remote MCP 不得 advertise/call `codegraph_explore`，也不得暴露 prepare；不得实现 P2D-006 RuntimeSlot/capacity/query process。

## Acceptance Criteria

- [ ] P0-006 evidence 包含本机 version/help、隔离 `install --target=none --yes`、未初始化/ready/stale/sync/index 的脱敏 JSON 与退出码。
- [ ] Adapter 验证 initialized、canonical projectPath、index.state、pendingChanges 与 reindexRecommended；缺 binary、畸形 JSON、缺字段、root mismatch、命令错误都 fail closed 且不泄露 raw stderr/root。
- [ ] Descriptor 的 stages/actions 为通用 Manager 数据；三个动作均为 `provider_prepare`、`warmRuntime=false`、`local_human`，并复用既有 single-flight/activity/cancellation。
- [ ] 覆盖 readiness fixtures、精确 argv/root、single-flight、cancel、失败与 post-status、Remote denial 和 Batch A regressions。

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.
