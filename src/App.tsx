import { useCallback, useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { McpLogs } from "./McpLogs";
import { ProjectPanel } from "./ProjectPanel";
import { useBroker } from "./useBroker";
import { api } from "./api";
import type { AppState, ManagerConfig, ServerStatus } from "./types";

const appLogo = new URL("../src-tauri/icons/128x128.png", import.meta.url).href;

const statusCopy: Record<ServerStatus, { label: string; detail: string }> = {
  stopped: { label: "已停止", detail: "MCP 端口未监听" },
  starting: { label: "启动中", detail: "正在等待本机端口响应" },
  running: { label: "运行中", detail: "本机 MCP 链路可用" },
  error: { label: "异常", detail: "Serena 未能保持运行" },
};

const initialConfig: ManagerConfig = {
  broker: { enabled: false, port: 9120 },
  workspaces: [],
  serenaPath: null,
  port: 9121,
  dashboardEnabled: true,
  openDashboardOnLaunch: false,
  autoStartServer: true,
  minimizeToTray: true,
};

function Toggle({
  checked,
  onChange,
  label,
  hint,
  disabled = false,
}: {
  checked: boolean;
  onChange: (value: boolean) => void;
  label: string;
  hint: string;
  disabled?: boolean;
}) {
  return (
    <label className={`toggle-row ${disabled ? "is-disabled" : ""}`}>
      <span>
        <strong>{label}</strong>
        <small>{hint}</small>
      </span>
      <input
        type="checkbox"
        checked={checked}
        onChange={(event) => onChange(event.target.checked)}
        disabled={disabled}
      />
      <span className="toggle" aria-hidden="true">
        <span />
      </span>
    </label>
  );
}

function Toast({
  message,
  kind,
  onClose,
}: {
  message: string;
  kind: "error" | "notice";
  onClose: () => void;
}) {
  return (
    <div
      className={`toast toast-${kind}`}
      role={kind === "error" ? "alert" : "status"}
    >
      <span className="toast-indicator" aria-hidden="true" />
      <span>{message}</span>
      <button aria-label="关闭提醒" onClick={onClose}>
        ×
      </button>
    </div>
  );
}

function App() {
  const [tab, setTab] = useState<"console" | "serena" | "settings" | "logs">("console");
  const [state, setState] = useState<AppState | null>(null);
  const [draft, setDraft] = useState<ManagerConfig>(initialConfig);
  const [busy, setBusy] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [brokerPort, setBrokerPort] = useState(9120);
  const hydrated = useRef(false);
  const requestEpoch = useRef(0);
  const mutationActive = useRef(false);
  const pickerActive = useRef(false);

  const applySnapshot = useCallback((next: AppState) => {
    if (!hydrated.current) {
      hydrated.current = true;
      setDraft(next.config);
      setBrokerPort(next.config.broker.port);
    }
    setState(next);
  }, []);

  const refresh = async () => {
    const next = await api.getState();
    applySnapshot(next);
    return next;
  };

  const brokerController = useBroker(refresh);

  useEffect(() => {
    let active = true;
    api
      .getState()
      .then((next) => {
        if (!active) return;
        applySnapshot(next);
      })
      .catch((reason: unknown) => active && setError(String(reason)));

    const timer = window.setInterval(() => {
      if (!active || mutationActive.current) return;
      const epoch = requestEpoch.current;
      api
        .getState()
        .then((next) => {
          if (active && epoch === requestEpoch.current) applySnapshot(next);
        })
        .catch(() => undefined);
    }, 1500);

    return () => {
      active = false;
      window.clearInterval(timer);
    };
  }, [applySnapshot]);

  useEffect(() => {
    if (!error && !notice) return;
    const timer = window.setTimeout(() => {
      setError(null);
      setNotice(null);
    }, 4500);
    return () => window.clearTimeout(timer);
  }, [error, notice]);

  const run = async (
    label: string,
    action: () => Promise<AppState>,
    success?: string,
  ) => {
    requestEpoch.current += 1;
    mutationActive.current = true;
    setBusy(label);
    setError(null);
    setNotice(null);
    try {
      const next = await action();
      setState(next);
      if (success) setNotice(success);
    } catch (reason) {
      setError(String(reason));
      await refresh().catch(() => undefined);
    } finally {
      mutationActive.current = false;
      setBusy(null);
    }
  };

  const saveFields = async (patch: Partial<ManagerConfig>, success: string) => {
    if (!state || busy !== null) return;
    const unchanged = Object.entries(patch).every(
      ([key, value]) => state.config[key as keyof ManagerConfig] === value,
    );
    if (unchanged) return;

    requestEpoch.current += 1;
    mutationActive.current = true;
    setBusy("field");
    setError(null);
    setNotice(null);
    try {
      const snapshot = await api.saveConfig({ ...state.config, ...patch });
      setState(snapshot);
      setDraft((current) => ({ ...current, ...patch }));
      setNotice(success);
    } catch (reason) {
      setError(String(reason));
      await refresh().catch(() => undefined);
    } finally {
      mutationActive.current = false;
      setBusy(null);
    }
  };

  const saveToggle = async (patch: Partial<ManagerConfig>, success: string) => {
    if (!state) return;
    const previous = draft;
    const optimistic = { ...draft, ...patch };
    const persisted = { ...state.config, ...patch };
    setDraft(optimistic);
    requestEpoch.current += 1;
    mutationActive.current = true;
    setBusy("toggle");
    setError(null);
    setNotice(null);
    try {
      const snapshot = await api.saveConfig(persisted);
      setState(snapshot);
      setDraft((current) => ({ ...current, ...patch }));
      setNotice(success);
    } catch (reason) {
      setDraft(previous);
      setError(String(reason));
      await refresh().catch(() => undefined);
    } finally {
      mutationActive.current = false;
      setBusy(null);
    }
  };

  const runSideEffect = async (label: string, action: () => Promise<void>) => {
    setBusy(label);
    setError(null);
    try {
      await action();
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(null);
    }
  };

  const setAutostart = async (enabled: boolean) => {
    const previous = state?.autostartEnabled ?? null;
    setState((current) =>
      current ? { ...current, autostartEnabled: enabled } : current,
    );
    requestEpoch.current += 1;
    mutationActive.current = true;
    setBusy("autostart");
    setError(null);
    setNotice(null);
    try {
      const snapshot = await api.setAutostart(enabled);
      setState(snapshot);
      setNotice(
        enabled ? "已启用 Windows 登录自启。" : "已关闭 Windows 登录自启。",
      );
    } catch (reason) {
      setState((current) =>
        current ? { ...current, autostartEnabled: previous } : current,
      );
      setError(String(reason));
      await refresh().catch(() => undefined);
    } finally {
      mutationActive.current = false;
      setBusy(null);
    }
  };

  const chooseSerenaExecutable = async () => {
    setError(null);
    pickerActive.current = true;
    try {
      const selected = await open({
        multiple: false,
        directory: false,
        defaultPath: draft.serenaPath ?? undefined,
        filters: [{ name: "Serena executable", extensions: ["exe"] }],
      });
      if (typeof selected === "string") {
        setDraft((current) => ({ ...current, serenaPath: selected }));
        await saveFields({ serenaPath: selected }, "Serena 可执行文件已保存。");
      } else {
        await saveFields(
          { serenaPath: draft.serenaPath },
          "Serena 可执行文件已保存。",
        );
      }
    } catch (reason) {
      setError(String(reason));
    } finally {
      pickerActive.current = false;
    }
  };

  const toastMessage = error ?? notice;
  const toast = toastMessage ? (
    <Toast
      message={toastMessage}
      kind={error ? "error" : "notice"}
      onClose={() => {
        setError(null);
        setNotice(null);
      }}
    />
  ) : null;

  if (!state) {
    return (
      <main className="boot-screen">
        <img className="boot-mark" src={appLogo} alt="Serena Desktop" />
        <p>正在读取本机 Serena 状态…</p>
        {toast}
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
    <div className="app-shell">
      {toast}
      <aside className="sidebar">
        <div className="brand">
          <img className="brand-mark" src={appLogo} alt="" />
          <span>Serena<small>Desktop</small></span>
        </div>
        <nav aria-label="主导航">
          <button
            className={tab === "console" ? "active" : ""}
            onClick={() => setTab("console")}
          >
            首页
          </button>
          <button
            className={tab === "serena" ? "active" : ""}
            onClick={() => setTab("serena")}
          >
            Serena
          </button>
          <button
            className={tab === "settings" ? "active" : ""}
            onClick={() => setTab("settings")}
          >
            设置
          </button>
          <button className={tab === "logs" ? "active" : ""} onClick={() => setTab("logs")}>
            日志
          </button>
        </nav>
      </aside>

      <main className={`workspace ${tab === "logs" ? "workspace-logs" : ""}`}>
        {tab === "console" ? (
          <section className="console-page">
            <ProjectPanel
              state={state}
              controller={brokerController}
              onSettings={() => setTab("settings")}
              onSerena={() => setTab("serena")}
            />
          </section>
        ) : tab === "serena" ? (
          <section className="serena-page">
            <div className="serena-controls">
              <div className="page-heading">
                <div>
                  <h1>Serena</h1>
                  <p>确认本机服务链路，完成启动、停止和故障定位。</p>
                </div>
                <button
                  className="ghost-button"
                  disabled={busy !== null}
                  onClick={() =>
                    run("detect", api.detect, "已重新检测 Serena 和 Git。")
                  }
                >
                  重新检测
                </button>
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
                        <button
                          className="primary-button"
                          disabled={busy !== null || !canStart}
                          onClick={() =>
                            run("start", api.start, "Serena 已启动。")
                          }
                        >
                          {busy === "start" ? "启动中…" : "启动 Serena"}
                        </button>
                      ) : (
                        <button
                          className="danger-button"
                          disabled={busy !== null}
                          onClick={() =>
                            run("stop", api.stop, "Serena 已停止。")
                          }
                        >
                          {busy === "stop" ? "停止中…" : "停止"}
                        </button>
                      )}
                      <button
                        className="secondary-button"
                        disabled={busy !== null || !canStart}
                        onClick={() =>
                          run("restart", api.restart, "Serena 已重新启动。")
                        }
                      >
                        重新启动
                      </button>
                    </div>
                  ) : (
                    <button
                      className="primary-button"
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
                    >
                      {busy === "install"
                        ? "正在安装…"
                        : state.managedRuntimePresent
                          ? "修复 官方 Serena"
                          : "安装 官方 Serena"}
                    </button>
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
                  <code title={state.activeInstallation?.path}>
                    {state.activeInstallation?.path ?? "尚未发现"}
                  </code>
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
                  <code title={state.git.path ?? undefined}>
                    {state.git.available ? state.git.version : state.git.error}
                  </code>
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
                  <button
                    disabled={busy !== null}
                    onClick={() =>
                      runSideEffect("open-git", () => api.openExternal("git"))
                    }
                  >
                    打开 Git 下载页面 ↗
                  </button>
                )}
                <button
                  disabled={busy !== null}
                  onClick={() => run("git", api.detectGit, "Git 检测完成。")}
                >
                  重新检测 Git
                </button>
                {state.managedRuntimePresent && isInstalled && (
                  <button
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
                  >
                    {busy === "repair" ? "正在修复…" : "修复 Managed Serena"}
                  </button>
                )}
                {!isInstalled && (
                  <button
                    disabled={busy !== null}
                    onClick={() =>
                      runSideEffect("open-uv", () => api.openExternal("uv"))
                    }
                  >
                    uv 安装说明 ↗
                  </button>
                )}
                <button
                  disabled={
                    !state.dashboardEnabled ||
                    state.serverStatus !== "running" ||
                    busy !== null
                  }
                  onClick={() =>
                    runSideEffect("open-dashboard", api.openDashboard)
                  }
                >
                  在浏览器中打开管理面板 ↗
                </button>
                <button
                  disabled={busy !== null}
                  onClick={() => runSideEffect("open-logs", api.openLogs)}
                >
                  打开日志目录 ↗
                </button>
                {!isInstalled && (
                  <button
                    disabled={busy !== null}
                    onClick={() =>
                      runSideEffect("open-docs", () => api.openExternal("docs"))
                    }
                  >
                    官方安装说明 ↗
                  </button>
                )}
                {!isInstalled && (
                  <button
                    disabled={busy !== null}
                    onClick={() =>
                      runSideEffect("open-github", () =>
                        api.openExternal("github"),
                      )
                    }
                  >
                    GitHub ↗
                  </button>
                )}
              </div>

              {(brokerController.error ||
                brokerController.broker?.lastError) && (
                <div className="project-error" role="alert">
                  <strong>项目 / Broker 操作详情</strong>
                  <p>
                    {brokerController.error ||
                      brokerController.broker?.lastError}
                  </p>
                  <button
                    onClick={() => setTab("logs")}
                  >
                    查看 MCP 日志
                  </button>
                </div>
              )}
            </div>
          </section>
        ) : tab === "logs" ? (
          <McpLogs />
        ) : (
          <section className="settings-page">
            <div className="page-heading">
              <div>
                <h1>设置</h1>
                <p>开关与文件选择即时生效；输入项在离开时自动保存。</p>
              </div>
            </div>

            <div className="settings-section">
              <header>
                <span>01</span>
                <div>
                  <h2>General</h2>
                  <p>Windows 与应用生命周期</p>
                </div>
              </header>
              <div className="settings-body">
                <Toggle
                  checked={state.autostartEnabled ?? false}
                  onChange={setAutostart}
                  disabled={busy !== null || state.autostartEnabled === null}
                  label="Windows 登录后启动"
                  hint={
                    state.autostartError ?? "由系统登录项启动 Serena Desktop"
                  }
                />
                <Toggle
                  checked={draft.autoStartServer}
                  onChange={(value) =>
                    saveToggle(
                      { autoStartServer: value },
                      value
                        ? "已启用自动启动 Serena。"
                        : "已关闭自动启动 Serena。",
                    )
                  }
                  disabled={busy !== null}
                  label="自动启动 Serena"
                  hint="应用启动后自动启动 Serena 代码分析服务"
                />
                <Toggle
                  checked={draft.minimizeToTray}
                  onChange={(value) =>
                    saveToggle(
                      { minimizeToTray: value },
                      value
                        ? "关闭窗口时将进入托盘。"
                        : "关闭窗口时将退出应用。",
                    )
                  }
                  disabled={busy !== null}
                  label="关闭窗口时进入托盘"
                  hint="只有托盘菜单中的“退出”会结束应用"
                />
              </div>
            </div>

            <div className="settings-section">
              <header>
                <span>02</span>
                <div>
                  <h2>Serena</h2>
                  <p>可执行文件发现</p>
                </div>
              </header>
              <div className="settings-body field-stack">
                <div className="text-field">
                  <label htmlFor="serena-executable">Executable</label>
                  <div className="input-action-row">
                    <input
                      id="serena-executable"
                      disabled={busy !== null}
                      value={draft.serenaPath ?? ""}
                      onChange={(event) =>
                        setDraft({
                          ...draft,
                          serenaPath: event.target.value || null,
                        })
                      }
                      onBlur={(event) => {
                        if (
                          event.relatedTarget instanceof HTMLElement &&
                          event.relatedTarget.dataset.executablePicker ===
                            "true"
                        )
                          return;
                        saveFields(
                          { serenaPath: draft.serenaPath },
                          "Serena 可执行文件已保存。",
                        );
                      }}
                      onKeyDown={(event) => {
                        if (event.key === "Enter") event.currentTarget.blur();
                      }}
                      placeholder="使用 Managed 官方 Serena"
                      spellCheck={false}
                    />
                    <button
                      type="button"
                      className="ghost-button"
                      data-executable-picker="true"
                      disabled={busy !== null}
                      onClick={chooseSerenaExecutable}
                      onBlur={() => {
                        if (!pickerActive.current)
                          saveFields(
                            { serenaPath: draft.serenaPath },
                            "Serena 可执行文件已保存。",
                          );
                      }}
                    >
                      选择…
                    </button>
                  </div>
                  <small>
                    留空时优先使用应用私有 runtime；未安装时检测
                    PATH。指定外部路径必须为受支持的官方 Serena 1.7.0 或以上。
                  </small>
                </div>
                <div className="detected-path">
                  <span>当前检测</span>
                  <code>{state.installation?.path ?? "未发现"}</code>
                </div>
              </div>
            </div>

            <div className="settings-section">
              <header>
                <span>03</span>
                <div>
                  <h2>Serena 内部服务</h2>
                  <p>提供代码分析能力，由 MCP 连接入口调用</p>
                </div>
              </header>
              <div className="settings-body field-stack">
                <label className="text-field port-field">
                  <span>内部服务端口</span>
                  <input
                    disabled={busy !== null}
                    type="number"
                    min={1024}
                    max={65535}
                    value={draft.port}
                    onChange={(event) =>
                      setDraft({ ...draft, port: Number(event.target.value) })
                    }
                    onBlur={() =>
                      saveFields(
                        { port: draft.port },
                        "端口已保存；重新启动 Serena 后生效。",
                      )
                    }
                    onKeyDown={(event) => {
                      if (event.key === "Enter") event.currentTarget.blur();
                    }}
                  />
                  <small>
                    仅供本机内部通信，无需填入 Cloudflare。允许范围
                    1024–65535；变更后需重新启动 Serena。
                  </small>
                </label>
                <Toggle
                  checked={draft.dashboardEnabled}
                  onChange={(value) =>
                    saveToggle(
                      {
                        dashboardEnabled: value,
                        openDashboardOnLaunch:
                          value && state.config.openDashboardOnLaunch,
                      },
                      value ? "已启用 Dashboard。" : "已关闭 Dashboard。",
                    )
                  }
                  disabled={busy !== null}
                  label="启用浏览器管理面板"
                  hint="使用浏览器查看 Serena 运行信息；更改后需重新启动 Serena"
                />
                <Toggle
                  checked={draft.openDashboardOnLaunch}
                  onChange={(value) =>
                    saveToggle(
                      { openDashboardOnLaunch: value },
                      value
                        ? "启动 Serena 时将自动打开 Dashboard。"
                        : "已关闭 Dashboard 自动打开。",
                    )
                  }
                  disabled={!draft.dashboardEnabled || busy !== null}
                  label="启动时在浏览器打开管理面板"
                  hint="默认关闭；也可从 Serena 页面或托盘手工打开"
                />
              </div>
            </div>

            <div className="settings-section">
              <header>
                <div>
                  <h2>MCP 连接入口</h2>
                  <p>汇集 Serena 代码分析与 Git 查询能力</p>
                </div>
              </header>
              <div className="settings-body field-stack">
                <label className="text-field port-field">
                  <span>连接入口端口</span>
                  <input
                    type="number"
                    min={1024}
                    max={65535}
                    value={brokerPort}
                    disabled={
                      !!brokerController.busy ||
                      brokerController.broker?.running
                    }
                    onChange={(e) => setBrokerPort(Number(e.target.value))}
                  />
                  <small>
                    Cloudflare MCP upstream
                    使用此入口，地址可在首页复制。仅监听本机
                    127.0.0.1；启用时应用端口设置。
                  </small>
                </label>
                <div>
                  <button
                    className="secondary-button"
                    disabled={
                      busy !== null ||
                      !!brokerController.busy ||
                      !brokerController.broker
                    }
                    onClick={() =>
                      brokerController.perform("更新连接入口", () =>
                        api.setBroker(
                          !brokerController.broker?.running,
                          brokerPort,
                        ),
                      )
                    }
                  >
                    {brokerController.broker?.running
                      ? "停止连接入口"
                      : "启用连接入口"}
                  </button>
                </div>
                {brokerController.busy && (
                  <p role="status">{brokerController.busy}…</p>
                )}
                {(brokerController.error ||
                  brokerController.broker?.lastError) && (
                  <p className="inline-error" role="alert">
                    {brokerController.error ||
                      brokerController.broker?.lastError}
                  </p>
                )}
              </div>
            </div>
            <div className="settings-section">
              <header>
                <span>04</span>
                <div>
                  <h2>Logs</h2>
                  <p>运行输出与故障定位</p>
                </div>
              </header>
              <div className="settings-body log-location">
                <code>{state.logDirectory}</code>
                <button
                  className="ghost-button"
                  disabled={busy !== null}
                  onClick={() => runSideEffect("open-logs", api.openLogs)}
                >
                  打开目录 ↗
                </button>
              </div>
            </div>
          </section>
        )}
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
