import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import { api } from "../../api";
import type { AppState } from "../../types";
import type { AppController } from "../../app/useAppController";
import { TooltipHint } from "@/components/TooltipHint";
import type { ServerStatus } from "../../types";
const statusCopy: Record<ServerStatus, { label: string; detail: string }> = {
  stopped: { label: "已停止", detail: "MCP 端口未监听" },
  starting: { label: "启动中", detail: "正在等待本机端口响应" },
  running: { label: "运行中", detail: "本机 MCP 链路可用" },
  error: { label: "异常", detail: "Serena 未能保持运行" },
};


export default function StatusPage({ state, busy, codexLoading, codexVersion, codexError, run, detectCodex, runSideEffect }: { state: AppState } & Pick<AppController, "busy" | "codexLoading" | "codexVersion" | "codexError" | "run" | "detectCodex" | "runSideEffect">) {
  const status = statusCopy[state.serverStatus];
  const isRunning =
    state.serverStatus === "running" ||
    state.serverStatus === "starting" ||
    state.managedProcessPresent;
  const installation = state.activeInstallation;
  const isInstalled = installation?.state === "standard";
  const canStart =
    state.installation?.state === "standard" && state.git.available;
  const installationLabel = installation
    ? (
        {
          missing: "未安装",
          standard: "官方 Serena",
          invalid: "安装不兼容或已损坏",
        } as const
      )[installation.state]
    : "检测中";
  const sourceLabel = installation
    ? ({ managed: "Managed", external: "External", path: "PATH" } as const)[
        installation.source
      ]
    : "—";
  return (
          <section className="serena-page">
            <div className="serena-controls">
              <div className="page-heading">
                <div>
                  <h1>状态</h1>
                  <p>确认本机服务链路，完成启动、停止和故障定位。</p>
                </div>
                <Button
                  variant="outline"
                  disabled={busy !== null || codexLoading}
                  onClick={() =>
                    run(
                      "detect",
                      async () => {
                        const [next] = await Promise.all([api.detect(), detectCodex(true)]);
                        return next;
                      },
                      "检测已完成，请查看各项状态。",
                    )
                  }
                  aria-busy={busy === "detect"}
                >
                  {busy === "detect" && (
                    <Spinner data-icon="inline-start" aria-hidden="true" />
                  )}
                  重新检测
                </Button>
              </div>

              <div className={`link-console status-${state.serverStatus}`}>
                <div className="status-summary">
                  <div>
                    <span className="status-kicker">当前状态</span>
                    <h2>
                      <span className="status-light" />
                      {isRunning || isInstalled
                        ? status.label
                        : installationLabel}
                    </h2>
                    <p>
                      {isInstalled
                        ? status.detail
                        : (installation?.error ?? "正在检测 官方 Serena。")}
                    </p>
                  </div>
                  {isInstalled || isRunning ? (
                    <div className="primary-actions">
                      {!isRunning ? (
                        <Button
                          variant="default"
                          disabled={busy !== null || !canStart}
                          onClick={() =>
                            run("start", api.start, "Serena 已启动。")
                          }
                          aria-busy={busy === "start"}
                        >
                          {busy === "start" && (
                            <Spinner
                              data-icon="inline-start"
                              aria-hidden="true"
                            />
                          )}
                          {busy === "start" ? "启动中…" : "启动 Serena"}
                        </Button>
                      ) : (
                        <Button
                          variant="destructive"
                          disabled={busy !== null}
                          onClick={() =>
                            run("stop", api.stop, "Serena 已停止。")
                          }
                          aria-busy={busy === "stop"}
                        >
                          {busy === "stop" && (
                            <Spinner
                              data-icon="inline-start"
                              aria-hidden="true"
                            />
                          )}
                          {busy === "stop" ? "停止中…" : "停止"}
                        </Button>
                      )}
                      <Button
                        variant="outline"
                        disabled={busy !== null || !canStart}
                        onClick={() =>
                          run("restart", api.restart, "Serena 已重新启动。")
                        }
                        aria-busy={busy === "restart"}
                      >
                        {busy === "restart" && (
                          <Spinner
                            data-icon="inline-start"
                            aria-hidden="true"
                          />
                        )}
                        重新启动
                      </Button>
                    </div>
                  ) : (
                    <Button
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
                      {busy === "install" && (
                        <Spinner data-icon="inline-start" aria-hidden="true" />
                      )}
                      {busy === "install"
                        ? "正在安装…"
                        : state.managedRuntimePresent
                          ? "修复 官方 Serena"
                          : "安装 官方 Serena"}
                    </Button>
                  )}
                </div>
              </div>

              <div className="facts-panel">
                <div className="fact-row">
                  <span>Serena 内部地址</span>
                  <code>{state.endpoint}</code>
                </div>
                <div className="fact-row">
                  <span>Serena Executable</span>
                  <TooltipHint content={state.activeInstallation?.path}><code tabIndex={0}>
                    {state.activeInstallation?.path ?? "尚未发现"}
                  </code></TooltipHint>
                </div>
                <div className="fact-row">
                  <span>Installation / Context</span>
                  <code>
                    {sourceLabel} ·{" "}
                    {installation?.context ?? "受管只读 Context"}
                  </code>
                </div>
                <div
                  className={`fact-row ${state.git.available ? "" : "error-row"}`}
                >
                  <span>
                    Git ·{" "}
                    {state.git.available
                      ? "Available"
                      : state.git.status === "error"
                        ? "检测失败"
                        : "必需依赖"}
                  </span>
                  <TooltipHint content={state.git.path ?? undefined}><code tabIndex={0}>
                    {state.git.available ? state.git.version : state.git.error}
                  </code></TooltipHint>
                </div>
                <div className="fact-row">
                  <span>CodeGraph 版本</span>
                  <code>{state.codegraphVersion ?? "版本未检测到"}</code>
                </div>
                <div className={`fact-row ${codexError ? "error-row" : ""}`}>
                  <span>本地 Codex 版本</span>
                  <TooltipHint content={codexError || undefined}><code tabIndex={0}>{codexError ? `不可用：${codexError}` : codexVersion || "正在检测…"}</code></TooltipHint>
                </div>
                <div className="fact-row">
                  <span>浏览器管理面板</span>
                  <code>
                    {state.dashboardEnabled ? state.dashboardUrl : "已关闭"}
                  </code>
                </div>
                {state.lastError && (
                  <div className="fact-row error-row">
                    <span>Last Error</span>
                    <code>{state.lastError}</code>
                  </div>
                )}
              </div>

              <div className="quick-actions">
                <span>快捷操作</span>
                {!state.git.available && (
                  <Button
                    variant="ghost"
                    disabled={busy !== null}
                    onClick={() =>
                      runSideEffect("open-git", () => api.openExternal("git"))
                    }
                    aria-busy={busy === "open-git"}
                  >
                    {busy === "open-git" && (
                      <Spinner data-icon="inline-start" aria-hidden="true" />
                    )}
                    打开 Git 下载页面 ↗
                  </Button>
                )}
                <Button
                  variant="ghost"
                  disabled={busy !== null}
                  onClick={() => run("git", api.detectGit, "Git 检测完成。")}
                  aria-busy={busy === "git"}
                >
                  {busy === "git" && (
                    <Spinner data-icon="inline-start" aria-hidden="true" />
                  )}
                  重新检测 Git
                </Button>
                {state.managedRuntimePresent && isInstalled && (
                  <Button
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
                    {busy === "repair" && (
                      <Spinner data-icon="inline-start" aria-hidden="true" />
                    )}
                    {busy === "repair" ? "正在修复…" : "修复 Managed Serena"}
                  </Button>
                )}
                {!isInstalled && (
                  <Button
                    variant="ghost"
                    disabled={busy !== null}
                    onClick={() =>
                      runSideEffect("open-uv", () => api.openExternal("uv"))
                    }
                    aria-busy={busy === "open-uv"}
                  >
                    {busy === "open-uv" && (
                      <Spinner data-icon="inline-start" aria-hidden="true" />
                    )}
                    uv 安装说明 ↗
                  </Button>
                )}
                <Button
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
                  {busy === "open-dashboard" && (
                    <Spinner data-icon="inline-start" aria-hidden="true" />
                  )}
                  在浏览器中打开管理面板 ↗
                </Button>
                <Button
                  variant="ghost"
                  disabled={busy !== null}
                  onClick={() => runSideEffect("open-logs", api.openLogs)}
                  aria-busy={busy === "open-logs"}
                >
                  {busy === "open-logs" && (
                    <Spinner data-icon="inline-start" aria-hidden="true" />
                  )}
                  打开日志目录 ↗
                </Button>
                {!isInstalled && (
                  <Button
                    variant="ghost"
                    disabled={busy !== null}
                    onClick={() =>
                      runSideEffect("open-docs", () => api.openExternal("docs"))
                    }
                    aria-busy={busy === "open-docs"}
                  >
                    {busy === "open-docs" && (
                      <Spinner data-icon="inline-start" aria-hidden="true" />
                    )}
                    官方安装说明 ↗
                  </Button>
                )}
                  <Button
                    variant="ghost"
                    disabled={busy !== null}
                    onClick={() =>
                      runSideEffect("open-github", () =>
                        api.openExternal("github"),
                      )
                    }
                    aria-busy={busy === "open-github"}
                  >
                    {busy === "open-github" && (
                      <Spinner data-icon="inline-start" aria-hidden="true" />
                    )}
                    Serena GitHub ↗
                  </Button>
                <Button variant="ghost" disabled={busy !== null} aria-busy={busy === "open-codegraph"} onClick={() => runSideEffect("open-codegraph", () => api.openExternal("codegraph"))}>
                  {busy === "open-codegraph" && <Spinner data-icon="inline-start" aria-hidden="true" />}
                  CodeGraph GitHub ↗
                </Button>
              </div>
            </div>
          </section>
  );
}
