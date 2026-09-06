import { useCallback, useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api } from "./api";
import type { AppState, ManagerConfig, ServerStatus } from "./types";

const statusCopy: Record<ServerStatus, { label: string; detail: string }> = {
  stopped: { label: "已停止", detail: "MCP 端口未监听" },
  starting: { label: "启动中", detail: "正在等待本机端口响应" },
  running: { label: "运行中", detail: "本机 MCP 链路可用" },
  error: { label: "异常", detail: "Serena 未能保持运行" },
};

const initialConfig: ManagerConfig = {
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
      <span className="toggle" aria-hidden="true"><span /></span>
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
    <div className={`toast toast-${kind}`} role={kind === "error" ? "alert" : "status"}>
      <span className="toast-indicator" aria-hidden="true" />
      <span>{message}</span>
      <button aria-label="关闭提醒" onClick={onClose}>×</button>
    </div>
  );
}

function App() {
  const [tab, setTab] = useState<"console" | "dashboard" | "settings">("console");
  const [state, setState] = useState<AppState | null>(null);
  const [draft, setDraft] = useState<ManagerConfig>(initialConfig);
  const [busy, setBusy] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [dashboardFrameKey, setDashboardFrameKey] = useState(0);
  const hydrated = useRef(false);
  const requestEpoch = useRef(0);
  const mutationActive = useRef(false);
  const pickerActive = useRef(false);

  const applySnapshot = useCallback((next: AppState) => {
    if (!hydrated.current) {
      hydrated.current = true;
      setDraft(next.config);
    }
    setState(next);
  }, []);

  const refresh = async () => {
    const next = await api.getState();
    applySnapshot(next);
    return next;
  };

  useEffect(() => {
    let active = true;
    api.getState()
      .then((next) => {
        if (!active) return;
        applySnapshot(next);
      })
      .catch((reason: unknown) => active && setError(String(reason)));

    const timer = window.setInterval(() => {
      if (!active || mutationActive.current) return;
      const epoch = requestEpoch.current;
      api.getState()
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
    setState((current) => current ? { ...current, autostartEnabled: enabled } : current);
    requestEpoch.current += 1;
    mutationActive.current = true;
    setBusy("autostart");
    setError(null);
    setNotice(null);
    try {
      const snapshot = await api.setAutostart(enabled);
      setState(snapshot);
      setNotice(enabled ? "已启用 Windows 登录自启。" : "已关闭 Windows 登录自启。");
    } catch (reason) {
      setState((current) => current ? { ...current, autostartEnabled: previous } : current);
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
        await saveFields({ serenaPath: draft.serenaPath }, "Serena 可执行文件已保存。");
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
      onClose={() => { setError(null); setNotice(null); }}
    />
  ) : null;

  if (!state) {
    return (
      <main className="boot-screen">
        <div className="boot-mark">S</div>
        <p>正在读取本机 Serena 状态…</p>
        {toast}
      </main>
    );
  }

  const status = statusCopy[state.serverStatus];
  const isRunning = state.serverStatus === "running" || state.serverStatus === "starting" || state.managedProcessPresent;
  const isInstalled = state.activeInstallation !== null;

  return (
    <div className="app-shell">
      {toast}
      <header className="titlebar">
        <div className="brand">
          <span className="brand-mark">S</span>
          <span>Serena Desktop</span>
        </div>
        <nav aria-label="主导航">
          <button className={tab === "console" ? "active" : ""} onClick={() => setTab("console")}>运行台</button>
          <button className={tab === "dashboard" ? "active" : ""} onClick={() => setTab("dashboard")}>Serena 面板</button>
          <button className={tab === "settings" ? "active" : ""} onClick={() => setTab("settings")}>设置</button>
        </nav>
      </header>

      <main className={`workspace ${tab === "dashboard" ? "workspace-dashboard" : ""}`}>
        {tab === "console" ? (
          <section className="console-page">
            <div className="page-heading">
              <div>
                <p className="eyebrow">LOCAL SUPERVISOR</p>
                <h1>Serena 运行台</h1>
                <p>确认本机服务链路，完成启动、停止和故障定位。</p>
              </div>
              <button className="ghost-button" disabled={busy !== null} onClick={() => run("detect", api.detect, "已重新检测 Serena。")}>重新检测</button>
            </div>

            <div className={`link-console status-${state.serverStatus}`}>
              <div className="link-track" aria-label={`Desktop 到 Serena 到 MCP：${status.label}`}>
                <div className="link-node desktop-node">
                  <span className="node-icon">D</span>
                  <div><strong>Desktop</strong><small>Supervisor</small></div>
                </div>
                <div className="link-wire"><i /><i /><i /></div>
                <div className="link-node serena-node">
                  <span className="status-light" />
                  <div><strong>Serena</strong><small>{isInstalled ? state.activeInstallation!.version : "未安装"}</small></div>
                </div>
                <div className="link-wire"><i /><i /><i /></div>
                <div className="link-node endpoint-node">
                  <span className="node-icon">M</span>
                  <div><strong>MCP</strong><small>{state.activePort}</small></div>
                </div>
              </div>

              <div className="status-summary">
                <div>
                  <span className="status-kicker">当前状态</span>
                  <h2><span className="status-light" />{isInstalled ? status.label : "未安装"}</h2>
                  <p>{isInstalled ? status.detail : "需要 Serena 后端才能启动本机 MCP 服务。"}</p>
                </div>
                {isInstalled ? (
                  <div className="primary-actions">
                    {!isRunning ? (
                      <button className="primary-button" disabled={busy !== null} onClick={() => run("start", api.start, "Serena 已启动。")}>{busy === "start" ? "启动中…" : "启动 Serena"}</button>
                    ) : (
                      <button className="danger-button" disabled={busy !== null} onClick={() => run("stop", api.stop, "Serena 已停止。")}>{busy === "stop" ? "停止中…" : "停止"}</button>
                    )}
                    <button className="secondary-button" disabled={busy !== null} onClick={() => run("restart", api.restart, "Serena 已重新启动。")}>重新启动</button>
                  </div>
                ) : (
                  <button className="primary-button" disabled={busy !== null} onClick={() => run("install", api.install, "Serena 安装完成。")}>{busy === "install" ? "正在安装…" : "自动安装 Serena"}</button>
                )}
              </div>
            </div>

            <div className="facts-panel">
              <div className="fact-row">
                <span>MCP Endpoint</span>
                <code>{state.endpoint}</code>
              </div>
              <div className="fact-row">
                <span>Serena Executable</span>
                <code title={state.activeInstallation?.path}>{state.activeInstallation?.path ?? "尚未发现"}</code>
              </div>
              <div className="fact-row">
                <span>Dashboard</span>
                <code>{state.dashboardEnabled ? state.dashboardUrl : "已关闭"}</code>
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
              <button disabled={!state.dashboardEnabled || state.serverStatus !== "running" || busy !== null} onClick={() => runSideEffect("open-dashboard", api.openDashboard)}>打开 Dashboard ↗</button>
              <button disabled={busy !== null} onClick={() => runSideEffect("open-logs", api.openLogs)}>打开日志目录 ↗</button>
              {!isInstalled && <button disabled={busy !== null} onClick={() => runSideEffect("open-docs", () => api.openExternal("docs"))}>官方安装说明 ↗</button>}
              {!isInstalled && <button disabled={busy !== null} onClick={() => runSideEffect("open-github", () => api.openExternal("github"))}>GitHub ↗</button>}
            </div>
          </section>
        ) : tab === "dashboard" ? (
          <section className="dashboard-page">
            <div className="page-heading dashboard-heading">
              <div>
                <p className="eyebrow">LOCAL WEB DASHBOARD</p>
                <h1>Serena 面板</h1>
                <p>在应用内查看和操作当前 Serena Web Dashboard。</p>
              </div>
              <div className="dashboard-actions">
                <button
                  className="ghost-button"
                  disabled={!state.dashboardEnabled || state.serverStatus !== "running"}
                  onClick={() => setDashboardFrameKey((key) => key + 1)}
                >
                  刷新
                </button>
                <button
                  className="ghost-button"
                  disabled={!state.dashboardEnabled || state.serverStatus !== "running" || busy !== null}
                  onClick={() => runSideEffect("open-dashboard", api.openDashboard)}
                >
                  在浏览器中打开 ↗
                </button>
              </div>
            </div>

            {state.dashboardEnabled && state.serverStatus === "running" ? (
              <div className="dashboard-frame-shell">
                <div className="dashboard-frame-bar">
                  <span>Dashboard 地址</span>
                  <code title={state.dashboardUrl}>{state.dashboardUrl}</code>
                </div>
                <iframe
                  key={`${state.dashboardUrl}-${dashboardFrameKey}`}
                  className="dashboard-frame"
                  src={state.dashboardUrl}
                  title="Serena Dashboard"
                  sandbox="allow-forms allow-popups allow-same-origin allow-scripts"
                />
              </div>
            ) : (
              <div className="dashboard-empty">
                <span className="dashboard-empty-mark">S</span>
                <h2>{state.dashboardEnabled ? "Serena 尚未运行" : "Dashboard 已关闭"}</h2>
                <p>{state.dashboardEnabled ? "启动 Serena 后，面板会在这里自动可用。" : "请先在设置中启用 Dashboard。"}</p>
                <button className="primary-button" onClick={() => setTab(state.dashboardEnabled ? "console" : "settings")}>
                  {state.dashboardEnabled ? "前往运行台" : "前往设置"}
                </button>
              </div>
            )}
          </section>
        ) : (
          <section className="settings-page">
            <div className="page-heading">
              <div>
                <p className="eyebrow">LOCAL PREFERENCES</p>
                <h1>设置</h1>
                <p>开关与文件选择即时生效；输入项在离开时自动保存。</p>
              </div>
            </div>

            <div className="settings-section">
              <header><span>01</span><div><h2>General</h2><p>Windows 与应用生命周期</p></div></header>
              <div className="settings-body">
                <Toggle
                  checked={state.autostartEnabled ?? false}
                  onChange={setAutostart}
                  disabled={busy !== null || state.autostartEnabled === null}
                  label="Windows 登录后启动"
                  hint={state.autostartError ?? "由系统登录项启动 Serena Desktop"}
                />
                <Toggle checked={draft.autoStartServer} onChange={(value) => saveToggle({ autoStartServer: value }, value ? "已启用自动启动 Serena。" : "已关闭自动启动 Serena。")} disabled={busy !== null} label="自动启动 Serena" hint="应用启动后自动拉起本机 MCP Server" />
                <Toggle checked={draft.minimizeToTray} onChange={(value) => saveToggle({ minimizeToTray: value }, value ? "关闭窗口时将进入托盘。" : "关闭窗口时将退出应用。")} disabled={busy !== null} label="关闭窗口时进入托盘" hint="只有托盘菜单中的“退出”会结束应用" />
              </div>
            </div>

            <div className="settings-section">
              <header><span>02</span><div><h2>Serena</h2><p>可执行文件发现</p></div></header>
              <div className="settings-body field-stack">
                <div className="text-field">
                  <label htmlFor="serena-executable">Executable</label>
                  <div className="input-action-row">
                    <input
                      id="serena-executable"
                      disabled={busy !== null}
                      value={draft.serenaPath ?? ""}
                      onChange={(event) => setDraft({ ...draft, serenaPath: event.target.value || null })}
                      onBlur={(event) => {
                        if (event.relatedTarget instanceof HTMLElement && event.relatedTarget.dataset.executablePicker === "true") return;
                        saveFields({ serenaPath: draft.serenaPath }, "Serena 可执行文件已保存。");
                      }}
                      onKeyDown={(event) => { if (event.key === "Enter") event.currentTarget.blur(); }}
                      placeholder="自动检测"
                      spellCheck={false}
                    />
                    <button
                      type="button"
                      className="ghost-button"
                      data-executable-picker="true"
                      disabled={busy !== null}
                      onClick={chooseSerenaExecutable}
                      onBlur={() => {
                        if (!pickerActive.current) saveFields({ serenaPath: draft.serenaPath }, "Serena 可执行文件已保存。");
                      }}
                    >
                      选择…
                    </button>
                  </div>
                  <small>留空时依次检查 PATH 和 %USERPROFILE%\.local\bin\serena.exe</small>
                </div>
                <div className="detected-path"><span>当前检测</span><code>{state.installation?.path ?? "未发现"}</code></div>
              </div>
            </div>

            <div className="settings-section">
              <header><span>03</span><div><h2>MCP Server</h2><p>固定监听 127.0.0.1</p></div></header>
              <div className="settings-body field-stack">
                <label className="text-field port-field">
                  <span>Port</span>
                  <input
                    disabled={busy !== null}
                    type="number"
                    min={1024}
                    max={65535}
                    value={draft.port}
                    onChange={(event) => setDraft({ ...draft, port: Number(event.target.value) })}
                    onBlur={() => saveFields({ port: draft.port }, "端口已保存；重新启动 Serena 后生效。")}
                    onKeyDown={(event) => { if (event.key === "Enter") event.currentTarget.blur(); }}
                  />
                  <small>允许范围 1024–65535；变更后需重新启动 Serena。</small>
                </label>
                <Toggle checked={draft.dashboardEnabled} onChange={(value) => saveToggle({ dashboardEnabled: value, openDashboardOnLaunch: value && state.config.openDashboardOnLaunch }, value ? "已启用 Dashboard。" : "已关闭 Dashboard。")} disabled={busy !== null} label="启用 Dashboard" hint="同步 Serena 全局配置的 web_dashboard" />
                <Toggle checked={draft.openDashboardOnLaunch} onChange={(value) => saveToggle({ openDashboardOnLaunch: value }, value ? "启动 Serena 时将自动打开 Dashboard。" : "已关闭 Dashboard 自动打开。" )} disabled={!draft.dashboardEnabled || busy !== null} label="启动时自动打开 Dashboard" hint="默认关闭；仍可从运行台或托盘手工打开" />
              </div>
            </div>

            <div className="settings-section">
              <header><span>04</span><div><h2>Logs</h2><p>运行输出与故障定位</p></div></header>
              <div className="settings-body log-location">
                <code>{state.logDirectory}</code>
                <button className="ghost-button" disabled={busy !== null} onClick={() => runSideEffect("open-logs", api.openLogs)}>打开目录 ↗</button>
              </div>
            </div>
          </section>
        )}
      </main>

      <footer>
        <span className={`footer-status status-${state.serverStatus}`}><i />{isInstalled ? status.label : "未安装"}</span>
        <span className="mono">127.0.0.1:{state.activePort}</span>
      </footer>
    </div>
  );
}

export default App;
