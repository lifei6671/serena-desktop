import { useEffect, useState, type ComponentProps } from "react";
import { Check, Copy, GitBranch, Network, RefreshCw, Server, SquareTerminal } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import { TooltipHint } from "@/components/TooltipHint";
import { api } from "../../api";
import type { AppState, BrokerState, ServerStatus } from "../../types";
import type { AppController } from "../../app/useAppController";

type ComponentStatus = { label: string; tone: "healthy" | "pending" | "warning" | "error" | "inactive" };
const statusCopy: Record<ServerStatus, ComponentStatus & { detail: string }> = {
  stopped: { label: "已停止", detail: "Serena 内部端口未监听", tone: "inactive" },
  starting: { label: "启动中", detail: "正在等待本机端口响应", tone: "pending" },
  running: { label: "运行中", detail: "Serena 本机链路可用", tone: "healthy" },
  error: { label: "异常", detail: "Serena 未能保持运行", tone: "error" },
};
const codegraphCopy: Record<NonNullable<BrokerState["codegraph"]>["status"], ComponentStatus> = {
  ready: { label: "已就绪", tone: "healthy" },
  starting: { label: "启动中", tone: "pending" },
  not_initialized: { label: "未初始化", tone: "warning" },
  unavailable: { label: "不可用", tone: "error" },
  start_failed: { label: "启动失败", tone: "error" },
  runtime_lost: { label: "连接丢失", tone: "error" },
};

function StatusActionButton({ children, ...props }: Omit<ComponentProps<typeof Button>, "children"> & { children: string }) {
  return <Button {...props} className="status-action-button" aria-label={children}>
    <span className="status-action-label">{children}</span>
    {props["aria-busy"] && <span className="status-action-busy-indicator" aria-hidden="true"><Spinner /></span>}
  </Button>;
}

function EnvironmentValue({ name, value, copyable }: { name: string; value: string; copyable: boolean }) {
  const [copying, setCopying] = useState(false);
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    if (!copied) return;
    const timer = window.setTimeout(() => setCopied(false), 1400);
    return () => window.clearTimeout(timer);
  }, [copied]);
  async function copy() {
    setCopying(true);
    try {
      await navigator.clipboard.writeText(value);
      setCopied(true);
    } catch (error) {
      toast.error(`复制失败：${String(error)}`);
    } finally {
      setCopying(false);
    }
  }
  const label = copied ? `已复制 ${name}` : copying ? `正在复制 ${name}` : `复制 ${name}`;
  return <div className="status-environment-value">
    <TooltipHint content={value}><code className="status-truncate" tabIndex={0}>{value}</code></TooltipHint>
    {copyable && <TooltipHint content={label}>
      <Button variant="ghost" size="icon" className="status-copy-button" aria-label={label} aria-busy={copying} data-copied={copied} disabled={copying || copied} onClick={() => void copy()}>
        {copied ? <Check aria-hidden="true" /> : <Copy aria-hidden="true" />}
      </Button>
    </TooltipHint>}
  </div>;
}

export default function StatusPage({ state, busy, codexLoading, codexVersion, codexError, brokerController, run, detectCodex, runSideEffect }: { state: AppState } & Pick<AppController, "busy" | "codexLoading" | "codexVersion" | "codexError" | "brokerController" | "run" | "detectCodex" | "runSideEffect">) {
  const status = statusCopy[state.serverStatus];
  const isRunning = state.serverStatus === "running" || state.serverStatus === "starting" || state.managedProcessPresent;
  const installation = state.activeInstallation;
  const isInstalled = installation?.state === "standard";
  const canStart = state.installation?.state === "standard" && state.git.available;
  const installationLabel = installation
    ? ({ missing: "未安装", standard: "官方 Serena", invalid: "安装不兼容或已损坏" } as const)[installation.state]
    : "检测中";
  const runtimeStatus = isRunning || isInstalled ? status : {
    label: installationLabel,
    tone: installation?.state === "invalid" ? "error" : installation ? "inactive" : "pending",
  };
  const runtimeDetail = isRunning || isInstalled ? status.detail
    : installation?.error || (installation ? installationLabel : "正在检测官方 Serena。");
  const broker = brokerController.broker;
  const brokerEndpoint = broker?.running ? `http://127.0.0.1:${broker.port}/mcp` : null;
  const brokerStatus: ComponentStatus = broker
    ? { label: broker.running ? "运行中" : "已停止", tone: broker.running ? "healthy" : "inactive" }
    : { label: "状态不可用", tone: "inactive" };
  const workspace = broker?.activeWorkspace;
  const codexStatus: ComponentStatus = codexLoading ? { label: "检测中", tone: "pending" }
    : codexError ? { label: "不可用", tone: "error" }
    : codexVersion ? { label: "CLI 可用", tone: "healthy" } : { label: "未检测到", tone: "inactive" };
  const codegraphStatus: ComponentStatus = broker?.codegraph ? codegraphCopy[broker.codegraph.status]
    : { label: state.codegraphVersion ? (workspace ? "已安装" : "待激活") : "未检测到", tone: "inactive" };
  const codegraphDetail = broker?.codegraph
    ? `${workspace?.name ?? "工作区"} · ${codegraphStatus.label}`
    : workspace ? "工作区运行状态不可用" : "未激活工作区";
  const components = [
    { name: "Serena Runtime", icon: Server, ...runtimeStatus, value: installation?.version || "版本未检测到", detail: `内部端口 ${state.activePort} · ${installation?.context ?? runtimeDetail}` },
    { name: "MCP Broker", icon: Network, ...brokerStatus, value: broker ? `HTTP · ${broker.port}` : "端口不可用", detail: brokerEndpoint ?? brokerStatus.label },
    { name: "Codex CLI", icon: SquareTerminal, ...codexStatus, value: codexLoading ? "正在检测…" : codexVersion || "版本未检测到", detail: codexLoading ? "正在检测本地 CLI" : codexError || "本地 CLI 可用性" },
    { name: "CodeGraph", icon: GitBranch, ...codegraphStatus, value: state.codegraphVersion ?? "版本未检测到", detail: codegraphDetail },
  ];
  const environment = [
    { name: "Serena", value: installation?.version || installationLabel, detail: installation?.path || installation?.error || "尚未发现可执行文件", copyable: !!installation?.path },
    { name: "MCP Broker", value: broker ? `HTTP · ${broker.port}` : "端口不可用", detail: brokerEndpoint ?? brokerStatus.label, copyable: !!brokerEndpoint },
    { name: "Codex", value: codexLoading ? "正在检测…" : codexVersion || codexStatus.label, detail: codexLoading ? "正在检测本地 CLI" : codexError || "本地 CLI 可用性", error: !codexLoading && !!codexError },
    { name: "CodeGraph", value: state.codegraphVersion ?? "版本未检测到", detail: codegraphDetail, error: codegraphStatus.tone === "error" },
    { name: "Git", value: state.git.version || (state.git.status === "error" ? "检测失败" : "未检测到"), detail: state.git.path || state.git.error || (state.git.available ? "可用" : "必需依赖未就绪"), error: !state.git.available, copyable: !!state.git.path },
    { name: "Dashboard", value: state.dashboardEnabled ? "已启用" : "已关闭", detail: state.dashboardEnabled ? state.dashboardUrl : "已关闭", copyable: state.dashboardEnabled && !!state.dashboardUrl },
  ];
  const redetect = () => run("detect", async () => {
    const [next] = await Promise.all([api.detect(), detectCodex(true)]);
    return next;
  }, "检测已完成，请查看各项状态。");

  return (
    <section className="serena-page status-page">
      <div className="page-heading">
        <div><h1>状态</h1><p>确认本机服务链路、运行组件与开发环境状态。</p></div>
        <Button variant="outline" disabled={busy !== null || codexLoading} onClick={redetect} aria-busy={busy === "detect"}>
          {busy === "detect" ? <Spinner aria-hidden="true" /> : <RefreshCw aria-hidden="true" />}
          重新检测
        </Button>
      </div>

      <section className="status-overview" aria-label="当前状态">
        <div className="status-overview-state">
          <span className="status-section-label">当前状态</span>
          <strong className="status-component-state" data-tone={runtimeStatus.tone}><i aria-hidden="true" />{runtimeStatus.label}</strong>
          <TooltipHint content={runtimeDetail}><span className="status-truncate" tabIndex={0}>{runtimeDetail}</span></TooltipHint>
        </div>
        <dl className="status-overview-facts">
          {[
            { label: "Serena Endpoint", value: state.endpoint },
            { label: "MCP Broker", value: brokerEndpoint ?? brokerStatus.label },
            { label: "当前工作区", value: workspace?.name ?? "未激活" },
          ].map(item => <div key={item.label}>
            <dt>{item.label}</dt>
            <dd><TooltipHint content={item.value}><span className={item.label === "当前工作区" ? "status-truncate" : "status-truncate mono"} tabIndex={0}>{item.value}</span></TooltipHint></dd>
          </div>)}
        </dl>
      </section>

      <section className="status-section" aria-labelledby="status-components-heading">
        <h2 id="status-components-heading">运行组件</h2>
        <div className="status-table-container">
          <table className="status-component-table">
            <colgroup><col /><col /><col /><col /></colgroup>
            <thead><tr><th scope="col">组件</th><th scope="col">状态</th><th scope="col">版本或端口</th><th scope="col">详情</th></tr></thead>
            <tbody>{components.map(component => <tr key={component.name}>
              <th scope="row"><span className="status-component-name"><component.icon aria-hidden="true" />{component.name}</span></th>
              <td><span className="status-component-state" data-tone={component.tone}><i aria-hidden="true" />{component.label}</span></td>
              <td><TooltipHint content={component.value}><code className="status-truncate" tabIndex={0}>{component.value}</code></TooltipHint></td>
              <td><TooltipHint content={component.detail}><span className="status-truncate" tabIndex={0}>{component.detail}</span></TooltipHint></td>
            </tr>)}</tbody>
          </table>
        </div>
      </section>

      <section className="status-section" aria-labelledby="status-environment-heading">
        <h2 id="status-environment-heading">环境与版本</h2>
        <div className="status-environment">
          {environment.map(item => <div className="status-environment-row" data-error={item.error || undefined} key={item.name}>
            <span>{item.name}</span>
            <TooltipHint content={item.value}><code className="status-truncate" tabIndex={0}>{item.value}</code></TooltipHint>
            <EnvironmentValue key={item.detail} name={item.name} value={item.detail} copyable={!!item.copyable} />
          </div>)}
          {state.lastError && <div className="status-environment-row status-last-error" data-error="true">
            <span>Last Error</span><TooltipHint content={state.lastError}><code className="status-truncate" tabIndex={0}>{state.lastError}</code></TooltipHint>
          </div>}
        </div>
      </section>

      <section className="status-section" aria-labelledby="status-actions-heading">
        <h2 id="status-actions-heading">快捷操作</h2>
        <div className="status-action-toolbar">
          <div className="status-action-links">
                {!state.git.available && (
                  <StatusActionButton
                    variant="ghost"
                    disabled={busy !== null}
                    onClick={() =>
                      runSideEffect("open-git", () => api.openExternal("git"))
                    }
                    aria-busy={busy === "open-git"}
                  >
                    打开 Git 下载页面 ↗
                  </StatusActionButton>
                )}
                {!state.git.available && (
                <StatusActionButton
                  variant="ghost"
                  disabled={busy !== null}
                  onClick={() => run("git", api.detectGit, "Git 检测完成。")}
                  aria-busy={busy === "git"}
                >
                  重新检测 Git
                </StatusActionButton>
                )}
                {state.managedRuntimePresent && isInstalled && (
                  <StatusActionButton
                    variant="ghost"
                    disabled={
                      busy !== null || isRunning || !state.git.available
                    }
                    onClick={() =>
                      run(
                        "repair",
                        api.repair,
                        "Managed 官方 Serena 修复完成。",
                      )
                    }
                    aria-busy={busy === "repair"}
                  >
                    修复 Managed Serena
                  </StatusActionButton>
                )}
                {!isInstalled && (
                  <StatusActionButton
                    variant="ghost"
                    disabled={busy !== null}
                    onClick={() =>
                      runSideEffect("open-uv", () => api.openExternal("uv"))
                    }
                    aria-busy={busy === "open-uv"}
                  >
                    uv 安装说明 ↗
                  </StatusActionButton>
                )}
                <StatusActionButton
                  variant="ghost"
                  disabled={
                    !state.dashboardEnabled ||
                    state.serverStatus !== "running" ||
                    busy !== null
                  }
                  onClick={() =>
                    runSideEffect("open-dashboard", api.openDashboard)
                  }
                  aria-busy={busy === "open-dashboard"}
                >
                  打开 Dashboard
                </StatusActionButton>
                <StatusActionButton
                  variant="ghost"
                  disabled={busy !== null}
                  onClick={() => runSideEffect("open-logs", api.openLogs)}
                  aria-busy={busy === "open-logs"}
                >
                  打开日志目录
                </StatusActionButton>
                {!isInstalled && (
                  <StatusActionButton
                    variant="ghost"
                    disabled={busy !== null}
                    onClick={() =>
                      runSideEffect("open-docs", () => api.openExternal("docs"))
                    }
                    aria-busy={busy === "open-docs"}
                  >
                    官方安装说明 ↗
                  </StatusActionButton>
                )}
                  <StatusActionButton
                    variant="ghost"
                    disabled={busy !== null}
                    onClick={() =>
                      runSideEffect("open-github", () =>
                        api.openExternal("github"),
                      )
                    }
                    aria-busy={busy === "open-github"}
                  >
                    Serena GitHub ↗
                  </StatusActionButton>
                <StatusActionButton variant="ghost" disabled={busy !== null} aria-busy={busy === "open-serena-desktop"} onClick={() => runSideEffect("open-serena-desktop", () => api.openExternal("serena-desktop"))}>
                  SerenaDesktop GitHub ↗
                </StatusActionButton>
                <StatusActionButton variant="ghost" disabled={busy !== null} aria-busy={busy === "open-codegraph"} onClick={() => runSideEffect("open-codegraph", () => api.openExternal("codegraph"))}>
                  CodeGraph GitHub ↗
                </StatusActionButton>
          </div>
          <div className="status-lifecycle-action">
                  {isInstalled || isRunning ? (
                    <>
                      {!isRunning ? (
                        <StatusActionButton
                          variant="default"
                          disabled={busy !== null || !canStart}
                          onClick={() =>
                            run("start", api.start, "Serena 已启动。")
                          }
                          aria-busy={busy === "start"}
                        >
                          启动 Serena
                        </StatusActionButton>
                      ) : null}
                    </>
                  ) : (
                    <StatusActionButton
                      variant="default"
                      disabled={busy !== null || !state.git.available}
                      onClick={() =>
                        run(
                          "install",
                          state.managedRuntimePresent
                            ? api.repair
                            : api.install,
                          state.config.serenaPath
                            ? "Managed 官方 Serena 已就绪；请在设置中清空外部路径以使用它。"
                            : "官方 Serena 已就绪。",
                        )
                      }
                      aria-busy={busy === "install"}
                    >
                      {state.managedRuntimePresent
                          ? "修复 官方 Serena"
                          : "安装 官方 Serena"}
                    </StatusActionButton>
                  )}
          </div>
        </div>
      </section>
    </section>
  );
}
