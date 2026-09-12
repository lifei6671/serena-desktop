import { test, afterEach } from 'node:test';
import assert from 'node:assert/strict';
import { registerHooks } from 'node:module';
import { readFileSync, existsSync } from 'node:fs';
import { fileURLToPath, pathToFileURL } from 'node:url';
import path from 'node:path';
import ts from 'typescript';
import { JSDOM } from 'jsdom';

registerHooks({
  resolve(specifier, context, next) {
    let target;
    if (specifier.startsWith('@/')) target = path.resolve('src', specifier.slice(2));
    else if (specifier.startsWith('.') && context.parentURL?.startsWith('file:')) target = path.resolve(path.dirname(fileURLToPath(context.parentURL)), specifier);
    if (target) for (const suffix of ['', '.ts', '.tsx']) {
      if (/\.tsx?$/.test(target + suffix) && existsSync(target + suffix)) return { url: pathToFileURL(target + suffix).href, shortCircuit: true };
    }
    return next(specifier, context);
  },
  load(url, context, next) {
    if (url.startsWith('file:') && /\.tsx?$/.test(url)) return { format: 'module', shortCircuit: true,
      source: ts.transpileModule(readFileSync(fileURLToPath(url), 'utf8'), { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext, jsx: ts.JsxEmit.ReactJSX } }).outputText };
    return next(url, context);
  }
});
const dom = new JSDOM('<!doctype html><html><body><div id="root"></div></body></html>', { url: 'http://localhost/', pretendToBeVisual: true });
// JSDOM has no layout; actual tooltip positioning is checked in Chromium.
globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} };
globalThis.requestAnimationFrame = dom.window.requestAnimationFrame.bind(dom.window);
globalThis.cancelAnimationFrame = dom.window.cancelAnimationFrame.bind(dom.window);
// Use one timer registry for components that mix window and global timer calls.
dom.window.setInterval = globalThis.setInterval;
dom.window.clearInterval = globalThis.clearInterval;
Object.assign(globalThis, { window: dom.window, document: dom.window.document, HTMLElement: dom.window.HTMLElement, Element: dom.window.Element, Node: dom.window.Node, NodeFilter: dom.window.NodeFilter, CustomEvent: dom.window.CustomEvent, MutationObserver: dom.window.MutationObserver, HTMLInputElement: dom.window.HTMLInputElement, getComputedStyle: dom.window.getComputedStyle, IS_REACT_ACT_ENVIRONMENT: true });
for (const key of ["HTMLFormElement", "DocumentFragment", "HTMLSelectElement", "HTMLOptionElement", "Event", "KeyboardEvent", "MouseEvent"]) globalThis[key] = dom.window[key];
const { createElement, act } = await import('react');
const { createRoot } = await import('react-dom/client');
const { TooltipProvider } = await import('./components/ui/tooltip.tsx');
const { default: App } = await import('./App.tsx');
const { api } = await import('./api.ts');
let root;
afterEach(async () => { if (root) await act(async () => root.unmount()); root = null; });
test('lazy pages preserve settings draft and keep project navigation mounted', async () => {
  const config = { agentEnabled: false, broker: { enabled: false, port: 9120, allowLan: false }, workspaces: [], serenaPath: null, port: 9121, dashboardEnabled: true, openDashboardOnLaunch: false, autoStartServer: true, minimizeToTray: true };
  const snapshot = { config, git: { available: false, status: 'missing' }, serverStatus: 'stopped', installation: null, activeInstallation: null, managedRuntimePresent: false, managedProcessPresent: false, activePort: 9121, autostartEnabled: false, codegraphVersion: null };
  api.getState = async () => structuredClone(snapshot);
  api.broker = async () => ({ running: false, projects: [{ id: 'W', name: 'Persistent project', root: 'E:/project', configured: true }], projectSources: [], syncWarnings: [], activeWorkspace: null, codegraph: null });
  api.agentHistory = async () => ({ executions: [], nextCursor: null });
  api.codexVersion = async () => 'test-version';
  api.remoteState = async () => ({ mode: 'quick_tunnel', status: 'stopped', publicContext: null, lastError: null, authorizedClients: 0, pending: [], active: false });
  root = createRoot(document.getElementById('root'));
  await act(async () => root.render(createElement(TooltipProvider, null, createElement(App))));
  const navigation = document.querySelector('.project-navigation-slot');
  assert.match(navigation.textContent, /Persistent project/);
  async function navigate(label) {
    const button = [...document.querySelectorAll('nav[aria-label="主导航"] button')].find(item => item.textContent === label);
    await act(async () => button.click());
    // Flush the first dynamic module import and Suspense commit.
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 350)); });
  }
  await navigate('设置');
  const port = document.getElementById('broker-port');
  assert.ok(port);
  await act(async () => {
    Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value').set.call(port, '9234');
    port.dispatchEvent(new window.Event('input', { bubbles: true }));
  });
  await navigate('状态');
  assert.ok(document.querySelector('.serena-page'));
  assert.match(document.querySelector('.serena-page').textContent, /test-version/);
  await navigate('设置');
  assert.equal(document.getElementById('broker-port').value, '9234');
  assert.equal(document.querySelector('.project-navigation-slot'), navigation);
  assert.match(navigation.textContent, /Persistent project/);
});

test('remote access uses a compact accessible mode switcher, switches running modes and removes old address', async () => {
  const { default: RemoteAccessPage } = await import('./RemoteAccessPage.tsx');
  const state = { mode: 'mcp_only', status: 'stopped', publicContext: null, lastError: null, authorizedClients: 0, pending: [], active: false };
  const calls = [];
  api.remoteStart = async mode => { calls.push(mode); };
  const controller = { state, busy: '', error: '', operate: async (_label, action) => { await action(); return true; } };
  root = createRoot(document.getElementById('root'));
  const render = async () => act(async () => root.render(createElement(RemoteAccessPage, { controller, port: 9120, allowLan: false, onSettings() {} })));
  await render();
  assert.equal(document.querySelectorAll('input[name="remote-mode"]').length, 3);
  assert.equal(document.querySelectorAll('.remote-mode').length, 3);
  assert.equal(document.querySelectorAll('.remote-mode small').length, 0);
  assert.equal(document.querySelector('.remote-current-mode'), null);
  assert.equal(document.querySelector('.remote-status').dataset.state, 'stopped');
  assert.match(document.querySelector('.remote-status').textContent, /当前：仅 MCP · 本地模式/);
  const button = text => [...document.querySelectorAll('button')].find(b => b.textContent === text);
  const selectedModeName = () => document.querySelector('.remote-mode[data-selected="true"] strong').textContent;
  const modeLabel = value => document.querySelector(`input[name="remote-mode"][value="${value}"]`).closest('.remote-mode');
  assert.ok(document.querySelector('input[value="mcp_only"]').checked);
  assert.equal(selectedModeName(), '仅 MCP');
  assert.match(modeLabel('mcp_only').textContent, /当前配置/);
  assert.ok(button('管理 MCP 服务'));
  assert.equal(button('已应用').disabled, true);
  assert.equal(document.querySelectorAll('.mcp-only-protection-choice')[0].dataset.selected, 'true');
  assert.equal(document.querySelectorAll('.mcp-only-protection-choice')[1].dataset.selected, 'false');
  assert.equal(button('应用此方式'), undefined);
  await act(async () => document.querySelector('input[value="quick_tunnel"]').click());
  assert.equal(selectedModeName(), '快捷隧道');
  assert.match(document.querySelector('.remote-status').textContent, /当前：仅 MCP · 本地模式/);
  assert.match(modeLabel('mcp_only').textContent, /当前配置/);
  assert.ok(button('切换到快捷隧道'));
  assert.match(document.querySelector('.remote-detail').textContent, /自动创建临时 HTTPS 地址/);
  assert.match(document.querySelector('.remote-detail').textContent, /http:\/\/127\.0\.0\.1:9120\/mcp/);
  assert.doesNotMatch(document.querySelector('.remote-detail').textContent, /启动本地 MCP 服务|cloudflared/);
  await act(async () => button('切换到快捷隧道').click());
  assert.deepEqual(calls, ['quick_tunnel']);
  await act(async () => document.querySelector('input[value="self_hosted_oauth"]').click());
  assert.ok(document.getElementById("self-origin"));
  assert.match(document.body.textContent, /\.well-known/);
  assert.equal(button('切换到自有 HTTPS').disabled, true);
  await act(async () => document.querySelector('input[value="mcp_only"]').click());
  assert.match(document.body.textContent, /不验证外部网关的认证配置/);
  await act(async () => document.querySelectorAll('input[name="mcp-security"]')[1].click());
  assert.equal(document.querySelectorAll('.mcp-only-protection-choice')[0].dataset.selected, 'false');
  assert.equal(document.querySelectorAll('.mcp-only-protection-choice')[1].dataset.selected, 'true');
  assert.match(document.body.textContent, /包括 Agent/);
  await act(async () => document.querySelectorAll('input[name="mcp-security"]')[0].click());
  await act(async () => document.querySelector('input[value="quick_tunnel"]').click());
  controller.state = { ...state, mode: 'quick_tunnel', status: 'ready', active: true, config: { mode: 'quick_tunnel', selfHosted: { provider: 'custom_https', publicOrigin: 'https://saved.example.com' }, mcpOnly: { securityDeclaration: 'external_auth', publicOrigin: null } }, publicContext: { publicOrigin: 'https://old.trycloudflare.com', mcpResource: 'https://old.trycloudflare.com/mcp', instanceId: 'one' } };
  await render();
  assert.equal(document.getElementById('remote-url').value, 'https://old.trycloudflare.com/mcp');
  assert.equal(document.querySelector('fieldset').disabled, false);
  assert.equal(selectedModeName(), '快捷隧道');
  assert.match(document.querySelector('.remote-status').textContent, /当前：快捷隧道 · 已连接/);
  assert.match(modeLabel('quick_tunnel').textContent, /使用中/);
  assert.ok(button('复制地址'));
  assert.ok(button('停止远程访问'));
  const switches = [];
  api.remoteStart = async (...args) => { switches.push(args); };
  await act(async () => document.querySelector('input[value="self_hosted_oauth"]').click());
  assert.equal(selectedModeName(), '自建接入');
  assert.match(modeLabel('quick_tunnel').textContent, /使用中/);
  assert.equal(document.getElementById('self-origin').disabled, false);
  assert.equal(document.getElementById('self-origin').value, 'https://saved.example.com');
  assert.equal(button('切换到自有 HTTPS').disabled, false);
  await act(async () => button('切换到自有 HTTPS').click());
  assert.deepEqual(switches, [['self_hosted_oauth', 'https://saved.example.com']]);
  await act(async () => document.querySelector('input[value="mcp_only"]').click());
  assert.equal(selectedModeName(), '仅 MCP');
  await act(async () => button('切换为仅 MCP').click());
  assert.deepEqual(switches[1], ['mcp_only', undefined, 'external_auth', false]);
  assert.equal(selectedModeName(), '仅 MCP');
  await act(async () => document.querySelector('input[value="quick_tunnel"]').click());
  controller.state = { ...state, mode: 'quick_tunnel', status: 'disconnected', lastError: 'QUICK_TUNNEL_DISCONNECTED' };
  await render();
  assert.equal(document.getElementById('remote-url'), null);
  assert.equal(selectedModeName(), '快捷隧道');
  assert.equal(button('复制地址'), undefined);
  assert.ok(button('启动快捷隧道'));
  await act(async () => root.render(createElement(RemoteAccessPage, { controller, port: 9120, allowLan: true, onSettings() {} })));
  assert.equal(button('启动快捷隧道').disabled, false);
  assert.equal(document.querySelectorAll('.remote-notice').length, 1);
  assert.match(document.querySelector('.remote-notice').textContent, /局域网客户端仍需要 OAuth 授权/);
  await act(async () => button('启动快捷隧道').click());
  assert.deepEqual(calls, ['quick_tunnel']);
  assert.deepEqual(switches.at(-1), ['quick_tunnel']);
  controller.state = { ...state, mode: 'quick_tunnel', status: 'starting', active: true, publicContext: null };
  await render();
  assert.equal(document.getElementById('remote-url'), null);
  assert.match(document.querySelector('.remote-detail').textContent, /自动创建临时 HTTPS 地址/);
  assert.doesNotMatch(document.querySelector('.remote-detail').textContent, /启动本地 MCP 服务|cloudflared/);
  assert.ok(button('取消启动'));
  controller.state = { ...state, mode: 'self_hosted_oauth', status: 'stopped', active: false, publicContext: null };
  await render();
  assert.ok(button('切换到快捷隧道'));
});

test('self hosted entry submits origin and hides resources from other modes', async () => {
  const { default: RemoteAccessPage } = await import('./RemoteAccessPage.tsx');
  const calls = [];
  api.remoteStart = async (...args) => calls.push(args);
  const controller = { state: { mode: 'mcp_only', status: 'stopped', active: false }, busy: '', error: '', operate: async (_label, action) => { await action(); return true; } };
  root = createRoot(document.getElementById('root'));
  const render = async () => act(async () => root.render(createElement(RemoteAccessPage, { controller, port: 9120, allowLan: false, onSettings() {} })));
  await render();
  await act(async () => document.querySelector('input[value="self_hosted_oauth"]').click());
  const input = document.getElementById('self-origin');
  assert.equal([...document.querySelectorAll('button')].find(b => b.textContent === '切换到自有 HTTPS').disabled, true);
  await act(async () => {
    Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value').set.call(input, 'https://self.example.com');
    input.dispatchEvent(new window.Event('input', { bubbles: true }));
  });
  await act(async () => [...document.querySelectorAll('button')].find(b => b.textContent === '切换到自有 HTTPS').click());
  assert.deepEqual(calls, [['self_hosted_oauth', 'https://self.example.com']]);
  controller.state = { mode: 'self_hosted_oauth', status: 'ready', active: true, config: { mode: 'self_hosted_oauth', selfHosted: { provider: 'custom_https', publicOrigin: 'https://self.example.com' }, mcpOnly: { securityDeclaration: 'external_auth', publicOrigin: null } }, publicContext: { publicOrigin: 'https://self.example.com', mcpResource: 'https://self.example.com/mcp' } };
  await render();
  assert.equal(document.getElementById('self-origin').disabled, true);
  assert.equal(document.getElementById('self-mcp-url').value, 'https://self.example.com/mcp');
  assert.ok([...document.querySelectorAll('button')].find(b => b.textContent === '复制地址'));
  assert.ok([...document.querySelectorAll('button')].find(b => b.textContent === '停止远程访问'));
  await act(async () => document.querySelector('input[name="self-hosted-provider"][value="ngrok"]').click());
  assert.equal(document.getElementById('self-origin'), null);
  assert.equal(document.getElementById('self-mcp-url'), null);
  assert.ok(document.getElementById('ngrok-auth-token'));
  assert.equal(document.getElementById('ngrok-auth-token').disabled, false);
  assert.equal(document.querySelector('.self-hosted-current'), null);
  assert.match(document.querySelector('input[name="self-hosted-provider"][value="custom_https"]').closest('.self-hosted-provider-choice').textContent, /使用中/);
  assert.equal([...document.querySelectorAll('button')].find(b => b.textContent === '切换到 ngrok').disabled, true);
  await act(async () => document.querySelector('input[name="self-hosted-provider"][value="custom_https"]').click());
  await act(async () => document.querySelector('input[value="quick_tunnel"]').click());
  assert.equal(document.getElementById('remote-url'), null);
  assert.equal([...document.querySelectorAll('button')].find(b => b.textContent === '取消启动'), undefined);
  assert.equal([...document.querySelectorAll('button')].find(b => b.textContent === '切换到快捷隧道').disabled, false);
  await act(async () => [...document.querySelectorAll('button')].find(b => b.textContent === '切换到快捷隧道').click());
  assert.deepEqual(calls.at(-1), ['quick_tunnel']);
  await act(async () => document.querySelector('input[value="self_hosted_oauth"]').click());
  controller.state = { ...controller.state, status: 'error', publicContext: null };
  await render();
  assert.equal(document.getElementById('self-mcp-url'), null);
  assert.ok([...document.querySelectorAll('button')].some(b => b.textContent === '测试连接'));
  controller.state = { ...controller.state, status: 'stopped', active: false };
  await render();
  assert.match(document.querySelector('input[name="self-hosted-provider"][value="custom_https"]').closest('.self-hosted-provider-choice').textContent, /当前配置/);
  assert.equal([...document.querySelectorAll('button')].find(b => b.textContent === '启用自有 HTTPS').disabled, false);
  await act(async () => document.querySelector('input[value="quick_tunnel"]').click());
  assert.equal([...document.querySelectorAll('button')].find(b => b.textContent === '切换到快捷隧道').disabled, false);
  assert.doesNotMatch(document.body.textContent, /请先停止|应用「仅 MCP」/);
});

test('managed ngrok self-hosted flow keeps token private and uses dedicated commands', async () => {
  const { default: RemoteAccessPage } = await import('./RemoteAccessPage.tsx');
  const calls = [];
  api.remoteSaveNgrokAuth = async token => calls.push(['save', token]);
  api.remoteClearNgrokAuth = async () => calls.push(['clear']);
  api.remoteStartNgrok = async () => calls.push(['start']);
  api.remoteStop = async () => calls.push(['stop']);
  const stopped = {
    mode: 'mcp_only', status: 'stopped', active: false, ngrokAuthConfigured: false,
    config: { mode: 'mcp_only', selfHosted: { provider: 'custom_https', publicOrigin: null }, mcpOnly: { securityDeclaration: 'external_auth', publicOrigin: null } },
    publicContext: null, lastError: null, authorizedClients: 0, pending: [],
  };
  const controller = { state: stopped, busy: '', error: '', operate: async (_label, action) => { await action(); return true; } };
  root = createRoot(document.getElementById('root'));
  const render = async () => act(async () => root.render(createElement(RemoteAccessPage, { controller, port: 9120, allowLan: false, onSettings() {} })));
  const button = text => [...document.querySelectorAll('button')].find(item => item.textContent === text);
  await render();
  await act(async () => document.querySelector('input[value="self_hosted_oauth"]').click());
  await act(async () => document.querySelector('input[name="self-hosted-provider"][value="ngrok"]').click());
  assert.equal(document.getElementById('self-origin'), null);
  const tokenInput = document.getElementById('ngrok-auth-token');
  assert.ok(tokenInput);
  assert.equal(document.querySelector('.self-hosted-credentials').contains(button('切换到 ngrok')), false);
  assert.ok([...document.querySelector('.self-hosted-credentials').querySelectorAll('button')].find(item => item.textContent === '保存凭据'));
  assert.equal(button('切换到 ngrok').disabled, true);

  const token = 'test-ngrok-token-must-stay-private';
  await act(async () => {
    Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value').set.call(tokenInput, token);
    tokenInput.dispatchEvent(new window.Event('input', { bubbles: true }));
  });
  assert.equal(button('切换到 ngrok').disabled, false);
  assert.doesNotMatch(document.body.textContent, new RegExp(token));
  await act(async () => button('切换到 ngrok').click());
  assert.deepEqual(calls, [['save', token], ['start']]);
  assert.equal(document.getElementById('ngrok-auth-token').value, '');
  assert.doesNotMatch(document.body.textContent, new RegExp(token));

  controller.state = {
    ...stopped, mode: 'self_hosted_oauth', ngrokAuthConfigured: true,
    config: { ...stopped.config, mode: 'self_hosted_oauth', selfHosted: { provider: 'ngrok', publicOrigin: null } },
  };
  await render();
  assert.match(document.body.textContent, /已保存 Auth Token/);
  assert.equal(document.querySelector('.self-hosted-current'), null);
  assert.match(document.querySelector('input[name="self-hosted-provider"][value="ngrok"]').closest('.self-hosted-provider-choice').textContent, /当前配置/);
  assert.ok([...document.querySelector('.self-hosted-credentials').querySelectorAll('button')].find(item => item.textContent === '清除凭据'));
  assert.ok(button('清除凭据'));
  calls.length = 0;
  await act(async () => button('启动 ngrok').click());
  assert.deepEqual(calls, [['start']]);
  const replacement = 'replacement-ngrok-token-must-stay-private';
  await act(async () => {
    Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value').set.call(tokenInput, replacement);
    tokenInput.dispatchEvent(new window.Event('input', { bubbles: true }));
  });
  assert.equal(button('更新并启动 ngrok').disabled, false);
  assert.doesNotMatch(document.body.textContent, new RegExp(replacement));
  await act(async () => button('更新并启动 ngrok').click());
  assert.deepEqual(calls, [['start'], ['save', replacement], ['start']]);
  await act(async () => button('清除凭据').click());
  assert.deepEqual(calls, [['start'], ['save', replacement], ['start'], ['clear']]);

  controller.state = {
    ...controller.state, status: 'ready', active: true, authorizedClients: 1,
    publicContext: { publicOrigin: 'https://managed.example', mcpResource: 'https://managed.example/mcp', instanceId: 'managed-one' },
  };
  await render();
  assert.equal(document.getElementById('self-mcp-url').value, 'https://managed.example/mcp');
  assert.equal(document.getElementById('ngrok-auth-token').disabled, true);
  assert.match(document.querySelector('input[name="self-hosted-provider"][value="ngrok"]').closest('.self-hosted-provider-choice').textContent, /使用中/);
  assert.equal(document.querySelector('.self-hosted-credentials').contains(button('停止远程访问')), false);
  assert.equal(document.querySelector('.self-hosted-credentials').contains(button('测试连接')), false);
  assert.ok(button('复制地址'));
  assert.ok(button('测试连接'));
  assert.ok(button('停止远程访问'));
  await act(async () => root.render(createElement(RemoteAccessPage, { controller, port: 9120, allowLan: true, onSettings() {} })));
  assert.match(document.querySelector('.remote-notice').textContent, /局域网客户端仍需要 OAuth 授权/);
});

test('remote copy confirms only after clipboard success and self-hosted probe shows inline outcomes', async () => {
  const { default: RemoteAccessPage } = await import('./RemoteAccessPage.tsx');
  const { toast } = await import('sonner');
  const originalSuccess = toast.success, originalError = toast.error;
  const clipboard = Object.getOwnPropertyDescriptor(navigator, 'clipboard');
  const notices = [];
  toast.success = message => notices.push(['success', message]);
  toast.error = message => notices.push(['error', message]);
  let finishCopy;
  const resource = 'https://copy.trycloudflare.com/mcp';
  Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText: async value => {
    assert.equal(value, resource);
    await new Promise(resolve => { finishCopy = resolve; });
  } } });
  const controller = { state: { mode: 'quick_tunnel', status: 'ready', active: true, pending: [], publicContext: { mcpResource: resource } }, busy: '', error: '', operate: async (_label, action) => { try { await action(); return true; } catch (e) { controller.error = String(e); return false; } } };
  const button = text => [...document.querySelectorAll('button')].find(b => b.textContent === text);
  try {
    root = createRoot(document.getElementById('root'));
    await act(async () => root.render(createElement(RemoteAccessPage, { controller, port: 9120, allowLan: false, onSettings() {} })));
    await act(async () => button('复制地址').click());
    assert.ok(button('正在复制…').disabled);
    assert.equal(button('已复制'), undefined);
    await act(async () => finishCopy());
    assert.equal(button('已复制').dataset.copied, 'true');
    await act(async () => new Promise(resolve => setTimeout(resolve, 1700)));
    assert.equal(button('复制地址').disabled, false);
    navigator.clipboard.writeText = async () => { throw new Error('clipboard denied'); };
    await act(async () => button('复制地址').click());
    assert.match(notices.at(-1)[1], /复制失败/);
    assert.equal(button('已复制'), undefined);
    await act(async () => document.querySelector('input[value="self_hosted_oauth"]').click());
    controller.state = { mode: 'self_hosted_oauth', status: 'ready', active: true, pending: [], config: { mode: 'self_hosted_oauth', selfHosted: { provider: 'custom_https', publicOrigin: 'https://self.example.com' }, mcpOnly: { securityDeclaration: 'external_auth', publicOrigin: null } }, publicContext: { publicOrigin: 'https://self.example.com', mcpResource: 'https://self.example.com/mcp' } };
    await act(async () => root.render(createElement(RemoteAccessPage, { controller, port: 9120, allowLan: false, onSettings() {} })));
    api.remoteProbe = async () => {};
    await act(async () => button('测试连接').click());
    assert.equal(notices.at(-1)[0], 'success');
    assert.equal(document.querySelector('.remote-probe-feedback').dataset.result, 'success');
    assert.equal(document.querySelector('.remote-probe-feedback').getAttribute('role'), 'status');
    assert.match(document.querySelector('.remote-probe-feedback').textContent, /连接测试成功/);
    api.remoteProbe = async () => { throw new Error('HTTPS connect failed'); };
    await act(async () => button('测试连接').click());
    assert.equal(notices.at(-1)[0], 'error');
    assert.match(notices.at(-1)[1], /连接测试失败/);
    assert.match(controller.error, /HTTPS connect failed/);
    assert.equal(document.querySelector('.remote-probe-feedback').dataset.result, 'error');
    assert.equal(document.querySelector('.remote-probe-feedback').getAttribute('role'), 'alert');
    assert.match(document.querySelector('.remote-probe-feedback').textContent, /连接测试失败/);
    assert.equal(button('测试连接').dataset.variant, 'default');
    assert.equal(button('停止远程访问').dataset.variant, 'destructive');
  } finally {
    toast.success = originalSuccess; toast.error = originalError;
    if (clipboard) Object.defineProperty(navigator, 'clipboard', clipboard); else delete navigator.clipboard;
  }
});

test('native approval escapes client claims, focuses deny, and binds decisions to request id', async () => {
  const { RemoteApprovalDialog } = await import('./RemoteApprovalDialog.tsx');
  const pending = { id: 'flow-1', clientName: '<script>untrusted</script>', redirectUri: 'https://client.example/callback', confirmationCode: '482731', expiresInSeconds: 100, scope: 'serena:mcp offline_access', refreshAllowed: true };
  const controller = { state: { pending: [pending] }, busy: '测试连接', approvalBusy: false, approvalError: '', approve: (id, allow) => api.remoteApprove(id, allow) };
  const decisions = [];
  api.remoteApprove = async (id, allow) => { decisions.push([id, allow]); };
  root = createRoot(document.getElementById('root'));
  await act(async () => root.render(createElement(RemoteApprovalDialog, { controller })));
  assert.equal(document.activeElement.textContent, '拒绝');
  const dialog = document.querySelector('[role="dialog"]');
  assert.match(dialog.textContent, /482731/);
  assert.equal(dialog.querySelector('script'), null);
  await act(async () => [...dialog.querySelectorAll('button')].find(b => b.textContent === '允许连接').click());
  assert.deepEqual(decisions, [['flow-1', true]]);
  assert.match(document.querySelector('[role="dialog"]').textContent, /客户端可自动刷新/);
  controller.state = { pending: [{ ...pending, id: 'flow-2', expiresInSeconds: 0, scope: 'serena:mcp', refreshAllowed: false }] };
  await act(async () => root.render(createElement(RemoteApprovalDialog, { controller })));
  assert.equal(document.activeElement.textContent, '拒绝');
  assert.ok([...document.querySelectorAll('[role="dialog"] button')].find(b => b.textContent === '允许连接').disabled);
  await act(async () => document.activeElement.click());
  assert.deepEqual(decisions, [['flow-1', true], ['flow-2', false]]);
  assert.match(document.querySelector('[role="dialog"]').textContent, /不签发刷新令牌/);
});


test('MCP Only requires risk acceptance and submits the selected declaration', async () => {
  const { default: RemoteAccessPage } = await import('./RemoteAccessPage.tsx');
  const calls = [];
  api.remoteStart = async (...args) => calls.push(args);
  const controller = { state: { mode: 'quick_tunnel', status: 'stopped', active: false, config: { mode: 'quick_tunnel', selfHosted: { provider: 'custom_https', publicOrigin: 'https://saved.example' }, mcpOnly: { securityDeclaration: 'external_auth' } } }, busy: '', error: '', operate: async (_label, action) => { await action(); return true; } };
  root = createRoot(document.getElementById('root'));
  await act(async () => root.render(createElement(RemoteAccessPage, { controller, port: 9120, allowLan: false, onSettings() {} })));
  await act(async () => document.querySelector('input[value="self_hosted_oauth"]').click());
  assert.equal(document.getElementById('self-origin').value, 'https://saved.example');
  await act(async () => document.querySelector('input[value="mcp_only"]').click());
  const apply = () => [...document.querySelectorAll('button')].find(b => b.textContent === '切换为仅 MCP');
  assert.match(document.querySelector('.remote-detail').textContent, /不验证外部网关的认证配置，也不启用 SerenaDesktop OAuth/);
  assert.match(document.querySelector('.remote-notice').textContent, /切换为仅 MCP 会移除 SerenaDesktop OAuth 保护/);
  await act(async () => apply().click());
  assert.deepEqual(calls, [['mcp_only', undefined, 'external_auth', false]]);
  assert.match(document.querySelector('.remote-detail').textContent, /不表示认证已验证/);
  assert.doesNotMatch(document.querySelector('.remote-detail').textContent, /OAuth 已启用/);
  await act(async () => document.querySelectorAll('input[name="mcp-security"]')[1].click());
  assert.equal(apply().disabled, true);
  await act(async () => apply().click());
  assert.equal(calls.length, 1);
  assert.equal(document.querySelector('input[type="checkbox"]').checked, false);
  await act(async () => document.querySelector('input[type="checkbox"]').click());
  assert.equal(apply().disabled, false);
  await act(async () => apply().click());
  assert.deepEqual(calls[1], ['mcp_only', undefined, 'none', true]);
});


test('MCP Only restores and submits its own public Origin without OAuth', async () => {
  const { default: RemoteAccessPage } = await import('./RemoteAccessPage.tsx');
  const calls = [];
  api.remoteStart = async (...args) => calls.push(args);
  const controller = { state: { mode: 'mcp_only', status: 'stopped', active: false, config: { mode: 'mcp_only', selfHosted: { provider: 'custom_https', publicOrigin: 'https://oauth.example' }, mcpOnly: { securityDeclaration: 'external_auth', publicOrigin: 'https://gateway.example:8443' } } }, busy: '', error: '', operate: async (_label, action) => { await action(); return true; } };
  root = createRoot(document.getElementById('root'));
  await act(async () => root.render(createElement(RemoteAccessPage, { controller, port: 9120, allowLan: false, onSettings() {} })));
  const input = document.getElementById('mcp-only-origin');
  assert.equal(input.value, 'https://gateway.example:8443');
  assert.match(document.querySelector('.remote-detail').textContent, /https:\/\/gateway.example:8443\/mcp/);
  assert.match(document.querySelector('.remote-detail').textContent, /不启用 SerenaDesktop OAuth/);
  assert.match(document.querySelector('.mcp-local-service').textContent, /http:\/\/127\.0\.0\.1:9120\/mcp/);
  assert.ok([...document.querySelectorAll('button')].find(b => b.textContent === '管理 MCP 服务'));
  const applied = [...document.querySelectorAll('button')].find(b => b.textContent === '已应用');
  assert.equal(applied.disabled, true);
  assert.equal(applied.dataset.applied, 'true');
  assert.doesNotMatch(document.querySelector('.mcp-only-footer').textContent, /当前设置已应用/);
  await act(async () => {
    Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value').set.call(input, 'https://new.example');
    input.dispatchEvent(new window.Event('input', { bubbles: true }));
  });
  const apply = [...document.querySelectorAll('button')].find(b => b.textContent === '保存设置');
  assert.equal(apply.dataset.applied, undefined);
  assert.match(document.querySelector('.mcp-only-footer').textContent, /更改将在保存后生效/);
  assert.equal(apply.disabled, false);
  await act(async () => apply.click());
  assert.deepEqual(calls, [['mcp_only', 'https://new.example', 'external_auth', false]]);
});
