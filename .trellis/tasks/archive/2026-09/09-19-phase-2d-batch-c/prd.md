# Phase 2D Batch C Capability Health UI and cleanup

## Goal

P2D-007 and P2D-008 only: descriptor-driven local capability health UI/actions plus remove and host-shutdown ownership cleanup tests; Remote CodeGraph remains disabled.

## Requirements

- P2D-007: ProjectPanel 必须通过 local-only `workspace_capability_observe` 展示当前显式 `workspaceId` 的 DTO provider/stage/action；installation、readiness、runtimeState 必须分别呈现。
- P2D-007: 所有 provider、stage、action 直接从 DTO 渲染；禁止按 Serena/CodeGraph provider ID 分支，且不得把 PID、port、绝对 root、raw stderr 或私有 identity 投影到 UI。
- P2D-007: `actions[]` 按 DTO 调用 local IPC prepare，支持 activity、成功、失败和 operationId cancel，并在终态重新 observe；listener 在组件卸载时释放。
- P2D-007: 移除 ProjectPanel 与 Health DTO 重复的 legacy capability truth，保留其他页面仍需的 Broker 字段；provider failure 只影响本 provider card。
- P2D-008: 以现有 Manager/Provider ownership 模型验证 Source/Git 无 runtime、Serena/CodeGraph workspace-scoped runtime 的 Remove、Host shutdown、A/B isolation、stop failure、generation drift 与 concurrent remove/call/shutdown。
- P2D-008: 不新增后台清理，不作 provider-specific global kill，不改变 Manager 公共契约，不恢复 Remote CodeGraph，也不进入 P2D-009。

## Acceptance Criteria

- [ ] Fake 第三 provider 可由同一 UI 数据映射呈现，无 provider ID 特判。
- [ ] UI action activity、pending/success/error/cancel 均可观察，且 event listener 不泄漏。
- [ ] Remote MCP 未增加 observe/prepare/cancel，CodeGraph Remote 工具继续 disabled。
- [ ] Remove/Shutdown 不留下 orphan；stop failure 保留 entry/handle/ownership；A 的清理不影响 B。
- [ ] 完成前端与 Rust 聚焦验证、format/check/typecheck、diff check；不 commit/push。

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.
