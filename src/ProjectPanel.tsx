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

function CopyCommand({
  text,
  label,
  onCopied,
}: {
  text: string;
  label: string;
  onCopied: () => void;
}) {
  const [copying, setCopying] = useState(false);
  const copy = async () => {
    setCopying(true);
    try {
      await navigator.clipboard.writeText(text);
      onCopied();
    } catch {
      toast.error("复制失败，请手动选择命令复制。");
    } finally {
      setCopying(false);
    }
  };
  return (
    <div>
      <div className="endpoint-copy">
        <code className="project-path" style={{ whiteSpace: "pre-wrap" }}>
          {text}
        </code>
        <Button
          variant="outline"
          aria-label={label}
          disabled={copying}
          aria-busy={copying}
          onClick={() => void copy()}
        >
          {copying && <Spinner data-icon="inline-start" aria-hidden="true" />}
          复制
        </Button>
      </div>
    </div>
  );
}

export function ProjectPanel({
  state,
  controller,
  onSettings,
  onRemote,
  onSerena,
  onCopied,
}: {
  state: AppState;
  controller: ReturnType<typeof useBroker>;
  onSettings: () => void;
  onRemote: () => void;
  onSerena: () => void;
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
  const [candidateRoot, setCandidateRoot] = useState<string | null>(null);
  const [pickingDirectory, setPickingDirectory] = useState(false);
  const syncing = busy === "同步项目中";
  const [syncFeedback, setSyncFeedback] = useState<"idle" | "pending" | "success">("idle");
  const feedbackTimers = useRef(new Set<ReturnType<typeof window.setTimeout>>());
  const mounted = useRef(true);
  const [selectorOpen, setSelectorOpen] = useState(false);
  const [helpOpen, setHelpOpen] = useState<boolean | undefined>(undefined);
  const project = broker?.projects.find((p) => p.id === selected);
  const active = broker?.activeWorkspace;
  const pending = !!busy || !!broker?.operation;
  const activating = busy === "激活中" || busy === "切换中";
  const deactivating = busy === "取消激活中";
  const projectBusy = activating || deactivating;
  const graph = active ? broker?.codegraph : null;
  const graphState = !active
    ? { label: "待激活", tone: "idle", detail: "激活项目后连接" }
    : graph
      ? {
          ready: { label: "就绪", tone: "good", detail: "" },
          starting: {
            label: "启动中",
            tone: "waiting",
            detail: "正在连接当前项目",
          },
          not_initialized: {
            label: "未初始化",
            tone: "idle",
            detail: "当前项目尚无 CodeGraph 索引",
          },
          unavailable: {
            label: "不可用",
            tone: "bad",
            detail: "请检查 CodeGraph 安装",
          },
          start_failed: {
            label: "启动失败",
            tone: "bad",
            detail: "请查看 MCP 日志",
          },
          runtime_lost: {
            label: "连接中断",
            tone: "bad",
            detail: "下次查询可尝试有限恢复",
          },
        }[graph.status]
      : { label: "读取中", tone: "waiting", detail: "正在读取能力状态" };
  const current = !!project && project.id === active?.id;
  const unavailable = state.activeInstallation?.state !== "standard";
  const serenaLabel =
    state.serverStatus === "running"
      ? "运行中"
      : state.serverStatus === "starting"
        ? "启动中"
        : state.serverStatus === "error"
          ? "运行异常"
          : unavailable
            ? "不可用"
            : "未运行";
  const serenaTone =
    state.serverStatus === "running"
      ? "good"
      : state.serverStatus === "starting"
        ? "waiting"
        : state.serverStatus === "error" || unavailable
          ? "bad"
          : "idle";
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
    setSelected(active?.id ?? broker?.projects[0]?.id ?? "");
  };
  const executable = state.installation?.path;
  const registryHome = broker?.projectSources[0]?.replace(/[\\/][^\\/]+$/, "");
  const quote = (value: string) => "'" + value.replaceAll("'", "''") + "'";
  const command =
    executable && registryHome
      ? `$env:SERENA_HOME = ${quote(registryHome)}\n& ${quote(executable)} project create --index`
      : null;
  const sync = async () => {
    if (syncFeedback !== "idle" || syncing) return;
    const startedAt = Date.now();
    let syncedCount: number | null = null;
    setSyncFeedback("pending");
    await perform("同步项目中", async () => {
      try {
        syncedCount = await api.syncProjects();
      } catch (reason) {
        await waitForFeedback(
          Math.max(0, SYNC_MIN_PENDING_MS - (Date.now() - startedAt)),
        );
        throw reason;
      }
      await waitForFeedback(
        Math.max(0, SYNC_MIN_PENDING_MS - (Date.now() - startedAt)),
      );
      toast.success(`已同步 ${syncedCount} 个项目`);
    });
    if (!mounted.current) return;
    if (syncedCount === null) {
      setSyncFeedback("idle");
      return;
    }
    setSyncFeedback("success");
    resetAfterSuccess(() => setSyncFeedback("idle"));
  };
  const pickDirectory = async () => {
    setPickingDirectory(true);
    try {
      const root = await api.workspacePickDirectory();
      if (root) setCandidateRoot(root);
    } catch (reason) {
      toast.error(String(reason));
    } finally {
      setPickingDirectory(false);
    }
  };
  const syncVisualState = syncFeedback === "success"
    ? "success"
    : syncing || syncFeedback === "pending"
      ? "pending"
      : "idle";
  return (
    <Dialog open={selectorOpen} onOpenChange={setSelectorOpen}>
      <div className="project-panel">
        <div className="page-heading">
          <div>
            <h1>开始使用</h1>
            <p>选择一个项目，连接本地代码能力。</p>
          </div>
          <Button
            variant="outline"
            disabled={pickingDirectory}
            aria-busy={pickingDirectory}
            onClick={() => void pickDirectory()}
          >
            {pickingDirectory && <Spinner data-icon="inline-start" aria-hidden="true" />}
            {pickingDirectory ? "选择目录中…" : "添加项目"}
          </Button>
          <Button
            className="sync-project-button"
            disabled={pending || !broker || syncVisualState !== "idle"}
            aria-busy={syncVisualState === "pending"}
            onClick={() => void sync()}
          >
            {syncVisualState === "pending" ? (
              <Spinner data-icon="inline-start" aria-hidden="true" />
            ) : syncVisualState === "success" ? (
              <Check data-icon="inline-start" aria-hidden="true" />
            ) : (
              <RefreshCw data-icon="inline-start" aria-hidden="true" />
            )}
            {syncVisualState === "pending"
              ? "同步中…"
              : syncVisualState === "success"
                ? "已同步"
                : "同步项目"}
          </Button>
        </div>
        {candidateRoot && (
          <p className="helper" role="status">
            待登记目录：<code className="project-path">{displayProjectPath(candidateRoot)}</code>
          </p>
        )}
        <section className="home-section" aria-labelledby="workspace-title">
          <h2 id="workspace-title">当前工作区</h2>
          <div className="workspace-summary">
            <div className="workspace-identity">
              {active ? (
                <div className="workspace-active-identity">
                  <span className="workspace-folder-icon" aria-hidden="true">
                    <Folder />
                  </span>
                  <div className="workspace-active-details">
                    <div className="workspace-name">
                      <strong>{active.name}</strong>
                      <Badge className="workspace-active-badge" variant="secondary">
                        已激活
                      </Badge>
                    </div>
                    <code className="project-path">
                      {displayProjectPath(active.root)}
                    </code>
                  </div>
                </div>
              ) : (
                <>
                  <div className="workspace-name">
                    <strong>{broker ? "尚未激活项目" : "正在读取工作区…"}</strong>
                  </div>
                  <p>选择一个项目开始使用 Serena。</p>
                </>
              )}
            </div>
            <DialogTrigger asChild>
              <Button
                variant={active ? "outline" : "default"}
                disabled={!broker || pending}
                aria-busy={projectBusy}
                onClick={openSelector}
              >
                {projectBusy ? (
                  <>
                    <Spinner data-icon="inline-start" aria-hidden="true" />
                    {busy}…
                  </>
                ) : active ? (
                  "切换项目"
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
                如何初始化并同步项目
                <ChevronDownIcon data-icon="inline-end" />
              </Button>
            </CollapsibleTrigger>
            <CollapsibleContent className="flex flex-col gap-3 pt-3">
              <p>
                在已有 Git 仓库的根目录打开
                PowerShell，执行以下命令，为当前仓库创建 Serena
                项目配置并建立索引。完成后点击“同步项目”。
              </p>
              <CopyCommand
                text="serena project create --index"
                label="复制初始化命令"
                onCopied={onCopied}
              />
              {command && (
                <Collapsible>
                  <CollapsibleTrigger asChild>
                    <Button variant="ghost">
                      找不到 serena 命令时
                      <ChevronDownIcon data-icon="inline-end" />
                    </Button>
                  </CollapsibleTrigger>
                  <CollapsibleContent className="flex flex-col gap-3 pt-3">
                    <p>
                      仍在仓库根目录执行以下命令，使用 Desktop 检测到的 Serena
                      和同步配置目录。
                    </p>
                    <CopyCommand
                      text={command}
                      label="复制完整初始化命令"
                      onCopied={onCopied}
                    />
                  </CollapsibleContent>
                </Collapsible>
              )}
              <p className="helper">
                如果仓库已有 .serena/project.yml，请改用以下命令：
              </p>
              <CopyCommand
                text="serena project index"
                label="复制索引命令"
                onCopied={onCopied}
              />
              <p className="helper">
                大项目可在终端查看进度；等待命令执行完成后再同步。
              </p>
              <Collapsible>
                <CollapsibleTrigger asChild>
                  <Button variant="ghost">
                    同步来源
                    <ChevronDownIcon data-icon="inline-end" />
                  </Button>
                </CollapsibleTrigger>
                <CollapsibleContent className="flex flex-col gap-3 pt-3">
                  <p>
                    读取 Serena
                    的项目登记表及项目配置，不扫描磁盘。启动时自动同步，也可手动刷新；同步不切换当前工作区。
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
            <h2 id="services-title">服务状态</h2>
            <Button variant="link" onClick={onSerena}>
              查看状态 →
            </Button>
          </div>
          <div className="service-list">
            <div className="service-row">
              <h3>Serena</h3>
              <Badge
                variant={
                  serenaTone === "bad"
                    ? "destructive"
                    : serenaTone === "good"
                      ? "success"
                      : serenaTone === "waiting"
                        ? "warning"
                        : "secondary"
                }
              >
                {serenaLabel}
              </Badge>
              <span className="service-detail">
                {state.serverStatus === "running" && !active
                  ? "未绑定项目"
                  : state.activeInstallation?.version || "—"}
              </span>
            </div>
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
              <h3>CodeGraph</h3>
              <Badge
                variant={
                  graphState.tone === "bad"
                    ? "destructive"
                    : graphState.tone === "good"
                      ? "success"
                      : graphState.tone === "waiting"
                        ? "warning"
                        : "secondary"
                }
              >
                {graphState.label}
              </Badge>
              <span className="service-detail">
                {state.codegraphVersion
                  ? `CodeGraph ${state.codegraphVersion}`
                  : "版本未检测到"}
                {graphState.detail && (
                  <>
                    <br />
                    {graphState.detail}
                  </>
                )}
              </span>
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
              当前工作区：{active?.name ?? "尚未激活"}
              。选择列表项不会切换工作区。
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
                  {broker?.projects.map((p) => (
                    <SelectItem key={p.id} value={p.id}>
                      {p.name}
                      {p.id === active?.id ? " · 已激活" : " · 已同步"}
                    </SelectItem>
                  ))}
                </SelectGroup>
              </SelectContent>
            </Select>
          </Field>
          {!broker?.projects.length && (
            <p>暂无可用项目，请先在终端初始化，然后点击首页的“同步项目”。</p>
          )}
          {project && (
            <>
              <code className="project-path">
                {displayProjectPath(project.root)}
              </code>
              <div className="selection-action">
                {(current && !activating) || deactivating ? (
                  <>
                    <Badge variant="secondary">已激活</Badge>
                    <Button
                      variant="outline"
                      disabled={pending}
                      aria-busy={deactivating}
                      onClick={() =>
                        perform(
                          "取消激活中",
                          api.deactivateProject,
                          "项目已取消激活",
                        )
                      }
                    >
                      {deactivating ? (
                        <>
                          <Spinner
                            data-icon="inline-start"
                            aria-hidden="true"
                          />
                          取消激活中…
                        </>
                      ) : (
                        "取消激活"
                      )}
                    </Button>
                  </>
                ) : (
                  <Button
                    variant="default"
                    disabled={pending}
                    aria-busy={activating}
                    onClick={() =>
                      perform(
                        active ? "切换中" : "激活中",
                        async () => {
                          await api.activateProject(project.id);
                          setSelectorOpen(false);
                        },
                        active ? "项目已切换" : "项目已激活",
                      )
                    }
                  >
                    {activating ? (
                      <>
                        <Spinner data-icon="inline-start" aria-hidden="true" />
                        {busy}…
                      </>
                    ) : active ? (
                      "切换到此项目"
                    ) : (
                      "激活"
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
