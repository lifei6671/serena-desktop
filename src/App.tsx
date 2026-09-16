import { AgentPanel } from "./AgentPanel";
import { House, Activity, Settings, ScrollText, Bot, Globe } from "lucide-react";
import { useRemoteAccess } from "./useRemoteAccess";
import { RemoteApprovalDialog } from "./RemoteApprovalDialog";
import { Button } from "@/components/ui/button";
import { lazy, Suspense, useState } from "react";
import { toast } from "sonner";
import { ProjectPanel } from "./ProjectPanel";
import { useAppController } from "./app/useAppController";
import { api } from "./api";
import type { ServerStatus } from "./types";

const StatusPage = lazy(() => import("./features/status/StatusPage"));
const SettingsPage = lazy(() => import("./features/settings/SettingsPage"));
const RemoteAccessPage = lazy(() => import("./RemoteAccessPage"));
const McpLogs = lazy(() => import("./McpLogs").then(module => ({ default: module.McpLogs })));

const appLogo = new URL("../src-tauri/icons/128x128.png", import.meta.url).href;

const statusCopy: Record<ServerStatus, { label: string; detail: string }> = {
  stopped: { label: "已停止", detail: "MCP 端口未监听" },
  starting: { label: "启动中", detail: "正在等待本机端口响应" },
  running: { label: "运行中", detail: "本机 MCP 链路可用" },
  error: { label: "异常", detail: "Serena 未能保持运行" },
};

function App() {
  const [tab, setTab] = useState<"console" | "serena" | "settings" | "logs" | "agent" | "task" | "remote">("console");
  const remote = useRemoteAccess();
  const [projectNavigation, setProjectNavigation] = useState<HTMLDivElement | null>(null);
  const controller = useAppController(tab === "serena");
  const { state, brokerController } = controller;
  if (!state) {
    return (
      <main className="boot-screen">
        <img className="boot-mark" src={appLogo} alt="Serena Desktop" />
        <p>正在读取本机 Serena 状态…</p>
      </main>
    );
  }

  const status = statusCopy[state.serverStatus];
  const isRunning =
    state.serverStatus === "running" ||
    state.serverStatus === "starting" ||
    state.managedProcessPresent;
  const installation = state.activeInstallation;
  const isInstalled = installation?.state === "standard";
  const installationLabel = installation
    ? (
        {
          missing: "未安装",
          standard: "官方 Serena",
          invalid: "安装不兼容或已损坏",
        } as const
      )[installation.state]
    : "检测中";
  const setMcpRunning = (enabled: boolean) => {
    brokerController.perform(
      "更新连接入口",
      () => api.setBroker(enabled, state.config.broker.port, state.config.broker.allowLan),
      enabled ? "MCP 连接入口已启用" : "MCP 连接入口已停止",
    );
  };
  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand">
          <img className="brand-mark" src={appLogo} alt="" />
          <span>Serena<small>Desktop</small></span>
        </div>
        <p className="sidebar-section-label">NAVIGATION</p>
        <nav aria-label="主导航">
          <Button
            variant={tab === "console" ? "secondary" : "ghost"}
            className="justify-start"
            aria-current={tab === "console" ? "page" : undefined}
            onClick={() => setTab("console")}
          >
            <House aria-hidden="true" />首页
          </Button>
          <Button
            variant={tab === "serena" ? "secondary" : "ghost"}
            className="justify-start"
            aria-current={tab === "serena" ? "page" : undefined}
            onClick={() => setTab("serena")}
          >
            <Activity aria-hidden="true" />服务状态
          </Button>
          <Button
            variant={tab === "agent" ? "secondary" : "ghost"}
            className="justify-start"
            aria-current={tab === "agent" ? "page" : undefined}
            onClick={() => setTab("agent")}
          >
            <Bot aria-hidden="true" />Agent
          </Button>
          <Button
            variant={tab === "logs" ? "secondary" : "ghost"}
            className="justify-start"
            aria-current={tab === "logs" ? "page" : undefined}
            onClick={() => setTab("logs")}
          >
            <ScrollText aria-hidden="true" />日志终端
          </Button>
          <Button className="justify-start" aria-current={tab === "remote" ? "page" : undefined} variant={tab === "remote" ? "secondary" : "ghost"} onClick={() => setTab("remote")}><Globe aria-hidden="true" />远程访问</Button>
          <Button
            variant={tab === "settings" ? "secondary" : "ghost"}
            className="justify-start"
            aria-current={tab === "settings" ? "page" : undefined}
            onClick={() => setTab("settings")}
          >
            <Settings aria-hidden="true" />设置
          </Button>
        </nav>
        <div className="project-navigation-slot" ref={setProjectNavigation} />
      </aside>

      <main className={`workspace ${tab === "logs" ? "workspace-logs" : ""}`}>
        <Suspense fallback={<p role="status">正在加载页面…</p>}>
        {tab === "console" ? (
          <section className="console-page">
            <ProjectPanel
              state={state}
              controller={brokerController}
              onSettings={() => setTab("settings")}
              onRemote={() => setTab("remote")}
              onSerena={() => setTab("serena")}
              onSelectWorkspace={controller.selectWorkspace}
              onCopied={() => {
                toast.success("复制成功");
              }}
            />
          </section>
        ) : tab === "remote" ? (
          <RemoteAccessPage controller={remote} port={state.config.broker.port} allowLan={state.config.broker.allowLan} mcpRunning={brokerController.broker?.running ?? null} mcpStartedAt={brokerController.broker?.startedAt ?? null} mcpBusy={!!brokerController.busy} onSetMcpRunning={setMcpRunning} onSettings={() => setTab("settings")} />
        ) : tab === "serena" ? (
          <StatusPage {...controller} state={state} />
        ) : tab === "agent" || tab === "task" ? (
          null
        ) : tab === "logs" ? (
          <McpLogs />
        ) : (
          <SettingsPage {...controller} state={state} />
        )}
        </Suspense>
        <div hidden={tab !== "agent" && tab !== "task"}><AgentPanel detailView={tab === "task"} sidebarContainer={projectNavigation} onShowTask={() => setTab("task")} onShowAgent={() => setTab("agent")} workspace={state.desktopSelectedWorkspace} workspaces={state.config.workspaces} onSelectWorkspace={() => setTab("console")} /></div>
        <RemoteApprovalDialog controller={remote} />
      </main>

      <footer>
        <span className={`footer-status status-${state.serverStatus}`}>
          <i />
          Serena：{isRunning || isInstalled ? status.label : installationLabel}
        </span>
        <span className="mono">Serena 内部端口：{state.activePort}</span>
      </footer>
    </div>
  );
}

export default App;
