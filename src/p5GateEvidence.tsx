import { createRoot } from "react-dom/client";
import { TooltipProvider } from "@/components/ui/tooltip";
import { AgentPanel } from "./AgentPanel";
import { api } from "./api";
import "./styles.css";
import type { ExecutionView, Workspace } from "./types";

const workspace: Workspace = { id: "p5-evidence", name: "P5 UI 验收工作区", root: "E:\\p5-evidence", generation: 1 };

/** P5-005 截图专用冻结 DTO；只驱动前端展示，不代表后端实测数据。 */
function execution(overrides: Partial<ExecutionView>): ExecutionView {
  const base: ExecutionView = {
    executionId: "p5-complete", agentId: "p5-agent", workspaceId: workspace.id, canonicalWorkspaceRoot: workspace.root,
    prompt: "验证 Provider、Activity 与 Token 展示", status: "running", attention: "none", revision: "P5-R1", controlRevision: "P5-C1", activityRevision: "P5-A1",
    resultAvailable: false, provider: { id: "acme-worker", displayName: "Acme Worker", version: "2.4.1" }, providerSessionLabel: "续接会话 · S-42",
    usage: { inputTokens: 91, cachedInputTokens: 12, cacheWriteInputTokens: 7, outputTokens: 3, reasoningTokens: 5, totalTokens: 0, modelContextWindow: 128000, completeness: "complete", usageRevision: 3, updatedAt: 1_726_000_000_000 },
    progress: { phase: "finalizing", summaryCode: "execution.finalizing", activityPhase: "tool", toolCategory: "command", lastActivityAt: null, activityAgeMs: null, silenceLevel: "prolonged" },
    nextAction: { action: "observe", waitMs: 1000 }, dispatchState: "dispatched", threadId: "thread-p5", threadName: "P5 UI Gate", turnId: "turn-p5",
    providerTerminalStatus: null, errorCode: null, errorMessage: null, resultCompleteness: "none", interruptRequested: false, interruptAcknowledged: false, interruptTimedOut: false,
    createdAt: 1_726_000_000_000, updatedAt: 1_726_000_001_000, completedAt: null, availableActions: { canCancel: false, canContinue: false, canResumePending: false },
  };
  Object.assign(base, overrides);
  return base;
}

const executions = [
  execution({ executionId: "p5-complete", prompt: "完整统计 · Provider 与真实 0", status: "completed", progress: { phase: "terminal", summaryCode: "execution.finalizing", activityPhase: null, toolCategory: null, lastActivityAt: null, activityAgeMs: null, silenceLevel: null } }),
  execution({ executionId: "p5-partial", prompt: "部分统计", provider: { id: "custom-runner", displayName: "", version: null }, usage: { inputTokens: null, cachedInputTokens: null, cacheWriteInputTokens: null, outputTokens: null, reasoningTokens: null, totalTokens: 12531, modelContextWindow: null, completeness: "partial", usageRevision: 2, updatedAt: null } }),
  execution({ executionId: "p5-unknown", prompt: "未知统计", usage: { inputTokens: null, cachedInputTokens: null, cacheWriteInputTokens: null, outputTokens: null, reasoningTokens: null, totalTokens: null, modelContextWindow: null, completeness: "unknown", usageRevision: 0, updatedAt: null } }),
  execution({ executionId: "p5-zero", prompt: "真实零 Token", usage: { inputTokens: 9, cachedInputTokens: 2, cacheWriteInputTokens: 1, outputTokens: 1, reasoningTokens: 1, totalTokens: 0, modelContextWindow: 128000, completeness: "complete", usageRevision: 1, updatedAt: null } }),
  execution({ executionId: "p5-historical", prompt: "历史任务（无新字段）", provider: { id: "legacy-provider", displayName: "", version: null }, providerSessionLabel: null, usage: { inputTokens: null, cachedInputTokens: null, cacheWriteInputTokens: null, outputTokens: null, reasoningTokens: null, totalTokens: null, modelContextWindow: null, completeness: "unknown", usageRevision: 0, updatedAt: null } }),
];

// 截图入口拦截前端 API，保持组件、格式化器与真实交互路径不变。
api.agentHistory = async () => ({ executions, nextCursor: null });
api.agent = async request => {
  const executionId = "executionId" in request ? request.executionId : undefined;
  return { ok: true, data: executions.find(item => item.executionId === executionId) ?? executions[0], control: null };
};

createRoot(document.getElementById("root")!).render(
  <TooltipProvider>
    <AgentPanel workspace={workspace} workspaces={[workspace]} sidebarContainer={document.getElementById("p5-evidence-sidebar")} />
  </TooltipProvider>,
);
