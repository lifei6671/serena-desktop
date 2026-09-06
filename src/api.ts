import { invoke } from "@tauri-apps/api/core";
import type { AppState, ManagerConfig } from "./types";

export const api = {
  getState: () => invoke<AppState>("get_app_state"),
  detect: () => invoke<AppState>("detect_serena"),
  install: () => invoke<AppState>("install_serena"),
  start: () => invoke<AppState>("start_serena"),
  stop: () => invoke<AppState>("stop_serena"),
  restart: () => invoke<AppState>("restart_serena"),
  saveConfig: (config: ManagerConfig) => invoke<AppState>("save_config", { config }),
  setAutostart: (enabled: boolean) =>
    invoke<AppState>("set_autostart", { enabled }),
  openDashboard: () => invoke<void>("open_dashboard"),
  openLogs: () => invoke<void>("open_log_directory"),
  openExternal: (target: "docs" | "github") =>
    invoke<void>("open_external_url", { target }),
};
