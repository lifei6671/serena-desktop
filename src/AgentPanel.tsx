import { lazy, Suspense, useCallback, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { ProjectTaskNavigation } from "./ProjectTaskNavigation";
import { toast } from "sonner";
import { Folder } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription, DialogFooter } from "@/components/ui/dialog";
import { Select, SelectTrigger, SelectValue, SelectContent, SelectItem } from "@/components/ui/select";
import { api } from "./api";
import { agentRequests } from "./agentRequests";
import { providerCardPresentation, executionStatus, executionWorkspace, taskSummary } from "./agentPresentation";
const ExecutionDetails = lazy(() => import("./ExecutionDetails").then(module => ({ default: module.ExecutionDetails })));
import type { AgentAction, AgentProviderSettings, AgentRoleProviderDefaults, ExecutionConfigurationCatalog, ExecutionView, ProviderCatalogSnapshot, Workspace } from "./types";

// Role 是固定协议域；Provider ID 与名称始终来自目录。
const roleLabels = { development: "开发", testing: "测试", review: "评审", analysis: "分析", general: "通用" };
type TaskRole = keyof AgentProviderSettings["roleRouting"];
type RoleEdits = Partial<Record<TaskRole, { value: string | null; pending: boolean }>>;
type ProviderEdits = Partial<Record<string, { value: boolean; pending: boolean }>>;
type DefaultsValue = { model: string | null; reasoning: string | null };
type DefaultsEdits = Record<string, { value: DefaultsValue; pending: boolean } | undefined>;
type ConfigurationCatalogState = { loading: boolean; value?: ExecutionConfigurationCatalog; error?: boolean };

export function AgentPanel({ workspace, workspaces = [], onSelectWorkspace, sidebarContainer, onShowTask, onShowAgent, onWorkspaceRename = async () => false, onWorkspaceRemove = async () => false, detailView = true }: { detailView?: boolean; sidebarContainer?: HTMLElement | null; onShowTask?: () => void; onShowAgent?: () => void; workspace: Workspace | null; workspaces?: Workspace[]; onSelectWorkspace?: () => void; onWorkspaceRename?: (id: string, name: string) => Promise<boolean>; onWorkspaceRemove?: (id: string) => Promise<boolean> }) {
  const [rows, setRows] = useState<ExecutionView[]>([]);
  const [catalog, setCatalog] = useState<ProviderCatalogSnapshot | null>(null);
  const [catalogError, setCatalogError] = useState(false);
  const [roleEdits, setRoleEdits] = useState<RoleEdits>({});
  const roleEditsRef = useRef<RoleEdits>({});
  const roleGenerations = useRef<Partial<Record<TaskRole, number>>>({});
  const [providerEdits, setProviderEdits] = useState<ProviderEdits>({});
  const providerEditsRef = useRef<ProviderEdits>({});
  const providerGenerations = useRef<Partial<Record<string, number>>>({});
  const [defaultsEdits, setDefaultsEdits] = useState<DefaultsEdits>({});
  const defaultsEditsRef = useRef<DefaultsEdits>({});
  const defaultsGenerations = useRef<Record<string, number>>({});
  const [configurationCatalogs, setConfigurationCatalogs] = useState<Record<string, ConfigurationCatalogState>>({});
  // in-flight 与成功缓存分离：失败必须允许后续轮询重试，成功则避免重复启动临时 Runtime。
  const configurationRequests = useRef(new Set<string>());
  const configurationLoaded = useRef(new Set<string>());
  // 目录独立读取，失败不阻塞侧栏导航或详情操作。
  useEffect(() => {
    let disposed = false;
    let waiting = false;
    /** 仅获取当前目录快照，不触发健康刷新。 */
    async function refreshCatalog() {
      if (waiting) return;
      waiting = true;
      const generations = { ...roleGenerations.current };
      const enabledGenerations = { ...providerGenerations.current };
      const savedDefaultsGenerations = { ...defaultsGenerations.current };
      try {
        const snapshot = await api.agentProviderCatalog();
        if (!disposed) {
          // 只有在该角色最近一次保存结束后发出的轮询，才可替换提交值。
          // 保存开始/结束都递增版本，保护跨越任一边界的旧请求。
          const edits = { ...roleEditsRef.current };
          for (const role of Object.keys(edits) as TaskRole[]) {
            if (!edits[role]?.pending && generations[role] === roleGenerations.current[role]) delete edits[role];
          }
          roleEditsRef.current = edits;
          setRoleEdits(edits);
          // Provider 与 Role 独立保护；跨越启停开始/结束的轮询不能覆盖本地值。
          const enabledEdits = { ...providerEditsRef.current };
          for (const id of Object.keys(enabledEdits)) {
            if (!enabledEdits[id]?.pending && enabledGenerations[id] === providerGenerations.current[id]) delete enabledEdits[id];
          }
          providerEditsRef.current = enabledEdits;
          setProviderEdits(enabledEdits);
          const defaultEdits = { ...defaultsEditsRef.current };
          for (const key of Object.keys(defaultEdits)) {
            if (!defaultEdits[key]?.pending && savedDefaultsGenerations[key] === defaultsGenerations.current[key]) delete defaultEdits[key];
          }
          defaultsEditsRef.current = defaultEdits;
          setDefaultsEdits(defaultEdits);
          setCatalog(snapshot);
          setCatalogError(false);
        }
      } catch {
        if (!disposed) setCatalogError(true);
      } finally { waiting = false; }
    }
    void refreshCatalog();
    const timer = setInterval(() => void refreshCatalog(), 1500);
    return () => { disposed = true; clearInterval(timer); };
  }, []);
  // 只为当前角色实际选中的 Provider 查询动态目录；查询失败不改写设置。
  useEffect(() => {
    if (!catalog || !workspace) return;
    const providers = new Set((Object.keys(roleLabels) as TaskRole[])
      .map(role => roleEditsRef.current[role] ? roleEditsRef.current[role]!.value : catalog.roleRouting[role] ?? null)
      .filter((value): value is string => value !== null));
    for (const providerId of providers) {
      const key = `${workspace.id}:${workspace.generation}:${providerId}`;
      if (configurationLoaded.current.has(key) || configurationRequests.current.has(key)) continue;
      configurationRequests.current.add(key);
      setConfigurationCatalogs(old => ({ ...old, [key]: { loading: true } }));
      void api.agentProviderConfigurationCatalog(providerId, workspace.id).then(value => {
        configurationLoaded.current.add(key);
        setConfigurationCatalogs(old => ({ ...old, [key]: { loading: false, value } }));
      }).catch(() => {
        // 失败不能进入成功缓存；下一次 Provider catalog poll 会再次尝试同一个 key。
        setConfigurationCatalogs(old => ({ ...old, [key]: { loading: false, error: true } }));
      }).finally(() => configurationRequests.current.delete(key));
    }
  }, [catalog, roleEdits, workspace]);
  /** 独立提交一个角色，返回全量 settings 时仅采纳当前角色，避免乱序响应覆盖其它保存。 */
  async function saveRoleRoute(role: TaskRole, providerId: string | null) {
    if (!catalog || roleEditsRef.current[role]?.pending) return;
    // 显式 null 也必须保留，不能通过空值合并恢复旧目录绑定。
    const previous = roleEditsRef.current[role] ? roleEditsRef.current[role].value : catalog.roleRouting[role] ?? null;
    if (previous === providerId) return;
    roleGenerations.current[role] = (roleGenerations.current[role] ?? 0) + 1;
    roleEditsRef.current = { ...roleEditsRef.current, [role]: { value: providerId, pending: true } };
    setRoleEdits(roleEditsRef.current);
    let value = previous;
    try {
      const settings = await api.agentProviderSetRoleRoute(role, providerId);
      value = settings.roleRouting[role];
    } catch {
      toast.error(`${roleLabels[role]}角色保存失败，已恢复原绑定，请重试。`);
    } finally {
      roleGenerations.current[role] = (roleGenerations.current[role] ?? 0) + 1;
      roleEditsRef.current = { ...roleEditsRef.current, [role]: { value, pending: false } };
      setRoleEdits(roleEditsRef.current);
    }
  }
  /** 保存当前 role/provider 的完整稀疏 entry，避免模型与推理并行写互相覆盖。 */
  async function saveRoleDefaults(role: TaskRole, providerId: string, next: DefaultsValue) {
    if (!catalog) return;
    const key = `${role}:${providerId}`;
    if (defaultsEditsRef.current[key]?.pending) return;
    const persisted = catalog.roleDefaults?.[role]?.[providerId];
    const previous = defaultsEditsRef.current[key]?.value ?? {
      model: persisted?.model ?? null,
      reasoning: persisted?.reasoning ?? null,
    };
    if (previous.model === next.model && previous.reasoning === next.reasoning) return;
    defaultsGenerations.current[key] = (defaultsGenerations.current[key] ?? 0) + 1;
    defaultsEditsRef.current = { ...defaultsEditsRef.current, [key]: { value: next, pending: true } };
    setDefaultsEdits(defaultsEditsRef.current);
    let value = previous;
    try {
      const settings = await api.agentProviderSetRoleDefaults(role, providerId, next satisfies AgentRoleProviderDefaults);
      const saved = settings.roleDefaults?.[role]?.[providerId];
      value = { model: saved?.model ?? null, reasoning: saved?.reasoning ?? null };
    } catch {
      toast.error(`${roleLabels[role]}角色默认配置保存失败，已恢复原设置，请重试。`);
    } finally {
      defaultsGenerations.current[key] = (defaultsGenerations.current[key] ?? 0) + 1;
      defaultsEditsRef.current = { ...defaultsEditsRef.current, [key]: { value, pending: false } };
      setDefaultsEdits(defaultsEditsRef.current);
    }
  }
  /** 只采纳本 Provider 的持久化结果，保留并行角色保存与其它 Provider 的状态。 */
  async function saveProviderEnabled(providerId: string, enabled: boolean) {
    const provider = catalog?.providers.find(entry => entry.id === providerId);
    if (!provider || providerEditsRef.current[providerId]?.pending) return;
    const previous = providerEditsRef.current[providerId]?.value ?? provider.enabled;
    if (previous === enabled) return;
    providerGenerations.current[providerId] = (providerGenerations.current[providerId] ?? 0) + 1;
    providerEditsRef.current = { ...providerEditsRef.current, [providerId]: { value: enabled, pending: true } };
    setProviderEdits(providerEditsRef.current);
    let value = previous;
    try {
      const settings = await api.agentProviderSetEnabled(providerId, enabled);
      value = settings.providers[providerId].enabled;
    } catch {
      toast.error(`${provider.displayName?.trim() || providerId} 启用状态保存失败，已恢复原状态，请重试。`);
    } finally {
      providerGenerations.current[providerId] = (providerGenerations.current[providerId] ?? 0) + 1;
      providerEditsRef.current = { ...providerEditsRef.current, [providerId]: { value, pending: false } };
      setProviderEdits(providerEditsRef.current);
    }
  }
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
  const [listError, setListError] = useState("");
  const [operationError, setOperationError] = useState("");
  const [errorExecutionId, setErrorExecutionId] = useState<string>();
  const [busy, setBusy] = useState(agentRequests.inFlight);
  const [retry, setRetry] = useState<AgentAction | null>(agentRequests.pending);
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

  const refresh = useCallback(async () => {
    if (listWaiting.current || detailWaiting.current || agentRequests.inFlight) return;
    listWaiting.current = true;
    const version = epoch.current;
    try {
      // 首屏任务快照继续支撑 Provider 活动数与 Claim 提示；侧栏独立负责历史分页。
      const response = await api.agentHistory(null, null);
      if (!mounted.current || version !== epoch.current) return;
      const next = response.executions;
      setRows(next);
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
      setListError("");
    } catch (e) {
      if (mounted.current && version === epoch.current) {
        setListError(`任务列表更新失败：${String(e)}`);
      }
    } finally { listWaiting.current = false; }
  }, []);

  useEffect(() => {
    mounted.current = true;
    void refresh();
    const timer = setInterval(() => {
      setBusy(agentRequests.inFlight); setRetry(agentRequests.pending); void refresh();
    }, 1500);
    return () => { mounted.current = false; clearInterval(timer); };
  }, [refresh]);
  const openDetails = useCallback(async (id: string, initial?: ExecutionView) => {
    onShowTask?.();
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
  }, [onShowTask]);

  const detailId = detail?.executionId;
  const detailRevision = detail?.revision;
  const resultAvailable = detail?.resultAvailable;
  const resultMissing = detail?.finalResult === undefined;
  useEffect(() => {
    if (detailView && detailId && resultAvailable && resultMissing && !detailWaiting.current) void openDetails(detailId);
  }, [detailId, detailRevision, resultAvailable, resultMissing, detailView, openDetails]);

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
        setRows(old => [row, ...old.filter(value => value.executionId !== row.executionId)]
          .sort((a, b) => b.createdAt - a.createdAt || b.executionId.localeCompare(a.executionId)).slice(0, 5));
        setDetail(old => old?.executionId === row.executionId ? row : old);
      }
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

  // 此请求只能走专用 Local Tauri IPC，不能被包装为 agent_operation action。
  async function manualResolve(executionId: string): Promise<boolean> {
    if (agentRequests.inFlight) return false;
    agentRequests.inFlight = true; epoch.current++;
    setBusy(true); setOperationError(""); setErrorExecutionId(undefined);
    try {
      const row = await api.agentManualResolve(executionId, "interrupt_and_release");
      if (!mounted.current) return true;
      setRows(old => [row, ...old.filter(value => value.executionId !== row.executionId)]
        .sort((a, b) => b.createdAt - a.createdAt || b.executionId.localeCompare(a.executionId)).slice(0, 5));
      setDetail(old => old?.executionId === row.executionId ? row : old);
      toast.success("任务已人工结束，工作区已释放");
      return true;
    } catch (error) {
      if (mounted.current) {
        setOperationError(String(error)); setErrorExecutionId(executionId);
        toast.error("人工结束未能完成，请查看错误信息");
      }
      return false;
    } finally {
      agentRequests.inFlight = false;
      if (mounted.current) { setBusy(false); void refresh(); }
    }
  }

  const disabled = busy || !!retry;
  function closeDetails() { detailRequest.current++; detailWaiting.current = false; setDetail(null); setDetailLoading(false); onShowAgent?.(); requestAnimationFrame(() => opener.current?.focus()); }

  const feedback = (retry || operationError) && <div role="alert" className="agent-notice">
      <strong>{retry ? "请求结果未确认" : "操作未完成"}</strong>
      {retry && <><p>连接中断，尚未确认请求是否被接受。重试会发送完全相同的原请求；请先确认原请求的执行上下文。</p><p className="agent-muted">{retry.action === "start" ? "新任务请求" : retry.action === "continue" ? "继续对话请求" : retry.action === "cancel" ? "取消请求" : "恢复请求"}{"prompt" in retry ? ` · ${taskSummary(retry.prompt)}` : ""}</p><Button variant="outline" disabled={busy} onClick={() => void operate(retry)}>{busy ? "正在确认…" : "重试原请求"}</Button></>}
      {operationError && <details><summary>错误详情</summary><p className="agent-prose">{operationError}</p></details>}
      {errorExecutionId && <Button variant="outline" disabled={detailLoading} onClick={e => { if (!detail) opener.current = e.currentTarget; void openDetails(errorExecutionId); }}>{detailLoading ? "正在加载…" : "查看相关任务"}</Button>}
    </div>;

  return <>
    {sidebarContainer && createPortal(<ProjectTaskNavigation workspaces={workspaces} hiddenIds={hiddenIds} selectedId={detailView ? detail?.executionId : undefined} onDelete={requestDelete} onSelect={(row, trigger) => { opener.current = trigger; void openDetails(row.executionId, row); }} onWorkspaceRename={onWorkspaceRename} onWorkspaceRemove={onWorkspaceRemove} />, sidebarContainer)}
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
    <section className={`settings-page agent-page${detailView && detail ? " agent-detail-view" : " agent-list-view"}`}>
    <div hidden={detailView && !!detail}>
    <div className="page-heading agent-list-page-heading"><div><div><h1>Agent 管理</h1></div><p>查看 Agent 接入状态，配置 Provider 启停与角色分工。</p></div></div>
    <section className="agent-providers" aria-labelledby="agent-providers-heading">
      <h2 id="agent-providers-heading">Agent 接入</h2>
      <p className="agent-muted">活动任务按当前已加载的任务统计；Runtime 为任务活动展示，不代表系统进程状态。</p>
      {listError && <p className="agent-list-error" role="alert">{listError}。保留上次读取的任务状态，稍后自动重试。</p>}
      {catalogError && <p role="alert" className="agent-list-error">Agent 接入信息读取失败，稍后自动重试。{catalog && "以下保留上次读取的接入信息。"}</p>}
      {!catalog && !catalogError && <p role="status" className="agent-muted">正在加载 Agent 接入…</p>}
      {catalog?.providers.length === 0 && <p className="agent-muted">暂无已注册的 Agent。</p>}
      <div className="agent-provider-grid">{catalog?.providers.map(provider => {
        const edit = providerEdits[provider.id];
        const enabled = edit?.value ?? provider.enabled;
        const card = providerCardPresentation({ ...provider, enabled }, rows);
        return <section className="agent-provider-card" key={provider.id} aria-label={`${card.name} 接入`}>
          <header><h3>{card.name}</h3><code>{provider.id}</code></header>
          <dl>
            <div><dt>接入</dt><dd className={`tone-${card.enabledTone}`}>{card.enabledLabel}</dd></div>
            <div><dt>可用性</dt><dd className={`tone-${card.healthTone}`}>{card.healthLabel}</dd></div>
            <div><dt>版本</dt><dd><code>{card.version}</code></dd></div>
            <div><dt>协议</dt><dd>{card.protocol}</dd></div>
            <div><dt>Runtime</dt><dd className={card.runtime === "running" ? "tone-blue" : "tone-slate"}>{card.runtimeLabel}</dd></div>
            <div><dt>活动任务</dt><dd>{card.activeExecutions}</dd></div>
          </dl>
          <div className="mt-3 flex items-center gap-2">
            <Switch aria-label={`启用 ${card.name}`} checked={enabled} disabled={edit?.pending} onCheckedChange={value => void saveProviderEnabled(provider.id, value)} />
            <span className="agent-muted">启用 Provider</span>
            {edit?.pending && <span role="status" className="agent-muted">正在保存…</span>}
          </div>
          {card.unsupportedVersionNotice && <p className="mt-3 text-sm text-amber-700">{card.unsupportedVersionNotice}</p>}
          {!enabled && card.pendingBlockers.length > 0 && <div className="mt-3 rounded-md border border-amber-200 bg-amber-50 p-3 text-sm text-amber-700">
            <p>该 Provider 有待恢复任务仍占用 Workspace Claim。</p>
            <p>可以取消任务释放工作区，或重新启用 Provider 后继续恢复。</p>
            <ul className="mt-2 space-y-2">{card.pendingBlockers.map(row => <li key={row.executionId}>
              <p>{taskSummary(row.prompt)}</p>
              <div className="flex flex-wrap gap-2">
                <Button variant="outline" size="sm" onClick={event => { opener.current = event.currentTarget; void openDetails(row.executionId, row); }}>查看任务</Button>
                <Button variant="outline" size="sm" disabled={disabled || !!listError || !row.availableActions.canCancel} onClick={() => void operate({ action: "cancel", executionId: row.executionId })}>取消任务</Button>
              </div>
            </li>)}</ul>
            <Button className="mt-2" variant="outline" size="sm" disabled={edit?.pending} onClick={() => void saveProviderEnabled(provider.id, true)}>重新启用 Provider</Button>
          </div>}
        </section>;
      })}</div>
    </section>
    <section className="agent-role-routing" aria-labelledby="agent-role-heading">
      <h2 id="agent-role-heading">角色分工</h2>
      <p className="agent-muted">为不同角色指定 Agent、默认模型和推理强度，仅影响后续任务；停用或未注册的绑定与配置会保留。</p>
      {!catalog && <p role="status" className="agent-muted">{catalogError ? "角色分工暂不可用，等待接入信息恢复。" : "正在加载角色分工…"}</p>}
      {catalog && !workspace && <p role="status" className="agent-muted">请选择工作区后读取 Provider 模型与推理目录；已保存配置会继续保留。</p>}
      {catalog && <div className="agent-role-grid">{(Object.keys(roleLabels) as TaskRole[]).map(role => {
        const edit = roleEdits[role];
        const value = edit ? edit.value : catalog.roleRouting[role] ?? null;
        const unknown = value !== null && !catalog.providers.some(provider => provider.id === value);
        const defaultsKey = value === null ? null : `${role}:${value}`;
        const persistedDefaults = value === null ? undefined : catalog.roleDefaults?.[role]?.[value];
        const defaultsEdit = defaultsKey ? defaultsEdits[defaultsKey] : undefined;
        const defaults = defaultsEdit?.value ?? { model: persistedDefaults?.model ?? null, reasoning: persistedDefaults?.reasoning ?? null };
        const configurationKey = workspace && value ? `${workspace.id}:${workspace.generation}:${value}` : null;
        const configuration = configurationKey ? configurationCatalogs[configurationKey] : undefined;
        const loadedCatalog = configuration?.value;
        const selectableModels = loadedCatalog?.models.filter(model => !model.hidden) ?? [];
        const unavailableModel = defaults.model !== null && !selectableModels.some(model => model.id === defaults.model);
        const effectiveModel = defaults.model ?? loadedCatalog?.currentModel ?? loadedCatalog?.defaultModel ?? null;
        const selectedModel = loadedCatalog?.models.find(model => model.id === effectiveModel);
        // 模型级空列表表示明确不支持 reasoning；只有目录未给出模型时才使用全局 fallback。
        const reasoningOptions = selectedModel
          ? selectedModel.reasoningOptions
          : loadedCatalog?.reasoningOptions ?? [];
        const unavailableReasoning = defaults.reasoning !== null && !reasoningOptions.some(option => option.id === defaults.reasoning);
        const loadingConfiguration = !!value && !!workspace && (!configuration || configuration.loading);
        const configurationUnavailable = !!configuration?.error;
        const reasoningUnsupported = !!loadedCatalog && reasoningOptions.length === 0;
        const defaultsPending = defaultsEdit?.pending;
        const modelControlValue = !value ? "no-provider" : !workspace ? "no-workspace" : loadingConfiguration ? "loading" : configurationUnavailable ? "unavailable" : defaults.model === null ? "default" : `value:${defaults.model}`;
        const reasoningControlValue = !value ? "no-provider" : !workspace ? "no-workspace" : loadingConfiguration ? "loading" : configurationUnavailable ? "unavailable" : defaults.reasoning !== null ? `value:${defaults.reasoning}` : reasoningUnsupported ? "unsupported" : "default";
        return <div className="agent-role-row" key={role}>
          <label id={`agent-role-label-${role}`} htmlFor={`agent-role-${role}`}>{roleLabels[role]}</label>
          {/* 给所有 Provider 值加前缀，避免合法 ID 与清空 sentinel 冲突。 */}
          <Select value={value === null ? "none" : `provider:${value}`} disabled={edit?.pending} onValueChange={next => void saveRoleRoute(role, next === "none" ? null : next.slice("provider:".length))}>
            <SelectTrigger id={`agent-role-${role}`} size="sm" aria-labelledby={`agent-role-label-${role}`}><SelectValue /></SelectTrigger>
            <SelectContent position="popper" align="start">
              <SelectItem value="none">未指定 Agent</SelectItem>
              {catalog.providers.map(provider => <SelectItem key={provider.id} value={`provider:${provider.id}`}>{provider.displayName?.trim() || provider.id}{!(providerEdits[provider.id]?.value ?? provider.enabled) && " · 已停用"}</SelectItem>)}
              {unknown && <SelectItem value={`provider:${value}`}>{value} · 未注册</SelectItem>}
            </SelectContent>
          </Select>
          <Select value={modelControlValue} disabled={!value || !workspace || loadingConfiguration || configurationUnavailable || defaultsPending} onValueChange={next => value && void saveRoleDefaults(role, value, { ...defaults, model: next === "default" ? null : next.slice("value:".length) })}>
            <SelectTrigger size="sm" aria-label={`${roleLabels[role]}默认模型`}><SelectValue /></SelectTrigger>
            <SelectContent position="popper" align="start">
              {!value && <SelectItem value="no-provider">请先指定 Agent</SelectItem>}
              {value && !workspace && <SelectItem value="no-workspace">需要工作区</SelectItem>}
              {loadingConfiguration && <SelectItem value="loading">正在加载…</SelectItem>}
              {configurationUnavailable && <SelectItem value="unavailable">目录不可用</SelectItem>}
              {value && workspace && !loadingConfiguration && !configurationUnavailable && <SelectItem value="default">跟随 Provider 默认</SelectItem>}
              {selectableModels.map(model => <SelectItem key={model.id} value={`value:${model.id}`}>{model.name}{model.isDefault && " · Provider 默认"}</SelectItem>)}
              {!!loadedCatalog && unavailableModel && <SelectItem disabled value={`value:${defaults.model}`}>{defaults.model} · 当前不可用</SelectItem>}
            </SelectContent>
          </Select>
          <Select value={reasoningControlValue} disabled={!value || !workspace || loadingConfiguration || configurationUnavailable || reasoningUnsupported || defaultsPending} onValueChange={next => value && void saveRoleDefaults(role, value, { ...defaults, reasoning: next === "default" ? null : next.slice("value:".length) })}>
            <SelectTrigger size="sm" aria-label={`${roleLabels[role]}推理强度`}><SelectValue /></SelectTrigger>
            <SelectContent position="popper" align="start">
              {!value && <SelectItem value="no-provider">请先指定 Agent</SelectItem>}
              {value && !workspace && <SelectItem value="no-workspace">需要工作区</SelectItem>}
              {loadingConfiguration && <SelectItem value="loading">正在加载…</SelectItem>}
              {configurationUnavailable && <SelectItem value="unavailable">目录不可用</SelectItem>}
              {reasoningUnsupported && defaults.reasoning === null && <SelectItem value="unsupported">不支持</SelectItem>}
              {value && workspace && !loadingConfiguration && !configurationUnavailable && !reasoningUnsupported && <SelectItem value="default">跟随 Provider 默认</SelectItem>}
              {reasoningOptions.map(option => <SelectItem key={option.id} value={`value:${option.id}`}>{option.name}{selectedModel?.defaultReasoning === option.id && " · Provider 默认"}</SelectItem>)}
              {!!loadedCatalog && unavailableReasoning && <SelectItem disabled value={`value:${defaults.reasoning}`}>{defaults.reasoning} · 当前不可用</SelectItem>}
            </SelectContent>
          </Select>
          {(edit?.pending || defaultsPending) && <span role="status" className="agent-muted">正在保存…</span>}
          {configurationUnavailable && <span role="status" className="agent-muted">目录不可用，已保留设置</span>}
        </div>;
      })}</div>}
    </section>
    <div className="agent-workspace-bar"><div><Folder aria-hidden="true" /><span>当前工作区:</span><strong>{workspace?.name ?? "未选择可用工作区"}</strong>{workspace && <code>{workspace.root}</code>}</div>{onSelectWorkspace && <Button variant="ghost" onClick={onSelectWorkspace}>{workspace ? "管理工作区" : "选择工作区"}</Button>}</div>
    {!detail && feedback}
    </div>
    {detail && detailView && <Suspense fallback={<p role="status">正在加载任务详情…</p>}><ExecutionDetails row={detail} workspaceName={executionWorkspace(detail, [...workspaces, ...(workspace ? [workspace] : [])])} busy={busy} loading={detailLoading} error={detailError} disabled={disabled} onReload={() => void openDetails(detail.executionId, detail)} onOperate={operate} onManualResolve={manualResolve} /></Suspense>}
  </section></>;
}
