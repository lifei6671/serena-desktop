import { TooltipHint } from "@/components/TooltipHint";
import { lazy, Suspense, useCallback, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { ProjectTaskNavigation } from "./ProjectTaskNavigation";
import { toast } from "sonner";
import { Folder, Plus, RefreshCw, LoaderCircle, Trash2, FileText } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription, DialogFooter } from "@/components/ui/dialog";
import { Select, SelectTrigger, SelectValue, SelectContent, SelectItem } from "@/components/ui/select";
import { api } from "./api";
import { agentRequests } from "./agentRequests";
import { executionStatus, executionTime, executionDuration, executionWorkspace, resultText, taskSummary } from "./agentPresentation";
const ExecutionDetails = lazy(() => import("./ExecutionDetails").then(module => ({ default: module.ExecutionDetails })));
import type { AgentAction, ExecutionView, Workspace } from "./types";

export function AgentPanel({ workspace, workspaces = [], onSelectWorkspace, sidebarContainer, onShowTask }: { sidebarContainer?: HTMLElement | null; onShowTask?: () => void; workspace: Workspace | null; workspaces?: Workspace[]; onSelectWorkspace?: () => void }) {
  const [prompt, setPrompt] = useState("");
  const [rows, setRows] = useState<ExecutionView[]>([]);
  const visibleRows = useRef<ExecutionView[]>([]);
  useEffect(() => { visibleRows.current = rows; }, [rows]);
  const [workspaceFilter, setWorkspaceFilter] = useState("all");
  const [nextCursor, setNextCursor] = useState<string | null>(null);
  const [loadingMore, setLoadingMore] = useState(false);
  const [moreError, setMoreError] = useState("");
  const moreFailed = useRef(false);
  const pageCount = useRef(1);
  const workspaceChoices = new Map(workspaces.map(w => [w.root, w.name]));
  if (workspace) workspaceChoices.set(workspace.root, workspace.name);
  for (const row of rows) workspaceChoices.set(row.canonicalWorkspaceRoot, executionWorkspace(row, [...workspaces, ...(workspace ? [workspace] : [])]));
  if (workspaceFilter !== "all" && !workspaceChoices.has(workspaceFilter)) workspaceChoices.set(workspaceFilter, executionWorkspace({canonicalWorkspaceRoot:workspaceFilter}, workspaces));
  const [hiddenIds, setHiddenIds] = useState<string[]>(() => {
    try {
      const value: unknown = JSON.parse(window.localStorage.getItem("agent-hidden-executions") ?? "[]");
      return Array.isArray(value) ? value.filter((id): id is string => typeof id === "string") : [];
    } catch { return []; }
  });
  const [deleteRequest, setDeleteRequest] = useState<{ row: ExecutionView; afterDelete?: () => void } | null>(null);
  const deleteTrigger = useRef<HTMLElement | null>(null);
  function requestDelete(row: ExecutionView, afterDelete?: () => void) {
    if (executionStatus(row).tone === "blue" || row.status === "reconciling") {
      deleteTrigger.current = document.activeElement as HTMLElement | null;
      setDeleteRequest({ row, afterDelete });
    } else if (hideExecution(row.executionId)) afterDelete?.();
  }
  function hideExecution(id: string) {
    const next = [...hiddenIds, id];
    try {
      window.localStorage.setItem("agent-hidden-executions", JSON.stringify(next));
      setHiddenIds(next);
      if (detail?.executionId === id) closeDetails();
      toast.success("已从本机列表删除，执行记录仍保留");
      return true;
    } catch (error) { toast.error(`删除失败：${String(error)}`); return false; }
  }
  const [loaded, setLoaded] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [listError, setListError] = useState("");
  const [operationError, setOperationError] = useState("");
  const [errorExecutionId, setErrorExecutionId] = useState<string>();
  const [busy, setBusy] = useState(agentRequests.inFlight);
  const [retry, setRetry] = useState<AgentAction | null>(agentRequests.pending);
  const [filter, setFilter] = useState("all");
  const [detail, setDetail] = useState<ExecutionView | null>(null);
  const detailRecord = useRef<ExecutionView | null>(null);
  useEffect(() => { detailRecord.current = detail; }, [detail]);
  const [detailLoading, setDetailLoading] = useState(false);
  const [detailError, setDetailError] = useState("");
  const mounted = useRef(false);
  const listWaiting = useRef(false);
  const detailWaiting = useRef(false);
  const epoch = useRef(0);
  const detailRequest = useRef(0);
  const opener = useRef<HTMLElement | null>(null);
  const input = useRef<HTMLTextAreaElement>(null);

  const refresh = useCallback(async (manual = false) => {
    if (listWaiting.current || detailWaiting.current || agentRequests.inFlight) return;
    listWaiting.current = true;
    const version = epoch.current;
    if (manual) setRefreshing(true);
    try {
      const next: ExecutionView[] = [];
      let cursor: string | null = null;
      for (let page = 0; page < pageCount.current; page++) {
        const response = await api.agentHistory(cursor, workspaceFilter === "all" ? null : workspaceFilter);
        if (!mounted.current || version !== epoch.current) return;
        next.push(...response.executions);
        cursor = response.nextCursor;
        if (!cursor) break;
      }
      if (moreFailed.current) {
        for (const old of visibleRows.current) {
          if (next.some(row => row.executionId === old.executionId)) continue;
          const response = await api.agent({action:"observe",executionId:old.executionId,waitMs:0});
          if (!mounted.current || version !== epoch.current) return;
          if (!response.ok) throw new Error(response.error.message);
          if (!("executionId" in response.data)) throw new Error("任务状态响应格式不正确");
          next.push(response.data);
        }
      } else setNextCursor(cursor);
      setRows(old => moreFailed.current ? [...next, ...old.filter(row => !next.some(n => n.executionId === row.executionId))] : next);
      const selected = detailRecord.current;
      let selectedRow = selected && next.find(row => row.executionId === selected.executionId);
      if (selected && !selectedRow) {
        const response = await api.agent({ action: "observe", executionId: selected.executionId, waitMs: 0 });
        if (!mounted.current || version !== epoch.current) return;
        if (!response.ok) throw new Error(response.error.message);
        if ("executionId" in response.data) selectedRow = response.data;
      }
      setDetail(old => {
        if (!old) return null;
        const row = selectedRow?.executionId === old.executionId ? selectedRow : undefined;
        if (!row) return old;
        return row.revision === old.revision ? { ...row, finalResult: old.finalResult } : row;
      });
      setLoaded(true); setListError("");
      if (manual) toast.success("任务列表已刷新");
    } catch (e) {
      if (mounted.current && version === epoch.current) {
        setListError(`任务列表更新失败：${String(e)}`);
        if (manual) toast.error("刷新失败，请稍后再试");
      }
    } finally { listWaiting.current = false; if (mounted.current) setRefreshing(false); }
  }, [workspaceFilter]);

  async function loadMore() {
    if (!nextCursor || listWaiting.current || detailWaiting.current || agentRequests.inFlight) return;
    listWaiting.current = true;
    setLoadingMore(true); setMoreError("");
    const version = epoch.current;
    try {
      const response = await api.agentHistory(nextCursor, workspaceFilter === "all" ? null : workspaceFilter);
      if (!mounted.current || version !== epoch.current) return;
      setRows(old => [...old, ...response.executions.filter(row => !old.some(r => r.executionId === row.executionId))]);
      setNextCursor(response.nextCursor);
      moreFailed.current = false;
      pageCount.current++;
    } catch (error) {
      if (mounted.current && version === epoch.current) { moreFailed.current = true; setMoreError(`加载更多失败：${String(error)}`); }
    } finally { listWaiting.current = false; if (mounted.current) setLoadingMore(false); }
  }

  useEffect(() => {
    mounted.current = true;
    void refresh();
    const timer = setInterval(() => {
      setBusy(agentRequests.inFlight); setRetry(agentRequests.pending); void refresh();
    }, 1500);
    return () => { mounted.current = false; clearInterval(timer); };
  }, [refresh]);
  useEffect(() => {
    if (input.current) { input.current.style.height = "auto"; input.current.style.height = `${Math.min(260, Math.max(112, input.current.scrollHeight))}px`; }
  }, [prompt]);

  const openDetails = useCallback(async (id: string, initial?: ExecutionView) => {
    const request = ++detailRequest.current;
    detailWaiting.current = true;
    epoch.current++;
    setDetailLoading(true); setDetailError("");
    if (initial) setDetail(initial);
    try {
      const response = await api.agent({ action: "observe", executionId: id, waitMs: 0, includeResult: true });
      if (!mounted.current || request !== detailRequest.current) return;
      if (!response.ok) throw new Error(`${response.error.code}: ${response.error.message}`);
      if (!("executionId" in response.data)) throw new Error("任务详情响应格式不正确");
      setDetail(response.data);
    } catch (e) {
      if (mounted.current && request === detailRequest.current) { setDetailError(String(e)); toast.error(`详情加载失败：${String(e)}`); }
    } finally { if (request === detailRequest.current) { detailWaiting.current = false; if (mounted.current) setDetailLoading(false); } }
  }, []);

  const detailId = detail?.executionId;
  const detailRevision = detail?.revision;
  const resultAvailable = detail?.resultAvailable;
  const resultMissing = detail?.finalResult === undefined;
  useEffect(() => {
    if (detailId && resultAvailable && resultMissing && !detailWaiting.current) void openDetails(detailId);
  }, [detailId, detailRevision, resultAvailable, resultMissing, openDetails]);

  async function operate(action: AgentAction): Promise<boolean> {
    if (agentRequests.inFlight || (agentRequests.pending && action !== agentRequests.pending)) return false;
    agentRequests.inFlight = true; epoch.current++;
    setBusy(true); setOperationError(""); setErrorExecutionId(undefined);
    try {
      const response = await api.agent(action);
      // A received product envelope confirms the outcome, including errors.
      agentRequests.accepted(action);
      if (!mounted.current) return response.ok;
      setRetry(null);
      if (!response.ok) {
        setOperationError(`${response.error.code}: ${response.error.message}`);
        setErrorExecutionId(response.error.executionId);
        toast.error("操作未能完成，请查看错误信息"); return false;
      }
      if ("executionId" in response.data) {
        const row = response.data;
        if (workspaceFilter === "all" || row.canonicalWorkspaceRoot === workspaceFilter) {
          const sorted = [row, ...rows.filter(value => value.executionId !== row.executionId)].sort((a, b) => b.createdAt - a.createdAt || b.executionId.localeCompare(a.executionId));
          const kept = moreFailed.current ? sorted : sorted.slice(0, pageCount.current * 5);
          setRows(kept);
          if (!moreFailed.current && sorted.length > kept.length) {
            setNextCursor(kept.at(-1)!.executionId);
            moreFailed.current = false; setMoreError("");
          }
        }
        setDetail(old => old?.executionId === row.executionId ? row : old);
      }
      if (action.action === "start") setPrompt(old => old === action.prompt ? "" : old);
      toast.success(action.action === "cancel" ? "取消请求已处理" : action.action === "resume_pending" ? "恢复请求已接受" : "任务请求已接受");
      return true;
    } catch (e) {
      agentRequests.remember(action);
      if (mounted.current) { setRetry(action); setOperationError(String(e)); toast.error("请求结果未确认，请核对后重试原请求"); }
      return false;
    } finally {
      agentRequests.inFlight = false;
      if (mounted.current) { setBusy(false); void refresh(); }
    }
  }

  const disabled = busy || !!retry;
  const listed = rows.filter(row => !hiddenIds.includes(row.executionId));
  const visible = listed.filter(row => filter === "all" || (filter === "attention" ? row.attention !== "none" || row.status === "failed" : filter === "completed" ? row.status === "completed" : ["dispatch_pending", "running", "cancel_requested", "cancelling", "finalizing", "reconciling"].includes(row.status)));
  function closeDetails() { detailRequest.current++; detailWaiting.current = false; setDetail(null); setDetailLoading(false); requestAnimationFrame(() => opener.current?.focus()); }

  const feedback = (retry || operationError) && <div role="alert" className="agent-notice">
      <strong>{retry ? "请求结果未确认" : "操作未完成"}</strong>
      {retry && <><p>连接中断，尚未确认请求是否被接受。重试会发送完全相同的原请求；请先确认原请求的执行上下文。</p><p className="agent-muted">{retry.action === "start" ? "新任务请求" : retry.action === "continue" ? "继续对话请求" : retry.action === "cancel" ? "取消请求" : "恢复请求"}{"prompt" in retry ? ` · ${taskSummary(retry.prompt)}` : ""}</p><Button variant="outline" disabled={busy} onClick={() => void operate(retry)}>{busy ? "正在确认…" : "重试原请求"}</Button></>}
      {operationError && <details><summary>错误详情</summary><p className="agent-prose">{operationError}</p></details>}
      {errorExecutionId && <Button variant="outline" disabled={detailLoading} onClick={e => { if (!detail) opener.current = e.currentTarget; void openDetails(errorExecutionId); }}>{detailLoading ? "正在加载…" : "查看相关任务"}</Button>}
    </div>;

  return <>
    {sidebarContainer && createPortal(<ProjectTaskNavigation workspaces={workspaces} hiddenIds={hiddenIds} selectedId={detail?.executionId} onDelete={requestDelete} onSelect={(row, trigger) => { opener.current = trigger; onShowTask?.(); void openDetails(row.executionId, row); }} />, sidebarContainer)}
    <Dialog open={deleteRequest !== null} onOpenChange={open => { if (!open) setDeleteRequest(null); }}>
      <DialogContent showCloseButton={false} onCloseAutoFocus={event => { event.preventDefault(); if (deleteTrigger.current?.isConnected) deleteTrigger.current.focus(); }}>
        <DialogHeader><DialogTitle>从列表删除正在处理的任务？</DialogTitle>
          <DialogDescription>删除仅会隐藏本机列表中的任务，不会停止 Agent，也不会删除执行记录或撤销已修改的文件。任务可能继续修改工作区，并在安全结束前继续占用工作区。若希望停止执行，请先返回任务详情使用“取消任务”。</DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <Button variant="outline" onClick={() => setDeleteRequest(null)}>保留任务</Button>
          <Button variant="destructive" onClick={() => {
            if (deleteRequest && hideExecution(deleteRequest.row.executionId)) {
              deleteRequest.afterDelete?.();
              setDeleteRequest(null);
            }
          }}>仅从列表删除</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
    <section className="settings-page agent-page">
    <div hidden={!!detail}>
    <div className="page-heading"><div><h1>Agent</h1><p>在当前工作区中创建和管理 Codex Agent 任务</p></div><span className="agent-local-label">本地 Codex 工作台</span></div>
    <div className="agent-context"><Folder aria-hidden="true" /><div><span className="agent-eyebrow">当前工作区</span><strong>{workspace?.name ?? "未选择可用工作区"}</strong>{workspace && <span className="agent-path">{workspace.root}</span>}</div>{onSelectWorkspace && <Button variant="ghost" onClick={onSelectWorkspace}>{workspace ? "管理工作区" : "选择工作区"}</Button>}</div>
    <form className="agent-composer" onSubmit={event => { event.preventDefault(); if (!disabled && workspace && prompt.trim()) void operate(agentRequests.fresh(prompt, workspace.id)); }}>
      <label htmlFor="agent-prompt">新任务</label>
      <textarea ref={input} id="agent-prompt" className="agent-input" value={prompt} onChange={e => setPrompt(e.target.value)} placeholder="描述希望 Agent 在当前工作区完成的任务……" />
      <div className="agent-composer-footer"><span className="agent-muted">Codex · 工作区执行</span><Button className="agent-primary" type="submit" disabled={disabled || !workspace || !prompt.trim()}>{busy ? <LoaderCircle className="animate-spin" /> : <Plus />}{busy ? "正在处理…" : "开始新任务"}</Button></div>
    </form>
    {!detail && feedback}
    <section className="agent-history" aria-label="最近任务">
      <header className="agent-list-heading"><h2>最近任务 <span>{loaded ? listed.length : ""}</span></h2><div className="agent-row-actions">
        <Select value={workspaceFilter} disabled={busy || !!retry} onValueChange={value => { if (agentRequests.inFlight) return; moreFailed.current = false; epoch.current++; pageCount.current = 1; setRows([]); setNextCursor(null); setLoaded(false); setListError(""); setMoreError(""); setWorkspaceFilter(value); }}><SelectTrigger size="sm" aria-label="筛选工作区"><SelectValue /></SelectTrigger><SelectContent><SelectItem value="all">全部工作区</SelectItem>{[...workspaceChoices].map(([root, name]) => <SelectItem key={root} value={root}>{name}</SelectItem>)}</SelectContent></Select>
        <Select value={filter} onValueChange={setFilter}><SelectTrigger size="sm" aria-label="筛选任务"><SelectValue /></SelectTrigger><SelectContent><SelectItem value="all">全部状态</SelectItem><SelectItem value="active">进行中</SelectItem><SelectItem value="attention">需要关注</SelectItem><SelectItem value="completed">已完成</SelectItem></SelectContent></Select>
        <Button variant="ghost" disabled={refreshing || busy} onClick={() => void refresh(true)}><RefreshCw className={refreshing ? "animate-spin" : ""} />{refreshing ? "刷新中…" : "刷新"}</Button>
      </div></header>
      {listError && <p className="agent-list-error" role="alert">{listError}{loaded ? "。以下保留上次读取的记录。" : "。请点击刷新重试。"}</p>}
      {!loaded && !listError && <div className="agent-empty" role="status">正在加载任务…</div>}
      {loaded && !visible.length && <div className="agent-empty"><strong>{listed.length ? "没有符合筛选条件的任务" : "暂无 Agent 任务"}</strong><p>{listed.length ? "切换筛选条件查看其他任务。" : "在上方输入任务，Agent 会在当前工作区中执行。"}</p></div>}
      <div className="agent-list">{visible.map(row => {
        const status = executionStatus(row);
        return <article className="agent-row" key={row.executionId}>
          <span className={`agent-marker tone-${status.tone}`} aria-hidden="true" />
          <div className="agent-row-main"><div className="agent-row-title"><TooltipHint content={taskSummary(row.prompt)}><h3 tabIndex={0}>{taskSummary(row.prompt)}</h3></TooltipHint><span className={`agent-status tone-${status.tone}`}><i />{status.label}</span></div>
            <div className="agent-meta"><TooltipHint content={row.canonicalWorkspaceRoot}><span tabIndex={0}>{executionWorkspace(row, [...workspaces, ...(workspace ? [workspace] : [])])}</span></TooltipHint><span>·</span><TooltipHint content={executionTime(row.createdAt)}><time tabIndex={0} dateTime={new Date(row.createdAt).toISOString()}>{executionTime(row.createdAt)}</time></TooltipHint><span>·</span><span>耗时 {executionDuration(row)}</span></div>
            <div className="agent-row-bottom"><p>{row.status === "completed" ? taskSummary(resultText(row.finalResult)) || status.description : status.description}</p><div className="agent-row-actions">
              {row.availableActions.canResumePending && <Button variant="outline" size="sm" disabled={disabled || !!listError} onClick={() => void operate({ action: "resume_pending", executionId: row.executionId })}>恢复任务</Button>}
              {row.availableActions.canCancel && <Button variant="ghost" size="sm" disabled={disabled || !!listError} onClick={() => void operate({ action: "cancel", executionId: row.executionId })}>取消任务</Button>}
              <Button variant="ghost" size="sm" onClick={e => { opener.current = e.currentTarget; void openDetails(row.executionId, row); }}><FileText aria-hidden="true" />详情</Button>
              <TooltipHint content="仅从本机列表删除，不取消任务或删除执行记录"><Button className="agent-delete-action" variant="ghost" size="sm" onClick={() => requestDelete(row)}><Trash2 aria-hidden="true" />删除</Button></TooltipHint>
            </div></div>
          </div>
        </article>;
      })}</div>
      {moreError && <p role="alert" className="agent-list-error">{moreError}</p>}
      {nextCursor && <Button variant="ghost" disabled={loadingMore || refreshing || busy} onClick={() => void loadMore()}>{loadingMore ? "正在加载…" : moreError ? "重试加载更多" : "展开更多（5条）"}</Button>}
    </section>
    </div>
    {detail && <Suspense fallback={<p role="status">正在加载任务详情…</p>}><ExecutionDetails row={detail} workspaceName={executionWorkspace(detail, [...workspaces, ...(workspace ? [workspace] : [])])} feedback={feedback} busy={busy} loading={detailLoading} error={detailError} disabled={disabled} onBack={closeDetails} onReload={() => void openDetails(detail.executionId, detail)} onOperate={operate} /></Suspense>}
  </section></>;
}
