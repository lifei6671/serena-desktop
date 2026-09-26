export type ServerStatus = "stopped" | "starting" | "running" | "error";

export type RemoteAccessMode = "quick_tunnel" | "self_hosted_oauth" | "mcp_only";
export type SelfHostedProvider = "custom_https" | "ngrok" | "tailscale_funnel";
export type SecurityDeclaration = "external_auth" | "none";
export interface RemoteAccessConfig {
  mode: RemoteAccessMode;
  quickTunnelDesiredRunning: boolean;
  selfHosted: { provider: SelfHostedProvider; publicOrigin: string | null };
  mcpOnly: { securityDeclaration: SecurityDeclaration; publicOrigin: string | null };
}
export interface RemoteApproval {
  id: string;
  clientName: string;
  clientIdHostname: string | null;
  refreshAllowed: boolean;
  redirectUri: string;
  confirmationCode: string;
  scope: string;
  expiresInSeconds: number;
}
export interface RemoteState {
  config: RemoteAccessConfig;
  mode: RemoteAccessMode;
  status: "stopped" | "starting" | "installing" | "discovering_url" | "verifying" | "ready" | "stopping" | "error" | "disconnected";
  publicContext: { publicOrigin: string; mcpResource: string; instanceId: string } | null;
  lastError: string | null;
  authorizedClients: number;
  pending: RemoteApproval[];
  active: boolean;
  startedAt: number | null;
  ngrokAuthConfigured: boolean;
}

export interface ManagerConfig {
  remoteAccess: RemoteAccessConfig;
  agentEnabled: boolean;
  remoteSourceWriteEnabled: boolean;
  remoteCommandExecutionEnabled: boolean;
  agentSuccessNotificationEnabled: boolean;
  agentFailureNotificationEnabled: boolean;
  agentSystemNotificationEnabled: boolean;
  agentSoundEnabled: boolean;
  broker: { enabled: boolean; port: number; allowLan: boolean };
  workspaces: Workspace[];
  workspaceRegistryRevision: number;
  desktopSelectedWorkspaceId: string | null;
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
  desktopSelectedWorkspace: Workspace | null;
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
  generation: number;
}

export interface WorkspaceInspection {
  canonicalRoot: string;
  folderBasename: string | null;
}

export interface WorkspaceRegistrySnapshot {
  registryRevision: number;
  workspaces: Workspace[];
}

/** Capability 安装探测状态，与 workspace 准备和 Runtime 生命周期正交。 */
export type CapabilityInstallationState = "installed" | "not_installed" | "check_failed";
/** Capability 的 workspace 准备状态。 */
export type CapabilityReadinessState = "not_prepared" | "ready" | "degraded" | "error" | "unknown";
/** Provider 可用性状态。 */
export type CapabilityAvailability = "ready" | "unavailable" | "error";
/** Provider Runtime 生命周期状态。 */
export type CapabilityRuntimeState = "stopped" | "starting" | "ready" | "error" | "stopping";
/** Descriptor stage 的准备要求。 */
export type CapabilityStageRequirement = "required" | "optional";
/** Descriptor stage 的安全状态投影。 */
export type CapabilityStageState = "absent" | "pending" | "running" | "ready" | "stale" | "error" | "unknown";
/** Descriptor 驱动的单个 Capability stage。 */
export interface CapabilityStage {
  id: string;
  displayName: string;
  state: CapabilityStageState;
  requirement: CapabilityStageRequirement;
  messageCode: string | null;
}

/** 单个 Provider 的安全 Health 投影。 */
export interface WorkspaceProviderHealth {
  displayName: string;
  installation: CapabilityInstallationState;
  readiness: CapabilityReadinessState;
  status: CapabilityAvailability;
  runtimeState: CapabilityRuntimeState;
  checkedAt: number;
  stages: CapabilityStage[];
}

/** 显式 workspace 的 Capability Health DTO。 */
export interface WorkspaceCapabilityHealth {
  workspaceId: string;
  providers: Record<string, WorkspaceProviderHealth>;
}

export interface BrokerState {
  running: boolean;
  startedAt: number | null;
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
  | { action: "observe"; executionId: string; knownRevision?: string; knownControlRevision?: string; waitMs?: number; includeResult?: boolean; wakeOn?: "control" | "activity" }
  | { action: "cancel" | "resume_pending"; executionId: string }
  | { action: "list"; agentId?: string; workspaceId?: string; limit?: number };
export interface ExecutionView {
  prompt: string; canonicalWorkspaceRoot: string;
  executionId: string; agentId: string; workspaceId: string; status: string;
  provider: { id: string; displayName: string; version: string | null };
  taskRole: "development" | "testing" | "review" | "analysis" | "general";
  usage: {
    inputTokens: number | null; cachedInputTokens: number | null; cacheWriteInputTokens: number | null;
    outputTokens: number | null; reasoningTokens: number | null; totalTokens: number | null;
    modelContextWindow: number | null; completeness: "unknown" | "partial" | "complete";
    usageRevision: number; updatedAt: number | null;
  };
  dispatchState: string; threadId: string | null; threadName: string | null; turnId: string | null;
  providerSessionLabel: string | null;
  providerTerminalStatus: string | null; resultCompleteness: string; finalResult?: unknown;
  /** Last diagnostic, independent of lifecycle status and Provider terminal. */
  errorCode: string | null; errorMessage: string | null;
  /** Legacy alias of controlRevision. */
  revision: string; controlRevision: string; activityRevision: string;
  unchanged?: boolean;
  /** Observe 的唤醒原因仅作兼容投影，不参与页面控制判断。 */
  wakeReason?: "initial_mismatch" | "control" | "activity" | "terminal" | "result" | "timeout";
  /** 首次 Observe token 不匹配类别，仅作兼容投影。 */
  mismatchKind?: "control" | "activity";
  resultAvailable: boolean;
  progress: {
    phase: "pending" | "dispatching" | "running" | "finalizing" | "reconciling" | "terminal";
    summaryCode: string | null;
    activityPhase: "provider" | "tool" | null;
    toolCategory: "build" | "test" | "command" | "read" | "edit" | "tool" | null;
    lastActivityAt: number | null;
    activityAgeMs: number | null;
    silenceLevel: "fresh" | "quiet" | "prolonged" | null;
  };
  nextAction: { action: "observe"; waitMs: number } | { action: "review_result"; includeResult: boolean }
    | { action: "continue" | "resume_pending" | "manual_resolution" } | null;
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
