import { AgentPanel } from "./AgentPanel";
import { House, Activity, Settings, ScrollText, Bot } from "lucide-react";
import { Button } from "@/components/ui/button";
import { lazy, Suspense, useState } from "react";
import { toast } from "sonner";
import { ProjectPanel } from "./ProjectPanel";
import { useAppController } from "./app/useAppController";
import type { ServerStatus } from "./types";

const StatusPage = lazy(() => import("./features/status/StatusPage"));
const SettingsPage = lazy(() => import("./features/settings/SettingsPage"));
const McpLogs = lazy(() => import("./McpLogs").then(module => ({ default: module.McpLogs })));

const appLogo = new URL("../src-tauri/icons/128x128.png", import.meta.url).href;

const statusCopy: Record<ServerStatus, { label: string; detail: string }> = {
  stopped: { label: "已停止", detail: "MCP 端口未监听" },
  starting: { label: "启动中", detail: "正在等待本机端口响应" },
  running: { label: "运行中", detail: "本机 MCP 链路可用" },
  error: { label: "异常", detail: "Serena 未能保持运行" },
};

function App() {
  const [tab, setTab] = useState<"console" | "serena" | "settings" | "logs" | "agent">("console");
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
  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand">
          <img className="brand-mark" src={appLogo} alt="" />
          <span>
            Serena<small>Desktop</small>
          </span>
        </div>
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
            <Activity aria-hidden="true" />状态
          </Button>
          <Button
            variant={tab === "settings" ? "secondary" : "ghost"}
            className="justify-start"
            aria-current={tab === "settings" ? "page" : undefined}
            onClick={() => setTab("settings")}
          >
            <Settings aria-hidden="true" />设置
          </Button>
          <Button
            variant={tab === "logs" ? "secondary" : "ghost"}
            className="justify-start"
            aria-current={tab === "logs" ? "page" : undefined}
            onClick={() => setTab("logs")}
          >
            <ScrollText aria-hidden="true" />日志
          </Button>
          <Button className="justify-start" aria-current={tab === "agent" ? "page" : undefined} variant={tab === "agent" ? "secondary" : "ghost"} onClick={() => setTab("agent")}><Bot aria-hidden="true" />Agent</Button>
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
              onSerena={() => setTab("serena")}
              onCopied={() => {
                toast.success("复制成功");
              }}
            />
          </section>
        ) : tab === "serena" ? (
          <StatusPage {...controller} state={state} />
        ) : tab === "agent" ? (
          null
        ) : tab === "logs" ? (
          <McpLogs />
        ) : (
          <SettingsPage {...controller} state={state} />
        )}
        </Suspense>
        <div hidden={tab !== "agent"}><AgentPanel sidebarContainer={projectNavigation} onShowTask={() => setTab("agent")} workspace={brokerController.broker?.activeWorkspace ?? null} workspaces={brokerController.broker?.projects ?? state.config.workspaces} onSelectWorkspace={() => setTab("console")} /></div>
      </main>

      <footer>
        <span className={`footer-status status-${state.serverStatus}`}>
          <i />
          Serena：{isRunning || isInstalled ? status.label : installationLabel}
        </span>
        {tab !== "console" && (
          <span className="mono">Serena 内部端口：{state.activePort}</span>
        )}
      </footer>
    </div>
  );
}

export default App;
