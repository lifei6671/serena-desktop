# P2D-009 Multi-workspace Capability Integration Gate

## Goal

关闭 Phase 2D：恢复 Remote `codegraph_explore`，且只允许 `request.workspaceId -> WorkspaceResolver -> WorkspaceLease -> WorkspaceCapabilityManager -> CodeGraphCapabilityProvider` 权威链路；以自动化集成 Gate 证明 Source、Git、Serena、CodeGraph 在多 Workspace 下不串 Root、不发生全局 retarget。

## Requirements

- 先独立复现完整 lib suite 的两项已知失败，只作根因分类。仅当它们由本 Gate 或全局污染直接导致且阻塞时才作最小修复；不得为全绿改变 Activity、Usage 或 Agent lifecycle 业务语义。
- 移除 `codegraph_explore` 的 P2A2-era unconditional `UNKNOWN_TOOL` guard；tools/list 恢复该工具。输入 schema 必须为 explicit `workspaceId`、required `query`、optional `maxFiles`，不接收 root、canonicalRoot、path 作为 authority。
- Remote 调用不得读取 DesktopSelectedWorkspace、session、legacy activate、active workspace 或旧 `mcp/codegraph.rs` Global Binding；missing/unknown `workspaceId` 分别保持 `WORKSPACE_CONTEXT_REQUIRED` / `WORKSPACE_NOT_FOUND`。
- CodeGraph Manager errors 在 Adapter/Broker compatibility 边界映射为 `CODEGRAPH_BUSY`、`CODEGRAPH_NOT_INITIALIZED`、`CODEGRAPH_RUNTIME_START_FAILED`、`CODEGRAPH_RUNTIME_LOST`；Manager Core 不允许 providerId 特判。
- 新增或完善一个 P2D integration gate，覆盖 Source/Git/Serena/CodeGraph A/B、CodeGraph readiness、single-flight、capacity/LRU/BUSY/idle/crash、health/remove/shutdown、global-authority negatives、Remote tools/list/call 以及 Source Write Remote disabled。
- 复用 P0-006/P0-007 的冻结证据：`maxInstances=2`、`idleTimeoutMs=300000`、`perSlotConcurrency=1`，不修改真实用户配置或业务 index。
- 运行任务要求的前端、Rust、format/diff 与完整 lib 回归；若已知两项范围外失败仍存在，逐项给出当前复现证据和范围外理由。任何 Phase 2D 失败都使 Gate FAIL。

## Scope

允许修改：P2D-009 MCP registry/dispatcher/compat mapper/integration tests，以及本任务的 Trellis 文档与证据。

禁止修改：Phase 3、Activity/Usage、Agent lifecycle/summaryCode、旧 Global Binding 的再启用、真实用户 CodeGraph index/config，以及 commit/push。

## Acceptance Criteria

- 所有目标 Provider 只有成功 resolve 成服务端 `WorkspaceLease` 后才进入 Provider。
- `codegraph_explore` 已通过新 Adapter Remote advertise/call；公开链路不可到达旧 Global Binding。
- A/B 请求、capacity、LRU、Busy、idle、crash、remove、shutdown、health 全部由 deterministic integration tests 覆盖且通过。
- 前端与 Rust 指定回归通过；完整 `cargo test --locked --lib` 中剩余的失败仅可为两项已复现且可证明范围外的基线。
