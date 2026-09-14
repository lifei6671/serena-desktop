import { Badge } from "@/components/ui/badge";
import { ArrowDown, Check, Copy, FolderOpen, Search, Trash2, X } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { useEffect, useMemo, useRef, useState, type MouseEvent, type ReactNode } from "react";
import { api } from "./api";
import { countNewLogLines, filterMcpLogs, reconcileMcpLogEntries, sameLogLines, type McpLogEntry } from "./mcpLogPresentation";

const levelOrder = ["TRACE", "DEBUG", "INFO", "WARN", "ERROR"];
const knownSources = ["MCP", "TOOL"];
const allFilterValue = "all";

function orderedOptions(known: string[], observed: Array<string | null>) {
  const extras = [...new Set(observed.flatMap((value) => value ? [value] : []))]
    .filter((value) => !known.includes(value))
    .sort((left, right) => left.localeCompare(right));
  return [...known, ...extras];
}

function levelVariant(level: string | null) {
  if (level === "ERROR") return "destructive";
  if (level === "WARN") return "warning";
  return "outline";
}

function DetailField({ label, children }: { label: string; children: ReactNode }) {
  return <div className="log-detail-field"><span>{label}</span><span>{children}</span></div>;
}

export function McpLogs() {
  const [lines, setLines] = useState<string[] | null>(null);
  const [entries, setEntries] = useState<McpLogEntry[]>([]);
  const [clearing, setClearing] = useState(false);
  const [source, setSource] = useState(allFilterValue);
  const [level, setLevel] = useState(allFilterValue);
  const [queryInput, setQueryInput] = useState("");
  const [appliedQuery, setAppliedQuery] = useState("");
  const [following, setFollowing] = useState(true);
  const [newLogCount, setNewLogCount] = useState(0);
  const [selectedEntry, setSelectedEntry] = useState<McpLogEntry | null>(null);
  const [copying, setCopying] = useState(false);
  const [copiedEntryId, setCopiedEntryId] = useState<string | null>(null);
  const [selectedIds, setSelectedIds] = useState<Set<string>>(() => new Set());
  const [selectionAnchor, setSelectionAnchor] = useState<string | null>(null);
  const [copyingSelected, setCopyingSelected] = useState(false);
  const [copiedSelected, setCopiedSelected] = useState(false);
  const clearingRef = useRef(false);
  const readEpoch = useRef(0);
  const entryId = useRef(0);
  const knownLines = useRef<string[] | null>(null);
  const followingRef = useRef(true);

  const viewport = useRef<HTMLDivElement>(null);

  const sources = useMemo(
    () => orderedOptions(knownSources, entries.map((entry) => entry.source)),
    [entries],
  );
  const levels = useMemo(
    () => orderedOptions(levelOrder, entries.map((entry) => entry.level)),
    [entries],
  );
  const filteredEntries = useMemo(
    () => filterMcpLogs(entries, {
      source: source === allFilterValue ? "" : source,
      level: level === allFilterValue ? "" : level,
      query: appliedQuery,
    }),
    [entries, source, level, appliedQuery],
  );
  const selectedEntries = useMemo(
    () => filteredEntries.filter((entry) => selectedIds.has(entry.id)),
    [filteredEntries, selectedIds],
  );
  const copied = copiedEntryId === selectedEntry?.id;

  useEffect(() => {
    if (source !== allFilterValue && !sources.includes(source)) setSource(allFilterValue);
  }, [source, sources]);

  useEffect(() => {
    if (level !== allFilterValue && !levels.includes(level)) setLevel(allFilterValue);
  }, [level, levels]);

  useEffect(() => {
    followingRef.current = following;
  }, [following]);

  useEffect(() => {
    if (!copiedEntryId) return;
    const timer = window.setTimeout(() => setCopiedEntryId(null), 1500);
    return () => window.clearTimeout(timer);
  }, [copiedEntryId]);

  useEffect(() => {
    if (!copiedSelected) return;
    const timer = window.setTimeout(() => setCopiedSelected(false), 1500);
    return () => window.clearTimeout(timer);
  }, [copiedSelected]);

  useEffect(() => {
    const visibleIds = new Set(filteredEntries.map((entry) => entry.id));
    setSelectedIds((current) => {
      const next = new Set([...current].filter((id) => visibleIds.has(id)));
      return next.size === current.size ? current : next;
    });
    if (selectionAnchor && !visibleIds.has(selectionAnchor)) setSelectionAnchor(null);
  }, [filteredEntries, selectionAnchor]);

  useEffect(() => {
    const clearSelectionOnEscape = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || selectedIds.size === 0) return;
      event.preventDefault();
      setSelectedIds(new Set());
      setSelectionAnchor(null);
      setCopiedSelected(false);
    };
    window.addEventListener("keydown", clearSelectionOnEscape);
    return () => window.removeEventListener("keydown", clearSelectionOnEscape);
  }, [selectedIds.size]);

  useEffect(() => {
    let active = true;
    let timer: number | undefined;
    let lastError = "";
    const refresh = async () => {
      const epoch = readEpoch.current;
      try {
        if (clearingRef.current) return;
        const next = await api.mcpLogs();
        if (active && epoch === readEpoch.current) {
          const added = countNewLogLines(knownLines.current, next);
          if (!sameLogLines(knownLines.current, next)) {
            knownLines.current = next;
            setLines(next);
            setEntries((previous) => reconcileMcpLogEntries(previous, next, (raw) => `${raw}\u0000${entryId.current++}`));
            if (!followingRef.current && added) setNewLogCount((count) => count + added);
          }
          lastError = "";
        }
      } catch (reason) {
        if (
          active &&
          epoch === readEpoch.current &&
          lastError !== String(reason)
        ) {
          lastError = String(reason);
          toast.error(`日志读取失败：${lastError}`, { id: "mcp-log-error" });
        }
      } finally {
        if (active) timer = window.setTimeout(refresh, 1000);
      }
    };
    void refresh();
    return () => {
      active = false;
      window.clearTimeout(timer);
    };
  }, []);

  useEffect(() => {
    if (following && viewport.current) {
      viewport.current.scrollTop = viewport.current.scrollHeight;
    }
  }, [filteredEntries, following]);

  const clear = async () => {
    if (clearingRef.current) return;
    clearingRef.current = true;
    setClearing(true);
    readEpoch.current++;
    try {
      await api.clearMcpLogs();
      knownLines.current = [];
      setLines([]);
      setEntries([]);
      followingRef.current = true;
      setFollowing(true);
      setNewLogCount(0);
      setSelectedEntry(null);
      setSelectedIds(new Set());
      setSelectionAnchor(null);
      setCopiedEntryId(null);
      toast.success("日志已清空");
    } catch (reason) {
      toast.error(`清空日志失败：${String(reason)}`);
    } finally {
      clearingRef.current = false;
      setClearing(false);
    }
  };

  const openLogDirectory = async () => {
    try {
      await api.openLogs();
    } catch (reason) {
      toast.error(`打开日志目录失败：${String(reason)}`);
    }
  };

  const copyEntry = async () => {
    if (!selectedEntry) return;
    const entry = selectedEntry;
    setCopying(true);
    setCopiedEntryId(null);
    try {
      await navigator.clipboard.writeText(entry.raw);
      setCopiedEntryId(entry.id);
      toast.success("日志已复制");
    } catch (reason) {
      toast.error(`复制日志失败：${String(reason)}`);
    } finally {
      setCopying(false);
    }
  };

  const resumeFollowing = () => {
    followingRef.current = true;
    setFollowing(true);
    setNewLogCount(0);
  };

  const applyQuery = () => setAppliedQuery(queryInput);

  const clearSelection = () => {
    setSelectedIds(new Set());
    setSelectionAnchor(null);
    setCopiedSelected(false);
  };

  const selectLogEntry = (entry: McpLogEntry, event: MouseEvent<HTMLButtonElement>) => {
    if (event.shiftKey) {
      const anchorIndex = selectionAnchor ? filteredEntries.findIndex((item) => item.id === selectionAnchor) : -1;
      const currentIndex = filteredEntries.findIndex((item) => item.id === entry.id);
      const range = anchorIndex === -1
        ? [entry.id]
        : filteredEntries.slice(Math.min(anchorIndex, currentIndex), Math.max(anchorIndex, currentIndex) + 1).map((item) => item.id);
      setSelectedIds((current) => new Set([...current, ...range]));
      setSelectionAnchor(entry.id);
      setSelectedEntry(null);
      setCopiedEntryId(null);
      setCopiedSelected(false);
      return;
    }

    if (event.ctrlKey || event.metaKey) {
      setSelectedIds((current) => {
        const next = new Set(current);
        if (next.has(entry.id)) next.delete(entry.id);
        else next.add(entry.id);
        return next;
      });
      setSelectionAnchor(entry.id);
      setSelectedEntry(null);
      setCopiedEntryId(null);
      setCopiedSelected(false);
      return;
    }

    if (selectedEntries.length) clearSelection();
    setSelectedEntry(entry);
    setCopiedEntryId(null);
  };

  const copySelectedEntries = async () => {
    const raw = selectedEntries.map((entry) => entry.raw).join("\n");
    if (!raw) return;
    setCopyingSelected(true);
    setCopiedSelected(false);
    try {
      await navigator.clipboard.writeText(raw);
      setCopiedSelected(true);
      toast.success("已复制选中日志");
    } catch (reason) {
      toast.error(`复制选中日志失败：${String(reason)}`);
    } finally {
      setCopyingSelected(false);
    }
  };

  return (
    <section className="logs-page">
      <div className="page-heading">
        <div>
          <h1>日志终端</h1>
          <p>MCP Broker 实时日志与诊断；完整日志可从目录查看。</p>
        </div>
        <div className="logs-actions">
          <Button variant="outline" size="sm" onClick={() => void openLogDirectory()}>
            <FolderOpen />打开日志目录
          </Button>
          <Button
            variant="outline"
            size="sm"
            aria-label="清空日志"
            disabled={clearing}
            aria-busy={clearing}
            onClick={() => void clear()}
          >
            {clearing ? <Spinner /> : <Trash2 />}清空日志
          </Button>
        </div>
      </div>

      <div className="log-workspace">
        <div className="log-filter-toolbar" role="group" aria-label="日志筛选">
          <div className="log-filter-select">
            <Select value={source} onValueChange={setSource}>
              <SelectTrigger id="log-source-filter" size="sm" aria-label="按来源过滤">来源：<SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value={allFilterValue}>全部来源</SelectItem>
                {sources.map((item) => <SelectItem key={item} value={item}>{item}</SelectItem>)}
              </SelectContent>
            </Select>
          </div>
          <div className="log-filter-select">
            <Select value={level} onValueChange={setLevel}>
              <SelectTrigger id="log-level-filter" size="sm" aria-label="按级别过滤">级别：<SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value={allFilterValue}>全部级别</SelectItem>
                {levels.map((item) => <SelectItem key={item} value={item}>{item}</SelectItem>)}
              </SelectContent>
            </Select>
          </div>
          <form className="log-search-form" role="search" onSubmit={(event) => { event.preventDefault(); applyQuery(); }}>
            <div className="log-search-field">
              <Search aria-hidden="true" />
              <input aria-label="搜索日志" value={queryInput} onChange={(event) => setQueryInput(event.target.value)} placeholder="搜索日志" />
            </div>
            <Button type="submit" variant="outline" size="xs" aria-label="搜索"><Search />搜索</Button>
          </form>
          <Button variant="outline" size="xs" aria-pressed={following} onClick={resumeFollowing} className="log-follow-button">
            <span aria-hidden="true" className="log-follow-dot" data-following={following} />跟随最新
          </Button>
        </div>
        <div className="log-viewer" data-multiselect={selectedEntries.length > 0 || undefined}>
          <section className="log-table" aria-label="MCP 运行日志">
            <div className="log-table-header" aria-hidden="true">
              <span>TIME</span><span>LEVEL</span><span>SOURCE</span><span>MESSAGE / PAYLOAD</span>
            </div>
            <div
              className="log-stream"
              ref={viewport}
              role="region"
              aria-label="MCP 运行日志内容"
              tabIndex={0}
              onScroll={(event) => {
                const node = event.currentTarget;
                const isAtBottom = node.scrollHeight - node.scrollTop - node.clientHeight < 32;
                followingRef.current = isAtBottom;
                setFollowing(isAtBottom);
                if (isAtBottom) setNewLogCount(0);
              }}
            >
              {filteredEntries.length ? filteredEntries.map((entry) => (
                <button
                  key={entry.id}
                  type="button"
                  className="log-entry"
                  data-selected={selectedEntry?.id === entry.id || selectedIds.has(entry.id) || undefined}
                  data-level={entry.level?.toLocaleLowerCase() ?? "raw"}
                  onClick={(event) => selectLogEntry(entry, event)}
                >
                  <span className="log-time">{entry.timestamp?.slice(11) ?? "—"}</span>
                  <span className="log-level"><Badge variant={levelVariant(entry.level)}>{entry.level ?? "—"}</Badge></span>
                  <span className="log-source" title={entry.source ?? undefined}>{entry.source ?? "—"}</span>
                  <span className="log-message" title={entry.message}>{entry.message}</span>
                </button>
              )) : (
                <p className="log-empty">
                  {lines === null ? "正在读取日志…" : lines.length ? "没有匹配当前筛选条件的日志。" : "暂无 MCP 日志，启动连接入口或发起请求后会在这里显示。"}
                </p>
              )}
              {!following && newLogCount > 0 && (
                <Button size="xs" className="log-new-entries" onClick={resumeFollowing}>
                  <ArrowDown />{newLogCount} 条新日志 · 回到最新
                </Button>
              )}
            </div>
          </section>
          {selectedEntries.length > 0 && (
            <div className="log-batch-actions" aria-label="批量日志操作">
              <span><i aria-hidden="true" />已选择 {selectedEntries.length} 条</span>
              <div>
                <Button className="log-batch-copy-button" variant="secondary" size="xs" disabled={copyingSelected} aria-busy={copyingSelected} data-copied={copiedSelected || undefined} onClick={() => void copySelectedEntries()}>
                  {copyingSelected ? <Spinner /> : copiedSelected ? <Check /> : <Copy />}{copiedSelected ? "已复制" : "复制选中（完整内容）"}
                </Button>
                <Button variant="ghost" size="xs" onClick={clearSelection}>清除选择（Esc）</Button>
              </div>
            </div>
          )}
          {selectedEntry && selectedEntries.length === 0 && (
            <aside className="log-details" aria-label="日志详情">
              <header>
                <strong>日志详情</strong>
                <Button variant="ghost" size="icon-xs" aria-label="关闭详情" onClick={() => setSelectedEntry(null)}><X /></Button>
              </header>
              <div className="log-details-content">
                {(selectedEntry.timestamp || selectedEntry.level || selectedEntry.source) && (
                  <section>
                    <h2>基本信息</h2>
                    <div className="log-detail-card">
                      {selectedEntry.timestamp && <DetailField label="时间">{selectedEntry.timestamp}</DetailField>}
                      {selectedEntry.level && <DetailField label="级别"><Badge variant={levelVariant(selectedEntry.level)}>{selectedEntry.level}</Badge></DetailField>}
                      {selectedEntry.source && <DetailField label="来源">{selectedEntry.source}</DetailField>}
                    </div>
                  </section>
                )}
                <section>
                  <h2>{selectedEntry.message === selectedEntry.raw ? "原始内容" : "日志正文"}</h2>
                  <pre className="log-detail-body">{selectedEntry.message}</pre>
                </section>
                {selectedEntry.message !== selectedEntry.raw && (
                  <section>
                    <h2>原始内容</h2>
                    <pre className="log-detail-raw">{selectedEntry.raw}</pre>
                  </section>
                )}
                <Button variant="outline" size="sm" disabled={copying} aria-busy={copying} onClick={() => void copyEntry()}>
                  {copying ? <Spinner /> : copied ? <Check /> : <Copy />}{copied ? "已复制" : "复制日志"}
                </Button>
              </div>
            </aside>
          )}
        </div>
        <footer className="log-status-bar">
          <span><i aria-hidden="true" />实时 · {source === allFilterValue ? "全部来源" : source}</span>
          <span>显示 {filteredEntries.length} 条 · 最多保留 500 条</span>
        </footer>
      </div>
    </section>
  );
}
