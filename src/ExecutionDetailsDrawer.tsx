import { useEffect, useState, type ReactNode } from "react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Dialog, DialogPortal, DialogOverlay, DialogTitle, DialogDescription } from "@/components/ui/dialog";
import { Dialog as DialogPrimitive } from "radix-ui";
import { X, Copy, Check } from "lucide-react";
import { executionStatus, resultText } from "./agentPresentation";
import { agentRequests } from "./agentRequests";
import type { AgentAction, ExecutionView } from "./types";

export function ExecutionDetailsDrawer({ row, loading, error, disabled, feedback, busy, onClose, onRestoreFocus, onReload, onOperate }: {
  feedback: ReactNode; busy: boolean;
  row: ExecutionView; loading: boolean; error: string; disabled: boolean;
  onClose: () => void; onRestoreFocus: () => void; onReload: () => void; onOperate: (action: AgentAction) => Promise<boolean>;
}) {
  const [draft, setDraft] = useState({ executionId: row.executionId, text: "" });
  const continuation = draft.executionId === row.executionId ? draft.text : "";
  const [copying, setCopying] = useState(false);
  const [copiedExecution, setCopiedExecution] = useState<string | null>(null);
  const copied = copiedExecution === row.executionId;
  useEffect(() => {
    if (copiedExecution === null) return;
    const timer = window.setTimeout(() => setCopiedExecution(null), 1600);
    return () => window.clearTimeout(timer);
  }, [copiedExecution]);
  const state = executionStatus(row);
  const result = resultText(row.finalResult);
  async function copy() {
    setCopying(true);
    try { await navigator.clipboard.writeText(JSON.stringify(row, null, 2)); setCopiedExecution(row.executionId); toast.success("技术详情已复制"); }
    catch (e) { toast.error(`复制失败：${String(e)}`); }
    finally { setCopying(false); }
  }
  return <Dialog open onOpenChange={open => { if (!open) onClose(); }}>
    <DialogPortal>
      <DialogOverlay className="agent-drawer-overlay" />
      <DialogPrimitive.Content className="agent-drawer" onCloseAutoFocus={event => { event.preventDefault(); onRestoreFocus(); }}>
        <header className="agent-drawer-heading">
          <div><DialogTitle>任务详情</DialogTitle><DialogDescription>查看原始任务、结果与执行信息</DialogDescription></div>
          <DialogPrimitive.Close asChild><Button variant="ghost" size="icon" aria-label="关闭任务详情"><X /></Button></DialogPrimitive.Close>
        </header>
        <div className="agent-drawer-body" aria-busy={loading}>
          {feedback}
          {busy && <p role="status">正在处理请求…</p>}
          {loading && <p role="status">正在更新详情…</p>}
          {error && <div role="alert" className="agent-notice"><p>{error}</p><Button variant="outline" disabled={loading} onClick={onReload}>重新加载详情</Button></div>}
          <section><h3>任务</h3><div className="agent-prose">{row.prompt}</div></section>
          <section><h3>状态</h3><span className={`agent-status tone-${state.tone}`}><i />{state.label}</span>
            <p className="agent-muted">{state.description}</p>
            <dl className="agent-facts">
              <dt>工作区</dt><dd>{row.workspaceId}</dd>
              <dt>执行目录</dt><dd>{row.canonicalWorkspaceRoot}</dd>
              <dt>创建时间</dt><dd>{new Date(row.createdAt).toLocaleString("zh-CN")}</dd>
              <dt>更新时间</dt><dd>{new Date(row.updatedAt).toLocaleString("zh-CN")}</dd>
              {row.completedAt !== null && <><dt>结束时间</dt><dd>{new Date(row.completedAt).toLocaleString("zh-CN")}</dd></>}
            </dl>
          </section>
          {(row.finalResult !== null || row.status === "completed") && <section><h3>结果</h3>
            <div className="agent-prose agent-result">{result || "未提供可读的最终文本，可在技术详情中查看已有结果。"}</div>
          </section>}
          {(row.attention !== "none" || ["failed", "reconciling", "interrupted"].includes(row.status) || row.interruptTimedOut) && <section><h3>恢复 / 错误信息</h3>
            <p>{state.description}</p>
            {row.attention === "pending_explicit_resume" && <p>恢复将继续此任务的原始输入和执行目录。请确认该目录当前仍适合执行。</p>}
            {row.attention === "manual_resolution_required" && <p>本页面无法确认或解除该执行的安全约束。保留当前记录，交由人工诊断处理。</p>}
            {row.interruptTimedOut && <p>取消请求确认超时；这不代表任务已经停止。</p>}
          </section>}
          <div className="agent-row-actions">
            {row.availableActions.canResumePending && <Button variant="outline" disabled={disabled || loading || !!error} onClick={() => void onOperate({ action: "resume_pending", executionId: row.executionId })}>恢复任务</Button>}
            {row.availableActions.canCancel && <Button variant="outline" disabled={disabled || loading || !!error} onClick={() => void onOperate({ action: "cancel", executionId: row.executionId })}>取消任务</Button>}
          </div>
          {row.availableActions.canContinue && <section><h3>继续对话</h3><p className="agent-muted">在此任务的原工作区和对话中开始下一次执行。</p>
            <form onSubmit={event => { event.preventDefault(); if (!disabled && !loading && !error && continuation.trim()) void onOperate(agentRequests.continuation(row.executionId, continuation)).then(ok => { if (ok) setDraft({ executionId: row.executionId, text: "" }); }); }}>
              <label className="sr-only" htmlFor="agent-continuation">后续任务内容</label>
              <textarea id="agent-continuation" className="agent-input" value={continuation} disabled={disabled} onChange={e => setDraft({ executionId: row.executionId, text: e.target.value })} placeholder="描述接下来希望 Agent 完成的任务……" />
              <div className="agent-composer-footer"><Button variant="outline" disabled={disabled || loading || !!error || !continuation.trim()} type="submit">继续对话</Button></div>
            </form>
          </section>}
          <details className="agent-technical"><summary>技术详情</summary>
            <p className="agent-muted">完整 IPC 输出，包含 Execution ID、原始状态和结果。</p>
            <div className="agent-json">
              <Button className="agent-json-copy" variant="outline" size="icon" disabled={copying || copied} aria-label={copying ? "正在复制…" : copied ? "技术详情已复制" : "复制技术详情"} title={copied ? "已复制" : "复制技术详情"} data-copied={copied} onClick={() => void copy()}>
                <Copy className="agent-copy-icon" /><Check className="agent-copy-check" />
              </Button>
              <pre tabIndex={0} aria-label="原始执行数据">{JSON.stringify(row, null, 2)}</pre>
            </div>
          </details>
        </div>
      </DialogPrimitive.Content>
    </DialogPortal>
  </Dialog>;
}
