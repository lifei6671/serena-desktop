import { useRef, useState } from "react";
import { api } from "./api";
import type { AppState } from "./types";
import type { useBroker } from "./useBroker";

export function ProjectPanel({
  state,
  controller,
  onSettings,
  onSerena,
}: {
  state: AppState;
  controller: ReturnType<typeof useBroker>;
  onSettings: () => void;
  onSerena: () => void;
}) {
  const { broker, busy, error, setError, perform } = controller;
  const [selected, setSelected] = useState("");
  const [syncMessage, setSyncMessage] = useState("");
  const selector = useRef<HTMLDialogElement>(null);
  const project = broker?.projects.find((p) => p.id === selected);
  const active = broker?.activeWorkspace;
  const pending = !!busy || !!broker?.operation;
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
  const openSelector = () => {
    setSelected(active?.id ?? broker?.projects[0]?.id ?? "");
    selector.current?.showModal();
  };
  const executable = state.installation?.path;
  const registryHome = broker?.projectSources[0]?.replace(/[\\/][^\\/]+$/, "");
  const quote = (value: string) => "'" + value.replaceAll("'", "''") + "'";
  const command = executable && registryHome
    ? `$env:SERENA_HOME = ${quote(registryHome)}\n& ${quote(executable)} project create 'C:\\path\\to\\project'`
    : "serena project create 'C:\\path\\to\\project'";
  const sync = () => perform("同步项目中", async () => {
    setSyncMessage("");
    const count = await api.syncProjects();
    setSyncMessage(`已同步 ${count} 个项目`);
  });
  return (
    <div className="project-panel">
      <div className="page-heading">
        <div>
          <h1>开始使用</h1>
          <p>选择一个项目，连接本地代码能力。</p>
        </div>
        <button
          className="secondary-button"
          disabled={pending || !broker}
          onClick={() => void sync()}
        >
          ↻ 同步项目
        </button>
      </div>
      <section className="home-section" aria-labelledby="workspace-title">
        <h2 id="workspace-title">当前工作区</h2>
        <div className="workspace-summary">
          <div className="workspace-identity">
            <div className="workspace-name">
              <strong>
                {active?.name ?? (broker ? "尚未激活项目" : "正在读取工作区…")}
              </strong>
              {active && <span className="active-badge">已激活</span>}
            </div>
            {active ? (
              <code className="project-path">{active.root}</code>
            ) : (
              <p>选择一个项目开始使用 Serena。</p>
            )}
          </div>
          <button
            className={active ? "secondary-button" : "primary-button"}
            disabled={!broker}
            onClick={openSelector}
          >
            {active ? "切换项目" : "选择项目"}
          </button>
        </div>
        {(broker?.operation || busy) && (
          <div className="operation" role="status">
            {broker?.operation ?? busy}…{" "}
            {broker?.operation && (
              <button
                onClick={() =>
                  void api
                    .cancelProject()
                    .catch((e: unknown) => setError(String(e)))
                }
              >
                取消操作
              </button>
            )}
          </div>
        )}
        {(error || broker?.lastError) && (
          <div className="inline-error" role="alert">
            项目操作未完成 <button onClick={onSerena}>查看详情 →</button>
          </div>
        )}
      </section>
      <section className="home-section" aria-labelledby="project-sync-title">
        <details open={broker?.projects.length === 0}>
        <summary id="project-sync-title">如何初始化并同步项目</summary>
        <p>先在 PowerShell 中初始化 Git 工作树根目录，再点击“同步项目”。将下面的示例路径替换为你的项目目录。</p>
        <pre className="project-path">{command}</pre>
        <p className="helper">已有 .serena/project.yml 时，将命令中的 create 改为 index，完成登记和预建索引。大项目可在终端查看进度；Desktop 不执行初始化或索引。</p>
        <details>
          <summary>同步来源</summary>
          <p>读取 Serena 的项目登记表及项目配置，不扫描磁盘。启动时自动同步，也可手动刷新；同步不切换当前工作区。</p>
          {broker?.projectSources.map((source) => <p key={source}><code className="project-path">{source}</code></p>)}
        </details>
        </details>
        {syncMessage && <p role="status">{syncMessage}</p>}
        {!!broker?.syncWarnings.length && <div role="status">{broker.syncWarnings.map((warning) => <p key={warning}>{warning}</p>)}</div>}
      </section>
      <section className="home-section" aria-labelledby="services-title">
        <div className="section-heading">
          <h2 id="services-title">服务状态</h2>
          <button className="text-button" onClick={onSerena}>
            查看 Serena 状态 →
          </button>
        </div>
        <div className="service-list">
          <div className="service-row">
            <h3>Serena</h3>
            <span className={`service-status tone-${serenaTone}`}>
              <i />
              {serenaLabel}
            </span>
            <span className="service-detail">
              {state.serverStatus === "running" && !active
                ? "未绑定项目"
                : state.activeInstallation?.version || "—"}
            </span>
          </div>
          <div className="service-row">
            <h3>Git</h3>
            <span
              className={`service-status tone-${state.git.available ? "good" : "bad"}`}
            >
              <i />
              {state.git.available
                ? "可用"
                : state.git.status === "error"
                  ? "检测失败"
                  : "不可用"}
            </span>
            <span className="service-detail">{state.git.version || "—"}</span>
          </div>
          <div className="service-row">
            <h3>MCP 连接入口</h3>
            <span
              className={`service-status tone-${!broker ? "waiting" : broker.running ? "good" : "idle"}`}
            >
              <i />
              {!broker ? "读取中" : broker.running ? "监听中" : "已停止"}
            </span>
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
        <h2 id="connection-title">连接配置</h2>
        <p className="field-label">本机 MCP 地址</p>
        {endpoint ? (
          <>
            <div className="endpoint-copy">
              <code>{endpoint}</code>
              <button
                className="secondary-button"
                onClick={() => void navigator.clipboard.writeText(endpoint).catch((e: unknown) => setError(String(e)))}
              >
                复制
              </button>
            </div>
            <p className="helper">供 Cloudflare MCP upstream 使用。</p>
          </>
        ) : (
          <div className="connection-stopped">
            <span>
              {broker ? "MCP 连接入口尚未启动" : "正在读取连接入口状态…"}
            </span>
            <button className="text-button" onClick={onSettings}>
              前往设置 →
            </button>
          </div>
        )}
      </section>
      <dialog
        ref={selector}
        className="project-dialog"
        aria-labelledby="select-title"
      >
        <header>
          <h2 id="select-title">选择项目</h2>
          <button
            aria-label="关闭项目选择"
            onClick={() => selector.current?.close()}
          >
            ×
          </button>
        </header>
        <p>
          当前工作区：{active?.name ?? "尚未激活"}。选择列表项不会切换工作区。
        </p>
        <label>
          待操作项目
          <select
            aria-label="待操作项目"
            value={selected}
            disabled={pending}
            onChange={(e) => setSelected(e.target.value)}
          >
            <option value="">选择项目</option>
            {broker?.projects.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name}
                {p.id === active?.id
                  ? " · 已激活"
                  : " · 已同步"}
              </option>
            ))}
          </select>
        </label>
        {!broker?.projects.length && (
          <p>暂无可用项目，请先在终端初始化，然后点击首页的“同步项目”。</p>
        )}
        {project && (
          <>
            <code className="project-path">{project.root}</code>
            <div className="selection-action">
              {current ? (
                <>
                  <span className="active-badge">已激活</span>
                  <button
                    className="secondary-button"
                    disabled={pending}
                    onClick={() => perform("取消激活中", api.deactivateProject)}
                  >
                    取消激活
                  </button>
                </>
              ) : (
                <button
                  className="primary-button"
                  disabled={pending}
                  onClick={() =>
                    perform(
                      active ? "切换中" : "激活中",
                      async () => {
                        await api.activateProject(project.id);
                        selector.current?.close();
                      },
                    )
                  }
                >
                  {active ? "切换到此项目" : "激活"}
                </button>
              )}
            </div>

          </>
        )}
        {(busy || broker?.operation) && (
          <p role="status">
            {broker?.operation ?? busy}…{" "}
            {broker?.operation && (
              <button
                onClick={() =>
                  void api
                    .cancelProject()
                    .catch((e: unknown) => setError(String(e)))
                }
              >
                取消操作
              </button>
            )}
          </p>
        )}
        {(error || broker?.lastError) && (
          <p role="alert">
            操作未完成。
            <button
              onClick={() => {
                selector.current?.close();
                onSerena();
              }}
            >
              查看详情 →
            </button>
          </p>
        )}
      </dialog>

    </div>
  );
}
