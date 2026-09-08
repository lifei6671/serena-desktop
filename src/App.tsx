import { Spinner } from "@/components/ui/spinner";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { useCallback, useEffect, useId, useRef, useState } from "react";
import { Switch } from "@/components/ui/switch";
import {
  Field,
  FieldGroup,
  FieldContent,
  FieldLabel,
  FieldDescription,
} from "@/components/ui/field";
import { toast } from "sonner";
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
  broker: { enabled: false, port: 9120, allowLan: false },
  workspaces: [],
  serenaPath: null,
  port: 9121,
  dashboardEnabled: true,
  openDashboardOnLaunch: false,
  autoStartServer: true,
  minimizeToTray: true,
};

function SettingSwitch({
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
  const id = useId();
  return (
    <Field orientation="horizontal" data-disabled={disabled}>
      <FieldContent>
        <FieldLabel htmlFor={id}>{label}</FieldLabel>
        <FieldDescription id={`${id}-description`}>{hint}</FieldDescription>
      </FieldContent>
      <Switch
        id={id}
        aria-describedby={`${id}-description`}
        checked={checked}
        onCheckedChange={onChange}
        disabled={disabled}
      />
    </Field>
  );
}

function App() {
  const [tab, setTab] = useState<"console" | "serena" | "settings" | "logs">(
    "console",
  );
  const [state, setState] = useState<AppState | null>(null);
  const [draft, setDraft] = useState<ManagerConfig>(initialConfig);
  const [busy, setBusy] = useState<string | null>(null);
  const [brokerPort, setBrokerPort] = useState(9120);
  const [brokerAllowLan, setBrokerAllowLan] = useState(false);
  const hydrated = useRef(false);
  const requestEpoch = useRef(0);
  const mutationActive = useRef(false);
  const pickerActive = useRef(false);
  const [choosingExecutable, setChoosingExecutable] = useState(false);

  const applySnapshot = useCallback((next: AppState) => {
    if (!hydrated.current) {
      hydrated.current = true;
      setDraft(next.config);
      setBrokerPort(next.config.broker.port);
      setBrokerAllowLan(next.config.broker.allowLan);
    }
    setState(next);
  }, []);

  const refresh = async () => {
    const next = await api.getState();
    applySnapshot(next);
    return next;
  };

  const brokerController = useBroker(refresh);
  const updatingBroker = brokerController.busy === "更新连接入口";

  useEffect(() => {
    let active = true;
    let lastReadError = "";
    const reportReadError = (reason: unknown) => {
      if (active && lastReadError !== String(reason)) {
        lastReadError = String(reason);
        toast.error(`服务状态读取失败：${lastReadError}`, {
          id: "app-read-error",
        });
      }
    };
    api
      .getState()
      .then((next) => {
        if (!active) return;
        applySnapshot(next);
        lastReadError = "";
      })
      .catch(reportReadError);

    const timer = window.setInterval(() => {
      if (!active || mutationActive.current) return;
      const epoch = requestEpoch.current;
      api
        .getState()
        .then((next) => {
          if (active && epoch === requestEpoch.current) {
            applySnapshot(next);
            lastReadError = "";
          }
        })
        .catch((reason: unknown) => {
          if (epoch === requestEpoch.current) reportReadError(reason);
        });
    }, 1500);

    return () => {
      active = false;
      window.clearInterval(timer);
    };
  }, [applySnapshot]);

  const run = async (
    label: string,
    action: () => Promise<AppState>,
    success?: string,
  ) => {
    requestEpoch.current += 1;
    mutationActive.current = true;
    setBusy(label);
    try {
      const next = await action();
      setState(next);
      if (success) toast.success(success);
    } catch (reason) {
      toast.error(String(reason), { id: "app-feedback" });
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
    try {
      const snapshot = await api.saveConfig({ ...state.config, ...patch });
      setState(snapshot);
      setDraft((current) => ({ ...current, ...patch }));
      toast.success(success);
    } catch (reason) {
      toast.error(String(reason), { id: "app-feedback" });
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
    try {
      const snapshot = await api.saveConfig(persisted);
      setState(snapshot);
      setDraft((current) => ({ ...current, ...patch }));
      toast.success(success);
    } catch (reason) {
      setDraft(previous);
      toast.error(String(reason), { id: "app-feedback" });
      await refresh().catch(() => undefined);
    } finally {
      mutationActive.current = false;
      setBusy(null);
    }
  };

  const runSideEffect = async (label: string, action: () => Promise<void>) => {
    setBusy(label);
    try {
      await action();
    } catch (reason) {
      toast.error(String(reason), { id: "app-feedback" });
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
    try {
      const snapshot = await api.setAutostart(enabled);
      setState(snapshot);
      toast.success(
        enabled ? "已启用 Windows 登录自启。" : "已关闭 Windows 登录自启。",
      );
    } catch (reason) {
      setState((current) =>
        current ? { ...current, autostartEnabled: previous } : current,
      );
      toast.error(String(reason), { id: "app-feedback" });
      await refresh().catch(() => undefined);
    } finally {
      mutationActive.current = false;
      setBusy(null);
    }
  };

  const chooseSerenaExecutable = async () => {
    pickerActive.current = true;
    setChoosingExecutable(true);
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
      toast.error(String(reason), { id: "app-feedback" });
    } finally {
      pickerActive.current = false;
      setChoosingExecutable(false);
    }
  };

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
            首页
          </Button>
          <Button
            variant={tab === "serena" ? "secondary" : "ghost"}
            className="justify-start"
            aria-current={tab === "serena" ? "page" : undefined}
            onClick={() => setTab("serena")}
          >
            状态
          </Button>
          <Button
            variant={tab === "settings" ? "secondary" : "ghost"}
            className="justify-start"
            aria-current={tab === "settings" ? "page" : undefined}
            onClick={() => setTab("settings")}
          >
            设置
          </Button>
          <Button
            variant={tab === "logs" ? "secondary" : "ghost"}
            className="justify-start"
            aria-current={tab === "logs" ? "page" : undefined}
            onClick={() => setTab("logs")}
          >
            日志
          </Button>
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
              onCopied={() => {
                toast.success("复制成功");
              }}
            />
          </section>
        ) : tab === "serena" ? (
          <section className="serena-page">
            <div className="serena-controls">
              <div className="page-heading">
                <div>
                  <h1>状态</h1>
                  <p>确认本机服务链路，完成启动、停止和故障定位。</p>
                </div>
                <Button
                  variant="outline"
                  disabled={busy !== null}
                  onClick={() =>
                    run(
                      "detect",
                      api.detect,
                      "已重新检测 Serena、Git 和 CodeGraph。",
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
                  <span>CodeGraph 版本</span>
                  <code>{state.codegraphVersion ?? "版本未检测到"}</code>
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
                {!isInstalled && (
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
                    GitHub ↗
                  </Button>
                )}
              </div>
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
              <FieldGroup className="settings-body">
                <SettingSwitch
                  checked={state.autostartEnabled ?? false}
                  onChange={setAutostart}
                  disabled={busy !== null || state.autostartEnabled === null}
                  label="Windows 登录后启动"
                  hint={
                    state.autostartError ?? "由系统登录项启动 Serena Desktop"
                  }
                />
                <SettingSwitch
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
                <SettingSwitch
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
              </FieldGroup>
            </div>

            <div className="settings-section">
              <header>
                <span>02</span>
                <div>
                  <h2>Serena</h2>
                  <p>可执行文件发现</p>
                </div>
              </header>
              <FieldGroup className="settings-body field-stack">
                <Field className="text-field">
                  <FieldLabel htmlFor="serena-executable">
                    Executable
                  </FieldLabel>
                  <div className="input-action-row">
                    <Input
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
                    <Button
                      type="button"
                      variant="outline"
                      data-executable-picker="true"
                      aria-busy={choosingExecutable}
                      disabled={busy !== null || choosingExecutable}
                      onClick={chooseSerenaExecutable}
                      onBlur={() => {
                        if (!pickerActive.current)
                          saveFields(
                            { serenaPath: draft.serenaPath },
                            "Serena 可执行文件已保存。",
                          );
                      }}
                    >
                      {choosingExecutable && (
                        <Spinner data-icon="inline-start" aria-hidden="true" />
                      )}
                      选择…
                    </Button>
                  </div>
                  <FieldDescription>
                    留空时优先使用应用私有 runtime；未安装时检测
                    PATH。指定外部路径必须为受支持的官方 Serena 1.7.0 或以上。
                  </FieldDescription>
                </Field>
                <div className="detected-path">
                  <span>当前检测</span>
                  <code>{state.installation?.path ?? "未发现"}</code>
                </div>
              </FieldGroup>
            </div>

            <div className="settings-section">
              <header>
                <span>03</span>
                <div>
                  <h2>Serena 内部服务</h2>
                  <p>提供代码分析能力，由 MCP 连接入口调用</p>
                </div>
              </header>
              <FieldGroup className="settings-body field-stack">
                <Field className="max-w-md">
                  <FieldLabel htmlFor="serena-port">内部服务端口</FieldLabel>
                  <Input
                    disabled={busy !== null}
                    type="number"
                    min={1024}
                    max={65535}
                    id="serena-port"
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
                  <FieldDescription>
                    仅供本机内部通信，无需填入 Cloudflare。允许范围
                    1024–65535；变更后需重新启动 Serena。
                  </FieldDescription>
                </Field>
                <SettingSwitch
                  checked={draft.dashboardEnabled}
                  onChange={(value) =>
                    saveToggle(
                      {
                        dashboardEnabled: value,
                        openDashboardOnLaunch:
                          value && state.config.openDashboardOnLaunch,
                      },
                      value
                        ? "已保存：启用管理面板，重启 Serena 后生效。"
                        : "已保存：关闭管理面板，重启 Serena 后生效。",
                    )
                  }
                  disabled={busy !== null}
                  label="启用浏览器管理面板"
                  hint="使用浏览器查看 Serena 运行信息；更改后需重新启动 Serena"
                />
                <SettingSwitch
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
              </FieldGroup>
            </div>

            <div className="settings-section">
              <header>
                <div>
                  <h2>MCP 连接入口</h2>
                  <p>汇集 Serena 代码分析与 Git 查询能力</p>
                </div>
              </header>
              <FieldGroup className="settings-body field-stack">
                <Field className="max-w-md">
                  <FieldLabel htmlFor="broker-port">连接入口端口</FieldLabel>
                  <Input
                    type="number"
                    min={1024}
                    max={65535}
                    id="broker-port"
                    value={brokerPort}
                    disabled={
                      !!brokerController.busy ||
                      brokerController.broker?.running
                    }
                    onChange={(e) => setBrokerPort(Number(e.target.value))}
                  />
                  <FieldDescription>
                    Cloudflare MCP upstream
                    使用此入口，本机地址可在首页复制。停止入口后可修改端口和访问范围，重新启用时生效。
                  </FieldDescription>
                </Field>
                <SettingSwitch
                  label="允许局域网连接"
                  checked={brokerAllowLan}
                  onChange={setBrokerAllowLan}
                  disabled={!!brokerController.busy || !!brokerController.broker?.running}
                  hint={brokerAllowLan
                    ? `开启后监听所有 IPv4 网卡（0.0.0.0）。其他电脑使用 http://本机局域网IPv4地址:${brokerPort}/mcp。服务无内置认证，能访问端口的设备可读取和切换共享工作区，请仅在可信网络启用；如被防火墙拦截，需手动允许对应端口。`
                    : "关闭时仅本机可连接（127.0.0.1），其他电脑无法访问。"}
                />
                <div>
                  <Button
                    variant="outline"
                    disabled={
                      busy !== null ||
                      !!brokerController.busy ||
                      !brokerController.broker
                    }
                    onClick={() =>
                      brokerController.perform(
                        "更新连接入口",
                        () =>
                          api.setBroker(
                            !brokerController.broker?.running,
                            brokerPort,
                            brokerAllowLan,
                          ),
                        brokerController.broker?.running
                          ? "MCP 连接入口已停止"
                          : "MCP 连接入口已启用",
                      )
                    }
                    aria-busy={updatingBroker}
                  >
                    {updatingBroker ? (
                      <>
                        <Spinner data-icon="inline-start" aria-hidden="true" />
                        处理中…
                      </>
                    ) : brokerController.broker?.running ? (
                      "停止连接入口"
                    ) : (
                      "启用连接入口"
                    )}
                  </Button>
                </div>
              </FieldGroup>
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
