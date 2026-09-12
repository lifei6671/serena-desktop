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
  { id: "quick_tunnel", name: "快捷隧道", icon: Cloud, description: "无需域名和 Cloudflare 账号，自动生成临时公网地址。" },
  { id: "self_hosted_oauth", name: "自建接入", icon: Globe, description: "使用自己的公网 HTTPS 地址，由本机提供 OAuth。" },
  { id: "mcp_only", name: "仅 MCP", icon: Cable, description: "使用自己的认证网关，或只在可信网络中访问。" },
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
  const quickTunnelActive = active && state?.mode === "quick_tunnel";
  const runtimeProvider: SelfHostedUiProvider = state?.config?.selfHosted?.provider === "ngrok" ? "ngrok" : "custom_https";
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
    <div className="page-heading"><div><h1>远程访问</h1><p>让 ChatGPT 连接这台电脑上的 MCP 工具。</p></div><span className="remote-status" data-ready={state?.status === "ready"} role="status">{state ? states[state.status] : error ? "状态不可用" : "正在读取状态…"}</span></div>
    <fieldset className="remote-modes" disabled={!!busy}>
      <legend>选择连接方式</legend>
      {modes.map(({ id, name, icon: Icon, description }) => <label key={id} className="remote-mode" data-selected={mode === id}>
        <span className="remote-mode-title"><Icon size={18} aria-hidden="true"/><strong>{name}</strong>{state?.mode === id && <span className="remote-current-mode">当前使用</span>}<input type="radio" name="remote-mode" value={id} checked={mode === id} onChange={() => { setMode(id); setProbeResult(null); }} /></span>
        <span>{description}</span><small>{id === "quick_tunnel" ? "本机 OAuth · 自动创建隧道" : id === "mcp_only" ? "默认方式 · 使用现有 MCP 服务" : "本机 OAuth · 使用自己的公网入口"}</small>
      </label>)}
    </fieldset>
    <p className="helper">选择卡片只查看说明；点击应用后会自动停止当前连接并切换到所选方式。</p>
    {(error || state?.lastError) && <div className="remote-error" role="alert">{error || state?.lastError}</div>}
    {mode === "quick_tunnel" ? <section className="remote-detail" aria-labelledby="quick-title">
      <header><Cloud aria-hidden="true"/><div><h2 id="quick-title">快捷隧道</h2><p>一个临时地址，连接你的本机工具。</p></div></header>
      {resource ? <>
        <label className="field-label" htmlFor="remote-url">公网 MCP 地址</label>
        <div className="remote-address"><input id="remote-url" readOnly value={resource} aria-label="公网 MCP 地址"/><Button className="remote-copy" data-copied={copied} variant="outline" disabled={copying || copied} onClick={() => void copy()}><span className="remote-copy-icon" aria-hidden="true"><Copy size={16}/><Check size={16}/></span><span aria-live="polite">{copied ? "已复制" : copying ? "正在复制…" : "复制地址"}</span></Button></div>
        <dl className="remote-facts"><div><dt>认证</dt><dd><ShieldCheck size={15} aria-hidden="true"/>OAuth · SerenaDesktop</dd></div><div><dt>已授权客户端</dt><dd>{state?.authorizedClients ?? 0}</dd></div></dl>
        <p className="helper">将上方地址填入 ChatGPT 的 MCP 连接配置。浏览器发起授权后，请在本机核对确认码并允许连接。</p>
      </> : <>
        <ol className="remote-steps"><li>启动本地 MCP 服务</li><li>创建 Cloudflare 临时地址</li><li>连接时在这台电脑上批准 OAuth 授权</li></ol>
        <p className="helper">优先使用本机已安装的 cloudflared，未找到时才下载并校验。应用重启后不会自动创建隧道。</p>
      </>}
      <p className="remote-notice">地址是临时的。重新开启后，需要更新 ChatGPT 中的 MCP 地址。Quick Tunnel 适合临时使用，不保证持续可用。{allowLan && "当前已开启局域网访问；快捷隧道开启期间，局域网客户端也需要 OAuth 授权。"}</p>
      <div className="remote-actions">
        {quickTunnelActive ? <>
          {state?.mode === mode && (state.status === "ready" || state.status === "error") && <Button disabled={!!busy} onClick={() => void probe()}>{busy === "测试连接" ? "正在测试…" : "测试连接"}</Button>}
          <Button variant="destructive" disabled={!!busy} onClick={() => void stop()}>{busy === "停止" ? "正在停止…" : resource ? "停止远程访问" : state?.status === "stopping" ? "重试停止" : "取消启动"}</Button>
        </> : <Button disabled={!state || !!busy} onClick={() => void operate("开启", () => api.remoteStart("quick_tunnel"))}>{busy === "开启" ? "正在开启…" : "应用此方式"}</Button>}
        {quickTunnelActive && probeFeedback}
      </div>
    </section> : mode === "self_hosted_oauth" ? <section className="remote-detail" aria-labelledby="self-title">
      <header><Globe aria-hidden="true"/><div><h2 id="self-title">自建接入</h2><p>使用自己的公网入口，由这台电脑批准 OAuth 授权。</p></div></header>
      <fieldset disabled={!!busy} className="mcp-security">
        <legend>公网入口方式</legend>
        <p className="mcp-security-intro">选择由你管理公网入口，或由 SerenaDesktop 创建 ngrok 隧道。</p>
        <div className="mcp-security-options">
          <div className="mcp-security-option" data-selected={provider === "custom_https"}>
            <label className="mcp-security-choice">
              <Globe size={19} aria-hidden="true" />
              <span><strong>自有 HTTPS</strong><small>使用你自行管理的域名、外部 ngrok、Tailscale Funnel 或 HTTPS 反向代理。</small></span>
              <input type="radio" name="self-hosted-provider" value="custom_https" checked={provider === "custom_https"} onChange={() => { setProvider("custom_https"); setProbeResult(null); }} />
            </label>
          </div>
          <div className="mcp-security-option" data-selected={provider === "ngrok"}>
            <label className="mcp-security-choice">
              <Cloud size={19} aria-hidden="true" />
              <span><strong>ngrok 托管隧道</strong><small>由 SerenaDesktop 使用本机保存的 Auth Token 自动创建 HTTPS 隧道。</small></span>
              <input type="radio" name="self-hosted-provider" value="ngrok" checked={provider === "ngrok"} onChange={() => { setProvider("ngrok"); setProbeResult(null); }} />
            </label>
          </div>
        </div>
      </fieldset>
      {provider === "custom_https" ? <>
        <p>适用于自行管理外部 ngrok、Tailscale Funnel、自有域名或 HTTPS 反向代理的用户。</p>
        <dl className="remote-facts"><div><dt>公网地址示例</dt><dd><code>https://mcp.example.com</code></dd></div><div><dt>本地目标</dt><dd><code>http://127.0.0.1:{port}</code></dd></div></dl>
        <p className="helper">代理需要转发整个 Origin，包括 <code>/mcp</code>、<code>/.well-known/*</code> 和 <code>/oauth/*</code>。SerenaDesktop 将负责本机授权。</p>
        <label className="field-label" htmlFor="self-origin">公网 HTTPS 地址</label>
        <div className="remote-address"><input id="self-origin" type="url" placeholder="https://mcp.example.com" value={selectedSelfHostedActive && state.publicContext ? state.publicContext.publicOrigin : origin} disabled={selectedSelfHostedActive || !!busy} onChange={event => setOrigin(event.target.value)} aria-describedby="self-origin-help" /></div>
        <p id="self-origin-help" className="helper">只填写域名和可选端口，不包含 /mcp、查询参数或片段。自建接入运行期间不可修改地址。</p>
        {resource && <>
          <label className="field-label" htmlFor="self-mcp-url">公网 MCP 地址</label>
          <div className="remote-address"><input id="self-mcp-url" readOnly value={resource}/><Button variant="outline" disabled={copying || copied} onClick={() => void copy()}>{copied ? "已复制" : copying ? "正在复制…" : "复制地址"}</Button></div>
          <p className="helper">已授权客户端：{state?.authorizedClients ?? 0}。将 MCP 地址填入 ChatGPT，发起授权后在本机核对确认码。</p>
        </>}
        <p className="remote-notice">停止会撤销本次授权，并继续保护 MCP；你的公网代理仍由你管理。切换到「仅 MCP」会解除本机 OAuth 保护，请先关闭公网代理或配置自己的认证网关。退出或重启应用会保留未过期的授权，同一公网地址无需重新授权。{allowLan && "开启期间，局域网客户端也需要 OAuth 授权。"}</p>
        <div className="remote-actions">
          {selectedSelfHostedActive ? <>
            {state?.mode === mode && state.status !== "stopping" && state.status !== "verifying" && <Button disabled={!!busy} onClick={() => void probe()}>{busy === "测试连接" ? "正在测试…" : "测试连接"}</Button>}
            <Button variant="destructive" disabled={!!busy} onClick={() => void stop()}>停止远程访问</Button>
          </> : <Button disabled={!state || !!busy || !origin.trim()} onClick={() => void operate("开启", () => api.remoteStart("self_hosted_oauth", origin.trim()))}>{busy === "开启" ? "正在开启…" : "应用此方式"}</Button>}
          {selectedSelfHostedActive && probeFeedback}
        </div>
      </> : <>
        <p>SerenaDesktop 使用你在本机填写的 ngrok Auth Token 创建临时 HTTPS 隧道，公网地址无需手工填写。</p>
        <dl className="remote-facts"><div><dt>凭据状态</dt><dd>{state?.ngrokAuthConfigured ? "已保存 Auth Token" : "尚未保存 Auth Token"}</dd></div><div><dt>本地目标</dt><dd><code>http://127.0.0.1:{port}</code></dd></div></dl>
        <label className="field-label" htmlFor="ngrok-auth-token">ngrok Auth Token</label>
        <div className="remote-address"><input id="ngrok-auth-token" type="password" autoComplete="off" placeholder={state?.ngrokAuthConfigured ? "已保存，留空继续使用" : "输入 ngrok Auth Token"} value={ngrokToken} disabled={selectedSelfHostedActive || !!busy} onChange={event => setNgrokToken(event.target.value)} aria-describedby="ngrok-auth-token-help" /></div>
        <p id="ngrok-auth-token-help" className="helper">{state?.ngrokAuthConfigured ? "已保存 Auth Token。留空即可继续使用本机保存的值。" : "Auth Token 只会提交给 SerenaDesktop 并保存在本机。"}</p>
        <div className="remote-actions">
          <Button variant="outline" disabled={selectedSelfHostedActive || !!busy || !ngrokToken.trim()} onClick={() => void saveNgrokToken()}>{busy === "保存 ngrok 凭据" ? "正在保存…" : "保存凭据"}</Button>
          {state?.ngrokAuthConfigured && <Button variant="outline" disabled={selectedSelfHostedActive || !!busy} onClick={() => void clearNgrokToken()}>{busy === "清除 ngrok 凭据" ? "正在清除…" : "清除凭据"}</Button>}
        </div>
        {resource && <>
          <label className="field-label" htmlFor="self-mcp-url">公网 MCP 地址</label>
          <div className="remote-address"><input id="self-mcp-url" readOnly value={resource}/><Button variant="outline" disabled={copying || copied} onClick={() => void copy()}>{copied ? "已复制" : copying ? "正在复制…" : "复制地址"}</Button></div>
          <p className="helper">已授权客户端：{state?.authorizedClients ?? 0}。将 MCP 地址填入 ChatGPT，发起授权后在本机核对确认码。</p>
        </>}
        <p className="remote-notice">停止会关闭 SerenaDesktop 创建的 ngrok tunnel 并撤销本次授权。已应用此方式时，应用重启会自动重连；重新连接可能获得新地址，需要更新 ChatGPT 中的 MCP 地址。{allowLan && "开启期间，局域网客户端也需要 OAuth 授权。"}</p>
        <div className="remote-actions">
          {selectedSelfHostedActive ? <>
            {state?.mode === mode && state.status !== "stopping" && state.status !== "verifying" && <Button disabled={!!busy} onClick={() => void probe()}>{busy === "测试连接" ? "正在测试…" : "测试连接"}</Button>}
            <Button variant="destructive" disabled={!!busy} onClick={() => void stop()}>停止远程访问</Button>
          </> : <Button disabled={!state || !!busy || (!state.ngrokAuthConfigured && !ngrokToken.trim())} onClick={() => void startNgrok()}>{busy === "开启" ? "正在开启…" : "应用此方式"}</Button>}
          {selectedSelfHostedActive && probeFeedback}
        </div>
      </>}
    </section> : <section className="remote-detail" aria-labelledby="only-title">
      <header><Cable aria-hidden="true"/><div><h2 id="only-title">仅 MCP</h2><p>默认连接方式，沿用设置中的 MCP 服务，不创建公网隧道。</p></div></header>
      <div className="mcp-only-settings">
        <div className="mcp-local-service">
          <div><span className="mcp-section-label">本机 MCP 地址</span><code>http://127.0.0.1:{port}/mcp</code></div>
          <Button variant="outline" onClick={onSettings}>管理 MCP 服务</Button>
        </div>
        <fieldset disabled={!!busy} className="mcp-security">
          <legend>认证方式</legend>
          <p className="mcp-security-intro">选择由谁负责验证连接到 MCP 的客户端。</p>
          <div className="mcp-security-options">
            <div className="mcp-security-option" data-selected={declaration === "external_auth"}>
              <label className="mcp-security-choice">
                <Shield size={19} aria-hidden="true" />
                <span><strong>我的网关已经负责认证</strong><small>通过 Cloudflare Access、ngrok 等网关接入。</small></span>
                <input type="radio" name="mcp-security" checked={declaration === "external_auth"} onChange={() => { setDeclaration("external_auth"); setRiskAccepted(false); }} />
              </label>
              {declaration === "external_auth" && <div className="mcp-security-content">
                <p className="helper">这只是你的声明；SerenaDesktop 不验证网关的认证配置。</p>
                <label className="mcp-origin-label" htmlFor="mcp-only-origin">网关公网地址 <span>可选</span></label>
                <div className="remote-address"><input id="mcp-only-origin" type="url" value={onlyOrigin} placeholder="https://mcp.example.com" aria-describedby="mcp-origin-help" onChange={event => setOnlyOrigin(event.target.value)} /></div>
                <p id="mcp-origin-help" className="helper">填写 HTTPS 域名和可选端口，不包含 /mcp。</p>
                <details className="mcp-proxy-help">
                  <summary>什么时候需要填写？<ChevronDown size={14} aria-hidden="true" /></summary>
                  <p>代理保留公网 Host 时填写此地址；留空时，代理需将 Host 改写为 Broker 本机地址。此配置只允许对应 Host/Origin，不启用 SerenaDesktop OAuth。</p>
                </details>
                {state?.mode === "mcp_only" && state.config?.mcpOnly.securityDeclaration === "external_auth" && appliedOnlyOrigin && <div className="mcp-saved-endpoint"><span>已保存的网关地址 · 未验证外部认证</span><code>{appliedOnlyOrigin}/mcp</code></div>}
              </div>}
            </div>
            <div className="mcp-security-option" data-selected={declaration === "none"}>
              <label className="mcp-security-choice">
                <ShieldOff size={19} aria-hidden="true" />
                <span><strong>不使用认证</strong><small>允许能访问此地址的客户端直接调用工具。</small></span>
                <input type="radio" name="mcp-security" checked={declaration === "none"} onChange={() => { setDeclaration("none"); setRiskAccepted(false); }} />
              </label>
              {declaration === "none" && <div className="mcp-security-content">
                <div className="mcp-risk-confirmation">
                  <strong>公开到公网前，请确认访问风险</strong>
                  <label><input type="checkbox" checked={riskAccepted} onChange={event => setRiskAccepted(event.target.checked)} /><span>我理解如果该 MCP 被暴露到公网，任何能访问 Endpoint 的客户端都可能调用公开工具，包括 Agent。</span></label>
                </div>
              </div>}
            </div>
          </div>
        </fieldset>
        <div className="mcp-security-footer">
          <p className="helper" aria-live="polite">{onlyApplied ? "当前选择已应用" : declaration === "none" && !riskAccepted ? "确认访问风险后，即可应用。" : "更改将在应用后生效。"}</p>
          <Button disabled={!state || !!busy || onlyApplied || (declaration === "none" && !riskAccepted)} onClick={() => void operate("应用", () => api.remoteStart("mcp_only", declaration === "external_auth" ? onlyOrigin.trim() || undefined : undefined, declaration, riskAccepted))}>{busy === "应用" ? "正在应用…" : onlyApplied ? "已应用" : "应用此方式"}</Button>
        </div>
        {active && <p className="remote-notice">应用仅 MCP 将停止远程访问并撤销授权；请先关闭公网代理或配置自己的认证网关。</p>}
      </div>
    </section>}
  </section>;
}
