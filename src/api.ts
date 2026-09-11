import { invoke } from "@tauri-apps/api/core";
import type { AppState, ManagerConfig, BrokerState, AgentAction, AgentEnvelope, ExecutionView, RemoteState, RemoteAccessMode, SecurityDeclaration } from "./types";

export const api = {
  remoteState: () => invoke<RemoteState>("remote_state"),
  remoteStart: (mode: RemoteAccessMode, publicOrigin?: string, securityDeclaration?: SecurityDeclaration, riskAccepted = false) => invoke<void>("remote_start", { mode, publicOrigin, securityDeclaration, riskAccepted }),
  remoteStop: () => invoke<void>("remote_stop"),
  remoteProbe: () => invoke<void>("remote_probe"),
  remoteApprove: (id: string, allow: boolean) => invoke<void>("remote_approve", { id, allow }),
  codexVersion: () => invoke<string>("get_codex_version"),
  agent: (request: AgentAction) => invoke<AgentEnvelope>("agent_operation", { request }),
  agentHistory: (before: string | null = null, workspace: string | null = null) => invoke<{ executions: ExecutionView[]; nextCursor: string | null }>("agent_history", { before, workspace }),
  mcpLogs: () => invoke<string[]>("get_mcp_logs"),
  downloadMcpLogs: () => invoke<boolean>("download_mcp_logs"),
  clearMcpLogs: () => invoke<void>("clear_mcp_logs"),
  broker: () => invoke<BrokerState>("get_broker_state"),
  syncProjects: () => invoke<number>("sync_workspaces"),
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
  saveConfig: (config: ManagerConfig) =>
    invoke<AppState>("save_config", { config }),
  setAutostart: (enabled: boolean) =>
    invoke<AppState>("set_autostart", { enabled }),
  openDashboard: () => invoke<void>("open_dashboard"),
  openLogs: () => invoke<void>("open_log_directory"),
  openExternal: (target: "docs" | "github" | "codegraph" | "git" | "uv") =>
    invoke<void>("open_external_url", { target }),
};
