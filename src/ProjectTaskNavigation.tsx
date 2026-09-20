import { TooltipHint } from "@/components/TooltipHint";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { useEffect, useId, useRef, useState } from "react";
import { HoverCard, Popover } from "radix-ui";
import { CalendarDays, ChevronDown, CircleAlert, Ellipsis, Folder, LoaderCircle, Monitor, Pencil, RefreshCw, Trash2 } from "lucide-react";
import { api } from "./api";
import { executionStatus, executionTime, providerLabel, taskTitle, usageTotalLabel } from "./agentPresentation";
import type { ExecutionView, Workspace } from "./types";

import { toast } from "sonner";

type Props = {
  workspaces: Workspace[];
  hiddenIds: string[];
  selectedId?: string;
  onSelect: (row: ExecutionView, trigger: HTMLElement) => void;
  onDelete: (row: ExecutionView, afterDelete?: () => void) => void;
  onWorkspaceRename: (id: string, name: string) => Promise<boolean>;
  onWorkspaceRemove: (id: string) => Promise<boolean>;
};

/** 侧栏任务在当天仅显示时分，跨日时沿用紧凑的相对日期。 */
function sidebarTaskTime(updatedAt: number) {
  const now = new Date(Date.now());
  const updated = new Date(updatedAt);
  const days = Math.max(0, (Date.UTC(now.getFullYear(), now.getMonth(), now.getDate())
    - Date.UTC(updated.getFullYear(), updated.getMonth(), updated.getDate())) / 86400000);
  if (days === 0) {
    const pad = (value: number) => String(value).padStart(2, "0");
    return `${pad(updated.getHours())}:${pad(updated.getMinutes())}`;
  }
  return days === 1 ? "昨天" : `${days}天前`;
}

function TaskItem({ row, workspace, selected, onSelect, onDelete }: {
  row: ExecutionView; workspace: Workspace; selected: boolean;
  onSelect: Props["onSelect"]; onDelete: Props["onDelete"];
}) {
  const [open, setOpen] = useState(false);
  const infoId = useId();
  const status = executionStatus(row);
  const title = taskTitle(row);
  const provider = providerLabel(row);
  const usage = usageTotalLabel(row);
  const updated = new Date(row.updatedAt);
  return <li className="project-task" data-selected={selected}>
    <HoverCard.Root open={open} onOpenChange={setOpen} openDelay={350} closeDelay={100}>
      <HoverCard.Trigger asChild>
        <button className="project-task-link" aria-current={selected ? "page" : undefined}
          aria-describedby={open ? infoId : undefined}
          onFocus={() => setOpen(true)} onBlur={() => setOpen(false)}
          onClick={event => { setOpen(false); onSelect(row, event.currentTarget); }}>
          {status.tone === "blue" && <LoaderCircle className="project-task-state-icon tone-blue animate-spin motion-reduce:animate-none" role="img" aria-label={status.label} />}
          {status.tone === "red" && <CircleAlert className="project-task-state-icon tone-red" role="img" aria-label={status.label} />}
          <span className="project-task-content">
            <span className="project-task-title">{title}</span>
          </span>
          <time dateTime={updated.toISOString()}>{sidebarTaskTime(row.updatedAt)}</time>
        </button>
      </HoverCard.Trigger>
      <HoverCard.Portal>
        <HoverCard.Content id={infoId} className="project-task-preview" side="right" align="start" sideOffset={12} collisionPadding={16}>
          <strong>{title}</strong>
          <p><Monitor aria-hidden="true" /><span className={`agent-status tone-${status.tone}`}>{status.label}</span> · {provider}</p>
          <p>总 Token：{usage}</p>
          <p><Folder aria-hidden="true" />所属项目：{workspace.name}</p>
          <p><CalendarDays aria-hidden="true" />更新于 {executionTime(row.updatedAt)}</p>
        </HoverCard.Content>
      </HoverCard.Portal>
    </HoverCard.Root>
    <TooltipHint content="仅从本机列表删除，不取消执行"><button className="project-task-delete" aria-label={`删除任务：${title}`}
      onClick={event => {
        const item = event.currentTarget.closest("li");
        const target = item?.nextElementSibling?.querySelector<HTMLElement>(".project-task-link")
          ?? item?.previousElementSibling?.querySelector<HTMLElement>(".project-task-link")
          ?? item?.closest("section")?.querySelector<HTMLElement>(".project-task-heading");
        setOpen(false); onDelete(row, () => requestAnimationFrame(() => target?.focus()));
      }}><Trash2 aria-hidden="true" /></button></TooltipHint>
  </li>;
}

function ProjectTasks({ workspace, hiddenIds, selectedId, onSelect, onDelete, menuOpen, onMenuOpenChange, onEditWorkspace, onRemoveWorkspace }: Omit<Props, "workspaces" | "onWorkspaceRename" | "onWorkspaceRemove"> & {
  workspace: Workspace;
  menuOpen: boolean;
  onMenuOpenChange: (open: boolean) => void;
  onEditWorkspace: (workspace: Workspace) => void;
  onRemoveWorkspace: (workspace: Workspace) => void;
}) {
  const [expanded, setExpanded] = useState(true);
  const [rows, setRows] = useState<ExecutionView[]>([]);
  const [cursor, setCursor] = useState<string | null>(null);
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState("");
  const [moreError, setMoreError] = useState("");
  const [refreshing, setRefreshing] = useState(false);
  const [loadingMore, setLoadingMore] = useState(false);
  const pages = useRef(1);
  const waiting = useRef(false);
  const generation = useRef(0);
  const listId = useId();

  useEffect(() => {
    if (!expanded) return;
    const version = ++generation.current;
    let disposed = false;
    async function refresh() {
      if (waiting.current) return;
      waiting.current = true;
      setRefreshing(true);
      try {
        const next: ExecutionView[] = [];
        let after: string | null = null;
        for (let page = 0; page < pages.current; page++) {
          const result = await api.agentHistory(after, workspace.root);
          if (disposed) return;
          next.push(...result.executions);
          after = result.nextCursor;
          if (!after) break;
        }
        setRows(next); setCursor(after); setLoaded(true); setError("");
      } catch {
        if (!disposed) setError("任务加载失败");
      } finally { if (generation.current === version) { waiting.current = false; setRefreshing(false); } }
    }
    void refresh();
    const timer = window.setInterval(() => void refresh(), 3000);
    return () => { disposed = true; generation.current = version + 1; waiting.current = false; window.clearInterval(timer); };
  }, [workspace.root, expanded]);

  async function more() {
    if (!cursor || waiting.current) return;
    const version = generation.current;
    waiting.current = true; setLoadingMore(true); setMoreError("");
    try {
      const result = await api.agentHistory(cursor, workspace.root);
      if (version !== generation.current) return;
      setRows(old => [...old, ...result.executions.filter(row => !old.some(existing => existing.executionId === row.executionId))]);
      pages.current++; setCursor(result.nextCursor); setMoreError("");
    } catch { if (version === generation.current) setMoreError("加载更多失败，请重试"); }
    finally { if (version === generation.current) { waiting.current = false; setLoadingMore(false); } }
  }

  const visible = rows.filter(row => !hiddenIds.includes(row.executionId));
  return <section className="project-task-group" aria-label={workspace.name}>
    <div className="project-task-header" data-menu-open={menuOpen || undefined}>
      <TooltipHint content="拖动可排序；也可使用 Alt + ↑/↓"><button className="project-task-heading" aria-expanded={expanded} aria-controls={listId} onClick={() => { setExpanded(!expanded); setLoadingMore(false); }}>
        <Folder aria-hidden="true" /><span>{workspace.name}</span>
      </button></TooltipHint>
      <Popover.Root open={menuOpen} onOpenChange={onMenuOpenChange}>
        <Popover.Trigger asChild>
          <button
            className="project-workspace-more"
            aria-label={`打开工作区菜单：${workspace.name}`}
            aria-haspopup="menu"
            aria-expanded={menuOpen}
            onClick={(event) => event.stopPropagation()}
          >
            <Ellipsis aria-hidden="true" />
          </button>
        </Popover.Trigger>
        <Popover.Portal>
          {/* Portal 脱离侧栏滚动容器，菜单可以从右侧跨过滚动条展示。 */}
          <Popover.Content className="project-workspace-menu-content" role="menu" side="right" align="start" sideOffset={8} collisionPadding={12}>
            <button role="menuitem" onClick={() => { onMenuOpenChange(false); onEditWorkspace(workspace); }}><Pencil aria-hidden="true" />编辑</button>
            <button role="menuitem" className="project-workspace-menu-delete" onClick={() => { onMenuOpenChange(false); onRemoveWorkspace(workspace); }}><Trash2 aria-hidden="true" />删除</button>
          </Popover.Content>
        </Popover.Portal>
      </Popover.Root>
      <button
        className="project-workspace-collapse"
        aria-label={`${expanded ? "折叠" : "展开"}工作区：${workspace.name}`}
        aria-expanded={expanded}
        aria-controls={listId}
        onClick={() => { setExpanded(!expanded); setLoadingMore(false); }}
      >
        <ChevronDown className={expanded ? "" : "collapsed"} aria-hidden="true" />
      </button>
    </div>
    {expanded && <div id={listId}>
      {error && <p className="project-task-message" role="status">{error}</p>}
      {moreError && <p className="project-task-message" role="status">{moreError}</p>}
      {!loaded && !error && <p className="project-task-message">正在加载…</p>}
      {loaded && !visible.length && <p className="project-task-message">暂无任务</p>}
      <ul>{visible.map(row => <TaskItem key={row.executionId} row={row} workspace={workspace} selected={selectedId === row.executionId} onSelect={onSelect} onDelete={onDelete} />)}</ul>
      {cursor && <button className="project-task-more" disabled={loadingMore || refreshing} aria-busy={loadingMore}
        data-state={loadingMore ? "loading" : moreError ? "retry" : "idle"} onClick={() => void more()}>
        <span className="project-task-more-icon-slot" aria-hidden="true">
          <ChevronDown className="project-task-more-icon-idle" />
          <LoaderCircle className="project-task-more-icon-loading" />
          <RefreshCw className="project-task-more-icon-retry" />
        </span>
        <span key={loadingMore ? "loading" : moreError ? "retry" : "idle"} className="project-task-more-label">
          {loadingMore ? "正在加载…" : moreError ? "重试" : "查看更多"}
        </span>
      </button>}
    </div>}
  </section>;
}

export function ProjectTaskNavigation({ workspaces, onWorkspaceRename, onWorkspaceRemove, ...props }: Props) {
  const [order, setOrder] = useState<string[]>(() => {
    try { const value: unknown = JSON.parse(window.localStorage.getItem("agent-project-order") ?? "[]"); return Array.isArray(value) ? value.filter((id): id is string => typeof id === "string") : []; }
    catch { return []; }
  });
  const [target, setTarget] = useState<{root: string; after: boolean} | null>(null);
  const [openWorkspaceMenuId, setOpenWorkspaceMenuId] = useState<string | null>(null);
  const [editingWorkspace, setEditingWorkspace] = useState<Workspace | null>(null);
  const [workspaceName, setWorkspaceName] = useState("");
  const [removingWorkspace, setRemovingWorkspace] = useState<Workspace | null>(null);
  const [workspaceBusy, setWorkspaceBusy] = useState(false);
  const drag = useRef<{root: string; y: number; active: boolean} | null>(null);
  const suppressClick = useRef(false);
  const sorted = [...workspaces].sort((a,b) => {
    const rank = (id: string) => { const index = order.indexOf(id); return index < 0 ? order.length : index; };
    return rank(a.root) - rank(b.root);
  });
  function move(source: string, destination: string, after: boolean) {
    if (source === destination) return;
    const next = sorted.map(w => w.root).filter(id => id !== source);
    const index = next.indexOf(destination);
    if (index < 0) return;
    next.splice(index + Number(after), 0, source);
    try { window.localStorage.setItem("agent-project-order", JSON.stringify(next)); setOrder(next); }
    catch { toast.error("项目排序保存失败，请重试"); }
  }
  async function renameWorkspace() {
    if (!editingWorkspace || !workspaceName.trim() || workspaceBusy) return;
    setWorkspaceBusy(true);
    try {
      if (await onWorkspaceRename(editingWorkspace.id, workspaceName.trim())) setEditingWorkspace(null);
    } finally {
      setWorkspaceBusy(false);
    }
  }

  async function removeWorkspace() {
    if (!removingWorkspace || workspaceBusy) return;
    setWorkspaceBusy(true);
    try {
      if (await onWorkspaceRemove(removingWorkspace.id)) setRemovingWorkspace(null);
    } finally {
      setWorkspaceBusy(false);
    }
  }

  return <>
  <nav className="project-task-navigation" aria-label="工作区任务">
    <h2>工作区</h2>
    {sorted.length ? sorted.map((workspace, index) => <div key={workspace.root} data-project-root={workspace.root}
      className="project-sort-item" data-drop={target?.root === workspace.root ? (target.after ? "after" : "before") : undefined}
      onPointerDown={event => {
        if (event.button !== 0 || !(event.target as Element).closest(".project-task-heading")) return;
        suppressClick.current = false;
        drag.current = {root: workspace.root, y: event.clientY, active: false};
        (event.target as Element).closest<HTMLElement>(".project-task-heading")!.setPointerCapture(event.pointerId);
      }}
      onPointerMove={event => {
        const current = drag.current;
        if (!current) return;
        if (Math.abs(event.clientY - current.y) > 5) current.active = true;
        if (!current.active) return;
        const item = document.elementFromPoint(event.clientX, event.clientY)?.closest<HTMLElement>("[data-project-root]");
        if (!item || !event.currentTarget.parentElement?.contains(item)) { setTarget(null); return; }
        const bounds = item.getBoundingClientRect();
        setTarget({root: item.dataset.projectRoot!, after: event.clientY > bounds.top + bounds.height / 2});
      }}
      onPointerUp={() => {
        if (drag.current?.active) { suppressClick.current = true; if (target) move(drag.current.root, target.root, target.after); }
        drag.current = null; setTarget(null);
      }}
      onPointerCancel={() => { drag.current = null; setTarget(null); }}
      onLostPointerCapture={() => { drag.current = null; setTarget(null); }}
      onClickCapture={event => { if (suppressClick.current) { suppressClick.current = false; if ((event.target as Element).closest(".project-task-heading")) { event.stopPropagation(); event.preventDefault(); } } }}
      onKeyDown={event => {
        if (!event.altKey || !(event.target as Element).closest(".project-task-heading")) return;
        const offset = event.key === "ArrowUp" ? -1 : event.key === "ArrowDown" ? 1 : 0;
        if (!offset) return;
        event.preventDefault();
        const neighbor = sorted[index + offset];
        if (neighbor) move(workspace.root, neighbor.root, offset > 0);
      }}>
      <ProjectTasks
        workspace={workspace}
        {...props}
        menuOpen={openWorkspaceMenuId === workspace.id}
        onMenuOpenChange={(open) => setOpenWorkspaceMenuId(open ? workspace.id : null)}
        onEditWorkspace={(targetWorkspace) => { setWorkspaceName(targetWorkspace.name); setEditingWorkspace(targetWorkspace); }}
        onRemoveWorkspace={setRemovingWorkspace}
      />
    </div>) : <p className="project-task-message">暂无项目</p>}
  </nav>
  <Dialog open={editingWorkspace !== null} onOpenChange={(open) => { if (!open && !workspaceBusy) setEditingWorkspace(null); }}>
    <DialogContent>
      <DialogHeader>
        <DialogTitle>编辑工作区</DialogTitle>
        <DialogDescription>修改“{editingWorkspace?.name}”在 Serena Desktop 中的显示名称，不会更改本地目录。</DialogDescription>
      </DialogHeader>
      <form onSubmit={(event) => { event.preventDefault(); void renameWorkspace(); }}>
        <Input aria-label="工作区名称" autoFocus value={workspaceName} disabled={workspaceBusy} onChange={(event) => setWorkspaceName(event.target.value)} />
        <DialogFooter>
          <Button type="button" variant="outline" disabled={workspaceBusy} onClick={() => setEditingWorkspace(null)}>取消</Button>
          <Button type="submit" disabled={workspaceBusy || !workspaceName.trim()} aria-busy={workspaceBusy}>{workspaceBusy ? "保存中…" : "保存"}</Button>
        </DialogFooter>
      </form>
    </DialogContent>
  </Dialog>
  <Dialog open={removingWorkspace !== null} onOpenChange={(open) => { if (!open && !workspaceBusy) setRemovingWorkspace(null); }}>
    <DialogContent>
      <DialogHeader>
        <DialogTitle>删除工作区</DialogTitle>
        <DialogDescription>确定从 Serena Desktop 移除“{removingWorkspace?.name}”吗？不会删除本地目录、源码或 Git 仓库。</DialogDescription>
      </DialogHeader>
      <DialogFooter>
        <Button variant="outline" disabled={workspaceBusy} onClick={() => setRemovingWorkspace(null)}>取消</Button>
        <Button variant="destructive" disabled={workspaceBusy} aria-busy={workspaceBusy} onClick={() => void removeWorkspace()}>{workspaceBusy ? "删除中…" : "删除"}</Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
  </>;
}
