export type ServerStatus = "stopped" | "starting" | "running" | "error";

export interface ManagerConfig {
  agentEnabled: boolean;
  broker: { enabled: boolean; port: number; allowLan: boolean };
  workspaces: Workspace[];
  serenaPath: string | null;
  port: number;
  dashboardEnabled: boolean;
  openDashboardOnLaunch: boolean;
  autoStartServer: boolean;
  minimizeToTray: boolean;
}

export interface SerenaInstallation {
  state: "missing" | "standard" | "invalid";
  source: "managed" | "external" | "path";
  context: string | null;
  error: string | null;
  path: string;
  version: string;
}

export interface AppState {
  codegraphVersion: string | null;
  config: ManagerConfig;
  git: {
    status: "available" | "missing" | "error";
    available: boolean;
    path: string | null;
    version: string | null;
    error: string | null;
  };
  managedRuntimePresent: boolean;
  installation: SerenaInstallation | null;
  activeInstallation: SerenaInstallation | null;
  serverStatus: ServerStatus;
  managedProcessPresent: boolean;
  activePort: number;
  endpoint: string;
  dashboardUrl: string;
  dashboardEnabled: boolean;
  logDirectory: string;
  autostartEnabled: boolean | null;
  autostartError: string | null;
  lastError: string | null;
}

export interface Workspace {
  id: string;
  name: string;
  root: string;
}
export interface BrokerState {
  running: boolean;
  port: number;
  listenAddress: string;
  lanEndpoints: string[];
  activeWorkspace: Workspace | null;
  codegraph: {
    status:
      | "ready"
      | "starting"
      | "not_initialized"
      | "unavailable"
      | "start_failed"
      | "runtime_lost";
    workspaceId: string;
    root: string;
    generation: number;
  } | null;
  projectSources: string[];
  syncWarnings: string[];
  projects: (Workspace & { configured: boolean })[];
  operation: string | null;
  lastError: string | null;
}

export type AgentAction =
  | { action: "start"; workspaceId: string; agentId: string; requestKey: string; prompt: string }
  | { action: "continue"; executionId: string; requestKey: string; prompt: string }
  | { action: "observe"; executionId: string; knownRevision?: string; waitMs?: number; includeResult?: boolean }
  | { action: "cancel" | "resume_pending"; executionId: string }
  | { action: "list"; agentId?: string; workspaceId?: string; limit?: number };
export interface ExecutionView {
  prompt: string; canonicalWorkspaceRoot: string;
  executionId: string; agentId: string; workspaceId: string; status: string;
  dispatchState: string; threadId: string | null; turnId: string | null;
  providerTerminalStatus: string | null; resultCompleteness: string; finalResult?: unknown;
  revision: string; unchanged?: boolean; resultAvailable: boolean;
  progress: { phase: "pending" | "dispatching" | "running" | "finalizing" | "reconciling" | "terminal" };
  nextAction: { action: "observe"; waitMs: number } | { action: "review_result"; includeResult: boolean }
    | { action: "resume_pending" | "manual_resolution" } | null;
  interruptRequested: boolean; interruptAcknowledged: boolean; interruptTimedOut: boolean;
  attention: "none" | "pending_explicit_resume" | "manual_resolution_required";
  availableActions: { canCancel: boolean; canContinue: boolean; canResumePending: boolean };
  createdAt: number; updatedAt: number; completedAt: number | null;
}
export interface ControlReceipt {
  requestAccepted: boolean;
  providerInvoked: boolean | null;
  dispatchCertainty: "not_dispatched" | "dispatched" | "uncertain";
  nextAction: ((NonNullable<ExecutionView["nextAction"]> | { action: "correct_input" | "activate_workspace" | "list" }) & { executionId?: string }) | null;
}
export type AgentEnvelope = { ok: true; data: ExecutionView | { executions: ExecutionView[] }; control: ControlReceipt | null }
  | { ok: false; error: { code: string; message: string; executionId?: string }; control: ControlReceipt | null };
