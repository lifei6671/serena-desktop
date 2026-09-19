# Phase 2D Batch A Source Git Capability Adapters

## Goal

Converge existing Lease-rooted Source and Git MCP tools onto WorkspaceCapabilityProvider while preserving their public behavior and Git path contract.

## Requirements

- 仅完成 P2D-001、P2D-002、P2D-003：将既有 Lease-rooted Source 与 Git MCP 工具收敛到通用 `WorkspaceCapabilityProvider`。
- Provider 调用只能使用服务端由 `workspaceId` 解析的 `WorkspaceLease`；不得读取 Desktop、Broker、Transport 或 caller root。
- 保持 Source 四读六写、Git 六工具、既有错误、限制、OCC、schema 和 Git `path` 契约；Remote Source Write 仍不 advertise 或 route。
- Git path 必须继续复用 `WorkspacePathResolver`，拒绝绝对、UNC、`..` 与 reparse/junction 越界。
- 不进入 P2D-004、CodeGraph、Health UI、Activity/Usage；不修改 Workspace Resolver、Agent/Claim/Provider Runtime 状态机；不提交、推送或创建 PR。
- 保留既有 P2C 未提交变更，并排除 `src-tauri/src/mcp/server.rs` 的并发 hunk。

## Acceptance Criteria

- [ ] Source/Git 均通过 Provider-agnostic Registry/Manager 路由，且不创建 RuntimeSlot。
- [ ] Source 的 Remote 写工具继续不在 Remote tools/list，且不可调用。
- [ ] Git 在 `git -C <lease.canonical_root>` 下工作，A/B、non-Git、取消和既有六工具行为不变。
- [ ] Git 公共可选字段仍为 `path`，且 workspace-relative path 校验保持冻结语义。
- [ ] 聚焦回归、`cargo check --locked`、owned Rust 格式检查和 `git diff --check` 有如实记录的结果。

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.
