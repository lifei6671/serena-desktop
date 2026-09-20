import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import {
  Dialog,
  DialogTrigger,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
} from "@/components/ui/dialog";
import {
  Select,
  SelectTrigger,
  SelectValue,
  SelectContent,
  SelectGroup,
  SelectItem,
} from "@/components/ui/select";
import { Field, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import {
  Collapsible,
  CollapsibleTrigger,
  CollapsibleContent,
} from "@/components/ui/collapsible";
import { Check, ChevronDownIcon, Folder, RefreshCw } from "lucide-react";
import { Spinner } from "@/components/ui/spinner";
import { Button } from "@/components/ui/button";
import { useEffect, useRef, useState } from "react";
import { api } from "./api";
import type { AppState } from "./types";
import type { useBroker } from "./useBroker";

function displayProjectPath(path: string): string {
  if (path.startsWith("\\\\?\\UNC\\")) return "\\\\" + path.slice(8);
  return path.replace(/^\\\\\?\\(?=[A-Za-z]:\\)/, "");
}

const SYNC_MIN_PENDING_MS = 600;
const COPY_MIN_PENDING_MS = 500;
const SUCCESS_FEEDBACK_MS = 1500;

export function ProjectPanel({
  state,
  controller,
  onSettings,
  onRemote,
  onSelectWorkspace,
  onCopied,
}: {
  state: AppState;
  controller: ReturnType<typeof useBroker>;
  onSettings: () => void;
  onRemote: () => void;
  onSerena: () => void;
  onSelectWorkspace: (id: string) => Promise<void>;
  onCopied: () => void;
}) {
  const { broker, busy, perform } = controller;
  const [selected, setSelected] = useState("");
  const [copyingAddresses, setCopyingAddresses] = useState<Set<string>>(
    () => new Set(),
  );
  const [copiedAddresses, setCopiedAddresses] = useState<Set<string>>(
    () => new Set(),
  );
  const [cancelling, setCancelling] = useState(false);
  const [candidate, setCandidate] = useState<{ root: string; name: string } | null>(null);
  const [pickingDirectory, setPickingDirectory] = useState(false);
  const [registering, setRegistering] = useState(false);
  const importing = busy === "从 Serena 导入中";
  const [importFeedback, setImportFeedback] = useState<"idle" | "pending" | "success">("idle");
  // 浏览器定时器 ID 固定为数字，避免 Node 类型污染 Tauri 前端编译。
  const feedbackTimers = useRef(new Set<number>());
  const mounted = useRef(true);
  const [selectorOpen, setSelectorOpen] = useState(false);
  const [selecting, setSelecting] = useState(false);
  const [helpOpen, setHelpOpen] = useState<boolean | undefined>(undefined);
  const project = state.config.workspaces.find((workspace) => workspace.id === selected);
  const selectedWorkspace = state.desktopSelectedWorkspace;
  const pending = !!busy || !!broker?.operation || selecting;
  const current = !!project && project.id === selectedWorkspace?.id;
  const endpoint = broker?.running
    ? `http://127.0.0.1:${broker.port}/mcp`
    : null;
  useEffect(() => {
    const timers = feedbackTimers.current;
    mounted.current = true;
    return () => {
      mounted.current = false;
      timers.forEach((timer) => window.clearTimeout(timer));
      timers.clear();
    };
  }, []);
  const waitForFeedback = (duration: number) =>
    new Promise<void>((resolve) => {
      const timer = window.setTimeout(() => {
        feedbackTimers.current.delete(timer);
        resolve();
      }, duration);
      feedbackTimers.current.add(timer);
    });
  const resetAfterSuccess = (reset: () => void) => {
    const timer = window.setTimeout(() => {
      feedbackTimers.current.delete(timer);
      if (mounted.current) reset();
    }, SUCCESS_FEEDBACK_MS);
    feedbackTimers.current.add(timer);
  };
  const cancelOperation = async () => {
    setCancelling(true);
    try {
      await api.cancelProject();
      toast.info("已请求取消操作");
    } catch (reason) {
      toast.error(String(reason));
    } finally {
      setCancelling(false);
    }
  };
  const copyEndpoint = async (address: string) => {
    const startedAt = Date.now();
    setCopyingAddresses((current) => new Set(current).add(address));
    try {
      await navigator.clipboard.writeText(address);
      await waitForFeedback(
        Math.max(0, COPY_MIN_PENDING_MS - (Date.now() - startedAt)),
      );
      if (!mounted.current) return;
      setCopyingAddresses((current) => {
        const next = new Set(current);
        next.delete(address);
        return next;
      });
      setCopiedAddresses((current) => new Set(current).add(address));
      onCopied();
      resetAfterSuccess(() => {
        setCopiedAddresses((current) => {
          const next = new Set(current);
          next.delete(address);
          return next;
        });
      });
    } catch {
      await waitForFeedback(
        Math.max(0, COPY_MIN_PENDING_MS - (Date.now() - startedAt)),
      );
      if (!mounted.current) return;
      toast.error("复制失败，请手动选择地址复制。");
    } finally {
      if (mounted.current) {
        setCopyingAddresses((current) => {
          const next = new Set(current);
          next.delete(address);
          return next;
        });
      }
    }
  };
  const openSelector = () => {
    setSelected(selectedWorkspace?.id ?? "");
  };
  const selectWorkspace = async () => {
    if (!project || selecting) return;
    setSelecting(true);
    try {
      await onSelectWorkspace(project.id);
      setSelectorOpen(false);
    } finally {
      setSelecting(false);
    }
  };
  const importSerena = async () => {
    if (importFeedback !== "idle" || importing) return;
    const startedAt = Date.now();
    let importedCount: number | null = null;
    setImportFeedback("pending");
    await perform("从 Serena 导入中", async () => {
      try {
        importedCount = await api.workspaceImportSerena();
      } catch (reason) {
        await waitForFeedback(
          Math.max(0, SYNC_MIN_PENDING_MS - (Date.now() - startedAt)),
        );
        throw reason;
      }
      await waitForFeedback(
        Math.max(0, SYNC_MIN_PENDING_MS - (Date.now() - startedAt)),
      );
      toast.success(
        importedCount > 0
          ? `已导入 ${importedCount} 个项目`
          : "没有新的 Serena 项目",
      );
    });
    if (!mounted.current) return;
    if (importedCount === null) {
      setImportFeedback("idle");
      return;
    }
    setImportFeedback("success");
    resetAfterSuccess(() => setImportFeedback("idle"));
  };
  const pickDirectory = async () => {
    setPickingDirectory(true);
    try {
      const root = await api.workspacePickDirectory();
      if (!root) return;
      const inspection = await api.workspaceInspectDirectory(root);
      setCandidate({
        root: inspection.canonicalRoot,
        name: inspection.folderBasename ?? "",
      });
    } catch (reason) {
      toast.error(String(reason));
    } finally {
      setPickingDirectory(false);
    }
  };
  const registerCandidate = async () => {
    if (!candidate || registering || !candidate.name.trim()) return;
    let registered = false;
    setRegistering(true);
    await perform("登记项目中", async () => {
      await api.workspaceRegister(candidate.root, candidate.name.trim());
      registered = true;
    });
    if (registered && mounted.current) setCandidate(null);
    if (mounted.current) setRegistering(false);
  };
  const importVisualState = importFeedback === "success"
    ? "success"
    : importing || importFeedback === "pending"
      ? "pending"
      : "idle";
  return (
    <Dialog open={selectorOpen} onOpenChange={setSelectorOpen}>
      <div className="project-panel">
        <div className="page-heading">
          <div>
            <h1>开始使用</h1>
            <p>选择一个工作区，作为当前查看和新任务的默认项目。</p>
          </div>
          <div className="page-heading-actions">
            <Button
              className="sync-project-button"
              variant="outline"
              disabled={importVisualState !== "idle"}
              aria-busy={importVisualState === "pending"}
              onClick={() => void importSerena()}
            >
              {importVisualState === "pending" ? (
                <Spinner data-icon="inline-start" aria-hidden="true" />
              ) : importVisualState === "success" ? (
                <Check data-icon="inline-start" aria-hidden="true" />
              ) : (
                <RefreshCw data-icon="inline-start" aria-hidden="true" />
              )}
              {importVisualState === "pending"
                ? "导入中…"
                : importVisualState === "success"
                  ? "已导入"
                  : "从 Serena 导入"}
            </Button>
            <Button
              disabled={pickingDirectory || registering}
              aria-busy={pickingDirectory}
              onClick={() => void pickDirectory()}
            >
              {pickingDirectory && <Spinner data-icon="inline-start" aria-hidden="true" />}
              {pickingDirectory ? "选择并检查目录中…" : "添加项目"}
            </Button>
          </div>
        </div>
        {candidate && (
          <section className="project-registration" aria-labelledby="register-project-title">
            <h2 id="register-project-title">待添加项目</h2>
            <p>
              <code className="project-path">{displayProjectPath(candidate.root)}</code>
            </p>
            <Field>
              <FieldLabel htmlFor="workspace-register-name">项目名称</FieldLabel>
              <Input
                id="workspace-register-name"
                value={candidate.name}
                onChange={(event) => setCandidate({ ...candidate, name: event.target.value })}
              />
            </Field>
            <div className="project-registration-actions">
              <Button
                disabled={registering || !candidate.name.trim()}
                aria-busy={registering}
                onClick={() => void registerCandidate()}
              >
                {registering && <Spinner data-icon="inline-start" aria-hidden="true" />}
                {registering ? "登记中…" : "登记项目"}
              </Button>
              <Button variant="outline" disabled={registering} onClick={() => setCandidate(null)}>
                取消
              </Button>
            </div>
          </section>
        )}
        <section className="home-section workspace-overview" aria-labelledby="workspace-title">
          <div className="workspace-summary workspace-overview-summary">
            <div className="workspace-identity">
              {selectedWorkspace ? (
                <div className="workspace-active-identity">
                  <span className="workspace-folder-icon" aria-hidden="true">
                    <Folder />
                  </span>
                  <div className="workspace-active-details">
                    <div className="workspace-name">
                      <h2 id="workspace-title">项目能力 · {selectedWorkspace.name}</h2>
                      <Badge className="workspace-active-badge" variant="secondary">
                        已选择
                      </Badge>
                    </div>
                    <code className="project-path">
                      {displayProjectPath(selectedWorkspace.root)}
                    </code>
                  </div>
                </div>
              ) : (
                <>
                  <div className="workspace-name">
                    <h2 id="workspace-title">项目能力</h2>
                  </div>
                  <p>尚未选择工作区。选择一个工作区后，新任务会默认使用它。</p>
                </>
              )}
            </div>
            <DialogTrigger asChild>
              <Button
                variant={selectedWorkspace ? "outline" : "default"}
                disabled={pending}
                aria-busy={selecting}
                onClick={openSelector}
              >
                {selecting ? (
                  <>
                    <Spinner data-icon="inline-start" aria-hidden="true" />
                    选择中…
                  </>
                ) : selectedWorkspace ? (
                  "更换工作区"
                ) : (
                  "选择项目"
                )}
              </Button>
            </DialogTrigger>
          </div>
          {broker?.operation && (
            <div className="operation" role="status">
              {broker?.operation && (
                <Button
                  variant="ghost"
                  disabled={cancelling}
                  aria-busy={cancelling}
                  onClick={() => void cancelOperation()}
                >
                  {cancelling && (
                    <Spinner data-icon="inline-start" aria-hidden="true" />
                  )}
                  取消操作
                </Button>
              )}
            </div>
          )}
        </section>
        <section className="home-section project-help" aria-labelledby="project-sync-title">
          <Collapsible
            open={helpOpen ?? broker?.projects.length === 0}
            onOpenChange={setHelpOpen}
          >
            <CollapsibleTrigger asChild>
              <Button variant="ghost" id="project-sync-title">
                如何添加或导入项目
                <ChevronDownIcon data-icon="inline-end" />
              </Button>
            </CollapsibleTrigger>
            <CollapsibleContent className="flex flex-col gap-3 pt-3">
              <p>
                “添加项目”支持普通本地目录，不要求 Git，也不要求已有 .serena。
                新建请求会显式使用所选项目的工作区；本机命令与服务请在“服务状态”中查看。
              </p>
              <Collapsible>
                <CollapsibleTrigger asChild>
                  <Button variant="ghost">
                    Serena 导入说明
                    <ChevronDownIcon data-icon="inline-end" />
                  </Button>
                </CollapsibleTrigger>
                <CollapsibleContent className="flex flex-col gap-3 pt-3">
                  <p>
                    从 Serena 项目登记表及项目配置显式追加缺失项目，不扫描磁盘。
                    不会覆盖现有名称、排序或选择，也不会切换当前工作区。
                  </p>
                  {broker?.projectSources.map((source) => (
                    <p key={source}>
                      <code className="project-path">{source}</code>
                    </p>
                  ))}
                </CollapsibleContent>
              </Collapsible>
            </CollapsibleContent>
          </Collapsible>
        </section>
        <section
          className="home-section connection"
          aria-labelledby="connection-title"
        >
          <div className="section-heading">
            <h2 id="connection-title">连接配置</h2>
            <Button variant="outline" onClick={onRemote}>连接 ChatGPT</Button>
          </div>
          {endpoint ? (
            <>
              <div className="connection-endpoint-card">
                <div className="connection-endpoint-heading">
                  <p className="field-label">本机 MCP 地址</p>
                  <span>Port: {broker?.port}</span>
                </div>
                <div className="endpoint-copy endpoint-copy-primary">
                  <code>{endpoint}</code>
                  <Button
                    variant="outline"
                    disabled={copyingAddresses.has(endpoint) || copiedAddresses.has(endpoint)}
                    aria-busy={copyingAddresses.has(endpoint)}
                    onClick={() => void copyEndpoint(endpoint)}
                  >
                    {copyingAddresses.has(endpoint) && (
                      <Spinner data-icon="inline-start" aria-hidden="true" />
                    )}
                    {copiedAddresses.has(endpoint) && (
                      <Check data-icon="inline-start" aria-hidden="true" />
                    )}
                    {copyingAddresses.has(endpoint)
                      ? "复制中…"
                      : copiedAddresses.has(endpoint)
                        ? "已复制"
                        : "复制"}
                  </Button>
                </div>
              </div>
              <div className="lan-endpoints-card">
                <div className="lan-endpoints-heading">
                  <strong>{broker?.listenAddress === "0.0.0.0" ? "提供局域网使用" : "局域网访问"}</strong>
                  <p>
                    {broker?.listenAddress === "0.0.0.0"
                      ? "已允许局域网连接。请选择与另一台电脑同网段的地址；切换网络后请重新启用连接入口。"
                      : "当前仅允许本机连接；可在设置中开启局域网访问。"}
                  </p>
                </div>
                {broker?.lanEndpoints.map((address) => {
                  const copying = copyingAddresses.has(address);
                  const copied = copiedAddresses.has(address);
                  return (
                    <div className="lan-endpoint-row" key={address}>
                      <code>{address}</code>
                      <Button
                        variant="outline"
                        disabled={copying || copied}
                        aria-busy={copying}
                        onClick={() => void copyEndpoint(address)}
                      >
                        {copying && <Spinner data-icon="inline-start" aria-hidden="true" />}
                        {copied && <Check data-icon="inline-start" aria-hidden="true" />}
                        {copying
                          ? "复制中…"
                          : copied
                            ? "已复制"
                            : "复制局域网地址"}
                      </Button>
                    </div>
                  );
                })}
                {broker?.listenAddress === "0.0.0.0" && broker.lanEndpoints.length === 0 && (
                  <p className="lan-endpoints-empty">未发现可用的 IPv4 网卡地址，请连接网络后重新启用入口。</p>
                )}
              </div>
            </>
          ) : (
            <div className="connection-stopped">
              <span>
                {broker ? "MCP 连接入口尚未启动" : "正在读取连接入口状态…"}
              </span>
              <Button variant="link" onClick={onSettings}>
                前往设置 →
              </Button>
            </div>
          )}
        </section>
        <DialogContent
          className="max-h-[85vh] overflow-y-auto sm:max-w-xl"
          onInteractOutside={(event) => {
            if (
              event.target instanceof Element &&
              event.target.closest("[data-sonner-toaster]")
            )
              event.preventDefault();
          }}
        >
          <DialogHeader>
            <DialogTitle>选择项目</DialogTitle>
            <DialogDescription>
              当前选择：{selectedWorkspace?.name ?? "尚未选择"}
              。确认后只更新 Desktop 选择，不会启动或切换 Provider。
            </DialogDescription>
          </DialogHeader>
          <Field>
            <FieldLabel htmlFor="project-select">待操作项目</FieldLabel>
            <Select
              value={selected}
              disabled={pending}
              onValueChange={setSelected}
            >
              <SelectTrigger id="project-select" className="w-full">
                <SelectValue placeholder="选择项目" />
              </SelectTrigger>
              <SelectContent>
                <SelectGroup>
                  {state.config.workspaces.map((p) => (
                    <SelectItem key={p.id} value={p.id}>
                      {p.name}
                      {p.id === selectedWorkspace?.id ? " · 已选择" : ""}
                    </SelectItem>
                  ))}
                </SelectGroup>
              </SelectContent>
            </Select>
          </Field>
          {!state.config.workspaces.length && (
            <p>暂无已登记工作区。</p>
          )}
          {project && (
            <>
              <code className="project-path">
                {displayProjectPath(project.root)}
              </code>
              <div className="selection-action">
                {current ? (
                  <>
                    <Badge variant="secondary">已选择</Badge>
                  </>
                ) : (
                  <Button
                    variant="default"
                    disabled={pending}
                    aria-busy={selecting}
                    onClick={() => void selectWorkspace()}
                  >
                    {selecting ? (
                      <>
                        <Spinner data-icon="inline-start" aria-hidden="true" />
                        选择中…
                      </>
                    ) : (
                      "选择此工作区"
                    )}
                  </Button>
                )}
              </div>
            </>
          )}
          {broker?.operation && (
            <p role="status">
              {broker?.operation && (
                <Button
                  variant="ghost"
                  disabled={cancelling}
                  aria-busy={cancelling}
                  onClick={() => void cancelOperation()}
                >
                  {cancelling && (
                    <Spinner data-icon="inline-start" aria-hidden="true" />
                  )}
                  取消操作
                </Button>
              )}
            </p>
          )}
        </DialogContent>
      </div>
    </Dialog>
  );
}
