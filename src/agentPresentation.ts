import type { ExecutionView, Workspace } from "./types";

type ProviderDisplaySource = { provider?: { id?: string | null; displayName?: string | null; version?: string | null } | null };
type ActivityDisplaySource = Pick<ExecutionView, "progress">;

// Presentation only. Never use these labels or tones to authorize an operation.
const states: Record<string, { label: string; description: string; tone: string }> = {
  dispatch_pending: { label: "等待执行", description: "任务已保存，等待发送给 Agent。", tone: "amber" },
  running: { label: "执行中", description: "Codex 正在处理任务。", tone: "blue" },
  cancel_requested: { label: "正在取消", description: "已提交取消请求，等待执行结束。", tone: "blue" },
  cancelling: { label: "正在取消", description: "正在停止任务并确认执行状态。", tone: "blue" },
  finalizing: { label: "正在整理结果", description: "正在整理结果并完成收尾。", tone: "blue" },
  reconciling: { label: "正在恢复执行状态", description: "正在核对上次执行，请等待状态确认。", tone: "amber" },
  completed: { label: "已完成", description: "任务已完成，可查看执行结果。", tone: "green" },
  failed: { label: "执行失败", description: "任务未能完成，请查看详情。", tone: "red" },
  cancelled: { label: "已取消", description: "任务已取消。", tone: "gray" },
  interrupted: { label: "已中断", description: "执行已中断，可查看已保存的信息。", tone: "gray" },
  unknown: { label: "需要处理", description: "需要人工处理：无法确认上次执行已安全结束。", tone: "red" },
};

export function executionStatus(row: ExecutionView) {
  if (row.attention === "manual_resolution_required") return states.unknown;
  if (row.attention === "pending_explicit_resume") return {
    label: "等待恢复", description: "任务尚未派发。恢复前请确认继续原工作区中的任务。", tone: "amber",
  };
  if (row.status === "dispatch_pending") {
    if (row.progress?.phase === "dispatching") return {
      label: "正在派发", description: "正在发送任务，尚未确认 Provider 已接收。", tone: "blue",
    };
    if (row.progress?.phase === "reconciling") return states.reconciling;
    if (row.progress?.phase === "running") return states.running;
  }
  return states[row.status] ?? { label: "状态待确认", description: "请查看技术详情中的原始状态。", tone: "gray" };
}

/** 仅由当前状态、关注要求和取消超时决定是否展示恢复或错误区块。 */
export function showExecutionIssueSection(row: Pick<ExecutionView, "attention" | "status" | "interruptTimedOut">) {
  return row.attention !== "none" || ["failed", "unknown", "reconciling", "interrupted"].includes(row.status) || row.interruptTimedOut;
}

/** 历史诊断仅在当前 Execution 确实需要处理时作为具体错误展示。 */
export function showExecutionDiagnostic(row: Pick<ExecutionView, "attention" | "status" | "interruptTimedOut" | "errorCode" | "errorMessage">) {
  return showExecutionIssueSection(row) && !!(row.errorCode || row.errorMessage);
}

/** 将冻结的 Provider descriptor 转换为纯展示标签，不参与任何控制判断。 */
export function providerLabel(row: ProviderDisplaySource) {
  const provider = row.provider;
  const name = provider?.displayName?.trim() || provider?.id?.trim() || "未知 Provider";
  const version = provider?.version?.trim();
  return version ? `${name} · v${version}` : name;
}

/** 将 Product 的封闭 summaryCode 优先映射为安全的当前活动文案。 */
export function activityLabel(row: ActivityDisplaySource) {
  const progress = row.progress;
  const summaryLabels: Record<string, string> = {
    "execution.finalizing": "正在整理结果", "execution.reconciling": "正在恢复执行状态",
    "provider.processing": "Agent 处理中", "tool.read": "正在读取", "tool.edit": "正在修改文件",
    "tool.command": "正在执行命令", "tool.build": "正在构建", "tool.test": "正在测试", "tool.other": "正在调用工具",
  };
  const summary = progress.summaryCode === null ? undefined : summaryLabels[progress.summaryCode];
  if (summary) return summary;
  const phaseLabels: Record<string, string> = {
    pending: "等待执行", dispatching: "正在派发", running: "执行中", finalizing: "正在整理结果", reconciling: "正在恢复执行状态", terminal: "已结束",
  };
  const phase = phaseLabels[progress.phase];
  if (phase) return phase;
  const categoryLabels: Record<string, string> = {
    read: "正在读取", edit: "正在修改文件", command: "正在执行命令", build: "正在构建", test: "正在测试", tool: "正在调用工具",
  };
  return progress.toolCategory === null ? "暂无活动数据" : categoryLabels[progress.toolCategory] ?? "暂无活动数据";
}

/** 仅根据 silenceLevel 呈现中性的活动间隔，不把时间间隔解释为错误。 */
export function activitySilenceLabel(row: ActivityDisplaySource) {
  const labels: Record<string, string> = { fresh: "刚刚有活动", quiet: "暂时没有新活动", prolonged: "一段时间没有新活动" };
  return labels[row.progress.silenceLevel ?? ""] ?? "暂无活动数据";
}

/** 格式化活动年龄；传入时间只用于展示，不改变后端的 silence 语义。 */
export function recentActivity(row: ActivityDisplaySource, now = Date.now()) {
  const progress = row.progress;
  const age = progress.activityAgeMs ?? (progress.lastActivityAt === null || progress.lastActivityAt === undefined ? null : Math.max(0, now - progress.lastActivityAt));
  if (age === null) return "暂无活动数据";
  if (age < 5_000) return "刚刚";
  const seconds = Math.floor(age / 1_000);
  if (seconds < 60) return `${seconds}秒前`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}分钟前`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}小时前`;
  return progress.lastActivityAt === null || progress.lastActivityAt === undefined ? "暂无活动数据" : executionTime(progress.lastActivityAt);
}

/** 格式化事实 Token 值；null 与真实零必须保持语义不同。 */
export function formatTokenCount(value: number | null | undefined) {
  return value === null || value === undefined ? "—" : new Intl.NumberFormat("zh-CN").format(value);
}

/** 仅展示 Summary DTO 的总 Token；未知和真实零保持不同语义。 */
export function usageTotalLabel(row: Pick<ExecutionView, "usage">) {
  const { completeness, totalTokens } = row.usage;
  if (completeness === "unknown" || totalTokens === null || totalTokens === undefined) return "—";
  const total = formatTokenCount(totalTokens);
  return completeness === "partial" ? `${total} · ${usageCompletenessLabel(completeness)}` : total;
}

/** 将后端 completeness 枚举转换为展示文本，不从数字字段重新推导。 */
export function usageCompletenessLabel(completeness: ExecutionView["usage"]["completeness"]) {
  return ({ unknown: "未知", partial: "统计不完整", complete: "完整" } as const)[completeness];
}

export function taskSummary(prompt: string) {
  const text = prompt.replace(/\s+/gu, " ").trim();
  const chars = Array.from(text);
  return chars.length > 100 ? `${chars.slice(0, 100).join("")}…` : text;
}

export function taskTitle(row: Pick<ExecutionView, "threadName" | "prompt">) {
  return row.threadName?.trim() || taskSummary(row.prompt) || "未命名任务";
}

export function resultText(result: unknown): string {
  if (typeof result === "string") return result;
  if (!result || typeof result !== "object" || !("finalResult" in result) || !Array.isArray(result.finalResult)) return "";
  // The persisted RecoveredResult contains agentMessage items with their real phase.
  const messages = result.finalResult.filter((item): item is { type: string; text: string; phase?: string | null } =>
    !!item && typeof item === "object" && item.type === "agentMessage" && typeof item.text === "string");
  const final = messages.filter(item => item.phase === "final_answer");
  return (final.length ? final : messages.filter(item => item.phase == null)).map(item => item.text).join("\n\n");
}

export function executionTime(time: number) {
  const date = new Date(time);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

export function executionDuration(row: Pick<ExecutionView, "createdAt" | "completedAt">, now = Date.now()) {
  let seconds = Math.max(0, Math.floor(((row.completedAt ?? now) - row.createdAt) / 1000));
  const parts: string[] = [];
  for (const [unit, size] of [["天", 86400], ["小时", 3600], ["分", 60], ["秒", 1]] as const) {
    const count = Math.floor(seconds / size);
    if (count) parts.push(`${count}${unit}`);
    seconds %= size;
  }
  return parts.join("") || "不足1秒";
}

export function executionWorkspace(row: Pick<ExecutionView, "canonicalWorkspaceRoot">, workspaces: Workspace[]) {
  const normalize = (root: string) => root.replace(/^\\\\\?\\/, "").replaceAll("\\", "/").replace(/\/$/, "").toLowerCase();
  return workspaces.find(w => normalize(w.root) === normalize(row.canonicalWorkspaceRoot))?.name
    ?? row.canonicalWorkspaceRoot.replace(/[\\/]+$/, "").split(/[\\/]/).pop()
    ?? row.canonicalWorkspaceRoot;
}
