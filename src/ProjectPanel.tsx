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
import { listen } from "@tauri-apps/api/event";
import { api } from "./api";
import type { AppState, WorkspaceCapabilityActivity, WorkspaceCapabilityHealth } from "./types";
import type { useBroker } from "./useBroker";

function displayProjectPath(path: string): string {
  if (path.startsWith("\\\\?\\UNC\\")) return "\\\\" + path.slice(8);
  return path.replace(/^\\\\\?\\(?=[A-Za-z]:\\)/, "");
}

const SYNC_MIN_PENDING_MS = 600;
const COPY_MIN_PENDING_MS = 500;
const SUCCESS_FEEDBACK_MS = 1500;

/** 将安全枚举投影为紧凑本地标签，不依赖 Provider identity。 */
function capabilityLabel(value: string): string {
  return ({
    installed: "已安装",
    not_installed: "未安装",
    check_failed: "检测失败",
    not_prepared: "未准备",
    preparing: "准备中",
    ready: "就绪",
    degraded: "需更新",
    error: "异常",
    unknown: "未知",
    unavailable: "不可用",
    stopped: "已停止",
    starting: "启动中",
    stopping: "停止中",
    absent: "未建立",
    pending: "等待中",
    running: "执行中",
    stale: "已过期",
  } as Record<string, string>)[value] ?? value;
}

/** 为任意 DTO 状态选择通用 Badge 色调，不检查 Provider ID。 */
function capabilityBadgeVariant(value: string): "success" | "warning" | "destructive" | "secondary" {
  if (["ready", "installed"].includes(value)) return "success";
  if (["error", "unavailable", "not_installed", "check_failed"].includes(value)) return "destructive";
  if (["starting", "stopping", "preparing", "running", "pending", "stale", "degraded"].includes(value)) return "warning";
  return "secondary";
}

type CapabilityActionFeedback = {
  workspaceId: string;
  providerId: string;
  actionId: string;
  phase: "pending" | "success" | "error" | "cancelled";
  operationId: string | null;
};

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
  const [editingWorkspace, setEditingWorkspace] = useState<{ id: string; name: string } | null>(null);
  const [renamingWorkspaceId, setRenamingWorkspaceId] = useState<string | null>(null);
  const [removeConfirmation, setRemoveConfirmation] = useState<{ id: string; name: string; root: string } | null>(null);
  const [removingWorkspaceId, setRemovingWorkspaceId] = useState<string | null>(null);
  const [reorderingWorkspaceId, setReorderingWorkspaceId] = useState<string | null>(null);
  const [capabilityHealth, setCapabilityHealth] = useState<WorkspaceCapabilityHealth | null>(null);
  const [capabilityHealthError, setCapabilityHealthError] = useState(false);
  const [capabilityActivityReady, setCapabilityActivityReady] = useState(false);
  const [capabilityActionFeedback, setCapabilityActionFeedback] = useState<CapabilityActionFeedback | null>(null);
  const [capabilityActionInFlightWorkspaceId, setCapabilityActionInFlightWorkspaceId] = useState<string | null>(null);
  const feedbackTimers = useRef(new Set<ReturnType<typeof window.setTimeout>>());
  const mounted = useRef(true);
  const selectedWorkspaceId = state.desktopSelectedWorkspace?.id ?? null;
  const selectedWorkspaceIdRef = useRef<string | null>(selectedWorkspaceId);
  const cancelledCapabilityOperationId = useRef<string | null>(null);
  const [selectorOpen, setSelectorOpen] = useState(false);
  const [selecting, setSelecting] = useState(false);
  const [helpOpen, setHelpOpen] = useState<boolean | undefined>(undefined);
  const project = state.config.workspaces.find((workspace) => workspace.id === selected);
  const selectedWorkspace = state.desktopSelectedWorkspace;
  selectedWorkspaceIdRef.current = selectedWorkspaceId;
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
  useEffect(() => {
    let disposed = false;
    let unlistenActivity: (() => void) | undefined;
    setCapabilityHealth(null);
    setCapabilityHealthError(false);
    setCapabilityActivityReady(false);
    setCapabilityActionFeedback(null);
    setCapabilityActionInFlightWorkspaceId(null);
    cancelledCapabilityOperationId.current = null;
    if (!selectedWorkspaceId) return;

    const observe = async () => {
      try {
        const health = await api.workspaceCapabilityObserve(selectedWorkspaceId);
        if (!disposed && selectedWorkspaceIdRef.current === selectedWorkspaceId) {
          setCapabilityHealth(health);
        }
      } catch {
        if (!disposed && selectedWorkspaceIdRef.current === selectedWorkspaceId) {
          setCapabilityHealthError(true);
        }
      }
    };
    const subscribe = async () => {
      try {
        unlistenActivity = await listen<WorkspaceCapabilityActivity>(
          "workspace-capability-activity",
          ({ payload }) => {
            if (payload.workspaceId !== selectedWorkspaceId) return;
            setCapabilityActionFeedback((currentFeedback) => {
              if (
                !currentFeedback
                || currentFeedback.workspaceId !== payload.workspaceId
                || currentFeedback.providerId !== payload.providerId
                || currentFeedback.actionId !== payload.actionId
              ) return currentFeedback;
              return {
                ...currentFeedback,
                operationId: payload.operationId,
                phase: payload.state === "failed" ? "error" : currentFeedback.phase,
              };
            });
          },
        );
        if (disposed) unlistenActivity();
        else setCapabilityActivityReady(true);
      } catch {
        // Activity 订阅失败不影响显式 Health 查询与动作调用。
      }
    };
    void observe();
    void subscribe();
    return () => {
      disposed = true;
      unlistenActivity?.();
    };
  }, [selectedWorkspaceId]);
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
  const refreshCapabilityHealth = async (workspaceId: string) => {
    try {
      const health = await api.workspaceCapabilityObserve(workspaceId);
      if (mounted.current && selectedWorkspaceIdRef.current === workspaceId) {
        setCapabilityHealth(health);
        setCapabilityHealthError(false);
      }
    } catch {
      if (mounted.current && selectedWorkspaceIdRef.current === workspaceId) {
        setCapabilityHealthError(true);
      }
    }
  };
  const prepareCapabilityAction = async (providerId: string, actionId: string) => {
    if (!selectedWorkspaceId || capabilityActionInFlightWorkspaceId) return;
    const workspaceId = selectedWorkspaceId;
    cancelledCapabilityOperationId.current = null;
    setCapabilityActionInFlightWorkspaceId(workspaceId);
    setCapabilityActionFeedback({
      workspaceId,
      providerId,
      actionId,
      phase: "pending",
      operationId: null,
    });
    try {
      const result = await api.workspaceCapabilityPrepare(workspaceId, providerId, actionId);
      if (mounted.current && selectedWorkspaceIdRef.current === workspaceId) {
        setCapabilityActionFeedback({
          workspaceId,
          providerId,
          actionId,
          phase: "success",
          operationId: result.operationId,
        });
      }
    } catch {
      if (mounted.current && selectedWorkspaceIdRef.current === workspaceId) {
        setCapabilityActionFeedback((currentFeedback) => currentFeedback && {
          ...currentFeedback,
          phase: cancelledCapabilityOperationId.current !== null
            && cancelledCapabilityOperationId.current === currentFeedback.operationId
            ? "cancelled"
            : "error",
        });
      }
    } finally {
      setCapabilityActionInFlightWorkspaceId((currentWorkspaceId) =>
        currentWorkspaceId === workspaceId ? null : currentWorkspaceId,
      );
      await refreshCapabilityHealth(workspaceId);
    }
  };
  const cancelCapabilityAction = async () => {
    const operationId = capabilityActionFeedback?.operationId;
    if (!operationId || capabilityActionFeedback.phase !== "pending") return;
    try {
      await api.workspaceCapabilityCancel(operationId);
      cancelledCapabilityOperationId.current = operationId;
      setCapabilityActionFeedback((currentFeedback) => currentFeedback && {
        ...currentFeedback,
        phase: "cancelled",
      });
    } catch {
      setCapabilityActionFeedback((currentFeedback) => currentFeedback && {
        ...currentFeedback,
        phase: "error",
      });
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
  const renameWorkspace = async () => {
    if (!editingWorkspace || renamingWorkspaceId || !editingWorkspace.name.trim()) return;
    let renamed = false;
    setRenamingWorkspaceId(editingWorkspace.id);
    await perform("重命名项目", async () => {
      await api.workspaceRename(editingWorkspace.id, editingWorkspace.name.trim());
      renamed = true;
    });
    if (renamed && mounted.current) setEditingWorkspace(null);
    if (mounted.current) setRenamingWorkspaceId(null);
  };
  const removeWorkspace = async () => {
    if (!removeConfirmation || removingWorkspaceId) return;
    const workspace = removeConfirmation;
    let removed = false;
    setRemovingWorkspaceId(workspace.id);
    await perform("移除项目", async () => {
      try {
        await api.workspaceRemove(workspace.id);
        removed = true;
      } catch (reason) {
        if (String(reason).includes("WORKSPACE_IN_USE")) {
          throw new Error("项目正在被 Agent 任务使用，当前不能移除");
        }
        throw reason;
      }
    });
    if (removed && mounted.current) setRemoveConfirmation(null);
    if (mounted.current) setRemovingWorkspaceId(null);
  };
  const reorderWorkspace = async (index: number, direction: -1 | 1) => {
    if (reorderingWorkspaceId) return;
    const targetIndex = index + direction;
    const workspaces = state.config.workspaces;
    if (targetIndex < 0 || targetIndex >= workspaces.length) return;
    const ids = workspaces.map((workspace) => workspace.id);
    [ids[index], ids[targetIndex]] = [ids[targetIndex], ids[index]];
    setReorderingWorkspaceId(workspaces[index].id);
    await perform("调整项目顺序", () => api.workspaceReorder(ids));
    if (mounted.current) setReorderingWorkspaceId(null);
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
          <Button
            variant="outline"
            disabled={pickingDirectory || registering}
            aria-busy={pickingDirectory}
            onClick={() => void pickDirectory()}
          >
            {pickingDirectory && <Spinner data-icon="inline-start" aria-hidden="true" />}
            {pickingDirectory ? "选择并检查目录中…" : "添加项目"}
          </Button>
          <Button
            className="sync-project-button"
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
        <section className="home-section" aria-labelledby="workspace-title">
          <h2 id="workspace-title">当前工作区</h2>
          <div className="workspace-summary">
            <div className="workspace-identity">
              {selectedWorkspace ? (
                <div className="workspace-active-identity">
                  <span className="workspace-folder-icon" aria-hidden="true">
                    <Folder />
                  </span>
                  <div className="workspace-active-details">
                    <div className="workspace-name">
                      <strong>{selectedWorkspace.name}</strong>
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
                    <strong>尚未选择工作区</strong>
                  </div>
                  <p>选择一个工作区后，新任务会默认使用它。</p>
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
        {selectedWorkspace && (
          <section className="home-section" aria-labelledby="capability-health-title">
            <div className="section-heading">
              <h2 id="capability-health-title">能力状态</h2>
              <Button
                variant="link"
                disabled={!selectedWorkspaceId}
                onClick={() => selectedWorkspaceId && void refreshCapabilityHealth(selectedWorkspaceId)}
              >
                刷新 →
              </Button>
            </div>
            {capabilityHealthError ? (
              <p className="helper" role="status">暂时无法读取能力状态，请稍后刷新。</p>
            ) : !capabilityHealth ? (
              <p className="helper" role="status">正在读取能力状态…</p>
            ) : (
              <div className="service-list" data-capability-workspace={capabilityHealth.workspaceId}>
                {Object.entries(capabilityHealth.providers).map(([providerId, provider]) => {
                  const feedback = capabilityActionFeedback?.workspaceId === capabilityHealth.workspaceId
                    && capabilityActionFeedback.providerId === providerId
                    ? capabilityActionFeedback
                    : null;
                  return (
                    <div className="service-row" key={providerId} data-capability-provider={providerId}>
                      <div>
                        <h3>{provider.displayName}</h3>
                        <div className="flex flex-wrap gap-2 pt-1">
                          <Badge variant={capabilityBadgeVariant(provider.installation)}>
                            安装：{capabilityLabel(provider.installation)}
                          </Badge>
                          <Badge variant={capabilityBadgeVariant(provider.status)}>
                            可用性：{capabilityLabel(provider.status)}
                          </Badge>
                          <Badge variant={capabilityBadgeVariant(provider.readiness)}>
                            准备：{capabilityLabel(provider.readiness)}
                          </Badge>
                          <Badge variant={capabilityBadgeVariant(provider.runtimeState)}>
                            运行：{capabilityLabel(provider.runtimeState)}
                          </Badge>
                        </div>
                        {provider.stages.map((stage) => (
                          <p className="service-detail" key={stage.id}>
                            {stage.displayName}：{capabilityLabel(stage.state)}
                            （{capabilityLabel(stage.requirement)}）
                          </p>
                        ))}
                        {feedback && (
                          <p className="service-detail" role="status">
                            {feedback.phase === "pending"
                              ? "正在执行…"
                              : feedback.phase === "success"
                                ? "操作已完成"
                                : feedback.phase === "cancelled"
                                  ? "已请求取消"
                                  : "操作未完成"}
                          </p>
                        )}
                      </div>
                      <div className="flex flex-wrap gap-2">
                        {provider.actions.map((action) => (
                          <Button
                            key={action.id}
                            variant="outline"
                            disabled={!capabilityActivityReady || capabilityActionInFlightWorkspaceId !== null}
                            aria-busy={feedback?.actionId === action.id && feedback.phase === "pending"}
                            onClick={() => void prepareCapabilityAction(providerId, action.id)}
                          >
                            {feedback?.actionId === action.id && feedback.phase === "pending" && (
                              <Spinner data-icon="inline-start" aria-hidden="true" />
                            )}
                            {action.displayName}
                          </Button>
                        ))}
                        {feedback?.phase === "pending" && feedback.operationId && (
                          <Button variant="ghost" onClick={() => void cancelCapabilityAction()}>
                            取消
                          </Button>
                        )}
                      </div>
                    </div>
                  );
                })}
              </div>
            )}
          </section>
        )}
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
                选择项目后，可在“能力状态”中按当前 Provider 提供的动作准备或更新项目。
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
        <section className="home-section" aria-labelledby="services-title">
          <div className="section-heading">
            <h2 id="services-title">本机服务</h2>
          </div>
          <div className="service-list">
            <div className="service-row">
              <h3>Git</h3>
              <Badge variant={state.git.available ? "success" : "destructive"}>
                {state.git.available
                  ? "可用"
                  : state.git.status === "error"
                    ? "检测失败"
                    : "不可用"}
              </Badge>
              <span className="service-detail">{state.git.version || "—"}</span>
            </div>
            <div className="service-row">
              <h3>MCP 连接入口</h3>
              <Badge
                variant={
                  !broker ? "warning" : broker.running ? "success" : "secondary"
                }
              >
                {!broker ? "读取中" : broker.running ? "监听中" : "已停止"}
              </Badge>
              <span className="service-detail">
                {broker?.running ? `:${broker.port}` : "—"}
              </span>
            </div>
          </div>
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
          {!!state.config.workspaces.length && (
            <section className="workspace-management" aria-labelledby="workspace-management-title">
              <h3 id="workspace-management-title">项目管理</h3>
              {state.config.workspaces.map((workspace, index) => {
                const editor = editingWorkspace?.id === workspace.id
                  ? editingWorkspace
                  : null;
                const removal = removeConfirmation?.id === workspace.id
                  ? removeConfirmation
                  : null;
                const renaming = renamingWorkspaceId === workspace.id;
                const removing = removingWorkspaceId === workspace.id;
                const reordering = reorderingWorkspaceId === workspace.id;
                const orderingLocked = !!reorderingWorkspaceId;
                return (
                  <div className="workspace-management-row" data-workspace-id={workspace.id} key={workspace.id}>
                    {editor ? (
                      <div className="workspace-rename-editor">
                        <Input
                          id={`workspace-rename-${workspace.id}`}
                          value={editor.name}
                          disabled={renaming}
                          onChange={(event) => setEditingWorkspace({
                            id: workspace.id,
                            name: event.target.value,
                          })}
                        />
                        <div className="workspace-management-actions">
                          <Button
                            disabled={renaming || !editor.name.trim()}
                            aria-busy={renaming}
                            onClick={() => void renameWorkspace()}
                          >
                            {renaming && <Spinner data-icon="inline-start" aria-hidden="true" />}
                            {renaming ? "保存中…" : "保存"}
                          </Button>
                          <Button
                            variant="outline"
                            disabled={renaming}
                            onClick={() => setEditingWorkspace(null)}
                          >
                            取消
                          </Button>
                        </div>
                      </div>
                    ) : (
                      <>
                        <div className="workspace-management-identity">
                          <strong>{workspace.name}</strong>
                          <code className="project-path">{displayProjectPath(workspace.root)}</code>
                        </div>
                        <div className="workspace-management-actions">
                          <Button
                            variant="outline"
                            disabled={index === 0 || orderingLocked}
                            aria-busy={reordering}
                            onClick={() => void reorderWorkspace(index, -1)}
                          >
                            {reordering && <Spinner data-icon="inline-start" aria-hidden="true" />}
                            {reordering ? "调整中…" : "上移"}
                          </Button>
                          <Button
                            variant="outline"
                            disabled={index === state.config.workspaces.length - 1 || orderingLocked}
                            aria-busy={reordering}
                            onClick={() => void reorderWorkspace(index, 1)}
                          >
                            {reordering && <Spinner data-icon="inline-start" aria-hidden="true" />}
                            {reordering ? "调整中…" : "下移"}
                          </Button>
                          <Button
                            variant="outline"
                            disabled={removing}
                            onClick={() => setEditingWorkspace({
                              id: workspace.id,
                              name: workspace.name,
                            })}
                          >
                            重命名
                          </Button>
                          <Button
                            variant="outline"
                            disabled={removing}
                            onClick={() => setRemoveConfirmation({
                              id: workspace.id,
                              name: workspace.name,
                              root: workspace.root,
                            })}
                          >
                            移除
                          </Button>
                        </div>
                      </>
                    )}
                    {removal && (
                      <div className="workspace-remove-confirmation" role="alertdialog" aria-label={`确认移除 ${workspace.name}`}>
                        <p>
                          确定移除“{removal.name}”吗？仅从 Serena Desktop 项目列表移除，
                          不会删除本地目录、源码、Git 仓库、.serena 或 .codegraph 内容。
                        </p>
                        <code className="project-path">{displayProjectPath(removal.root)}</code>
                        <div className="workspace-management-actions">
                          <Button
                            variant="destructive"
                            disabled={removing}
                            aria-busy={removing}
                            onClick={() => void removeWorkspace()}
                          >
                            {removing && <Spinner data-icon="inline-start" aria-hidden="true" />}
                            {removing ? "移除中…" : "确认移除"}
                          </Button>
                          <Button
                            variant="outline"
                            disabled={removing}
                            onClick={() => setRemoveConfirmation(null)}
                          >
                            取消
                          </Button>
                        </div>
                      </div>
                    )}
                  </div>
                );
              })}
            </section>
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
