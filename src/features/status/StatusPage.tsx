import { useEffect, useState, type ComponentProps } from "react";
import { Check, Copy, GitBranch, Network, RefreshCw, Server, SquareTerminal } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import { TooltipHint } from "@/components/TooltipHint";
import { api } from "../../api";
import type { AppState } from "../../types";
import type { AppController } from "../../app/useAppController";

type ComponentStatus = { label: string; tone: "healthy" | "pending" | "warning" | "error" | "inactive" };

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
  const installation = state.activeInstallation;
  const serenaStatus: ComponentStatus = installation?.state === "standard"
    ? { label: "已发现", tone: "healthy" }
    : installation?.state === "invalid"
      ? { label: "安装异常", tone: "error" }
      : installation
        ? { label: "未检测到", tone: "inactive" }
        : { label: "检测中", tone: "pending" };
  const gitStatus: ComponentStatus = state.git.available
    ? { label: "已发现", tone: "healthy" }
    : state.git.status === "error"
      ? { label: "检测失败", tone: "error" }
      : { label: "未检测到", tone: "inactive" };
  const broker = brokerController.broker;
  const brokerEndpoint = broker?.running ? `http://127.0.0.1:${broker.port}/mcp` : null;
  const brokerStatus: ComponentStatus = broker
    ? { label: broker.running ? "运行中" : "已停止", tone: broker.running ? "healthy" : "inactive" }
    : { label: "状态不可用", tone: "inactive" };
  const codexStatus: ComponentStatus = codexLoading ? { label: "检测中", tone: "pending" }
    : codexError ? { label: "不可用", tone: "error" }
    : codexVersion ? { label: "CLI 可用", tone: "healthy" } : { label: "未检测到", tone: "inactive" };
  const codegraphStatus: ComponentStatus = state.codegraphVersion
    ? { label: "已发现", tone: "healthy" }
    : { label: "未检测到", tone: "inactive" };
  const components = [
    { name: "Serena", icon: Server, ...serenaStatus, value: installation?.version || "版本未检测到", detail: installation?.path || installation?.error || "仅检测本机安装，不管理启动状态" },
    { name: "MCP Broker", icon: Network, ...brokerStatus, value: broker ? `HTTP · ${broker.port}` : "端口不可用", detail: brokerEndpoint ?? brokerStatus.label },
    { name: "Codex CLI", icon: SquareTerminal, ...codexStatus, value: codexLoading ? "正在检测…" : codexVersion || "版本未检测到", detail: codexLoading ? "正在检测本地 CLI" : codexError || "本地 CLI 可用性" },
    { name: "CodeGraph CLI", icon: GitBranch, ...codegraphStatus, value: state.codegraphVersion ?? "版本未检测到", detail: "仅检测本机命令，不初始化或启动工作区能力" },
    { name: "Git CLI", icon: GitBranch, ...gitStatus, value: state.git.version || "版本未检测到", detail: state.git.path || state.git.error || "仅检测本机命令" },
  ];
  const environment = [
    { name: "Serena", value: installation?.version || serenaStatus.label, detail: installation?.path || installation?.error || "尚未发现可执行文件", copyable: !!installation?.path },
    { name: "MCP Broker", value: broker ? `HTTP · ${broker.port}` : "端口不可用", detail: brokerEndpoint ?? brokerStatus.label, copyable: !!brokerEndpoint },
    { name: "Codex", value: codexLoading ? "正在检测…" : codexVersion || codexStatus.label, detail: codexLoading ? "正在检测本地 CLI" : codexError || "本地 CLI 可用性", error: !codexLoading && !!codexError },
    { name: "CodeGraph", value: state.codegraphVersion ?? codegraphStatus.label, detail: "本机命令检测结果", copyable: false },
    { name: "Git", value: state.git.version || gitStatus.label, detail: state.git.path || state.git.error || "尚未发现 Git 命令", error: gitStatus.tone === "error", copyable: !!state.git.path },
  ];
  const redetect = () => run("detect", async () => {
    const [next] = await Promise.all([api.detect(), detectCodex(true)]);
    return next;
  }, "检测已完成，请查看各项状态。");

  return (
    <section className="serena-page status-page">
      <div className="page-heading">
        <div><h1>状态</h1><p>查看本机命令和服务是否已发现；项目请求会单独携带工作区。</p></div>
        <Button variant="outline" disabled={busy !== null || codexLoading} onClick={redetect} aria-busy={busy === "detect"}>
          {busy === "detect" ? <Spinner aria-hidden="true" /> : <RefreshCw aria-hidden="true" />}
          重新检测
        </Button>
      </div>

      <section className="status-overview" aria-label="当前状态">
        <div className="status-overview-state">
          <span className="status-section-label">当前状态</span>
          <strong className="status-component-state" data-tone={brokerStatus.tone}><i aria-hidden="true" />{brokerStatus.label}</strong>
          <TooltipHint content={brokerEndpoint ?? brokerStatus.label}><span className="status-truncate" tabIndex={0}>{brokerEndpoint ?? brokerStatus.label}</span></TooltipHint>
        </div>
        <dl className="status-overview-facts">
          {[
            { label: "MCP Broker", value: brokerEndpoint ?? brokerStatus.label },
          ].map(item => <div key={item.label}>
            <dt>{item.label}</dt>
            <dd><TooltipHint content={item.value}><span className="status-truncate mono" tabIndex={0}>{item.value}</span></TooltipHint></dd>
          </div>)}
        </dl>
      </section>

      <section className="status-section" aria-labelledby="status-components-heading">
        <h2 id="status-components-heading">本机命令与服务</h2>
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
                <StatusActionButton
                  variant="ghost"
                  disabled={busy !== null}
                  onClick={() => run("git", api.detectGit, "Git 检测完成。")}
                  aria-busy={busy === "git"}
                >
                  重新检测 Git
                </StatusActionButton>
                <StatusActionButton
                  variant="ghost"
                  disabled={busy !== null}
                  onClick={() => runSideEffect("open-logs", api.openLogs)}
                  aria-busy={busy === "open-logs"}
                >
                  打开日志目录
                </StatusActionButton>
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
        </div>
      </section>
    </section>
  );
}
