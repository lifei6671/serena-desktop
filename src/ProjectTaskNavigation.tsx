import { TooltipHint } from "@/components/TooltipHint";
import { useEffect, useId, useRef, useState } from "react";
import { HoverCard } from "radix-ui";
import { CalendarDays, ChevronDown, CircleAlert, Folder, LoaderCircle, Monitor, Trash2 } from "lucide-react";
import { api } from "./api";
import { executionStatus, executionTime, taskSummary } from "./agentPresentation";
import type { ExecutionView, Workspace } from "./types";

import { toast } from "sonner";

type Props = {
  workspaces: Workspace[];
  hiddenIds: string[];
  selectedId?: string;
  onSelect: (row: ExecutionView, trigger: HTMLElement) => void;
  onDelete: (row: ExecutionView, afterDelete?: () => void) => void;
};

function TaskItem({ row, workspace, selected, onSelect, onDelete }: {
  row: ExecutionView; workspace: Workspace; selected: boolean;
  onSelect: Props["onSelect"]; onDelete: Props["onDelete"];
}) {
  const [open, setOpen] = useState(false);
  const infoId = useId();
  const status = executionStatus(row);
  const days = Math.max(0, Math.floor((Date.now() - row.updatedAt) / 86400000));
  return <li className="project-task" data-selected={selected}>
    <HoverCard.Root open={open} onOpenChange={setOpen} openDelay={350} closeDelay={100}>
      <HoverCard.Trigger asChild>
        <button className="project-task-link" aria-current={selected ? "page" : undefined}
          aria-describedby={open ? infoId : undefined}
          onFocus={() => setOpen(true)} onBlur={() => setOpen(false)}
          onClick={event => { setOpen(false); onSelect(row, event.currentTarget); }}>
          {status.tone === "blue" && <LoaderCircle className="project-task-state-icon tone-blue animate-spin motion-reduce:animate-none" role="img" aria-label={status.label} />}
          {status.tone === "red" && <CircleAlert className="project-task-state-icon tone-red" role="img" aria-label={status.label} />}
          <span>{taskSummary(row.prompt) || "未命名任务"}</span>
          <time dateTime={new Date(row.updatedAt).toISOString()}>{days ? `${days}天前` : "今天"}</time>
        </button>
      </HoverCard.Trigger>
      <HoverCard.Portal>
        <HoverCard.Content id={infoId} className="project-task-preview" side="right" align="start" sideOffset={12} collisionPadding={16}>
          <strong>{taskSummary(row.prompt) || "未命名任务"}</strong>
          <p><Monitor aria-hidden="true" />本地任务 <span className={`agent-status tone-${status.tone}`}>{status.label}</span></p>
          <p><Folder aria-hidden="true" />所属项目：{workspace.name}</p>
          <p><CalendarDays aria-hidden="true" />更新于 {executionTime(row.updatedAt)}</p>
        </HoverCard.Content>
      </HoverCard.Portal>
    </HoverCard.Root>
    <TooltipHint content="仅从本机列表删除，不取消执行"><button className="project-task-delete" aria-label={`删除任务：${taskSummary(row.prompt) || "未命名任务"}`}
      onClick={event => {
        const item = event.currentTarget.closest("li");
        const target = item?.nextElementSibling?.querySelector<HTMLElement>(".project-task-link")
          ?? item?.previousElementSibling?.querySelector<HTMLElement>(".project-task-link")
          ?? item?.closest("section")?.querySelector<HTMLElement>(".project-task-heading");
        setOpen(false); onDelete(row, () => requestAnimationFrame(() => target?.focus()));
      }}><Trash2 aria-hidden="true" /></button></TooltipHint>
  </li>;
}

function ProjectTasks({ workspace, hiddenIds, selectedId, onSelect, onDelete }: Omit<Props, "workspaces"> & { workspace: Workspace }) {
  const [expanded, setExpanded] = useState(true);
  const [rows, setRows] = useState<ExecutionView[]>([]);
  const [cursor, setCursor] = useState<string | null>(null);
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState("");
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
      } finally { if (generation.current === version) waiting.current = false; }
    }
    void refresh();
    const timer = window.setInterval(() => void refresh(), 3000);
    return () => { disposed = true; generation.current = version + 1; waiting.current = false; window.clearInterval(timer); };
  }, [workspace.root, expanded]);

  async function more() {
    if (!cursor || waiting.current) return;
    const version = generation.current;
    waiting.current = true; setLoadingMore(true);
    try {
      const result = await api.agentHistory(cursor, workspace.root);
      if (version !== generation.current) return;
      setRows(old => [...old, ...result.executions.filter(row => !old.some(existing => existing.executionId === row.executionId))]);
      pages.current++; setCursor(result.nextCursor); setError("");
    } catch { if (version === generation.current) setError("加载更多失败，请重试"); }
    finally { if (version === generation.current) { waiting.current = false; setLoadingMore(false); } }
  }

  const visible = rows.filter(row => !hiddenIds.includes(row.executionId));
  return <section className="project-task-group" aria-label={workspace.name}>
    <TooltipHint content="拖动可排序；也可使用 Alt + ↑/↓"><button className="project-task-heading" aria-expanded={expanded} aria-controls={listId} onClick={() => { setExpanded(!expanded); setLoadingMore(false); }}>
      <Folder aria-hidden="true" /><span>{workspace.name}</span><ChevronDown className={expanded ? "" : "collapsed"} aria-hidden="true" />
    </button></TooltipHint>
    {expanded && <div id={listId}>
      {error && <p className="project-task-message" role="status">{error}</p>}
      {!loaded && !error && <p className="project-task-message">正在加载…</p>}
      {loaded && !visible.length && <p className="project-task-message">暂无任务</p>}
      <ul>{visible.map(row => <TaskItem key={row.executionId} row={row} workspace={workspace} selected={selectedId === row.executionId} onSelect={onSelect} onDelete={onDelete} />)}</ul>
      {cursor && <button className="project-task-more" disabled={loadingMore} onClick={() => void more()}>{loadingMore ? "正在加载…" : "查看更多"}</button>}
    </div>}
  </section>;
}

export function ProjectTaskNavigation({ workspaces, ...props }: Props) {
  const [order, setOrder] = useState<string[]>(() => {
    try { const value: unknown = JSON.parse(window.localStorage.getItem("agent-project-order") ?? "[]"); return Array.isArray(value) ? value.filter((id): id is string => typeof id === "string") : []; }
    catch { return []; }
  });
  const [target, setTarget] = useState<{root: string; after: boolean} | null>(null);
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
  return <nav className="project-task-navigation" aria-label="项目任务">
    <h2>项目</h2>
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
      <ProjectTasks workspace={workspace} {...props} />
    </div>) : <p className="project-task-message">暂无项目</p>}
  </nav>;
}
