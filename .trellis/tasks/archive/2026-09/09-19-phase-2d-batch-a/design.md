# Phase 2D Batch A Design

## 边界

复用现有 `WorkspaceCapabilityProvider`、`WorkspaceCapabilityRegistry`、`WorkspaceCapabilityManager` 与 `WorkspaceToolCall`。增加无状态 in-process Source/Git provider；其 descriptor 使用 `InProcess` runtime model，Manager 的无 runtime 分支直接把 server-resolved `WorkspaceLease` 交给 provider。

## 路由

`MCP request -> registry parse workspaceId -> WorkspaceResolver -> WorkspaceLease -> WorkspaceCapabilityManager::call -> provider.call(lease, None, WorkspaceToolCall)`。

Source provider 将 tool name 和 arguments 委托给现有 Rust read/write 入口，保留它们的 DTO、限制、错误和 OCC。Git provider 委托 `mcp::git::call`，由该入口继续以 lease 构造 `git -C` 与 `WorkspacePathResolver` path 校验。

## 非目标

不改变公共 schema 或错误投影；不引入 RuntimeSlot、Serena/CodeGraph 生命周期、Remote Source Write、Tauri IPC 或新的 path resolver。
