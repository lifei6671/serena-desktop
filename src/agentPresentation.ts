import type { ExecutionView, Workspace } from "./types";

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

export function taskSummary(prompt: string) {
  const text = prompt.replace(/\s+/gu, " ").trim();
  const chars = Array.from(text);
  return chars.length > 100 ? `${chars.slice(0, 100).join("")}…` : text;
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
