import { MarkdownContent } from "@/components/MarkdownContent";
import { useEffect, useState, type ReactNode } from "react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Check, Copy, Play, ShieldAlert } from "lucide-react";
import { activityLabel, activitySilenceLabel, executionDuration, executionStatus, formatTokenCount, providerLabel, recentActivity, resultText, taskTitle, usageCompletenessLabel } from "./agentPresentation";
import { agentRequests } from "./agentRequests";
import type { AgentAction, ExecutionView } from "./types";

type CopyTarget = "prompt" | "result" | "technical";
type CopyState = "idle" | "copying" | "copied";

function CopyFeedback({ state, label }: { state: CopyState; label: string }) {
  return <>
    <span className="agent-copy-icon-slot" aria-hidden="true">
      <Copy className="agent-copy-icon-copy" />
      <Check className="agent-copy-icon-check" />
    </span>
    <span className="agent-copy-label">{state === "copying" ? "复制中…" : state === "copied" ? "已复制" : label}</span>
  </>;
}

function timeWithSeconds(value: number) {
  const date = new Date(value);
  const pad = (part: number) => String(part).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`;
}

function technicalValue(value: string | number | null | undefined) {
  return value === null || value === undefined || value === "" ? "—" : String(value);
}

export function ExecutionDetails({ row, workspaceName, loading, error, disabled, feedback, busy, onReload, onOperate, onManualResolve }: {
  feedback: ReactNode; busy: boolean;
  row: ExecutionView; workspaceName: string; loading: boolean; error: string; disabled: boolean;
  onReload: () => void; onOperate: (action: AgentAction) => Promise<boolean>;
  onManualResolve: (executionId: string) => Promise<boolean>;
}) {
  const [draft, setDraft] = useState({ executionId: row.executionId, text: "" });
  const continuation = draft.executionId === row.executionId ? draft.text : "";
  const [copying, setCopying] = useState<CopyTarget | null>(null);
  const [copied, setCopied] = useState<CopyTarget | null>(null);
  const [manualResolutionOpen, setManualResolutionOpen] = useState(false);
  useEffect(() => {
    if (copied === null) return;
    const timer = window.setTimeout(() => setCopied(null), 1600);
    return () => window.clearTimeout(timer);
  }, [copied]);
  const state = executionStatus(row);
  const running = state.label === "执行中";
  const result = resultText(row.finalResult);
  const duration = executionDuration(row);
  const isActiveExecution = row.status === "running" || (row.status === "dispatch_pending" && (row.progress?.phase === "dispatching" || row.progress?.phase === "running"));
  const runningDuration = isActiveExecution ? `已运行 ${duration}` : `耗时 ${duration}`;
  const durationLabel = isActiveExecution ? "已运行" : "总耗时";
  const showResult = row.resultAvailable || row.status === "completed";
  const provider = providerLabel(row);
  const usageCompleteness = usageCompletenessLabel(row.usage.completeness);
  const technicalFields: Array<[string, string | number | null | undefined]> = [
    ["Execution ID", row.executionId], ["Agent ID", row.agentId], ["Workspace ID", row.workspaceId], ["Provider", provider], ["Provider Session", row.providerSessionLabel], ["Dispatch State", row.dispatchState],
    ["Thread ID", row.threadId], ["Thread Name", row.threadName], ["Turn ID", row.turnId], ["Provider Terminal Status", row.providerTerminalStatus],
    ["Result Completeness", row.resultCompleteness], ["Control Revision", row.controlRevision], ["Activity Revision", row.activityRevision], ["Next Action", row.nextAction?.action],
  ];
  async function copy(target: CopyTarget, text: string, successMessage: string) {
    setCopied(null);
    setCopying(target);
    try { await navigator.clipboard.writeText(text); setCopied(target); toast.success(successMessage); }
    catch (copyError) { toast.error(`复制失败：${String(copyError)}`); }
    finally { setCopying(null); }
  }
  const copyState = (target: CopyTarget): CopyState => copied === target ? "copied" : copying === target ? "copying" : "idle";
  const continuationForm = <div className="agent-continuation-card">
    <div className="agent-continuation-heading"><div><h2>继续任务</h2><p>在当前任务的原工作区和对话上下文中开始下一次执行。</p></div></div>
    <form onSubmit={event => { event.preventDefault(); if (!disabled && !loading && !error && continuation.trim()) void onOperate(agentRequests.continuation(row.executionId, continuation)).then(ok => { if (ok) setDraft({ executionId: row.executionId, text: "" }); }); }}>
      <label className="sr-only" htmlFor="agent-continuation">后续任务内容</label>
      <textarea id="agent-continuation" className="agent-input" value={continuation} disabled={disabled} onChange={event => setDraft({ executionId: row.executionId, text: event.target.value })} placeholder="在当前任务上下文中开始新的后续 Execution…" />
      <footer className="agent-continuation-footer"><span>{provider} · 当前工作区 ({workspaceName}) · 继承当前上下文</span><Button className="agent-detail-primary" disabled={disabled || loading || !!error || !continuation.trim()} type="submit"><Play aria-hidden="true" />继续任务</Button></footer>
    </form>
  </div>;

  return <section className="agent-detail" aria-label="任务详情">
    <header className="agent-detail-header">
      <div>
        <div className="agent-detail-title"><h1>{taskTitle(row)}</h1><span className={`agent-status tone-${state.tone}`}><i className={running ? "agent-task-pulse" : undefined} aria-hidden="true" />{state.label}</span></div>
        <p className="agent-detail-meta"><span>工作区: <code>{workspaceName}</code></span><span aria-hidden="true">·</span><span>创建于 {timeWithSeconds(row.createdAt)}</span><span aria-hidden="true">·</span><span>{runningDuration}</span><span aria-hidden="true">·</span><span>引擎: <strong>{provider}</strong></span></p>
      </div>
      <div className="agent-detail-actions">
        {row.availableActions.canResumePending && <Button variant="outline" disabled={disabled || loading || !!error} onClick={() => void onOperate({ action: "resume_pending", executionId: row.executionId })}>恢复任务</Button>}
        {row.availableActions.canCancel && <Button className="agent-cancel-action" variant="outline" disabled={disabled || loading || !!error} onClick={() => void onOperate({ action: "cancel", executionId: row.executionId })}>取消任务</Button>}
        {row.attention === "manual_resolution_required" && <Button variant="destructive" disabled={disabled || loading || !!error} onClick={() => setManualResolutionOpen(true)}>人工结束并释放工作区</Button>}
      </div>
    </header>
    <div className="agent-detail-body" aria-busy={loading}>
      {feedback}
      {busy && <p role="status" className="agent-detail-notice">正在处理请求…</p>}
      {loading && <p role="status" className="agent-detail-notice">正在更新详情…</p>}
      {error && <div role="alert" className="agent-notice"><p>{error}</p><Button variant="outline" disabled={loading} onClick={onReload}>重新加载详情</Button></div>}
      <section className="agent-detail-section agent-task-content">
        <header><h2>任务内容</h2><Button className="agent-copy-text" variant="ghost" size="sm" disabled={copying !== null} data-copy-state={copyState("prompt")} onClick={() => void copy("prompt", row.prompt, "任务内容已复制")}><CopyFeedback state={copyState("prompt")} label="复制内容" /></Button></header>
        <div className="agent-task-prompt"><MarkdownContent className="agent-prose">{row.prompt}</MarkdownContent></div>
      </section>
      <section className="agent-detail-section">
        <h2>执行信息</h2>
        <div className="agent-detail-info-card">
          <div className="agent-detail-live-grid">
            <div><span>执行状态</span><strong className={`agent-status tone-${state.tone}`}><i className={running ? "agent-task-pulse" : undefined} aria-hidden="true" />{state.label}</strong></div>
            <div><span>Provider</span><strong>{provider}</strong>{row.providerSessionLabel && <code>{row.providerSessionLabel}</code>}</div>
            <div><span>当前活动</span><strong>{activityLabel(row)}</strong></div>
            <div><span>最近活动</span><strong>{recentActivity(row)}</strong></div>
            <div><span>活跃状态</span><strong>{activitySilenceLabel(row)}</strong></div>
            <div><span>{durationLabel}</span><code>{duration}</code></div>
          </div>
          <div className="agent-detail-facts-grid">
            <div className="agent-detail-location"><span>执行位置</span><div><strong>{workspaceName}</strong><code>{row.canonicalWorkspaceRoot}</code></div></div>
            <div className="agent-detail-time-grid">
              <div><span>创建时间</span><code>{timeWithSeconds(row.createdAt)}</code></div><div><span>更新时间</span><code>{timeWithSeconds(row.updatedAt)}</code></div>
              {row.completedAt !== null && <div><span>结束时间</span><code>{timeWithSeconds(row.completedAt)}</code></div>}
            </div>
          </div>
        </div>
      </section>
      <section className="agent-detail-section agent-usage-section">
        <h2>Token 用量</h2>
        <div className="agent-usage-card">
          <div className="agent-usage-total"><span>Total Tokens</span><strong>{formatTokenCount(row.usage.totalTokens)}</strong><em data-completeness={row.usage.completeness}>{usageCompleteness}</em></div>
          <div className="agent-usage-grid">
            <div><span>Input</span><strong>{formatTokenCount(row.usage.inputTokens)}</strong></div>
            <div><span>Cached Input</span><strong>{formatTokenCount(row.usage.cachedInputTokens)}</strong></div>
            <div><span>Cache Write</span><strong>{formatTokenCount(row.usage.cacheWriteInputTokens)}</strong></div>
            <div><span>Output</span><strong>{formatTokenCount(row.usage.outputTokens)}</strong></div>
            <div><span>Reasoning</span><strong>{formatTokenCount(row.usage.reasoningTokens)}</strong></div>
            <div><span>Context Window</span><strong>{formatTokenCount(row.usage.modelContextWindow)}</strong></div>
          </div>
          {row.usage.updatedAt !== null && <small>统计更新于 {timeWithSeconds(row.usage.updatedAt)}</small>}
        </div>
      </section>
      {(row.attention !== "none" || ["failed", "reconciling", "interrupted"].includes(row.status) || row.interruptTimedOut || row.errorCode || row.errorMessage) && <section className="agent-detail-section agent-recovery-section">
        <h2><ShieldAlert aria-hidden="true" />恢复 / 错误信息</h2><div className="agent-detail-warning"><p>{state.description}</p>
          {row.attention === "pending_explicit_resume" && <p>恢复将继续此任务的原始输入和执行目录。请确认该目录当前仍适合执行。</p>}
          {row.attention === "manual_resolution_required" && <p>需要人工处理：系统无法自动证明上一次 Runtime 的最终状态。</p>}
          {row.interruptTimedOut && <p>取消请求确认超时；这不代表任务已经停止。</p>}
          {(row.errorCode || row.errorMessage) && <p className="agent-real-error">{row.errorCode && <code>{row.errorCode}</code>}{row.errorMessage && <span>{row.errorMessage}</span>}</p>}
        </div>
      </section>}
      {showResult && <section className="agent-detail-section agent-result-section">
        <header><div><h2>执行结果</h2><span className={`agent-status tone-${state.tone}`}>{row.status === "completed" ? `已完成 · 耗时 ${duration}` : `${state.label} · ${runningDuration}`}</span></div><Button className="agent-copy-text" variant="ghost" size="sm" disabled={!result || copying !== null} data-copy-state={copyState("result")} onClick={() => void copy("result", result, "执行结果已复制")}><CopyFeedback state={copyState("result")} label="复制结果" /></Button></header>
        <div className="agent-result-card"><MarkdownContent className="agent-result">{result || (row.resultAvailable && row.finalResult === undefined ? (loading ? "正在读取最终结果…" : "结果正文尚未读取，请刷新详情重试。") : "未提供可读的最终文本，可在技术信息中查看已有结果。")}</MarkdownContent></div>
      </section>}
      {row.availableActions.canContinue && <section className="agent-detail-section agent-continuation-section">{continuationForm}</section>}
      <section className="agent-detail-section agent-technical-section"><details className="agent-technical"><summary><span>技术信息</span><span onClick={event => { event.preventDefault(); event.stopPropagation(); }}><Button className="agent-technical-copy" variant="outline" size="sm" disabled={copying !== null} data-copy-state={copyState("technical")} onClick={() => void copy("technical", JSON.stringify(row, null, 2), "技术信息已复制")}><CopyFeedback state={copyState("technical")} label="复制技术信息" /></Button></span></summary>
        <div className="agent-technical-content"><div className="agent-technical-grid">{technicalFields.map(([label, value]) => <div key={label}><span>{label}</span><code>{technicalValue(value)}</code></div>)}</div>
          <details className="agent-raw-json"><summary>展开原始 Execution 数据 (JSON)</summary><div className="agent-json"><pre tabIndex={0} aria-label="原始执行数据">{JSON.stringify(row, null, 2)}</pre></div></details>
        </div>
      </details></section>
      <Dialog open={manualResolutionOpen} onOpenChange={open => { if (!busy) setManualResolutionOpen(open); }}>
        <DialogContent showCloseButton={!busy}>
          <DialogHeader><DialogTitle>确认人工结束并释放工作区？</DialogTitle>
            <DialogDescription>系统无法自动证明上一次 Runtime 的最终状态。只有在确认该执行不会继续修改工作区时才能继续。</DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" disabled={busy} onClick={() => setManualResolutionOpen(false)}>返回</Button>
            <Button variant="destructive" disabled={busy} onClick={() => void onManualResolve(row.executionId).then(ok => { if (ok) setManualResolutionOpen(false); })}>{busy ? "正在处理…" : "确认结束并释放"}</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  </section>;
}
