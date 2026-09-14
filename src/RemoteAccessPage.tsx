import { useEffect, useRef, useState } from "react";
import { ArrowRightLeft, Cable, Check, Cloud, Copy, FileText, Info, RefreshCw, ShieldCheck, Shield, ShieldOff, ChevronDown, Globe, Square, Zap } from "lucide-react";
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

export default function RemoteAccessPage({ controller, port, allowLan, onSettings }: { controller: RemoteController; port: number; allowLan: boolean; onSettings: () => void }) {
  const { state, error, busy, operate, startQuickTunnel, quickTunnelCommandPending, quickTunnelHandledError } = controller;
  const [probeResult, setProbeResult] = useState<"success" | "error" | null>(null);
  const errorBaseline = useRef<{ reportedError: string; mode: RemoteAccessMode | null; status: RemoteState["status"] | null } | null>(null);
  const lastErrorToast = useRef<{ key: string; at: number } | null>(null);
  const probeErrorQuietUntil = useRef(0);
  function showErrorToast(key: string, message: string) {
    const now = Date.now();
    if (lastErrorToast.current?.key === key && now - lastErrorToast.current.at < 1_500) return;
    lastErrorToast.current = { key, at: now };
    toast.error(message);
  }
  async function probe() {
    if (mode === "mcp_only") return;
    setProbeResult(null);
    probeErrorQuietUntil.current = Date.now() + 1_500;
    if (await operate("测试连接", api.remoteProbe)) {
      probeErrorQuietUntil.current = 0;
      setProbeResult("success");
      toast.success("连接测试成功");
    } else {
      setProbeResult("error");
      showErrorToast(`probe:${Date.now()}`, "连接测试失败，请查看远程状态中的错误详情");
    }
  }
  async function stop() {
    setProbeResult(null);
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
  const resource = state?.status === "ready" && state.mode === mode && (mode !== "self_hosted_oauth" || runtimeProvider === provider) ? state.publicContext?.mcpResource : null;
  const copied = !!resource && copiedResource === resource;
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
    if (reportedError && reportedError !== previous.reportedError && Date.now() >= probeErrorQuietUntil.current) {
      showErrorToast(`remote-error:${reportedError}`, "远程访问操作失败，请稍后重试。");
    }
    errorBaseline.current = current;
  }, [reportedError, quickTunnelCommandPending, quickTunnelHandledError, state?.mode, state?.status, state?.lastError]);
  useEffect(() => {
    setProbeResult(null);
  }, [mode, provider]);
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
  const runtimeMode = state?.mode ? modes.find(({ id }) => id === state.mode) : null;
  const runtimeLabel = state?.mode === "self_hosted_oauth" && runtimeProvider === "ngrok" ? "自建接入 · ngrok" : runtimeMode?.name;
  const runtimeEndpoint = state?.status === "ready" ? state.publicContext?.mcpResource ?? null : null;
  const quickEndpoint = state?.mode === "quick_tunnel" ? runtimeEndpoint : null;
  const quickStatusBadge = state?.mode === "quick_tunnel" ? state.status === "ready" ? "ACTIVE · 已连接" : statusText : "等待切换";
  const endpointCopied = !!runtimeEndpoint && copiedResource === runtimeEndpoint;
  const quickEndpointCopied = !!quickEndpoint && copiedResource === quickEndpoint;
  const ngrokReady = state?.mode === "self_hosted_oauth" && runtimeProvider === "ngrok" && state.status === "ready";
  const ngrokEndpoint = ngrokReady ? state.publicContext?.mcpResource ?? null : null;
  const ngrokEndpointCopied = !!ngrokEndpoint && copiedResource === ngrokEndpoint;
  const canProbe = mode !== "mcp_only" && !!state?.active && (state.status === "ready" || state.status === "error");
  const runtimeLifecycle = state?.mode === "quick_tunnel" ? "临时地址" : state?.mode === "self_hosted_oauth" ? "自有公网入口" : "本地 / 外部网关";
  const runtimeDescription = state?.mode === "quick_tunnel"
    ? "由应用管理的临时 HTTPS MCP 入口，重启后需要重新创建。"
    : state?.mode === "self_hosted_oauth"
      ? "使用已配置的公网入口，SerenaDesktop 继续负责 OAuth 访问保护。"
      : "保留本地 MCP 服务；公网暴露与外部认证由你的网关负责。";
  const protectionStatus = !state ? "状态未读取" : state.mode === "mcp_only"
    ? state.config?.mcpOnly.securityDeclaration === "none" ? "未启用认证" : "外部认证声明（未验证）"
    : "OAuth 2.0 · 已启用";
  const remoteDiagnostics = (items: readonly [string, string][]) => [
    { title: "整体远程检测", detail: "remoteProbe 仅返回整体成功或失败", status: probeResult === "success" ? "整体检测通过" : probeResult === "error" ? "整体检测失败" : "尚未执行", result: probeResult },
    ...items.map(([title, detail]) => ({ title, detail, status: "后端未提供逐项结果", result: null })),
  ];
  const isNgrok = mode === "self_hosted_oauth" && provider === "ngrok";
  const diagnostics = mode === "mcp_only"
    ? [
        { title: "本地 Endpoint", detail: `http://127.0.0.1:${port}/mcp`, status: "前端已知", result: null },
        { title: "本地 MCP 模式", detail: declaration === "external_auth" ? "外部认证声明" : "未使用认证", status: state?.mode === "mcp_only" ? "当前配置" : "正在查看", result: null },
        { title: "公网 OAuth 诊断", detail: "仅 MCP 模式不适用", status: "不适用", result: null },
      ]
    : isNgrok
      ? remoteDiagnostics([["DNS", "ngrok 公网入口；后端未提供逐项结果"], ["TLS", "ngrok HTTPS；后端未提供逐项结果"], ["公网 Endpoint", "后端未提供逐项结果"], ["访问保护", "后端未提供逐项结果"], ["OAuth/授权", "后端未提供逐项结果"], ["MCP Protocol Initialize", "后端未提供逐项结果"]])
      : remoteDiagnostics([["OAuth Metadata", "后端未提供逐项结果"], ["未授权访问保护", "后端未提供逐项结果"], ["Protected Resource", "后端未提供逐项结果"], ["MCP Initialize", "后端未提供逐项结果"], ["Tools List", "后端未提供逐项结果"]]);
  const diagnosticsTitle = mode === "mcp_only" ? "本地服务诊断" : isNgrok ? "ngrok 公网诊断" : mode === "quick_tunnel" ? "Quick Tunnel OAuth 公网诊断" : "自有 HTTPS OAuth 公网诊断";
  const diagnosticsDescription = mode === "mcp_only"
    ? "仅显示前端可确认的本地 Endpoint 与本地 MCP 模式；公网 OAuth 诊断不适用。"
    : probeResult
      ? "remoteProbe 只提供整体检测结果；其余项目的逐项结果后端未提供。"
      : "尚未执行。当前后端只提供整体 remoteProbe，不提供逐项结构化结果。";
  function showDiagnostics() {
    document.getElementById("remote-diagnostics")?.scrollIntoView?.({ behavior: "smooth", block: "start" });
  }
  const selfHostedProviderSelector = <fieldset disabled={!!busy} className="self-hosted-providers">
    <legend className="sr-only">选择公网入口方式</legend>
    <span className="self-hosted-provider-label">接入方式选择：</span>
    <div className="self-hosted-provider-options">
      <label className="self-hosted-provider-choice" data-selected={provider === "custom_https"}>
        <Globe size={16} aria-hidden="true"/><span>自有 HTTPS</span>{state?.mode === "self_hosted_oauth" && runtimeProvider === "custom_https" && <span className="self-hosted-provider-runtime" data-active={active}><span aria-hidden="true"/>{active ? state.status === "ready" ? "当前运行" : "当前生效" : "当前配置"}</span>}<input type="radio" name="self-hosted-provider" value="custom_https" checked={provider === "custom_https"} onChange={() => { setProvider("custom_https"); setProbeResult(null); }} />
      </label>
      <label className="self-hosted-provider-choice" data-selected={provider === "ngrok"}>
        <Cloud size={16} aria-hidden="true"/><span>ngrok</span>{state?.mode === "self_hosted_oauth" && runtimeProvider === "ngrok" && <span className="self-hosted-provider-runtime" data-active={active}><span aria-hidden="true"/>{active ? state.status === "ready" ? "当前运行" : "当前生效" : "当前配置"}</span>}<input type="radio" name="self-hosted-provider" value="ngrok" checked={provider === "ngrok"} onChange={() => { setProvider("ngrok"); setProbeResult(null); }} />
      </label>
    </div>
  </fieldset>;
  return <section className="remote-page">
    <div className="page-heading"><div><div className="remote-title"><h1>远程访问</h1><span>REMOTE MCP ACCESS</span></div><p>配置远程 MCP 接入方式，生成供 ChatGPT、Claude 等客户端连接的公网或本地 Endpoint。</p></div><div className="remote-page-actions"><Button variant="outline" onClick={showDiagnostics}><FileText size={15} aria-hidden="true"/>网络诊断报告</Button>{mode !== "mcp_only" && <Button disabled={!canProbe || !!busy} onClick={() => void probe()}><RefreshCw size={15} className={busy === "测试连接" ? "animate-spin" : ""} aria-hidden="true"/>{busy === "测试连接" ? "正在测试…" : "重新检测全部"}</Button>}</div></div>
    <section className="remote-runtime-summary" aria-label="当前远程访问运行状态">
      <div className="remote-summary-main">
        <div className="remote-summary-title"><span className="remote-status-indicator" data-ready={state?.status === "ready"} aria-hidden="true"/><strong>{runtimeLabel ? `${runtimeLabel}（当前生效）` : "正在读取远程状态"}</strong><span className="remote-summary-product">{state?.mode === "self_hosted_oauth" && runtimeProvider === "ngrok" ? "ngrok" : runtimeMode?.technology ?? "REMOTE ACCESS"}</span><span className="remote-summary-lifecycle">{state ? runtimeLifecycle : "状态未知"}</span></div>
        <p>{state ? runtimeDescription : "等待后端返回当前运行模式与访问状态。"}</p>
        <div className="remote-summary-endpoint"><span>PUBLIC ENDPOINT</span><code>{runtimeEndpoint ?? "—  尚未就绪"}</code><Button className="remote-copy" data-copied={endpointCopied} variant="outline" disabled={!runtimeEndpoint || copying || endpointCopied} onClick={() => void copy(runtimeEndpoint)}><span className="remote-copy-icon" aria-hidden="true"><Copy size={16}/><Check size={16}/></span><span aria-live="polite">{endpointCopied ? "已复制" : copying ? "正在复制…" : "复制"}</span></Button></div>
      </div>
      <dl className="remote-summary-metrics"><div data-ready={!!state && state.mode !== "mcp_only"}><dt>访问保护:</dt><dd>{protectionStatus}</dd></div><div data-ready={state?.status === "ready"}><dt>公网状态:</dt><dd>{state?.status === "ready" ? "已验证" : state ? statusText : "未知"}</dd></div></dl>
    </section>
    <section className="remote-mode-section" aria-labelledby="remote-mode-heading">
      <div className="remote-section-heading"><div><h2 id="remote-mode-heading">连接方式</h2><span>CONNECTION MODE</span></div><p>选定后需点击下方操作变更当前运行模式</p></div>
      <fieldset className="remote-modes" disabled={!!busy}>
        <legend className="sr-only">选择连接方式</legend>
        {modes.map(({ id, name, technology, icon: Icon }) => { const badge = runtimeBadge(id); return <label key={id} className="remote-mode" data-selected={mode === id}>
          <Icon size={17} aria-hidden="true"/><span className="remote-mode-copy"><strong>{name}</strong><span>{technology}</span></span>{badge && <span className="remote-mode-runtime" data-state={badge.state}><span aria-hidden="true"/>{badge.label}</span>}<input type="radio" name="remote-mode" value={id} checked={mode === id} onChange={() => { setMode(id); setProbeResult(null); }} />
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
          : quickTunnelReady ? <>
            {canProbe && <Button variant="outline" disabled={!!busy} onClick={() => void probe()}><RefreshCw size={15} className={busy === "测试连接" ? "animate-spin" : ""} aria-hidden="true"/>{busy === "测试连接" ? "正在测试…" : "测试连接"}</Button>}
            <Button variant="destructive" disabled={!!busy} onClick={() => void stop()}><Square size={15} aria-hidden="true"/>{busy === "停止" ? "正在停止…" : "停止远程访问"}</Button>
          </> : quickTunnelRetained ? <Button variant="destructive" disabled={!!busy} onClick={() => void stop()}><Square size={15} aria-hidden="true"/>{busy === "停止" ? "正在停止并清理…" : "停止并清理"}</Button>
            : <Button disabled={!state || !!busy || quickTunnelStopping} onClick={() => void startQuickTunnel()}><ArrowRightLeft size={15} aria-hidden="true"/>{quickTunnelStopping ? "正在停止…" : busy === "开启" ? quickTunnelConfigured ? "正在启动…" : "正在切换…" : quickTunnelFailed ? "重新启动快捷隧道" : quickTunnelConfigured ? "启动快捷隧道" : "切换到快捷隧道"}</Button>}
      </div></footer>
    </section> : mode === "self_hosted_oauth" ? <section className="remote-detail" aria-label="自建接入连接控制台">
      {provider === "custom_https" ? <>
        {selfHostedProviderSelector}
        <dl className="remote-facts self-hosted-facts"><div><dt>认证</dt><dd><ShieldCheck size={15} aria-hidden="true"/>SerenaDesktop OAuth</dd></div><div><dt>本地 MCP 目标</dt><dd><code>http://127.0.0.1:{port}/mcp</code></dd></div></dl>
        <p className="self-hosted-proxy">代理必须转发 <code>/mcp</code>、<code>/.well-known/*</code> 和 <code>/oauth/*</code>。</p>
        <label className="field-label" htmlFor="self-origin">公网 HTTPS 地址</label>
        <div className="remote-address"><input id="self-origin" type="url" placeholder="https://mcp.example.com" value={selectedSelfHostedActive && state.publicContext ? state.publicContext.publicOrigin : origin} disabled={selectedSelfHostedActive || !!busy} onChange={event => setOrigin(event.target.value)} aria-describedby="self-origin-help" /></div>
        <p id="self-origin-help" className="helper">仅填写 HTTPS origin，不含 <code>/mcp</code> 等路径。</p>
        {resource && <>
          <label className="field-label" htmlFor="self-mcp-url">公网 MCP 地址</label>
          <div className="remote-address remote-public-address"><input id="self-mcp-url" readOnly value={resource}/><Button variant="outline" disabled={copying || copied} onClick={() => void copy()}>{copied ? "已复制" : copying ? "正在复制…" : "复制地址"}</Button></div>
          <dl className="remote-facts self-hosted-resource-facts"><div><dt>已授权客户端</dt><dd>{state?.authorizedClients ?? 0}</dd></div></dl>
        </>}
        <p className="remote-notice">停止会撤销 SerenaDesktop OAuth，外部公网代理仍由你管理。切到「仅 MCP」会移除本机 OAuth 保护，请确保外部认证；重启时同一公网地址的未过期授权可继续恢复。{allowLan && "局域网客户端仍需要 OAuth 授权。"}</p>
        <div className="remote-actions">
          {selectedSelfHostedActive ? <>
            {state?.mode === mode && state.status !== "stopping" && state.status !== "verifying" && <Button disabled={!!busy} onClick={() => void probe()}>{busy === "测试连接" ? "正在测试…" : "测试连接"}</Button>}
            <Button variant="destructive" disabled={!!busy} onClick={() => void stop()}>停止远程访问</Button>
          </> : <Button disabled={!state || !!busy || !origin.trim()} onClick={() => void operate("开启", () => api.remoteStart("self_hosted_oauth", origin.trim()))}>{busy === "开启" ? customHttpsConfigured ? "正在启用…" : "正在切换…" : customHttpsConfigured ? "启用自有 HTTPS" : "切换到自有 HTTPS"}</Button>}
        </div>
      </> : <>
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
        <section className="ngrok-config-endpoint" aria-labelledby="ngrok-config-endpoint-title"><div><label id="ngrok-config-endpoint-title" className="field-label" htmlFor="ngrok-mcp-url">公网 MCP Endpoint <small>(Public Endpoint)</small></label><span data-ready={!!ngrokEndpoint}>{ngrokEndpoint ? "已分配" : "尚未分配"}</span></div><div className="remote-address remote-public-address"><input id="ngrok-mcp-url" readOnly value={ngrokEndpoint ?? ""} placeholder="切换并连接后生成" aria-label="ngrok Public Endpoint"/><Button className="remote-copy" data-copied={ngrokEndpointCopied} variant="outline" disabled={!ngrokEndpoint || copying || ngrokEndpointCopied} onClick={() => void copy(ngrokEndpoint)}><span className="remote-copy-icon" aria-hidden="true"><Copy size={16}/><Check size={16}/></span><span aria-live="polite">{ngrokEndpointCopied ? "已复制" : copying ? "正在复制…" : "复制"}</span></Button></div></section>
        <div className="ngrok-oauth-notice"><div><ShieldCheck size={16} aria-hidden="true"/><span>OAuth 2.0 已启用 · 远程 MCP 请求需要经过 SerenaDesktop OAuth 授权。{allowLan && " 局域网客户端仍需要 OAuth 授权。"}</span></div><button type="button" onClick={showDiagnostics}>查看授权说明</button></div>
        <footer className="ngrok-config-footer"><div><Info size={15} aria-hidden="true"/><span>{ngrokReady ? "当前配置已生效；修改配置后需重新生效隧道。" : "修改配置后需重新生效隧道。"}</span></div>
        {!ngrokReady && <div className="ngrok-config-footer-actions">
          {selectedSelfHostedActive ? <Button variant="destructive" disabled={!!busy} onClick={() => void stop()}><Square size={15} aria-hidden="true"/>{busy === "停止" ? "正在停止…" : "停止远程访问"}</Button>
            : <Button disabled={!state || !!busy || (!state.ngrokAuthConfigured && !ngrokToken.trim())} onClick={() => void startNgrok()}><ArrowRightLeft size={15} aria-hidden="true"/>{busy === "开启" ? ngrokConfigured ? ngrokToken.trim() ? state?.ngrokAuthConfigured ? "正在更新并启动…" : "正在保存并启动…" : "正在启动…" : "正在切换…" : ngrokConfigured ? state?.ngrokAuthConfigured ? ngrokToken.trim() ? "更新并启动 ngrok" : "启动 ngrok" : "保存并启动 ngrok" : "切换到 ngrok"}</Button>}
        </div>}</footer>
        </section>
        {ngrokReady && <section className="ngrok-result-card" aria-label="ngrok 已连接结果">
          <header className="ngrok-result-heading"><div><strong>已连接 (Connected)</strong><span>ngrok 公网入口已就绪</span></div><span><i aria-hidden="true"/>当前运行</span></header>
          <footer className="ngrok-runtime-footer"><div className="remote-actions">
            <Button variant="outline" disabled={!!busy} onClick={() => void probe()}><RefreshCw size={15} className={busy === "测试连接" ? "animate-spin" : ""} aria-hidden="true"/>{busy === "测试连接" ? "正在测试…" : "测试连接"}</Button>
            <Button variant="destructive" disabled={!!busy} onClick={() => void stop()}><Square size={15} aria-hidden="true"/>{busy === "停止" ? "正在停止…" : "停止远程访问"}</Button>
          </div></footer>
        </section>}
      </>}
    </section> : <section className="remote-detail" aria-label="仅 MCP 连接控制台">
      <div className="mcp-only-settings">
        <div className="mcp-local-service">
          <div><span className="mcp-section-label">本机 MCP Endpoint</span><code>http://127.0.0.1:{port}/mcp</code></div>
          <Button variant="outline" onClick={onSettings}>管理 MCP 服务</Button>
        </div>
        <fieldset disabled={!!busy} className="mcp-only-protection">
          <legend>访问保护</legend>
          <div className="mcp-only-protection-options">
            <label className="mcp-only-protection-choice" data-selected={declaration === "external_auth"}>
              <Shield size={16} aria-hidden="true"/><span>我的网关已经负责认证</span><input type="radio" name="mcp-security" checked={declaration === "external_auth"} onChange={() => { setDeclaration("external_auth"); setRiskAccepted(false); }} />
            </label>
            <label className="mcp-only-protection-choice" data-selected={declaration === "none"}>
              <ShieldOff size={16} aria-hidden="true"/><span>不使用认证</span><input type="radio" name="mcp-security" checked={declaration === "none"} onChange={() => { setDeclaration("none"); setRiskAccepted(false); }} />
            </label>
          </div>
        </fieldset>
        <div className="mcp-only-config">
          {declaration === "external_auth" ? <>
            <p className="mcp-only-declaration">这是用户声明；SerenaDesktop 不验证外部网关的认证配置，也不启用 SerenaDesktop OAuth。</p>
            <label className="mcp-origin-label" htmlFor="mcp-only-origin">网关公网地址 <span>可选</span></label>
            <div className="remote-address"><input id="mcp-only-origin" type="url" value={onlyOrigin} placeholder="https://mcp.example.com" aria-describedby="mcp-origin-help" onChange={event => setOnlyOrigin(event.target.value)} /></div>
            <p id="mcp-origin-help" className="helper">填写 HTTPS Origin，不含 <code>/mcp</code>；地址只用于 Host/Origin allowlist / Endpoint 诊断，不表示认证已验证。</p>
            <details className="mcp-proxy-help">
              <summary>什么时候需要填写？<ChevronDown size={14} aria-hidden="true" /></summary>
              <p>代理保留公网 Host 时填写；留空时，代理需将 Host 改写为 Broker 本机 Host。</p>
            </details>
            {state?.mode === "mcp_only" && state.config?.mcpOnly.securityDeclaration === "external_auth" && appliedOnlyOrigin && <div className="mcp-saved-endpoint"><span>已保存的网关地址 · 未验证外部认证</span><code>{appliedOnlyOrigin}/mcp</code></div>}
          </> : <div className="mcp-risk-confirmation">
            <strong>公开到公网前，请确认访问风险</strong>
            <label><input type="checkbox" checked={riskAccepted} onChange={event => setRiskAccepted(event.target.checked)} /><span>我理解如果该 MCP 被暴露到公网，任何能访问 Endpoint 的客户端都可能调用公开工具，包括 Agent。</span></label>
          </div>}
        </div>
        <div className="mcp-only-footer">
          {!onlyApplied && <p className="helper" aria-live="polite">{declaration === "none" && !riskAccepted ? "确认访问风险后可保存。" : "更改将在保存后生效。"}</p>}
          <Button data-applied={onlyApplied ? "true" : undefined} disabled={!state || !!busy || onlyApplied || (declaration === "none" && !riskAccepted)} onClick={() => void operate(state?.mode !== "mcp_only" ? "切换仅 MCP" : "保存 MCP 设置", () => api.remoteStart("mcp_only", declaration === "external_auth" ? onlyOrigin.trim() || undefined : undefined, declaration, riskAccepted))}>{busy === "切换仅 MCP" ? "正在切换…" : busy === "保存 MCP 设置" ? "正在保存…" : onlyApplied ? "已应用" : state?.mode !== "mcp_only" ? "切换为仅 MCP" : "保存设置"}</Button>
        </div>
        {state?.mode && state.mode !== "mcp_only" && <p className="remote-notice">切换为仅 MCP 会移除 SerenaDesktop OAuth 保护；如经公网访问，请先确保外部认证网关已就绪（或明确接受无认证风险）。</p>}
      </div>
    </section>}
    <section id="remote-diagnostics" className="remote-diagnostics" aria-labelledby="remote-diagnostics-title">
      <header><div><span>{mode === "mcp_only" ? "LOCAL SERVICE DIAGNOSTICS" : "NETWORK DIAGNOSTICS"}</span><h2 id="remote-diagnostics-title">{diagnosticsTitle}</h2><p>{diagnosticsDescription}</p></div>{mode !== "mcp_only" && <Button variant="outline" disabled={!canProbe || !!busy} onClick={() => void probe()}><RefreshCw size={15} className={busy === "测试连接" ? "animate-spin" : ""} aria-hidden="true"/>{busy === "测试连接" ? "正在测试…" : "重新执行"}</Button>}</header>
      <div className="remote-diagnostic-list">{diagnostics.map(({ title, detail, status, result }) => <div key={title} className="remote-diagnostic-item" data-result={result ?? "unknown"}><div><strong>{title}</strong><span>{detail}</span></div><span>{status}</span></div>)}</div>
    </section>
  </section>;
}
