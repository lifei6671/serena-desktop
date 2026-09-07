import { invoke } from "@tauri-apps/api/core";
import type { AppState, ManagerConfig, BrokerState } from "./types";

export const api = {
  mcpLogs: () => invoke<string[]>("get_mcp_logs"),
  broker: () => invoke<BrokerState>("get_broker_state"),
  syncProjects: () => invoke<number>("sync_workspaces"),
  activateProject: (id: string) => invoke<void>("activate_workspace", { id }),
  deactivateProject: () => invoke<void>("deactivate_workspace"),
  cancelProject: () => invoke<void>("cancel_workspace_operation"),
  setBroker: (enabled: boolean, port: number) =>
    invoke<void>("set_broker", { enabled, port }),
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
  openExternal: (target: "docs" | "github" | "git" | "uv") =>
    invoke<void>("open_external_url", { target }),
};
