import { useEffect, useState } from "react";
import { Cable, Check, Cloud, Copy, ShieldCheck, Shield, ShieldOff, ChevronDown, Globe } from "lucide-react";
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
  { id: "quick_tunnel", name: "快捷隧道", icon: Cloud },
  { id: "self_hosted_oauth", name: "自建接入", icon: Globe },
  { id: "mcp_only", name: "仅 MCP", icon: Cable },
] as const;

export default function RemoteAccessPage({ controller, port, allowLan, onSettings }: { controller: RemoteController; port: number; allowLan: boolean; onSettings: () => void }) {
  const { state, error, busy, operate } = controller;
  const [probeResult, setProbeResult] = useState<"success" | "error" | null>(null);
  async function probe() {
    setProbeResult(null);
    if (await operate("测试连接", api.remoteProbe)) {
      setProbeResult("success");
      toast.success("公网入口及授权 MCP initialize/tools/list 验证通过");
    } else {
      setProbeResult("error");
      toast.error("连接测试失败，请查看远程状态中的错误详情");
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
  const currentModeName = state ? modes.find(({ id }) => id === state.mode)?.name : null;
  const statusText = state ? state.mode === "mcp_only" && state.status === "stopped" ? "本地模式" : states[state.status] : error ? "状态不可用" : "正在读取状态…";
  const quickTunnelActive = active && state?.mode === "quick_tunnel";
  const runtimeProvider: SelfHostedUiProvider = state?.config?.selfHosted?.provider === "ngrok" ? "ngrok" : "custom_https";
  const quickTunnelConfigured = state?.mode === "quick_tunnel";
  const customHttpsConfigured = state?.mode === "self_hosted_oauth" && runtimeProvider === "custom_https";
  const ngrokConfigured = state?.mode === "self_hosted_oauth" && runtimeProvider === "ngrok";
  const selectedSelfHostedActive = active && state?.mode === "self_hosted_oauth" && runtimeProvider === provider;
  const resource = state?.status === "ready" && state.mode === mode && (mode !== "self_hosted_oauth" || runtimeProvider === provider) ? state.publicContext?.mcpResource : null;
  const copied = !!resource && copiedResource === resource;
  const probeFeedback = probeResult && <p className="remote-probe-feedback" data-result={probeResult} role={probeResult === "error" ? "alert" : "status"}>{probeResult === "success" ? "连接测试成功：公网入口和 MCP 服务均可用。" : "连接测试失败：请检查公网地址或查看上方错误详情。"}</p>;
  useEffect(() => {
    if (!copiedResource) return;
    const timer = window.setTimeout(() => setCopiedResource(null), 1600);
    return () => window.clearTimeout(timer);
  }, [copiedResource]);
  async function copy() {
    if (!resource) return;
    setCopying(true);
    try { await navigator.clipboard.writeText(resource); setCopiedResource(resource); }
    catch (e) { toast.error(`复制失败：${String(e)}`); }
    finally { setCopying(false); }
  }
  async function saveNgrokToken() {
    const token = ngrokToken.trim();
    if (!token) return;
    if (await operate("保存 ngrok 凭据", () => api.remoteSaveNgrokAuth(token))) setNgrokToken("");
  }
  async function clearNgrokToken() {
    if (await operate("清除 ngrok 凭据", api.remoteClearNgrokAuth)) setNgrokToken("");
  }
  async function startNgrok() {
    const token = ngrokToken.trim();
    const started = await operate("开启", async () => {
      if (token) await api.remoteSaveNgrokAuth(token);
      await api.remoteStartNgrok();
    });
    if (started) setNgrokToken("");
  }
  return <section className="remote-page">
    <div className="page-heading"><div><h1>远程访问</h1><p>让 ChatGPT 连接这台电脑上的 MCP 工具。</p></div><span className="remote-status" data-ready={state?.status === "ready"} data-state={state?.status} role="status"><span className="remote-status-indicator" aria-hidden="true"/>当前：{currentModeName && `${currentModeName} · `}{statusText}</span></div>
    <fieldset className="remote-modes" disabled={!!busy}>
      <legend>选择连接方式</legend>
      {modes.map(({ id, name, icon: Icon }) => <label key={id} className="remote-mode" data-selected={mode === id}>
        <Icon size={18} aria-hidden="true"/><strong>{name}</strong>{state?.mode === id && <span className="remote-mode-runtime" data-active={active}><span aria-hidden="true"/>{active ? "使用中" : "当前配置"}</span>}<input type="radio" name="remote-mode" value={id} checked={mode === id} onChange={() => { setMode(id); setProbeResult(null); }} />
      </label>)}
    </fieldset>
    {(error || state?.lastError) && <div className="remote-error" role="alert">{error || state?.lastError}</div>}
    {mode === "quick_tunnel" ? <section className="remote-detail" aria-label="快捷隧道连接控制台">
      {resource ? <>
        <label className="field-label" htmlFor="remote-url">公网 MCP 地址</label>
        <div className="remote-address remote-public-address"><input id="remote-url" readOnly value={resource} aria-label="公网 MCP 地址"/><Button className="remote-copy" data-copied={copied} variant="outline" disabled={copying || copied} onClick={() => void copy()}><span className="remote-copy-icon" aria-hidden="true"><Copy size={16}/><Check size={16}/></span><span aria-live="polite">{copied ? "已复制" : copying ? "正在复制…" : "复制地址"}</span></Button></div>
        <dl className="remote-facts"><div><dt>认证</dt><dd><ShieldCheck size={15} aria-hidden="true"/>OAuth · SerenaDesktop</dd></div><div><dt>已授权客户端</dt><dd>{state?.authorizedClients ?? 0}</dd></div></dl>
      </> : <>
        <p className="remote-quick-summary">启动后会自动创建临时 HTTPS 地址，并由 SerenaDesktop OAuth 保护访问。</p>
        <dl className="remote-facts remote-quick-facts"><div><dt>认证</dt><dd><ShieldCheck size={15} aria-hidden="true"/>OAuth · SerenaDesktop</dd></div><div><dt>本地 MCP 目标</dt><dd><code>http://127.0.0.1:{port}/mcp</code></dd></div></dl>
      </>}
      <p className="remote-notice">临时地址重新创建后可能变化；应用重启不会自动创建快捷隧道。{allowLan && "局域网客户端仍需要 OAuth 授权。"}</p>
      <div className="remote-actions">
        {quickTunnelActive ? <>
          {state?.mode === mode && (state.status === "ready" || state.status === "error") && <Button disabled={!!busy} onClick={() => void probe()}>{busy === "测试连接" ? "正在测试…" : "测试连接"}</Button>}
          <Button variant="destructive" disabled={!!busy} onClick={() => void stop()}>{busy === "停止" ? "正在停止…" : resource ? "停止远程访问" : state?.status === "stopping" ? "重试停止" : "取消启动"}</Button>
        </> : <Button disabled={!state || !!busy} onClick={() => void operate("开启", () => api.remoteStart("quick_tunnel"))}>{busy === "开启" ? quickTunnelConfigured ? "正在启动…" : "正在切换…" : quickTunnelConfigured ? "启动快捷隧道" : "切换到快捷隧道"}</Button>}
        {quickTunnelActive && probeFeedback}
      </div>
    </section> : mode === "self_hosted_oauth" ? <section className="remote-detail" aria-label="自建接入连接控制台">
      <fieldset disabled={!!busy} className="self-hosted-providers">
        <legend>公网入口方式</legend>
        <div className="self-hosted-provider-options">
          <label className="self-hosted-provider-choice" data-selected={provider === "custom_https"}>
            <Globe size={16} aria-hidden="true"/><span>自有 HTTPS</span>{state?.mode === "self_hosted_oauth" && runtimeProvider === "custom_https" && <span className="self-hosted-provider-runtime" data-active={active}><span aria-hidden="true"/>{active ? "使用中" : "当前配置"}</span>}<input type="radio" name="self-hosted-provider" value="custom_https" checked={provider === "custom_https"} onChange={() => { setProvider("custom_https"); setProbeResult(null); }} />
          </label>
          <label className="self-hosted-provider-choice" data-selected={provider === "ngrok"}>
            <Cloud size={16} aria-hidden="true"/><span>ngrok</span>{state?.mode === "self_hosted_oauth" && runtimeProvider === "ngrok" && <span className="self-hosted-provider-runtime" data-active={active}><span aria-hidden="true"/>{active ? "使用中" : "当前配置"}</span>}<input type="radio" name="self-hosted-provider" value="ngrok" checked={provider === "ngrok"} onChange={() => { setProvider("ngrok"); setProbeResult(null); }} />
          </label>
        </div>
      </fieldset>
      {provider === "custom_https" ? <>
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
          {selectedSelfHostedActive && probeFeedback}
        </div>
      </> : <>
        {resource && <>
          <label className="field-label" htmlFor="self-mcp-url">公网 MCP 地址</label>
          <div className="remote-address remote-public-address"><input id="self-mcp-url" readOnly value={resource}/><Button variant="outline" disabled={copying || copied} onClick={() => void copy()}>{copied ? "已复制" : copying ? "正在复制…" : "复制地址"}</Button></div>
          <dl className="remote-facts self-hosted-resource-facts"><div><dt>已授权客户端</dt><dd>{state?.authorizedClients ?? 0}</dd></div></dl>
        </>}
        <section className="self-hosted-credentials" aria-labelledby="ngrok-credentials-title">
          <div className="self-hosted-credential-heading"><span id="ngrok-credentials-title">ngrok 凭据</span><span>{state?.ngrokAuthConfigured ? "已保存 Auth Token" : "尚未保存 Auth Token"}</span></div>
          <label className="field-label" htmlFor="ngrok-auth-token">ngrok Auth Token</label>
          <div className="remote-address"><input id="ngrok-auth-token" type="password" autoComplete="off" placeholder={state?.ngrokAuthConfigured ? "已保存，留空继续使用" : "输入 ngrok Auth Token"} value={ngrokToken} disabled={selectedSelfHostedActive || !!busy} onChange={event => setNgrokToken(event.target.value)} aria-describedby="ngrok-auth-token-help" /></div>
          <p id="ngrok-auth-token-help" className="helper">{state?.ngrokAuthConfigured ? "留空即可继续使用本机保存的值。" : "Auth Token 只会提交给 SerenaDesktop 并保存在本机。"}</p>
          <div className="self-hosted-credential-actions">
            <Button variant="outline" disabled={selectedSelfHostedActive || !!busy || !ngrokToken.trim()} onClick={() => void saveNgrokToken()}>{busy === "保存 ngrok 凭据" ? "正在保存…" : "保存凭据"}</Button>
            {state?.ngrokAuthConfigured && <Button variant="outline" disabled={selectedSelfHostedActive || !!busy} onClick={() => void clearNgrokToken()}>{busy === "清除 ngrok 凭据" ? "正在清除…" : "清除凭据"}</Button>}
          </div>
        </section>
        <dl className="remote-facts self-hosted-facts self-hosted-ngrok-facts"><div><dt>本地 MCP 目标</dt><dd><code>http://127.0.0.1:{port}/mcp</code></dd></div></dl>
        <div className="remote-actions">
          {selectedSelfHostedActive ? <>
            {state?.mode === mode && state.status !== "stopping" && state.status !== "verifying" && <Button disabled={!!busy} onClick={() => void probe()}>{busy === "测试连接" ? "正在测试…" : "测试连接"}</Button>}
            <Button variant="destructive" disabled={!!busy} onClick={() => void stop()}>停止远程访问</Button>
          </> : <Button disabled={!state || !!busy || (!state.ngrokAuthConfigured && !ngrokToken.trim())} onClick={() => void startNgrok()}>{busy === "开启" ? ngrokConfigured ? ngrokToken.trim() ? state?.ngrokAuthConfigured ? "正在更新并启动…" : "正在保存并启动…" : "正在启动…" : "正在切换…" : ngrokConfigured ? state?.ngrokAuthConfigured ? ngrokToken.trim() ? "更新并启动 ngrok" : "启动 ngrok" : "保存并启动 ngrok" : "切换到 ngrok"}</Button>}
          {selectedSelfHostedActive && probeFeedback}
        </div>
        <p className="remote-notice">SerenaDesktop 管理此隧道；已应用 ngrok 时应用重启会自动重连。重新连接可能得到新地址，需要更新 ChatGPT。{allowLan && "局域网客户端仍需要 OAuth 授权。"}</p>
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
  </section>;
}
