# Design — P2A2-005 Local Tauri IPC Workspace Authority Foundation / Audit

## Safe boundary

The audit follows a Local request from its TypeScript DTO through
`api.agent` and `commands::agent_operation`. `DesktopSelectedWorkspace` may
provide the form's initial value only. This task does not resolve a Lease or
change the backend Root selected for an operation.

## Owner map

| Local IPC surface | Current role | Atomic Authority owner |
|---|---|---|
| Agent Start | DTO carries `workspaceId`; command forwards raw request | P2A2-007 |
| Git | No direct Tauri execution route; MCP Git uses its own request boundary | P2A2-009 |
| CodeGraph | No direct Tauri execution route; legacy Broker binding remains | P2A2-010, then Workspace Capability Adapter |
| Serena Semantic | No direct Tauri execution route | P2A3-010 |
| Source | No direct Tauri execution route | P2A3-011 |

## Tauri command audit

`api.agent` is the only current Local execution request: it serializes an
`AgentAction` to `agent_operation`, whose Rust Command forwards it to the
Broker. Its Start route is intentionally left unchanged here and belongs to
P2A2-007.

`agent_history` is a read-only persistence query whose optional workspace value
is a filter, not a Root authority. `workspace_list`, `workspace_get`,
`workspace_register`, `workspace_rename`, `workspace_reorder`,
`workspace_remove`, `workspace_import_serena`, directory pick/inspect, and
`workspace_select` are Registry or UI-selection management commands, not
Workspace-scoped execution operations. `activate_workspace` and
`deactivate_workspace` manage the legacy Broker binding and remain explicitly
outside this task; P2A2-011 owns removing that normal execution authority.

## Exclusions

P2A2-005 does not construct `WorkspaceLease` or an Execution snapshot, modify
AgentTaskManager, StateStore, Claim ownership, Provider Dispatch, Tool Schema,
or any legacy global route.
