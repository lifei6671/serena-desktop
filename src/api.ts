import { invoke } from "@tauri-apps/api/core";
import type { AppState, ManagerConfig, BrokerState, AgentAction, AgentEnvelope, AgentProviderSettings, ExecutionView, ProviderCatalogSnapshot, RemoteState, RemoteAccessMode, SecurityDeclaration, Workspace, WorkspaceInspection, WorkspaceRegistrySnapshot, WorkspaceCapabilityHealth } from "./types";

export const api = {
  remoteState: () => invoke<RemoteState>("remote_state"),
  remoteSaveNgrokAuth: (authToken: string) => invoke<void>("remote_save_ngrok_auth", { authToken }),
  remoteClearNgrokAuth: () => invoke<void>("remote_clear_ngrok_auth"),
  remoteStartNgrok: () => invoke<void>("remote_start_ngrok"),
  remoteStart: (mode: RemoteAccessMode, publicOrigin?: string, securityDeclaration?: SecurityDeclaration, riskAccepted = false) => invoke<void>("remote_start", { mode, publicOrigin, securityDeclaration, riskAccepted }),
  remoteStop: () => invoke<void>("remote_stop"),
  remoteProbe: () => invoke<void>("remote_probe"),
  remoteApprove: (id: string, allow: boolean) => invoke<void>("remote_approve", { id, allow }),
  codexVersion: () => invoke<string>("get_codex_version"),
  agent: (request: AgentAction) => invoke<AgentEnvelope>("agent_operation", { request }),
  // 只读本地 Catalog，不刷新健康状态或创建运行实例。
  agentProviderCatalog: () => invoke<ProviderCatalogSnapshot>("agent_provider_catalog_get"),
  // 启停仅修改本地 Human Policy，不取消运行任务或恢复待派发任务。
  agentProviderSetEnabled: (providerId: string, enabled: boolean) =>
    invoke<AgentProviderSettings>("agent_provider_set_enabled", { providerId, enabled }),
  // 角色策略只走本地专用 Authority；null 显式清空，不修改现有 Execution。
  agentProviderSetRoleRoute: (taskRole: keyof AgentProviderSettings["roleRouting"], providerId: string | null) =>
    invoke<AgentProviderSettings>("agent_provider_set_role_route", { taskRole, providerId }),
  // 专用 Local Tauri IPC；不得复用 Remote MCP 的 agent_execute action。
  agentManualResolve: (executionId: string, resolution: "interrupt_and_release", reason?: string) =>
    invoke<ExecutionView>("agent_manual_resolve", { executionId, resolution, reason }),
  agentHistory: (before: string | null = null, workspace: string | null = null) => invoke<{ executions: ExecutionView[]; nextCursor: string | null }>("agent_history", { before, workspace }),
  mcpLogs: () => invoke<string[]>("get_mcp_logs"),
  downloadMcpLogs: () => invoke<boolean>("download_mcp_logs"),
  clearMcpLogs: () => invoke<void>("clear_mcp_logs"),
  broker: () => invoke<BrokerState>("get_broker_state"),
  workspacePickDirectory: () => invoke<string | null>("workspace_pick_directory"),
  workspaceInspectDirectory: (root: string) => invoke<WorkspaceInspection>("workspace_inspect_directory", { root }),
  workspaceRegister: (root: string, name?: string) => invoke<Workspace>("workspace_register", { root, name }),
  workspaceImportSerena: () => invoke<number>("workspace_import_serena"),
  workspaceRename: (id: string, name: string) => invoke<Workspace>("workspace_rename", { id, name }),
  workspaceRemove: (id: string) => invoke<Workspace>("workspace_remove", { id }),
  workspaceReorder: (ids: string[]) => invoke<WorkspaceRegistrySnapshot>("workspace_reorder", { ids }),
  workspaceCapabilityObserve: (workspaceId: string) =>
    invoke<WorkspaceCapabilityHealth>("workspace_capability_observe", { workspaceId }),
  syncProjects: () => invoke<number>("sync_workspaces"),
  workspaceSelect: (id: string) => invoke<Workspace>("workspace_select", { id }),
  activateProject: (id: string) => invoke<void>("activate_workspace", { id }),
  deactivateProject: () => invoke<void>("deactivate_workspace"),
  cancelProject: () => invoke<void>("cancel_workspace_operation"),
  setBroker: (enabled: boolean, port: number, allowLan: boolean) =>
    invoke<void>("set_broker", { enabled, port, allowLan }),
  getState: () => invoke<AppState>("get_app_state"),
  detect: () => invoke<AppState>("detect_serena"),
  detectGit: () => invoke<AppState>("detect_git"),
  repair: () => invoke<AppState>("repair_serena"),
  install: () => invoke<AppState>("install_serena"),
  start: () => invoke<AppState>("start_serena"),
  stop: () => invoke<AppState>("stop_serena"),
  restart: () => invoke<AppState>("restart_serena"),
  // 无损传递配置；Rust 保留最新 Provider policy，策略修改仅走专用 Local IPC。
  saveConfig: (config: ManagerConfig) =>
    invoke<AppState>("save_config", { config }),
  setAutostart: (enabled: boolean) =>
    invoke<AppState>("set_autostart", { enabled }),
  openDashboard: () => invoke<void>("open_dashboard"),
  openLogs: () => invoke<void>("open_log_directory"),
  openExternal: (target: "docs" | "github" | "serena-desktop" | "codegraph" | "git" | "uv") =>
    invoke<void>("open_external_url", { target }),
};
