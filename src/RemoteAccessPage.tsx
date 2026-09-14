import { useEffect, useRef, useState } from "react";
import { ArrowRightLeft, Cable, Check, Cloud, Copy, Info, RefreshCw, ShieldCheck, Shield, ChevronDown, Globe, Monitor, Square, Zap } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { api } from "./api";
import type { RemoteAccessMode, RemoteState, SecurityDeclaration, SelfHostedProvider } from "./types";
import type { RemoteController } from "./useRemoteAccess";

type SelfHostedUiProvider = Extract<SelfHostedProvider, "custom_https" | "ngrok">;

const states: Record<RemoteState["status"], string> = {
  stopped: "未开启", starting: "正在准备本地服务", installing: "正在检查与安装组件",
  discovering_url: "正在获取公网地址", verifying: "正在验证公网入口", ready: "已连接",
  stopping: "正在停止并撤销授权", error: "连接失败", disconnected: "连接已断开",
};
const modes = [
  { id: "quick_tunnel", name: "快捷隧道", technology: "Cloudflare", icon: Cloud },
  { id: "self_hosted_oauth", name: "自建接入", technology: "自有 HTTPS / ngrok", icon: Globe },
  { id: "mcp_only", name: "仅 MCP", technology: "127.0.0.1", icon: Cable },
] as const;
const quickTunnelProgressStates = new Set<RemoteState["status"]>(["starting", "installing", "discovering_url", "verifying"]);

function formatRuntimeDuration(startedAt: number | null, now: number) {
  const totalSeconds = startedAt === null ? 0 : Math.max(0, Math.floor((now - startedAt) / 1000));
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor(totalSeconds / 60) % 60;
  const seconds = totalSeconds % 60;
  return [hours, minutes, seconds].map(value => String(value).padStart(2, "0")).join(":");
}

export default function RemoteAccessPage({ controller, port, allowLan, mcpRunning, mcpStartedAt, mcpBusy, onSetMcpRunning }: { controller: RemoteController; port: number; allowLan: boolean; mcpRunning: boolean | null; mcpStartedAt: number | null; mcpBusy: boolean; onSetMcpRunning: (enabled: boolean) => void; onSettings: () => void }) {
  const { state, error, busy, operate, startQuickTunnel, quickTunnelCommandPending, quickTunnelHandledError } = controller;
  const [clockNow, setClockNow] = useState(() => Date.now());
  const errorBaseline = useRef<{ reportedError: string; mode: RemoteAccessMode | null; status: RemoteState["status"] | null } | null>(null);
  const lastErrorToast = useRef<{ key: string; at: number } | null>(null);
  function showErrorToast(key: string, message: string) {
    const now = Date.now();
    if (lastErrorToast.current?.key === key && now - lastErrorToast.current.at < 1_500) return;
    lastErrorToast.current = { key, at: now };
    toast.error(message);
  }
  async function stop() {
    await operate("停止", api.remoteStop);
  }
  const [selectedMode, setMode] = useState<RemoteAccessMode | null>(null);
  const [providerDraft, setProvider] = useState<SelfHostedUiProvider | null>(null);
  const configuredProvider: SelfHostedUiProvider = state?.config?.selfHosted?.provider === "ngrok" ? "ngrok" : "custom_https";
  const provider = providerDraft ?? configuredProvider;
  const [originDraft, setOrigin] = useState<string | null>(null);
  const origin = originDraft ?? state?.config?.selfHosted.publicOrigin ?? "";
  const [ngrokToken, setNgrokToken] = useState("");
  const [declarationDraft, setDeclaration] = useState<SecurityDeclaration | null>(null);
  const declaration = declarationDraft ?? state?.config?.mcpOnly.securityDeclaration ?? "external_auth";
  const [onlyOriginDraft, setOnlyOrigin] = useState<string | null>(null);
  const onlyOrigin = onlyOriginDraft ?? state?.config?.mcpOnly.publicOrigin ?? "";
  const appliedOnlyOrigin = state?.config?.mcpOnly.publicOrigin ?? "";
  const [riskAccepted, setRiskAccepted] = useState(false);
  const onlyApplied = state?.mode === "mcp_only" && declaration === (state.config?.mcpOnly.securityDeclaration ?? "external_auth") && (declaration === "none" || onlyOrigin.trim() === appliedOnlyOrigin);
  const [copying, setCopying] = useState(false);
  const [copiedResource, setCopiedResource] = useState<string | null>(null);
  const active = state?.active ?? false;
  const mode = selectedMode ?? state?.mode ?? "mcp_only";
  const statusText = state ? state.mode === "mcp_only" && state.status === "stopped" ? "本地模式" : states[state.status] : error ? "状态不可用" : "正在读取状态…";
  const quickTunnelInProgress = state?.mode === "quick_tunnel" && quickTunnelProgressStates.has(state.status);
  const quickTunnelReady = state?.mode === "quick_tunnel" && state.status === "ready";
  const quickTunnelStopping = state?.mode === "quick_tunnel" && state.status === "stopping";
  const quickTunnelFailed = state?.mode === "quick_tunnel" && (state.status === "error" || state.status === "disconnected");
  const quickTunnelRetained = quickTunnelFailed && active;
  function runtimeBadge(id: RemoteAccessMode) {
    if (state?.mode !== id) return null;
    if (id === "mcp_only") {
      if (mcpRunning === true) return { state: "ready", label: "当前运行" };
      if (mcpRunning === false) return { state: "configured", label: "已停止" };
      return { state: "configured", label: "状态读取中" };
    }
    if (state.status === "ready") return { state: "ready", label: "当前运行" };
    if (state.status === "error" || state.status === "disconnected") return { state: "failed", label: "连接异常" };
    if (state.status === "stopping") return { state: "stopping", label: "停止中" };
    if (quickTunnelProgressStates.has(state.status)) return { state: "connecting", label: "连接中" };
    return { state: "configured", label: "当前配置" };
  }
  const runtimeProvider: SelfHostedUiProvider = state?.config?.selfHosted?.provider === "ngrok" ? "ngrok" : "custom_https";
  const quickTunnelConfigured = state?.mode === "quick_tunnel";
  const customHttpsConfigured = state?.mode === "self_hosted_oauth" && runtimeProvider === "custom_https";
  const ngrokConfigured = state?.mode === "self_hosted_oauth" && runtimeProvider === "ngrok";
  const selectedSelfHostedActive = active && state?.mode === "self_hosted_oauth" && runtimeProvider === provider;
  const customHttpsActive = active && state?.mode === "self_hosted_oauth" && runtimeProvider === "custom_https";
  const customHttpsReady = customHttpsActive && state?.status === "ready";
  const persistedCustomOrigin = state?.config?.selfHosted?.provider === "custom_https" ? state.config.selfHosted.publicOrigin ?? "" : "";
  const customOrigin = customHttpsActive && state?.publicContext?.publicOrigin ? state.publicContext.publicOrigin : origin;
  const customOriginMeta = customHttpsReady && state?.publicContext?.publicOrigin
    ? "当前生效配置"
    : customHttpsConfigured && persistedCustomOrigin
      ? "当前配置"
      : originDraft !== null && origin.trim()
        ? "待应用"
        : "尚未配置";
  const resource = state?.status === "ready" && state.mode === mode && (mode !== "self_hosted_oauth" || runtimeProvider === provider) ? state.publicContext?.mcpResource : null;
  const customOriginCopied = !!customOrigin && copiedResource === customOrigin;
  const reportedError = [error, state?.lastError].filter(Boolean).join("\n");
  useEffect(() => {
    if (!copiedResource) return;
    const timer = window.setTimeout(() => setCopiedResource(null), 1600);
    return () => window.clearTimeout(timer);
  }, [copiedResource]);
  useEffect(() => {
    const previous = errorBaseline.current;
    const current = { reportedError, mode: state?.mode ?? null, status: state?.status ?? null };
    if (!previous) {
      errorBaseline.current = current;
      return;
    }
    if (current.mode === "quick_tunnel") {
      errorBaseline.current = current;
      return;
    }
    if (quickTunnelCommandPending) {
      errorBaseline.current = current;
      return;
    }
    if (state?.lastError && state.lastError === quickTunnelHandledError) {
      errorBaseline.current = current;
      return;
    }
    if (reportedError && reportedError !== previous.reportedError) {
      showErrorToast(`remote-error:${reportedError}`, "远程访问操作失败，请稍后重试。");
    }
    errorBaseline.current = current;
  }, [reportedError, quickTunnelCommandPending, quickTunnelHandledError, state?.mode, state?.status, state?.lastError]);
  async function copy(target = resource) {
    if (!target) return;
    setCopying(true);
    try { await navigator.clipboard.writeText(target); setCopiedResource(target); }
    catch (e) { toast.error(`复制失败：${String(e)}`); }
    finally { setCopying(false); }
  }
  async function saveNgrokToken() {
    const token = ngrokToken.trim();
    if (!token) return;
    if (await operate("保存 ngrok 凭据", () => api.remoteSaveNgrokAuth(token))) setNgrokToken("");
  }
  async function startNgrok() {
    const token = ngrokToken.trim();
    const started = await operate("开启", async () => {
      if (token) await api.remoteSaveNgrokAuth(token);
      await api.remoteStartNgrok();
    });
    if (started) setNgrokToken("");
  }
  async function reconnectNgrok() {
    await operate("重新连接", async () => {
      await api.remoteStop();
      await api.remoteStartNgrok();
    });
  }
  async function startCustomHttps() {
    const candidate = origin.trim();
    const focusOrigin = () => document.getElementById("self-origin")?.focus();
    if (!candidate) {
      focusOrigin();
      toast.error("请先填写公网 HTTPS 地址");
      return;
    }
    let normalizedOrigin: string;
    try {
      const parsed = new URL(candidate);
      if (parsed.protocol !== "https:" || !parsed.hostname || parsed.username || parsed.password || parsed.search || parsed.hash || parsed.pathname !== "/") throw new Error("invalid origin");
      normalizedOrigin = parsed.origin;
    } catch {
      focusOrigin();
      toast.error("请输入有效的 HTTPS Origin");
      return;
    }
    await operate("开启", () => api.remoteStart("self_hosted_oauth", normalizedOrigin));
  }
  const runtimeMode = state?.mode ? modes.find(({ id }) => id === state.mode) : null;
  const localMcpEndpoint = `http://127.0.0.1:${port}/mcp`;
  const runtimeLabel = state?.mode === "mcp_only"
    ? "仅 MCP（纯本地）"
    : state?.mode === "self_hosted_oauth" && runtimeProvider === "ngrok" ? "自建接入 · ngrok" : runtimeMode?.name;
  const runtimeEndpoint = state?.mode === "mcp_only"
    ? localMcpEndpoint
    : state?.status === "ready" ? state.publicContext?.mcpResource ?? null : null;
  const quickEndpoint = state?.mode === "quick_tunnel" ? runtimeEndpoint : null;
  const quickStatusBadge = state?.mode === "quick_tunnel" ? state.status === "ready" ? "ACTIVE · 已连接" : statusText : "等待切换";
  const endpointCopied = !!runtimeEndpoint && copiedResource === runtimeEndpoint;
  const runtimeStartedAt = state?.mode === "mcp_only"
    ? mcpRunning === true ? mcpStartedAt ?? null : null
    : state?.active ? state.startedAt ?? null : null;
  const runtimeDuration = formatRuntimeDuration(runtimeStartedAt, clockNow);
  useEffect(() => {
    setClockNow(Date.now());
    if (runtimeStartedAt === null) return;
    const timer = window.setInterval(() => setClockNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [runtimeStartedAt]);
  const quickEndpointCopied = !!quickEndpoint && copiedResource === quickEndpoint;
  const ngrokReady = state?.mode === "self_hosted_oauth" && runtimeProvider === "ngrok" && state.status === "ready";
  const ngrokEndpoint = ngrokReady ? state.publicContext?.mcpResource ?? null : null;
  const ngrokEndpointCopied = !!ngrokEndpoint && copiedResource === ngrokEndpoint;
  const ngrokClientConfig = ngrokEndpoint ? JSON.stringify({ url: ngrokEndpoint }, null, 2) : null;
  const ngrokClientConfigCopied = !!ngrokClientConfig && copiedResource === ngrokClientConfig;
  const runtimeLifecycle = state?.mode === "quick_tunnel" ? "临时地址" : state?.mode === "self_hosted_oauth" ? "自有公网入口" : "本地模式";
  const runtimeDescription = state?.mode === "quick_tunnel"
    ? "由应用管理的临时 HTTPS MCP 入口，重启后需要重新创建。"
    : state?.mode === "self_hosted_oauth"
      ? "使用已配置的公网入口，SerenaDesktop 继续负责 OAuth 访问保护。"
      : mcpRunning === true
        ? "仅本地监听 · 无隧道穿透 · 适合反向代理或本地调用。"
        : mcpRunning === false
          ? "本地 MCP 接入已停止，当前仅保留配置。"
          : "正在读取本地 MCP 接入状态。";
  const protectionStatus = !state ? "状态未读取" : state.mode === "mcp_only"
    ? state.config?.mcpOnly.securityDeclaration === "none" ? "未使用认证" : "外部网关 / 无内置 OAuth"
    : "OAuth 2.0 · 已启用";
  const selfHostedProviderSelector = <fieldset disabled={!!busy} className="self-hosted-providers">
    <legend className="sr-only">选择公网入口方式</legend>
    <span className="self-hosted-provider-label">接入方式选择：</span>
    <div className="self-hosted-provider-options">
      <label className="self-hosted-provider-choice" data-selected={provider === "custom_https"}>
        <Globe size={16} aria-hidden="true"/><span>自有 HTTPS</span>{state?.mode === "self_hosted_oauth" && runtimeProvider === "custom_https" && <span className="self-hosted-provider-runtime" data-active={active}><span aria-hidden="true"/>{active ? state.status === "ready" ? "当前运行" : "当前生效" : "当前配置"}</span>}<input type="radio" name="self-hosted-provider" value="custom_https" checked={provider === "custom_https"} onChange={() => { setProvider("custom_https"); }} />
      </label>
      <label className="self-hosted-provider-choice" data-selected={provider === "ngrok"}>
        <Cloud size={16} aria-hidden="true"/><span>ngrok</span>{state?.mode === "self_hosted_oauth" && runtimeProvider === "ngrok" && <span className="self-hosted-provider-runtime" data-active={active}><span aria-hidden="true"/>{active ? state.status === "ready" ? "当前运行" : "当前生效" : "当前配置"}</span>}<input type="radio" name="self-hosted-provider" value="ngrok" checked={provider === "ngrok"} onChange={() => { setProvider("ngrok"); }} />
      </label>
    </div>
  </fieldset>;
  return <section className="remote-page">
    <div className="page-heading"><div><div className="remote-title"><h1>远程访问</h1><span>REMOTE MCP ACCESS</span></div><p>配置远程 MCP 接入方式，生成供 ChatGPT、Claude 等客户端连接的公网或本地 Endpoint。</p></div></div>
    <section className="remote-runtime-summary" aria-label="当前远程访问运行状态">
      <div className="remote-summary-main">
        <div className="remote-summary-title"><span className="remote-status-indicator" data-ready={state?.mode === "mcp_only" ? mcpRunning === true : state?.status === "ready"} aria-hidden="true"/><strong>{runtimeLabel ? `${runtimeLabel}（当前生效）` : "正在读取远程状态"}</strong>{state?.mode !== "mcp_only" && <span className="remote-summary-product">{state?.mode === "self_hosted_oauth" && runtimeProvider === "ngrok" ? "ngrok" : runtimeMode?.technology ?? "REMOTE ACCESS"}</span>}<span className="remote-summary-lifecycle">{state ? runtimeLifecycle : "状态未知"}</span></div>
        <p>{state ? runtimeDescription : "等待后端返回当前运行模式与访问状态。"}</p>
        <div className="remote-summary-endpoint"><span>{state?.mode === "mcp_only" ? "本地 Endpoint" : "公网 Endpoint"}</span><code>{runtimeEndpoint ?? "—  尚未就绪"}</code><Button className="remote-copy" data-copied={endpointCopied} variant="outline" disabled={!runtimeEndpoint || copying || endpointCopied} onClick={() => void copy(runtimeEndpoint)}><span className="remote-copy-icon" aria-hidden="true"><Copy size={16}/><Check size={16}/></span><span aria-live="polite">{endpointCopied ? "已复制" : copying ? "正在复制…" : "复制"}</span></Button></div>
      </div>
      <dl className="remote-summary-metrics"><div data-ready={!!state && state.mode !== "mcp_only"}><dt>访问保护:</dt><dd>{protectionStatus}</dd></div>{state?.mode === "mcp_only" ? <div><dt>监听范围:</dt><dd>{allowLan ? "局域网（0.0.0.0）" : "本机（127.0.0.1）"}</dd></div> : <div data-ready={state?.status === "ready"}><dt>公网状态:</dt><dd>{state?.status === "ready" ? "已验证" : state ? statusText : "未知"}</dd></div>}<div className="remote-summary-uptime"><dt>运行时长:</dt><dd>{runtimeDuration}</dd></div></dl>
    </section>
    <section className="remote-mode-section" aria-labelledby="remote-mode-heading">
      <div className="remote-section-heading"><div><h2 id="remote-mode-heading">连接方式</h2><span>CONNECTION MODE</span></div><p>选定后需点击下方操作变更当前运行模式</p></div>
      <fieldset className="remote-modes" disabled={!!busy}>
        <legend className="sr-only">选择连接方式</legend>
        {modes.map(({ id, name, technology, icon: Icon }) => { const badge = runtimeBadge(id); return <label key={id} className="remote-mode" data-selected={mode === id}>
          <Icon size={17} aria-hidden="true"/><span className="remote-mode-copy"><strong>{name}</strong><span>{technology}</span></span>{badge && <span className="remote-mode-runtime" data-state={badge.state}><span aria-hidden="true"/>{badge.label}</span>}<input type="radio" name="remote-mode" value={id} checked={mode === id} onChange={() => { setMode(id); }} />
        </label>; })}
      </fieldset>
    </section>
    {mode === "quick_tunnel" ? <section className="remote-detail remote-quick-card" aria-label="快捷隧道连接控制台">
      <header className="remote-quick-header"><div><Zap size={18} aria-hidden="true"/><div><span className="remote-quick-eyebrow">QUICK TUNNEL CONSOLE</span><span className="remote-quick-kicker">Cloudflare Quick Tunnel 运行状态</span></div><span className="remote-quick-status" data-ready={state?.mode === "quick_tunnel" && state.status === "ready"}>{quickStatusBadge}</span></div><span>内核自动代理 · 零配置</span></header>
      <div className="remote-quick-facts">
        <div><span className="remote-quick-fact-heading"><span>认证方式</span><small>系统内置</small></span><strong><ShieldCheck size={15} aria-hidden="true"/>SerenaDesktop OAuth 2.0</strong><small>强制内置访问保护，远程 MCP 请求由本机 SerenaDesktop 授权。</small></div>
        <div><span className="remote-quick-fact-heading"><span>本地 MCP 目标</span><small>固定路由</small></span><code>http://127.0.0.1:{port}/mcp</code><small>Cloudflare Tunnel 流量直接转发至本机核心服务端口。</small></div>
      </div>
      <section className="remote-quick-resource" aria-labelledby="quick-endpoint-title"><div className="remote-quick-resource-heading"><div><span className="remote-quick-eyebrow">TEMPORARY PUBLIC ENDPOINT</span><label id="quick-endpoint-title" className="field-label" htmlFor="remote-url">临时公网 Endpoint</label></div><span className="remote-endpoint-status" data-ready={!!quickEndpoint}>{quickEndpoint ? "Endpoint 已就绪" : "尚未生成"}</span></div><div className="remote-address remote-public-address"><input id="remote-url" readOnly value={quickEndpoint ?? ""} placeholder="等待运行状态返回公网 Endpoint" aria-label="临时公网 Endpoint"/><Button className="remote-copy" data-copied={quickEndpointCopied} variant="outline" disabled={!quickEndpoint || copying || quickEndpointCopied} onClick={() => void copy(quickEndpoint)}><span className="remote-copy-icon" aria-hidden="true"><Copy size={16}/><Check size={16}/></span><span aria-live="polite">{quickEndpointCopied ? "已复制" : copying ? "正在复制…" : "复制地址"}</span></Button></div><span>{quickEndpoint ? `已授权客户端：${state?.authorizedClients ?? 0}` : "启动后会自动创建临时 HTTPS 地址。"}</span></section>
      <section className="remote-quick-lifecycle" aria-label="临时地址生命周期说明"><div><Info size={16} aria-hidden="true"/><strong>运行机制与生命周期说明</strong></div><p>无需域名、TLS 证书或 Cloudflare 账户。应用重启不会自动恢复 Quick Tunnel；临时地址重新创建后可能变化。{allowLan && "局域网客户端仍需要 OAuth 授权。"}</p></section>
      <footer className="remote-quick-footer"><div className="remote-quick-connection-state"><span><i data-ready={quickTunnelReady} aria-hidden="true"/>{state?.mode === "quick_tunnel" ? statusText : state ? "快捷隧道未运行" : "等待切换"}</span></div><div className="remote-actions">
        {quickTunnelInProgress ? <Button variant="destructive" disabled={!!busy} onClick={() => void stop()}><Square size={15} aria-hidden="true"/>{busy === "停止" ? "正在取消…" : "取消启动"}</Button>
          : quickTunnelReady ? <Button variant="destructive" disabled={!!busy} onClick={() => void stop()}><Square size={15} aria-hidden="true"/>{busy === "停止" ? "正在停止…" : "停止远程访问"}</Button>
            : quickTunnelRetained ? <Button variant="destructive" disabled={!!busy} onClick={() => void stop()}><Square size={15} aria-hidden="true"/>{busy === "停止" ? "正在停止并清理…" : "停止并清理"}</Button>
            : <Button disabled={!state || !!busy || quickTunnelStopping} onClick={() => void startQuickTunnel()}><ArrowRightLeft size={15} aria-hidden="true"/>{quickTunnelStopping ? "正在停止…" : busy === "开启" ? quickTunnelConfigured ? "正在启动…" : "正在切换…" : quickTunnelFailed ? "重新启动快捷隧道" : quickTunnelConfigured ? "启动快捷隧道" : "切换到快捷隧道"}</Button>}
      </div></footer>
    </section> : mode === "self_hosted_oauth" ? <section className="remote-detail" aria-label="自建接入连接控制台">
      {provider === "custom_https" ? <section className="custom-https-config-card" aria-label="自有 HTTPS 入口配置">
        <header className="custom-https-config-header">
          {selfHostedProviderSelector}
          <span>需用户自行管理域名、TLS 与反向代理</span>
        </header>
        <div className="custom-https-facts-grid">
          <section className="custom-https-auth" aria-label="认证方式"><header><div><ShieldCheck size={16} aria-hidden="true"/><span>认证方式</span><em>强制保护</em></div><span>系统内置</span></header><div><strong>SerenaDesktop OAuth 2.0</strong><span>内置启用</span></div><p>强制内置访问保护，未授权的公网 MCP 请求将被阻断。</p></section>
          <section className="custom-https-local-target" aria-label="本地 MCP 目标"><header><span>本地 MCP 目标 <small>(Local Target)</small></span><span>Serena Core 内部端口</span></header><div><code>http://127.0.0.1:{port}/mcp</code><span>固定路由</span></div><p>反向代理目标地址，核心服务监听于本地 {port} 端口。</p></section>
        </div>
        <section className="custom-https-origin" aria-labelledby="self-origin-title"><div><label id="self-origin-title" className="field-label" htmlFor="self-origin">公网 HTTPS 地址</label><span>{customOriginMeta}</span></div><div className="remote-address remote-public-address"><input id="self-origin" type="url" placeholder="https://mcp.example.com" value={customOrigin} disabled={customHttpsActive || !!busy} onChange={event => setOrigin(event.target.value)} aria-describedby="self-origin-help"/><Button className="remote-copy" data-copied={customOriginCopied} variant="outline" disabled={!customOrigin || copying || customOriginCopied} onClick={() => void copy(customOrigin)}><span className="remote-copy-icon" aria-hidden="true"><Copy size={16}/><Check size={16}/></span><span aria-live="polite">{customOriginCopied ? "已复制" : copying ? "正在复制…" : "复制"}</span></Button></div><p id="self-origin-help" className="helper">仅填写 HTTPS Origin，不包含 <code>/mcp</code>、<code>/oauth</code> 等路径。支持标准 HTTPS 或自定义端口（如 <code>:8443</code>）。</p></section>
        <section className="custom-https-proxy-contract" aria-labelledby="custom-proxy-contract-title"><header><strong id="custom-proxy-contract-title">反向代理要求 <small>(Reverse Proxy Contract)</small></strong></header><p>你的公网代理需要将以下路径转发至本地 Serena Desktop：</p><div><code>{customOrigin || "尚未配置公网 Origin"}</code><span aria-hidden="true">→</span><code>127.0.0.1:{port}</code><span className="custom-https-contract-paths"><code>/mcp</code><code>/.well-known/*</code><code>/oauth/*</code></span></div></section>
        <div className="custom-https-oauth-notice"><div><ShieldCheck size={16} aria-hidden="true"/><span>SerenaDesktop OAuth 2.0 · 远程 MCP 请求由本机 SerenaDesktop 强制授权，无需用户手动配置。{allowLan && " 局域网客户端仍需要 OAuth 授权。"}</span></div></div>
        <footer className="custom-https-config-footer"><div><Info size={15} aria-hidden="true"/><span>{customHttpsActive ? customHttpsReady ? `当前生效地址：${customOrigin || "尚未配置"}` : `${statusText}${customOrigin ? ` · 当前配置地址：${customOrigin}` : ""}` : persistedCustomOrigin ? `当前配置地址：${persistedCustomOrigin}` : "配置将在应用后生效"}</span></div><div className="custom-https-config-actions">
          {customHttpsActive ? <Button variant="destructive" disabled={!!busy} onClick={() => void stop()}><Square size={15} aria-hidden="true"/>{busy === "停止" ? "正在停止…" : "停止远程访问"}</Button>
            : <Button disabled={!state || !!busy} onClick={() => void startCustomHttps()}><ArrowRightLeft size={15} aria-hidden="true"/>{busy === "开启" ? customHttpsConfigured ? "正在启用…" : "正在切换…" : customHttpsConfigured ? "启用自有 HTTPS" : "切换到自有 HTTPS"}</Button>}
        </div></footer>
      </section> : <>
        <section className="ngrok-config-card" aria-label="ngrok 入口配置">
        <header className="ngrok-config-header">
          {selfHostedProviderSelector}
          <span>{ngrokReady ? "内置隧道客户端运行中" : selectedSelfHostedActive ? statusText : ngrokConfigured ? "当前未运行" : "等待切换"}</span>
        </header>
        <div className="ngrok-config-grid">
          <section className="self-hosted-credentials" aria-labelledby="ngrok-credentials-title">
            <div className="self-hosted-credential-heading"><div className="ngrok-token-heading-group"><span id="ngrok-credentials-title">ngrok Auth Token</span><span>{state?.ngrokAuthConfigured ? "已保存" : "尚未保存"}</span></div></div>
            <div className="ngrok-token-row">
              <div className="remote-address"><input id="ngrok-auth-token" type="password" autoComplete="off" placeholder={state?.ngrokAuthConfigured ? "已保存，留空继续使用" : "输入 ngrok Auth Token"} value={ngrokToken} disabled={!!busy} onChange={event => setNgrokToken(event.target.value)} aria-describedby="ngrok-auth-token-help" /></div>
              <div className="self-hosted-credential-actions" aria-label="ngrok Token 操作">
                <Button className="ngrok-token-action" variant="outline" disabled={!!busy || !ngrokToken.trim()} onClick={() => void saveNgrokToken()}><Check size={15} aria-hidden="true"/>{busy === "保存 ngrok 凭据" ? "正在保存…" : state?.ngrokAuthConfigured ? "更新 Token" : "保存 Token"}</Button>
              </div>
            </div>
            <p id="ngrok-auth-token-help" className="helper">{state?.ngrokAuthConfigured ? "输入新 Token 后可更新本机保存的值。" : "Token 只会提交给 SerenaDesktop 并保存在本机。"}</p>
          </section>
          <section className="ngrok-local-target" aria-label="本地 MCP 目标"><header><span>本地 MCP 目标 <small>(Local Target)</small></span><span>Serena Core 内部端口</span></header><div><code>http://127.0.0.1:{port}/mcp</code><span>自动路由</span></div><small>ngrok 流量由 SerenaDesktop 转发到本机 MCP 服务。</small></section>
        </div>
        {!ngrokReady && <section className="ngrok-config-endpoint" aria-labelledby="ngrok-config-endpoint-title"><div><label id="ngrok-config-endpoint-title" className="field-label" htmlFor="ngrok-mcp-url">公网 MCP Endpoint <small>(Public Endpoint)</small></label><span data-ready="false">尚未分配</span></div><div className="remote-address remote-public-address"><input id="ngrok-mcp-url" readOnly value="" placeholder="切换并连接后生成" aria-label="ngrok Public Endpoint"/><Button className="remote-copy" data-copied="false" variant="outline" disabled><span className="remote-copy-icon" aria-hidden="true"><Copy size={16}/><Check size={16}/></span><span>复制</span></Button></div></section>}
        <div className="ngrok-oauth-notice"><div><ShieldCheck size={16} aria-hidden="true"/><span>OAuth 2.0 已启用 · 远程 MCP 请求需要经过 SerenaDesktop OAuth 授权。{allowLan && " 局域网客户端仍需要 OAuth 授权。"}</span></div></div>
        {!ngrokReady && <footer className="ngrok-config-footer"><div><Info size={15} aria-hidden="true"/><span>修改配置后需重新生效隧道。</span></div>
        <div className="ngrok-config-footer-actions">
          {selectedSelfHostedActive ? <Button variant="destructive" disabled={!!busy} onClick={() => void stop()}><Square size={15} aria-hidden="true"/>{busy === "停止" ? "正在停止…" : "停止远程访问"}</Button>
            : <Button disabled={!state || !!busy || (!state.ngrokAuthConfigured && !ngrokToken.trim())} onClick={() => void startNgrok()}><ArrowRightLeft size={15} aria-hidden="true"/>{busy === "开启" ? ngrokConfigured ? ngrokToken.trim() ? state?.ngrokAuthConfigured ? "正在更新并启动…" : "正在保存并启动…" : "正在启动…" : "正在切换…" : ngrokConfigured ? state?.ngrokAuthConfigured ? ngrokToken.trim() ? "更新并启动 ngrok" : "启动 ngrok" : "保存并启动 ngrok" : "切换到 ngrok"}</Button>}
        </div></footer>}
        </section>
        {ngrokReady && <section className="ngrok-result-card" aria-label="ngrok 已连接结果">
          <div className="ngrok-connected-top-row">
            <div className="ngrok-connected-endpoint">
              <div className="ngrok-connected-status"><i aria-hidden="true"/><strong>已连接 (Connected)</strong></div>
              <div className="ngrok-connected-resource"><span>公网 MCP Endpoint:</span><code>{ngrokEndpoint}</code></div>
              <div className="ngrok-connected-copy-actions">
                <Button className="remote-copy" data-copied={ngrokEndpointCopied} variant="outline" disabled={!ngrokEndpoint || copying || ngrokEndpointCopied} onClick={() => void copy(ngrokEndpoint)}><span className="remote-copy-icon" aria-hidden="true"><Copy size={16}/><Check size={16}/></span><span aria-live="polite">{ngrokEndpointCopied ? "已复制" : copying ? "正在复制…" : "复制 Endpoint"}</span></Button>
                <Button className="remote-copy" data-copied={ngrokClientConfigCopied} variant="outline" disabled={!ngrokClientConfig || copying || ngrokClientConfigCopied} onClick={() => void copy(ngrokClientConfig)}><span className="remote-copy-icon" aria-hidden="true"><Copy size={16}/><Check size={16}/></span><span aria-live="polite">{ngrokClientConfigCopied ? "已复制" : copying ? "正在复制…" : "复制客户端配置"}</span></Button>
              </div>
            </div>
            <div className="ngrok-connected-actions">
              <Button variant="outline" disabled={!!busy} onClick={() => void reconnectNgrok()}><RefreshCw size={15} className={busy === "重新连接" ? "animate-spin" : ""} aria-hidden="true"/>{busy === "重新连接" ? "正在重新连接…" : "重新连接"}</Button>
              <Button variant="destructive" disabled={!!busy} onClick={() => void stop()}><Square size={15} aria-hidden="true"/>{busy === "停止" ? "正在停止…" : "停止隧道"}</Button>
            </div>
          </div>
          <dl className="ngrok-connected-metadata">
            <div><dt>访问保护状态</dt><dd>OAuth 2.0 已启用 · 强制校验</dd></div>
            <div><dt>本地服务端口</dt><dd><code>127.0.0.1:{port}</code></dd></div>
            <div><dt>已授权客户端</dt><dd>{state.authorizedClients ?? 0}</dd></div>
          </dl>
        </section>}
      </>}
    </section> : <section className="remote-detail" aria-label="仅 MCP 连接控制台">
      <section className="mcp-only-card">
        <header className="mcp-only-card-header"><div><Cable size={18} aria-hidden="true"/><strong>仅 MCP（纯本地）运行状态</strong></div><span>仅本地回环 · 无公网隧道 · 零附加网络依赖</span></header>
        <section className="mcp-only-local-target" aria-labelledby="mcp-local-target-title">
          <header><div><Monitor size={15} aria-hidden="true"/><span id="mcp-local-target-title" className="mcp-only-local-target-title">本地 MCP 目标 <small>(Local Target)</small></span><span className="mcp-only-local-target-route">固定路由</span></div><code>127.0.0.1:{port}</code></header>
          <div className="remote-address remote-public-address"><input readOnly value={localMcpEndpoint} aria-label="本地 MCP Endpoint"/><Button className="remote-copy" data-copied={copiedResource === localMcpEndpoint} variant="outline" disabled={copying || copiedResource === localMcpEndpoint} onClick={() => void copy(localMcpEndpoint)}><span className="remote-copy-icon" aria-hidden="true"><Copy size={16}/><Check size={16}/></span><span aria-live="polite">{copiedResource === localMcpEndpoint ? "已复制" : copying ? "正在复制…" : "复制 Endpoint"}</span></Button></div>
          <p className="helper">Serena Desktop 仅在本地监听并提供 MCP Broker 协议转发，不创建公网 Tunnel，不签发 TLS。</p>
        </section>
        <fieldset disabled={!!busy} className="mcp-only-protection">
          <legend className="sr-only">访问保护声明</legend>
          <header className="mcp-only-section-heading"><div><Shield size={16} aria-hidden="true"/><strong>访问保护声明 <small>(Access Protection)</small></strong></div><span>Serena Desktop 不启用内置 OAuth 2.0，认证责任取决于你的网络架构</span></header>
          <div className="mcp-only-protection-options">
            <label className="mcp-only-protection-choice" data-selected={declaration === "external_auth"}>
              <input type="radio" name="mcp-security" checked={declaration === "external_auth"} onChange={() => { setDeclaration("external_auth"); setRiskAccepted(false); }} />
              <div className="mcp-only-protection-choice-copy"><div><strong>我的网关已经负责认证</strong>{declaration === "external_auth" && <span>当前选择</span>}</div><p>Serena Desktop 信任外部网关负责身份认证，不启用 SerenaDesktop OAuth，且不验证其真实性。</p></div><span className="mcp-only-protection-tag">外部网关负责</span>
            </label>
            <label className="mcp-only-protection-choice" data-selected={declaration === "none"}>
              <input type="radio" name="mcp-security" checked={declaration === "none"} onChange={() => { setDeclaration("none"); setRiskAccepted(false); }} />
              <div className="mcp-only-protection-choice-copy"><div><strong>不使用认证</strong><span data-risk="true">仅本地环境推荐</span></div><p>无身份认证直接暴露 MCP。如果通过公网反代直接暴露，任何能访问该地址的客户端都可调用工具。</p></div><span className="mcp-only-protection-tag" data-risk="true">未受保护</span>
            </label>
          </div>
        </fieldset>
        {declaration === "external_auth" ? <section className="mcp-only-gateway-origin" aria-labelledby="mcp-gateway-origin-title">
          <header><div><Globe size={15} aria-hidden="true"/><strong id="mcp-gateway-origin-title">网关公网地址 <small>(Gateway Public Origin)</small></strong><span>可选 (Optional)</span></div><span>Origin Allowlist</span></header>
          <div className="remote-address"><input id="mcp-only-origin" type="url" value={onlyOrigin} placeholder="https://mcp.example.com" aria-describedby="mcp-origin-help" onChange={event => setOnlyOrigin(event.target.value)} /></div>
          <p id="mcp-origin-help" className="helper">仅填写 HTTPS Origin，不包含 <code>/mcp</code> 等路径。用于识别允许的公网 Host / Origin allowlist，并辅助端点呈现；填写此地址不代表 Serena Desktop 已验证外部认证。</p>
          <details className="mcp-proxy-help"><summary>什么时候需要填写？<ChevronDown size={14} aria-hidden="true" /></summary><p>公网反代保留原始 Host 时建议填写；网关已重写 Host 或本地直连可留空。</p></details>
        </section> : <section className="mcp-risk-confirmation" aria-label="无认证访问风险确认"><strong>公开到公网前，请确认访问风险</strong><label><input type="checkbox" checked={riskAccepted} onChange={event => setRiskAccepted(event.target.checked)} /><span>我理解如果该 MCP 被暴露到公网，任何能访问 Endpoint 的客户端都可能调用公开工具，包括 Agent。</span></label></section>}
        <footer className="mcp-only-footer"><div><Info size={15} aria-hidden="true"/><span aria-live="polite">{onlyApplied ? mcpRunning === true ? "当前配置已与运行时一致 · 正在监听" : mcpRunning === false ? "当前配置已保存 · 本地接入已停止" : "正在读取本地 MCP 接入状态" : declaration === "none" && !riskAccepted ? "确认访问风险后可应用" : state?.mode === "mcp_only" ? "更改将在保存后生效" : "应用后将切换为仅 MCP（纯本地）"}</span></div>{onlyApplied ? <div className="mcp-only-footer-actions">{mcpRunning === true ? <Button variant="destructive" disabled={mcpBusy} onClick={() => onSetMcpRunning(false)}><Square size={15} aria-hidden="true"/>{mcpBusy ? "正在停止…" : "停止接入"}</Button> : mcpRunning === false ? <Button disabled={mcpBusy} onClick={() => onSetMcpRunning(true)}><Zap size={15} aria-hidden="true"/>{mcpBusy ? "正在启动…" : "启动接入"}</Button> : <Button disabled>状态读取中</Button>}</div> : <div className="mcp-only-footer-actions"><Button disabled={!state || !!busy || (declaration === "none" && !riskAccepted)} onClick={() => void operate(state?.mode !== "mcp_only" ? "切换仅 MCP" : "保存 MCP 设置", () => api.remoteStart("mcp_only", declaration === "external_auth" ? onlyOrigin.trim() || undefined : undefined, declaration, riskAccepted))}>{busy === "切换仅 MCP" ? "正在切换…" : busy === "保存 MCP 设置" ? "正在保存…" : state?.mode !== "mcp_only" ? "切换为仅 MCP" : "保存设置"}</Button></div>}</footer>
      </section>
    </section>}
  </section>;
}
