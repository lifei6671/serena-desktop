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
const { createElement, act, StrictMode } = await import('react');
const { createRoot } = await import('react-dom/client');
const { TooltipProvider } = await import('./components/ui/tooltip.tsx');
const { Toaster } = await import('./components/ui/sonner.tsx');
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
  await navigate('服务状态');
  assert.ok(document.querySelector('.serena-page'));
  assert.match(document.querySelector('.serena-page').textContent, /test-version/);
  await navigate('设置');
  assert.equal(document.getElementById('broker-port').value, '9234');
  assert.equal(document.querySelector('.project-navigation-slot'), navigation);
  assert.match(navigation.textContent, /Persistent project/);
});

test('Settings keeps its compact contract, truthful detection copy, and broker controls', async () => {
  const originals = { ...api };
  const navigatorDescriptor = Object.getOwnPropertyDescriptor(globalThis, 'navigator');
  const actualPath = 'D:/Serena/actual-runtime/serena.exe';
  const installation = { state: 'standard', source: 'managed', version: 'serena-actual-1.8.2', path: actualPath, context: null, error: null };
  const config = { agentEnabled: false, broker: { enabled: true, port: 9234, allowLan: true }, workspaces: [], serenaPath: null, port: 9345, dashboardEnabled: true, openDashboardOnLaunch: false, autoStartServer: true, minimizeToTray: true };
  const snapshot = { config, git: { available: false, status: 'missing', path: null, version: null, error: null }, serverStatus: 'stopped', installation, activeInstallation: installation, managedRuntimePresent: true, managedProcessPresent: false, activePort: 9345, endpoint: 'http://127.0.0.1:9345/mcp', dashboardEnabled: true, dashboardUrl: 'http://127.0.0.1:24283/dashboard/', autostartEnabled: false, autostartError: null, codegraphVersion: null, lastError: null };
  let brokerRunning = true;
  let resolveSetBroker;
  const clipboard = { writeText: async () => {} };
  Object.defineProperty(globalThis, 'navigator', { value: { clipboard }, configurable: true });
  api.getState = async () => structuredClone(snapshot);
  api.broker = async () => ({ running: brokerRunning, port: 9234, listenAddress: '127.0.0.1', lanEndpoints: [], projects: [], projectSources: [], syncWarnings: [], activeWorkspace: null, codegraph: null, operation: null, lastError: null });
  api.agentHistory = async () => ({ executions: [], nextCursor: null });
  api.codexVersion = async () => 'test-version';
  api.remoteState = async () => ({ mode: 'quick_tunnel', status: 'stopped', publicContext: null, lastError: null, authorizedClients: 0, pending: [], active: false });
  api.setBroker = () => new Promise(resolve => { resolveSetBroker = resolve; });
  const navigate = async () => {
    await act(async () => [...document.querySelectorAll('nav[aria-label="主导航"] button')].find(item => item.textContent === '设置').click());
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 350)); });
  };
  const mountSettings = async () => {
    root = createRoot(document.getElementById('root'));
    await act(async () => root.render(createElement(TooltipProvider, null, createElement(App))));
    await navigate();
    return document.querySelector('.settings-page');
  };
  try {
    let page = await mountSettings();
    assert.deepEqual([...page.querySelectorAll('.settings-section h2')].map(item => item.textContent), ['General', 'Serena', 'Serena 内部服务', 'MCP 连接入口']);
    assert.match(page.querySelector('.settings-autosave-status').textContent, /自动持久化就绪/);
    const detectedPath = page.querySelector('.detected-path');
    assert.match(detectedPath.textContent, new RegExp(actualPath.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')));
    assert.doesNotMatch(detectedPath.textContent, /AppData\\Roaming\\io\.github\.lifei6671\.serena-desktop/);
    const copy = detectedPath.querySelector('.detected-path-copy');
    const copied = [];
    clipboard.writeText = async value => { copied.push(value); };
    await act(async () => copy.click());
    assert.deepEqual(copied, [actualPath]);
    assert.equal(copy.dataset.copied, 'true');
    assert.ok(copy.querySelector('svg.lucide-check'));
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 1550)); });
    assert.equal(copy.dataset.copied, 'false');
    assert.ok(copy.querySelector('svg.lucide-copy'));
    clipboard.writeText = async () => { throw new Error('Clipboard denied'); };
    await act(async () => copy.click());
    assert.equal(copy.dataset.copied, 'false');
    assert.ok(copy.querySelector('svg.lucide-copy'));

    assert.equal(document.getElementById('broker-port').disabled, true);
    assert.equal([...page.querySelectorAll('[role="switch"]')].find(control => control.closest('[data-slot="field"]')?.textContent.includes('允许局域网连接'))?.disabled, true);
    const runningAction = page.querySelector('.settings-broker-action');
    assert.ok(runningAction.querySelector('[data-slot="settings-broker-action-icon"] svg.lucide-stop-circle'));
    assert.ok(runningAction.querySelector('[data-slot="settings-broker-action-label"]'));

    await act(async () => root.unmount());
    root = null;
    brokerRunning = false;
    page = await mountSettings();
    assert.equal(document.getElementById('broker-port').disabled, false);
    assert.equal([...page.querySelectorAll('[role="switch"]')].find(control => control.closest('[data-slot="field"]')?.textContent.includes('允许局域网连接'))?.disabled, false);
    const stoppedAction = page.querySelector('.settings-broker-action');
    assert.ok(stoppedAction.querySelector('[data-slot="settings-broker-action-icon"] svg.lucide-play-circle'));
    await act(async () => stoppedAction.click());
    assert.equal(stoppedAction.getAttribute('aria-busy'), 'true');
    assert.ok(stoppedAction.querySelector('[data-slot="settings-broker-action-icon"] [data-slot="spinner"]'));
    assert.ok(stoppedAction.querySelector('[data-slot="settings-broker-action-label"]'));
    await act(async () => resolveSetBroker());
  } finally {
    Object.assign(api, originals);
    if (navigatorDescriptor) Object.defineProperty(globalThis, 'navigator', navigatorDescriptor);
    else delete globalThis.navigator;
  }
});

test('Service Status renders component icons, truthful actions and independent path copy feedback', async () => {
  const originals = { ...api };
  const project = { id: 'status-workspace', name: 'Active status workspace', root: 'E:/status-workspace' };
  const installation = { state: 'standard', source: 'managed', version: 'serena-actual-1.8.2', path: 'C:/Serena/runtime/serena.exe', context: 'desktop-context', error: null };
  const config = { agentEnabled: false, broker: { enabled: true, port: 9234, allowLan: false }, workspaces: [project], serenaPath: null, port: 9345, dashboardEnabled: true, openDashboardOnLaunch: false, autoStartServer: true, minimizeToTray: true };
  const snapshot = { config, git: { available: true, status: 'available', version: 'git-actual-2.51.3', path: 'C:/Git/cmd/git.exe', error: null }, serverStatus: 'running', installation, activeInstallation: installation, managedRuntimePresent: true, managedProcessPresent: true, activePort: 9345, endpoint: 'http://127.0.0.1:9345/mcp', dashboardEnabled: true, dashboardUrl: 'http://127.0.0.1:24283/dashboard/', autostartEnabled: false, codegraphVersion: 'codegraph-actual-1.9.4', lastError: null };
  const broker = { running: true, port: 9234, listenAddress: '127.0.0.1', lanEndpoints: [], projects: [project], projectSources: [], syncWarnings: [], activeWorkspace: project, codegraph: { status: 'ready', workspaceId: project.id, root: project.root, generation: 1 } };
  let probes = 0;
  let detections = 0;
  let resolveDetection;
  api.getState = async () => structuredClone(snapshot);
  api.broker = async () => structuredClone(broker);
  api.agentHistory = async () => ({ executions: [], nextCursor: null });
  api.codexVersion = async () => { probes++; return 'codex-actual-0.159.7'; };
  api.detect = () => { detections++; return new Promise(resolve => { resolveDetection = resolve; }); };
  api.remoteState = async () => ({ mode: 'quick_tunnel', status: 'stopped', publicContext: null, lastError: null, authorizedClients: 0, pending: [], active: false });
  const page = () => document.querySelector('.serena-page');
  const button = label => [...page().querySelectorAll('button')].find(item => item.textContent === label);
  const navigate = async label => {
    await act(async () => [...document.querySelectorAll('nav[aria-label="主导航"] button')].find(item => item.textContent === label).click());
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 350)); });
  };
  try {
    root = createRoot(document.getElementById('root'));
    await act(async () => root.render(createElement(TooltipProvider, null, createElement(App))));
    await navigate('服务状态');
    const summary = page().querySelector('[aria-label="当前状态"]');
    assert.match(summary.textContent, /运行中/);
    assert.ok(summary.textContent.includes(snapshot.endpoint));
    assert.ok(summary.textContent.includes('http://127.0.0.1:9234/mcp'));
    assert.ok(summary.textContent.includes(project.name));
    assert.match(page().textContent, /运行组件/);
    assert.match(page().textContent, /环境与版本/);
    const rows = [...page().querySelectorAll('tbody tr')];
    assert.equal(rows.length, 4);
    assert.match(rows[0].textContent, /Serena Runtime.*运行中.*serena-actual-1\.8\.2.*9345/);
    assert.match(rows[1].textContent, /MCP Broker.*运行中.*HTTP · 9234.*http:\/\/127\.0\.0\.1:9234\/mcp/);
    assert.match(rows[2].textContent, /Codex CLI.*CLI 可用.*codex-actual-0\.159\.7/);
    assert.match(rows[3].textContent, /CodeGraph.*已就绪.*codegraph-actual-1\.9\.4/);
    const environment = page().querySelector('[aria-labelledby="status-environment-heading"]').textContent;
    for (const value of [installation.version, installation.path, snapshot.git.version, snapshot.git.path, snapshot.dashboardUrl, snapshot.codegraphVersion, 'codex-actual-0.159.7']) assert.ok(environment.includes(value), value);
    assert.doesNotMatch(page().textContent, /PID|CPU|内存|RAM|uptime|运行时长|运行时间|运行进程|Codex Runtime/i);
    for (const [index, icon] of ['server', 'network', 'square-terminal', 'git-branch'].entries()) {
      assert.ok(rows[index].querySelector(`th[scope="row"] svg.lucide-${icon}[aria-hidden="true"]`));
    }
    const toolbar = page().querySelector('.status-action-toolbar');
    assert.doesNotMatch(toolbar.textContent, /重新检测全部|重启 Serena|停止 Serena/);
    const toolbarButtons = [...toolbar.querySelectorAll('button')];
    const actionLabels = toolbarButtons.map(control => control.querySelector(':scope > .status-action-label'));
    for (const [index, control] of toolbarButtons.entries()) {
      assert.ok(control.classList.contains('status-action-button'));
      assert.ok(actionLabels[index]);
      assert.equal(control.children.length, 1);
      assert.equal(control.getAttribute('aria-label'), actionLabels[index].textContent);
    }
    const externalTargets = [];
    let resolveExternal;
    api.openExternal = async target => {
      externalTargets.push(target);
      if (target === 'serena-desktop') await new Promise(resolve => { resolveExternal = resolve; });
    };
    const desktopButton = button('SerenaDesktop GitHub ↗');
    await act(async () => desktopButton.click());
    assert.equal(desktopButton.getAttribute('aria-busy'), 'true');
    assert.equal(desktopButton.textContent, 'SerenaDesktop GitHub ↗');
    assert.equal(desktopButton.getAttribute('aria-label'), 'SerenaDesktop GitHub ↗');
    const indicator = desktopButton.querySelector(':scope > .status-action-busy-indicator');
    assert.ok(indicator.querySelector('[data-slot="spinner"]'));
    assert.equal(indicator.getAttribute('aria-hidden'), 'true');
    assert.equal(desktopButton.children.length, 2);
    assert.equal(toolbar.querySelector(':scope button > svg, [data-icon="inline-start"]'), null);
    assert.equal(toolbar.querySelectorAll('button').length, toolbarButtons.length);
    for (const [index, control] of [...toolbar.querySelectorAll('button')].entries()) {
      assert.equal(control, toolbarButtons[index]);
      assert.equal(control.querySelector(':scope > .status-action-label'), actionLabels[index]);
      assert.equal(control.disabled, true);
      if (control !== desktopButton) assert.equal(control.children.length, 1);
    }
    await act(async () => resolveExternal());
    assert.equal(desktopButton.getAttribute('aria-busy'), 'false');
    assert.equal(desktopButton.disabled, false);
    assert.equal(desktopButton.querySelector('.status-action-busy-indicator'), null);
    assert.equal(toolbar.querySelectorAll('button').length, toolbarButtons.length);
    for (const [index, control] of [...toolbar.querySelectorAll('button')].entries()) {
      assert.equal(control, toolbarButtons[index]);
      assert.equal(control.querySelector(':scope > .status-action-label'), actionLabels[index]);
      assert.equal(control.children.length, 1);
    }
    await act(async () => button('Serena GitHub ↗').click());
    await act(async () => button('CodeGraph GitHub ↗').click());
    assert.deepEqual(externalTargets, ['serena-desktop', 'github', 'codegraph']);

    const copyButtons = [...page().querySelectorAll('.status-environment-value button')];
    assert.deepEqual(copyButtons.map(item => item.getAttribute('aria-label')), ['复制 Serena', '复制 MCP Broker', '复制 Git', '复制 Dashboard']);
    const [serenaCopy, brokerCopy, gitCopy, dashboardCopy] = copyButtons;
    const navigatorDescriptor = Object.getOwnPropertyDescriptor(globalThis, 'navigator');
    const { toast } = await import('sonner');
    const originalToastError = toast.error;
    const errors = [];
    const copiedValues = [];
    let resolveCopy;
    const clipboard = { writeText: value => {
      copiedValues.push(value);
      return new Promise(resolve => { resolveCopy = resolve; });
    } };
    Object.defineProperty(globalThis, 'navigator', { configurable: true, value: { clipboard } });
    toast.error = message => { errors.push(message); };
    try {
      await act(async () => serenaCopy.click());
      assert.deepEqual(copiedValues, [installation.path]);
      assert.equal(serenaCopy.disabled, true);
      assert.equal(serenaCopy.getAttribute('aria-busy'), 'true');
      assert.equal(serenaCopy.dataset.copied, 'false');
      for (const other of [brokerCopy, gitCopy, dashboardCopy]) {
        assert.equal(other.disabled, false);
        assert.equal(other.dataset.copied, 'false');
        assert.ok(other.querySelector('svg.lucide-copy'));
      }
      await act(async () => resolveCopy());
      assert.equal(serenaCopy.getAttribute('aria-busy'), 'false');
      assert.equal(serenaCopy.getAttribute('aria-label'), '已复制 Serena');
      assert.ok(serenaCopy.querySelector('svg.lucide-check'));
      for (const other of [brokerCopy, gitCopy, dashboardCopy]) {
        assert.equal(other.disabled, false);
        assert.equal(other.dataset.copied, 'false');
      }
      assert.deepEqual(errors, []);

      clipboard.writeText = async value => { copiedValues.push(value); throw new Error('Clipboard denied'); };
      await act(async () => gitCopy.click());
      assert.equal(copiedValues.at(-1), snapshot.git.path);
      assert.deepEqual(errors, ['复制失败：Error: Clipboard denied']);
      assert.equal(gitCopy.disabled, false);
      assert.equal(gitCopy.getAttribute('aria-busy'), 'false');
      assert.equal(gitCopy.getAttribute('aria-label'), '复制 Git');
      assert.equal(gitCopy.dataset.copied, 'false');
      assert.equal(gitCopy.querySelector('svg.lucide-check'), null);
      assert.ok(gitCopy.querySelector('svg.lucide-copy'));
      assert.equal(serenaCopy.dataset.copied, 'true');
      await act(async () => { await new Promise(resolve => setTimeout(resolve, 1150)); });
      assert.equal(serenaCopy.dataset.copied, 'true');
      await act(async () => { await new Promise(resolve => setTimeout(resolve, 400)); });
      assert.equal(serenaCopy.getAttribute('aria-label'), '复制 Serena');
      assert.ok(serenaCopy.querySelector('svg.lucide-copy'));
      assert.equal(serenaCopy.disabled, false);

      clipboard.writeText = async value => { copiedValues.push(value); };
      for (const [control, value] of [[brokerCopy, 'http://127.0.0.1:9234/mcp'], [gitCopy, snapshot.git.path], [dashboardCopy, snapshot.dashboardUrl]]) {
        await act(async () => control.click());
        assert.equal(copiedValues.at(-1), value);
        assert.ok(control.querySelector('svg.lucide-check'));
      }
      assert.equal(serenaCopy.dataset.copied, 'false');
    } finally {
      if (navigatorDescriptor) Object.defineProperty(globalThis, 'navigator', navigatorDescriptor);
      else delete globalThis.navigator;
      toast.error = originalToastError;
    }
    assert.equal(button('打开 Dashboard').disabled, false);
    assert.equal(probes, 1);
    await navigate('首页'); await navigate('服务状态');
    assert.equal(probes, 1);
    await act(async () => button('重新检测').click());
    assert.equal(probes, 2);
    assert.equal(detections, 1);
    assert.equal(button('重新检测').getAttribute('aria-busy'), 'true');
    assert.equal(button('重新检测').disabled, true);
    assert.equal(button('打开 Dashboard').disabled, true);
    await act(async () => resolveDetection(structuredClone(snapshot)));
    assert.equal(button('重新检测').getAttribute('aria-busy'), 'false');

    // A fresh controller snapshot must remove the running endpoint and workspace.
    await act(async () => root.unmount()); root = null;
    snapshot.serverStatus = 'stopped'; snapshot.managedProcessPresent = false;
    snapshot.dashboardEnabled = false; snapshot.lastError = 'Actual Serena error';
    snapshot.git = { available: false, status: 'error', version: null, path: null, error: 'Actual Git error' };
    broker.running = false; broker.activeWorkspace = null; broker.codegraph = null;
    api.codexVersion = async () => { throw new Error('Actual Codex error'); };
    root = createRoot(document.getElementById('root'));
    await act(async () => root.render(createElement(TooltipProvider, null, createElement(App))));
    await navigate('服务状态');
    assert.match(page().querySelector('[aria-label="当前状态"]').textContent, /已停止.*未激活/);
    assert.doesNotMatch(page().textContent, /http:\/\/127\.0\.0\.1:9234\/mcp/);
    assert.match(page().querySelector('tbody').textContent, /CodeGraph.*待激活/);
    assert.match(page().textContent, /Actual Codex error/);
    assert.match(page().textContent, /Actual Git error/);
    assert.match(page().textContent, /Last ErrorActual Serena error/);
    assert.match(page().textContent, /Dashboard已关闭/);
    assert.equal(button('停止 Serena'), undefined);
    assert.equal(button('启动 Serena').disabled, true);
    assert.equal(button('打开 Dashboard').disabled, true);
    assert.ok(button('打开 Git 下载页面 ↗'));
    assert.deepEqual([...page().querySelectorAll('.status-environment-value button')].map(item => item.getAttribute('aria-label')), ['复制 Serena']);
    for (const control of page().querySelectorAll('.status-action-toolbar button')) {
      assert.ok(control.classList.contains('status-action-button'));
      assert.equal(control.children.length, 1);
      assert.equal(control.firstElementChild.className, 'status-action-label');
    }
    assert.equal(button('启动 Serena').closest('.status-lifecycle-action') !== null, true);

    // The install branch uses the same stable label structure as stopped actions.
    await act(async () => root.unmount()); root = null;
    snapshot.installation = null; snapshot.activeInstallation = null;
    snapshot.managedRuntimePresent = false;
    root = createRoot(document.getElementById('root'));
    await act(async () => root.render(createElement(TooltipProvider, null, createElement(App))));
    await navigate('服务状态');
    const installButton = button('安装 官方 Serena');
    assert.ok(installButton.classList.contains('status-action-button'));
    assert.equal(installButton.children.length, 1);
    assert.equal(installButton.firstElementChild.className, 'status-action-label');
    assert.equal(installButton.getAttribute('aria-label'), '安装 官方 Serena');
  } finally { Object.assign(api, originals); }
});

test('task detail clears the Agent main-navigation selection until returning to the list', async () => {
  const project = { id: 'P', name: 'Task project', root: 'E:/task-project' };
  const config = { agentEnabled: true, broker: { enabled: true, port: 9120, allowLan: false }, workspaces: [project], serenaPath: null, port: 9121, dashboardEnabled: true, openDashboardOnLaunch: false, autoStartServer: true, minimizeToTray: true };
  const snapshot = { config, git: { available: true, status: 'available' }, serverStatus: 'running', installation: null, activeInstallation: { state: 'standard', version: '1.7.0' }, managedRuntimePresent: true, managedProcessPresent: true, activePort: 9121, autostartEnabled: false, codegraphVersion: '1.6.0' };
  const task = { executionId: 'task-1', agentId: 'agent-1', workspaceId: 'P', canonicalWorkspaceRoot: project.root, prompt: '验证任务详情导航', status: 'completed', attention: 'none', revision: 'R1', resultAvailable: false, finalResult: null, progress: { phase: 'completed' }, nextAction: null, dispatchState: 'accepted', threadId: null, threadName: null, turnId: null, providerTerminalStatus: null, errorCode: null, errorMessage: null, resultCompleteness: 'none', interruptRequested: false, interruptAcknowledged: false, interruptTimedOut: false, createdAt: 1000, updatedAt: 2000, completedAt: 3000, availableActions: { canCancel: false, canContinue: false, canResumePending: false } };
  api.getState = async () => structuredClone(snapshot);
  api.broker = async () => ({ running: true, port: 9120, listenAddress: '127.0.0.1', lanEndpoints: [], projects: [project], projectSources: [], syncWarnings: [], activeWorkspace: project, codegraph: { status: 'ready' } });
  api.agentHistory = async () => ({ executions: [structuredClone(task)], nextCursor: null });
  api.agent = async () => ({ ok: true, data: structuredClone(task) });
  api.codexVersion = async () => 'test-version';
  api.remoteState = async () => ({ mode: 'quick_tunnel', status: 'stopped', publicContext: null, lastError: null, authorizedClients: 0, pending: [], active: false });
  root = createRoot(document.getElementById('root'));
  await act(async () => root.render(createElement(TooltipProvider, null, createElement(App))));
  await act(async () => { await new Promise(resolve => setTimeout(resolve, 50)); });
  const taskLink = document.querySelector('.project-task-link');
  assert.ok(taskLink);
  await act(async () => taskLink.click());
  await act(async () => { await new Promise(resolve => setTimeout(resolve, 350)); });
  const agentNavigation = [...document.querySelectorAll('nav[aria-label="主导航"] button')].find(button => button.textContent === 'Agent 编排');
  assert.ok(document.querySelector('.agent-detail'));
  assert.equal(agentNavigation.getAttribute('aria-current'), null);
  assert.equal(document.querySelector('.project-task-link').getAttribute('aria-current'), 'page');
  await act(async () => agentNavigation.click());
  assert.equal(document.querySelector('.agent-detail'), null);
  assert.equal(agentNavigation.getAttribute('aria-current'), 'page');
  assert.equal(document.querySelector('.project-task-link').getAttribute('aria-current'), null);
  await act(async () => document.querySelector('.project-task-link').click());
  await act(async () => { await new Promise(resolve => setTimeout(resolve, 350)); });
  assert.ok(document.querySelector('.agent-detail'));
  assert.equal(agentNavigation.getAttribute('aria-current'), null);
  await act(async () => [...document.querySelectorAll('button')].find(button => button.textContent === '全部任务').click());
  assert.equal(document.querySelector('.agent-detail'), null);
  assert.equal(agentNavigation.getAttribute('aria-current'), 'page');
  assert.equal(document.querySelector('.project-task-link').getAttribute('aria-current'), null);
});

test('homepage keeps the real service and endpoint data in its compact shell', async () => {
  const config = { agentEnabled: false, broker: { enabled: true, port: 9120, allowLan: true }, workspaces: [], serenaPath: null, port: 9121, dashboardEnabled: true, openDashboardOnLaunch: false, autoStartServer: true, minimizeToTray: true };
  const snapshot = { config, git: { available: true, status: 'available', version: '2.50.0' }, serverStatus: 'running', installation: null, activeInstallation: { state: 'standard', version: '1.7.0' }, managedRuntimePresent: true, managedProcessPresent: true, activePort: 9121, autostartEnabled: false, codegraphVersion: '1.6.0' };
  api.getState = async () => structuredClone(snapshot);
  api.broker = async () => ({ running: true, port: 9120, listenAddress: '0.0.0.0', lanEndpoints: ['http://10.0.0.2:9120/mcp', 'http://192.168.1.2:9120/mcp'], projects: [{ id: 'W', name: 'serena-desktop', root: 'E:/serena-desktop', configured: true }], projectSources: [], syncWarnings: [], activeWorkspace: { id: 'W', name: 'serena-desktop', root: 'E:/serena-desktop' }, codegraph: { status: 'ready' } });
  api.syncProjects = async () => 1;
  api.agentHistory = async () => ({ executions: [], nextCursor: null });
  api.codexVersion = async () => 'test-version';
  api.remoteState = async () => ({ mode: 'quick_tunnel', status: 'stopped', publicContext: null, lastError: null, authorizedClients: 0, pending: [], active: false });
  root = createRoot(document.getElementById('root'));
  await act(async () => root.render(createElement(TooltipProvider, null, createElement(App))));
  assert.equal(document.querySelector('.app-titlebar'), null);
  assert.deepEqual([...document.querySelectorAll('nav[aria-label="主导航"] button')].map(button => button.textContent), ['首页', '服务状态', 'Agent 编排', '日志终端', '远程访问', '设置']);
  assert.equal(document.querySelector('nav[aria-label="主导航"] [aria-current="page"]').textContent, '首页');
  assert.match(document.querySelector('.workspace-summary').textContent, /serena-desktop/);
  assert.equal(document.querySelectorAll('.service-list > .service-row').length, 4);
  assert.equal(document.querySelector('.connection-endpoint-card code').textContent, 'http://127.0.0.1:9120/mcp');
  assert.equal(document.querySelectorAll('.lan-endpoint-row').length, 2);
  assert.equal(document.querySelector('footer .mono').textContent, 'Serena 内部端口：9121');

  const wait = async ms => act(async () => { await new Promise(resolve => setTimeout(resolve, ms)); });
  const syncButton = document.querySelector('.sync-project-button');
  await act(async () => syncButton.click());
  assert.match(syncButton.textContent, /同步中…/);
  assert.equal(syncButton.disabled, true);
  await wait(250);
  assert.match(syncButton.textContent, /同步中…/);
  await wait(450);
  assert.match(syncButton.textContent, /已同步/);
  assert.equal(syncButton.disabled, true);
  await wait(1600);
  assert.match(syncButton.textContent, /同步项目/);
  assert.equal(syncButton.disabled, false);

  let resolveCopy;
  const navigatorDescriptor = Object.getOwnPropertyDescriptor(globalThis, 'navigator');
  Object.defineProperty(globalThis, 'navigator', {
    configurable: true,
    value: { clipboard: { writeText: () => new Promise(resolve => { resolveCopy = resolve; }) } },
  });
  try {
    const localCopy = document.querySelector('.endpoint-copy-primary button');
    const lanCopies = [...document.querySelectorAll('.lan-endpoint-row button')];
    await act(async () => lanCopies[0].click());
    assert.equal(lanCopies[0].disabled, true);
    assert.equal(lanCopies[0].getAttribute('aria-busy'), 'true');
    assert.ok(lanCopies[0].querySelector('[data-icon="inline-start"]'));
    assert.match(lanCopies[0].textContent, /复制中…/);
    assert.equal(localCopy.disabled, false);
    assert.equal(localCopy.getAttribute('aria-busy'), 'false');
    assert.equal(localCopy.querySelector('[data-icon="inline-start"]'), null);
    assert.equal(lanCopies[1].disabled, false);
    assert.equal(lanCopies[1].getAttribute('aria-busy'), 'false');
    assert.equal(lanCopies[1].querySelector('[data-icon="inline-start"]'), null);
    await act(async () => { resolveCopy(); await Promise.resolve(); });
    await wait(250);
    assert.match(lanCopies[0].textContent, /复制中…/);
    assert.equal(lanCopies[0].disabled, true);
    assert.match(localCopy.textContent, /^复制$/);
    assert.match(lanCopies[1].textContent, /复制局域网地址/);
    await wait(350);
    assert.match(lanCopies[0].textContent, /已复制/);
    assert.equal(lanCopies[0].disabled, true);
    assert.equal(lanCopies[0].getAttribute('aria-busy'), 'false');
    assert.match(localCopy.textContent, /^复制$/);
    assert.match(lanCopies[1].textContent, /复制局域网地址/);
    await wait(1600);
    assert.match(lanCopies[0].textContent, /复制局域网地址/);
    assert.equal(lanCopies[0].disabled, false);
  } finally {
    if (navigatorDescriptor) Object.defineProperty(globalThis, 'navigator', navigatorDescriptor);
    else delete globalThis.navigator;
  }
});

test('remote access keeps viewed/runtime modes separate and renders an honest Quick Tunnel console', async () => {
  const { default: RemoteAccessPage } = await import('./RemoteAccessPage.tsx');
  const state = { mode: 'mcp_only', status: 'stopped', publicContext: null, lastError: null, authorizedClients: 0, pending: [], active: false };
  const calls = [];
  api.remoteStart = async mode => { calls.push(mode); };
  const controller = { state, busy: '', error: '', operate: async (_label, action) => { await action(); return true; }, startQuickTunnel: async () => { await api.remoteStart('quick_tunnel'); return true; } };
  root = createRoot(document.getElementById('root'));
  const render = async () => act(async () => root.render(createElement(RemoteAccessPage, { controller, port: 9120, allowLan: false, onSettings() {} })));
  await render();
  assert.equal(document.querySelectorAll('input[name="remote-mode"]').length, 3);
  assert.equal(document.querySelectorAll('.remote-mode').length, 3);
  assert.equal(document.querySelectorAll('.remote-mode small').length, 0);
  assert.equal(document.querySelector('.remote-current-mode'), null);
  assert.ok(document.querySelector('.remote-runtime-summary'));
  assert.ok(document.querySelector('.remote-summary-main'));
  assert.ok(document.querySelector('#remote-diagnostics'));
  assert.equal(document.querySelectorAll('.remote-diagnostic-item').length, 3);
  assert.match(document.querySelector('#remote-diagnostics').textContent, /本地服务诊断.*本地 Endpoint.*公网 OAuth 诊断.*不适用/);
  assert.doesNotMatch(document.querySelector('#remote-diagnostics').textContent, /OAuth Metadata|DNS|MCP Initialize/);
  assert.doesNotMatch(document.querySelector('.remote-summary-metrics').textContent, /连接持续时间|延迟|\d+\s*(ms|秒|分钟|小时)/);
  assert.equal(document.querySelector('.remote-quick-card'), null);
  assert.equal(document.querySelector('.remote-status'), null);
  assert.equal(document.querySelector('.remote-page-actions').closest('.page-heading') !== null, true);
  assert.equal(document.querySelector('.remote-summary-metrics > div').dataset.ready, 'false');
  const button = text => [...document.querySelectorAll('button')].find(b => b.textContent === text);
  assert.ok(button('网络诊断报告'));
  assert.equal(button('重新检测全部'), undefined);
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
  assert.match(document.querySelector('.remote-runtime-summary').textContent, /仅 MCP.*127\.0\.0\.1.*PUBLIC ENDPOINT.*尚未就绪/);
  assert.match(modeLabel('mcp_only').textContent, /当前配置/);
  assert.ok(document.querySelector('.remote-quick-card'));
  assert.match(document.querySelector('.remote-quick-card').textContent, /Cloudflare Quick Tunnel 运行状态.*内核自动代理.*临时公网 Endpoint.*运行机制与生命周期说明/);
  assert.doesNotMatch(document.querySelector('.remote-quick-card').textContent, /LIFECYCLE NOTE|当前运行：仅 MCP/);
  assert.equal(document.getElementById('remote-url').value, '');
  assert.equal(button('复制地址').disabled, true);
  assert.ok(button('切换到快捷隧道').querySelector('svg.lucide-arrow-right-left'));
  assert.match(document.querySelector('.remote-quick-footer').textContent, /快捷隧道未运行/);
  assert.match(document.querySelector('.remote-detail').textContent, /自动创建临时 HTTPS 地址/);
  assert.match(document.querySelector('.remote-detail').textContent, /http:\/\/127\.0\.0\.1:9120\/mcp/);
  assert.match(document.querySelector('.remote-detail').textContent, /SerenaDesktop OAuth 2\.0/);
  assert.doesNotMatch(document.querySelector('.remote-detail').textContent, /trycloudflare\.com/);
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
  assert.equal(document.querySelector('.remote-error'), null);
  assert.equal(document.getElementById('remote-url').value, 'https://old.trycloudflare.com/mcp');
  assert.match(document.querySelector('.remote-runtime-summary').textContent, /https:\/\/old\.trycloudflare\.com\/mcp/);
  assert.equal(document.querySelector('fieldset').disabled, false);
  assert.equal(selectedModeName(), '快捷隧道');
  assert.match(document.querySelector('.remote-runtime-summary').textContent, /快捷隧道（当前生效）.*公网状态:.*已验证/);
  assert.match(modeLabel('quick_tunnel').textContent, /当前运行/);
  assert.match(document.querySelector('.remote-quick-card').textContent, /已授权客户端：0/);
  assert.ok(button('复制地址'));
  assert.ok(button('停止远程访问'));
  const switches = [];
  api.remoteStart = async (...args) => { switches.push(args); };
  await act(async () => document.querySelector('input[value="self_hosted_oauth"]').click());
  assert.equal(selectedModeName(), '自建接入');
  assert.match(modeLabel('quick_tunnel').textContent, /当前运行/);
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
  assert.equal(document.getElementById('remote-url').value, '');
  assert.equal(selectedModeName(), '快捷隧道');
  assert.equal(button('复制地址').disabled, true);
  assert.ok(button('重新启动快捷隧道'));
  await act(async () => root.render(createElement(RemoteAccessPage, { controller, port: 9120, allowLan: true, onSettings() {} })));
  assert.equal(button('重新启动快捷隧道').disabled, false);
  assert.equal(document.querySelectorAll('.remote-quick-lifecycle').length, 1);
  assert.match(document.querySelector('.remote-quick-lifecycle').textContent, /局域网客户端仍需要 OAuth 授权/);
  await act(async () => button('重新启动快捷隧道').click());
  assert.deepEqual(calls, ['quick_tunnel']);
  assert.deepEqual(switches.at(-1), ['quick_tunnel']);
  controller.state = { ...state, mode: 'quick_tunnel', status: 'starting', active: true, publicContext: null };
  await render();
  assert.equal(document.getElementById('remote-url').value, '');
  assert.match(document.querySelector('.remote-detail').textContent, /自动创建临时 HTTPS 地址/);
  assert.doesNotMatch(document.querySelector('.remote-detail').textContent, /启动本地 MCP 服务|cloudflared/);
  assert.ok(button('取消启动'));
  controller.state = { ...state, mode: 'self_hosted_oauth', status: 'stopped', active: false, publicContext: null };
  await render();
  assert.ok(button('切换到快捷隧道'));
});

test('Quick Tunnel terminal transitions restore the matching action and badge', async () => {
  const { default: RemoteAccessPage } = await import('./RemoteAccessPage.tsx');
  let state = { mode: 'quick_tunnel', status: 'error', active: false, lastError: 'OLD_ERROR', publicContext: null, pending: [] };
  const controller = { get state() { return state; }, busy: '', error: '', operate: async (_label, action) => { await action(); return true; } };
  const button = text => [...document.querySelectorAll('button')].find(item => item.textContent === text);
  const render = async () => act(async () => root.render(createElement(RemoteAccessPage, { controller, port: 9120, allowLan: false, onSettings() {} })));
  try {
    root = createRoot(document.getElementById('root'));
    await render();
    assert.equal(button('取消启动'), undefined);
    assert.ok(button('重新启动快捷隧道').querySelector('svg.lucide-arrow-right-left'));
    assert.equal(document.querySelector('.remote-mode-runtime').dataset.state, 'failed');

    state = { ...state, status: 'starting', active: true, lastError: null };
    await render();
    assert.ok(button('取消启动').querySelector('svg.lucide-square'));
    assert.equal(document.querySelector('.remote-mode-runtime').dataset.state, 'connecting');
    assert.match(document.querySelector('.remote-mode-runtime').textContent, /连接中/);
    state = { ...state, status: 'error', active: false, lastError: 'START_FAILED' };
    await render();
    assert.equal(button('取消启动'), undefined);
    assert.ok(button('重新启动快捷隧道'));
    assert.equal(document.querySelector('.remote-mode-runtime').dataset.state, 'failed');
    await render();

    state = { ...state, status: 'starting', active: true, lastError: null };
    await render();
    state = { ...state, status: 'disconnected', active: false, lastError: 'TUNNEL_DISCONNECTED' };
    await render();
    assert.equal(button('取消启动'), undefined);
    assert.ok(button('重新启动快捷隧道'));
    await render();

    state = { ...state, status: 'error', active: true, lastError: 'QUICK_TUNNEL_STOP_FAILED' };
    await render();
    assert.match(document.querySelector('.remote-mode-runtime').textContent, /连接异常/);
    assert.equal(document.querySelector('.remote-mode-runtime').dataset.state, 'failed');
    assert.equal(button('重新启动快捷隧道'), undefined);
    assert.ok(button('停止并清理').querySelector('svg.lucide-square'));

    state = { ...state, status: 'stopped', active: false, lastError: null };
    await render();
    assert.equal(button('取消启动'), undefined);
    assert.ok(button('启动快捷隧道'));
    assert.equal(document.querySelector('.remote-mode-runtime').dataset.state, 'configured');
    state = { ...state, status: 'stopping', active: true };
    await render();
    assert.equal(document.querySelector('.remote-mode-runtime').dataset.state, 'stopping');
    assert.match(document.querySelector('.remote-mode-runtime').textContent, /停止中/);
    state = { ...state, status: 'ready', active: true, publicContext: { mcpResource: 'https://ready.example/mcp' } };
    await render();
    assert.equal(button('取消启动'), undefined);
    assert.ok(button('停止远程访问').querySelector('svg.lucide-square'));
  } finally {
  }
});

test('Quick Tunnel attempt survives remote page unmount and only toasts once', async () => {
  const { toast } = await import('sonner');
  const originalError = toast.error;
  const notices = [];
  toast.error = message => notices.push(message);
  const originals = { ...api };
  const config = { agentEnabled: false, broker: { enabled: false, port: 9120, allowLan: false }, workspaces: [], serenaPath: null, port: 9121, dashboardEnabled: false, openDashboardOnLaunch: false, autoStartServer: false, minimizeToTray: false };
  const appState = { config, git: { available: false, status: 'missing' }, serverStatus: 'stopped', installation: null, activeInstallation: null, managedRuntimePresent: false, managedProcessPresent: false, activePort: 9121, autostartEnabled: false, codegraphVersion: null };
  let remoteState = { mode: 'mcp_only', status: 'stopped', active: false, pending: [], lastError: null, publicContext: null, authorizedClients: 0 };
  api.getState = async () => structuredClone(appState);
  api.broker = async () => ({ running: false, projects: [], projectSources: [], syncWarnings: [], activeWorkspace: null, codegraph: null });
  api.agentHistory = async () => ({ executions: [], nextCursor: null });
  api.codexVersion = async () => 'test-version';
  api.remoteState = async () => structuredClone(remoteState);
  api.remoteStart = async () => { remoteState = { ...remoteState, mode: 'quick_tunnel', status: 'starting', active: true }; };
  const navigate = async label => {
    await act(async () => [...document.querySelectorAll('nav[aria-label="主导航"] button')].find(item => item.textContent === label).click());
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 350)); });
  };
  try {
    root = createRoot(document.getElementById('root'));
    await act(async () => root.render(createElement(TooltipProvider, null, createElement(App))));
    await navigate('远程访问');
    await act(async () => document.querySelector('input[value="quick_tunnel"]').click());
    await act(async () => [...document.querySelectorAll('button')].find(item => item.textContent === '切换到快捷隧道').click());
    await navigate('首页');
    remoteState = { ...remoteState, status: 'error', active: false, lastError: 'QUICK_TUNNEL_START_FAILED' };
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 1100)); });
    assert.deepEqual(notices, ['快捷隧道启动失败，请重新启动。']);
    await navigate('远程访问');
    assert.equal(notices.length, 1);
  } finally {
    Object.assign(api, originals);
    toast.error = originalError;
  }
});

test('Quick Tunnel terminal failure renders one visible Sonner toast', async () => {
  const originals = { ...api };
  const config = { agentEnabled: false, broker: { enabled: false, port: 9120, allowLan: false }, workspaces: [], serenaPath: null, port: 9121, dashboardEnabled: false, openDashboardOnLaunch: false, autoStartServer: false, minimizeToTray: false };
  const appState = { config, git: { available: false, status: 'missing' }, serverStatus: 'stopped', installation: null, activeInstallation: null, managedRuntimePresent: false, managedProcessPresent: false, activePort: 9121, autostartEnabled: false, codegraphVersion: null };
  let remoteState = { mode: 'mcp_only', status: 'stopped', active: false, pending: [], lastError: null, publicContext: null, authorizedClients: 0 };
  api.getState = async () => structuredClone(appState);
  api.broker = async () => ({ running: false, projects: [], projectSources: [], syncWarnings: [], activeWorkspace: null, codegraph: null });
  api.agentHistory = async () => ({ executions: [], nextCursor: null });
  api.codexVersion = async () => 'test-version';
  api.remoteState = async () => structuredClone(remoteState);
  api.remoteStart = async () => { remoteState = { ...remoteState, mode: 'quick_tunnel', status: 'error', active: false, lastError: 'QUICK_TUNNEL_START_FAILED' }; };
  const navigate = async label => {
    await act(async () => [...document.querySelectorAll('nav[aria-label="主导航"] button')].find(item => item.textContent === label).click());
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 350)); });
  };
  try {
    root = createRoot(document.getElementById('root'));
    await act(async () => root.render(
      createElement(StrictMode, null,
        createElement(TooltipProvider, null,
          createElement(App),
          createElement(Toaster, { theme: 'light', duration: 1600 }),
        ),
      ),
    ));
    await navigate('远程访问');
    await act(async () => document.querySelector('input[value="quick_tunnel"]').click());
    await act(async () => [...document.querySelectorAll('button')].find(item => item.textContent === '切换到快捷隧道').click());
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 40)); });
    const toasts = [...document.querySelectorAll('[data-sonner-toast]')].filter(item => item.textContent.includes('快捷隧道启动失败，请重新启动。'));
    assert.equal(toasts.length, 1);
    assert.match(toasts[0].textContent, /快捷隧道启动失败，请重新启动。/);
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 1100)); });
    assert.equal([...document.querySelectorAll('[data-sonner-toast]')].filter(item => item.textContent.includes('快捷隧道启动失败，请重新启动。')).length, 1);
  } finally {
    Object.assign(api, originals);
  }
});

test('Quick Tunnel command rejection shows only its dedicated toast in the mounted page', async () => {
  const { toast } = await import('sonner');
  const originalError = toast.error;
  const notices = [];
  toast.error = message => notices.push(message);
  const originals = { ...api };
  const config = { agentEnabled: false, broker: { enabled: false, port: 9120, allowLan: false }, workspaces: [], serenaPath: null, port: 9121, dashboardEnabled: false, openDashboardOnLaunch: false, autoStartServer: false, minimizeToTray: false };
  const appState = { config, git: { available: false, status: 'missing' }, serverStatus: 'stopped', installation: null, activeInstallation: null, managedRuntimePresent: false, managedProcessPresent: false, activePort: 9121, autostartEnabled: false, codegraphVersion: null };
  const remoteState = { mode: 'mcp_only', status: 'stopped', active: false, pending: [], lastError: null, publicContext: null, authorizedClients: 0 };
  api.getState = async () => structuredClone(appState);
  api.broker = async () => ({ running: false, projects: [], projectSources: [], syncWarnings: [], activeWorkspace: null, codegraph: null });
  api.agentHistory = async () => ({ executions: [], nextCursor: null });
  api.codexVersion = async () => 'test-version';
  api.remoteState = async () => structuredClone(remoteState);
  api.remoteStart = async () => { throw new Error('REMOTE_ACCESS_ALREADY_RUNNING'); };
  const navigate = async label => {
    await act(async () => [...document.querySelectorAll('nav[aria-label="主导航"] button')].find(item => item.textContent === label).click());
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 350)); });
  };
  try {
    root = createRoot(document.getElementById('root'));
    await act(async () => root.render(createElement(TooltipProvider, null, createElement(App))));
    await navigate('远程访问');
    await act(async () => document.querySelector('input[value="quick_tunnel"]').click());
    await act(async () => [...document.querySelectorAll('button')].find(item => item.textContent === '切换到快捷隧道').click());
    assert.deepEqual(notices, ['快捷隧道启动失败，请检查状态后重试。']);
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 1100)); });
    assert.deepEqual(notices, ['快捷隧道启动失败，请检查状态后重试。']);
  } finally {
    Object.assign(api, originals);
    toast.error = originalError;
  }
});

test('Quick Tunnel probe exposes its real busy transition until either outcome settles', async () => {
  const { default: RemoteAccessPage } = await import('./RemoteAccessPage.tsx');
  const { useState } = await import('react');
  const { toast } = await import('sonner');
  const originalError = toast.error;
  const errors = [];
  toast.error = message => errors.push(message);
  let pending = Promise.withResolvers();
  api.remoteProbe = () => pending.promise;
  function ProbeHarness() {
    const [busy, setBusy] = useState('');
    const [error, setError] = useState('');
    const state = { mode: 'quick_tunnel', status: 'ready', active: true, pending: [], publicContext: { mcpResource: 'https://ready.example/mcp' } };
    const controller = { state, busy, error, operate: async (label, action) => {
      setBusy(label);
      try { await action(); return true; }
      catch (failure) { setError(String(failure)); return false; }
      finally { setBusy(''); }
    } };
    return createElement(RemoteAccessPage, { controller, port: 9120, allowLan: false, onSettings() {} });
  }
  const quickButton = () => [...document.querySelector('.remote-quick-footer').querySelectorAll('button')].find(item => /测试连接|正在测试/.test(item.textContent));
  const allBusyButtons = () => [...document.querySelectorAll('button')].filter(item => item.textContent === '正在测试…');
  try {
    root = createRoot(document.getElementById('root'));
    await act(async () => root.render(createElement(ProbeHarness)));
    await act(async () => { quickButton().click(); await Promise.resolve(); });
    assert.equal(quickButton().textContent, '正在测试…');
    assert.equal(quickButton().disabled, true);
    assert.ok(quickButton().querySelector('svg.lucide-refresh-cw.animate-spin'));
    assert.equal(allBusyButtons().length, 3);
    assert.ok(allBusyButtons().every(item => item.disabled && item.querySelector('svg.lucide-refresh-cw.animate-spin')));
    await act(async () => { pending.resolve(); await Promise.resolve(); });
    assert.equal(quickButton().textContent, '测试连接');
    assert.equal(quickButton().disabled, false);

    pending = Promise.withResolvers();
    await act(async () => { quickButton().click(); await Promise.resolve(); });
    assert.equal(quickButton().textContent, '正在测试…');
    await act(async () => { pending.reject(new Error('probe failed')); await Promise.resolve(); });
    assert.equal(quickButton().textContent, '测试连接');
    assert.equal(quickButton().disabled, false);
    assert.deepEqual(errors, ['连接测试失败，请查看远程状态中的错误详情']);
  } finally {
    toast.error = originalError;
  }
});

test('remote ordinary UI inherits Alibaba PuHuiTi while endpoint data remains monospace', () => {
  const css = readFileSync('src/styles.css', 'utf8');
  assert.match(css, /--font-sans:\s*"Alibaba PuHuiTi"/);
  for (const selector of [
    '.remote-summary-product, .remote-summary-lifecycle, .remote-endpoint-status',
    '.remote-mode-copy > span',
    '.remote-quick-header > span',
    '.remote-quick-status',
    '.remote-quick-fact-heading small',
    '.remote-quick-facts strong',
    '.remote-diagnostic-item > div > span',
    '.remote-diagnostic-item > span',
  ]) {
    const rule = css.slice(css.indexOf(selector), css.indexOf('}', css.indexOf(selector)) + 1);
    assert.doesNotMatch(rule, /Consolas|font-family|font:\s*\d/);
  }
  for (const selector of ['.remote-summary-endpoint code', '.remote-address input', '.remote-quick-facts code', '.remote-confirmation']) {
    const rules = [...css.matchAll(new RegExp(`${selector.replace(/[.*+?^${}()|[\\]\\]/g, "\\\\$&")}\\s*\\{[^}]*\\}`, 'g'))].map(match => match[0]);
    const rule = rules.find(candidate => candidate.includes('font:'));
    assert.match(rule, /Consolas/);
  }
});

test('self hosted entry submits origin and hides resources from other modes', async () => {
  const { default: RemoteAccessPage } = await import('./RemoteAccessPage.tsx');
  const calls = [];
  api.remoteStart = async (...args) => calls.push(args);
  const controller = { state: { mode: 'mcp_only', status: 'stopped', active: false }, busy: '', error: '', operate: async (_label, action) => { await action(); return true; }, startQuickTunnel: async () => { await api.remoteStart('quick_tunnel'); return true; } };
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
  assert.match(document.querySelector('input[name="self-hosted-provider"][value="custom_https"]').closest('.self-hosted-provider-choice').textContent, /当前运行/);
  assert.equal([...document.querySelectorAll('button')].find(b => b.textContent === '切换到 ngrok').disabled, true);
  await act(async () => document.querySelector('input[name="self-hosted-provider"][value="custom_https"]').click());
  await act(async () => document.querySelector('input[value="quick_tunnel"]').click());
  assert.equal(document.getElementById('remote-url').value, '');
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
  const ngrokConfig = document.querySelector('.ngrok-config-card');
  const selectorHeader = document.querySelector('.ngrok-config-header');
  const tokenRow = document.querySelector('.ngrok-token-row');
  assert.ok(ngrokConfig);
  assert.deepEqual([...ngrokConfig.children].map(item => item.tagName), ['HEADER', 'DIV', 'SECTION', 'DIV', 'FOOTER']);
  const providerFieldset = document.querySelector('fieldset.self-hosted-providers');
  const [providerLegend, providerLabel, providerOptions] = [...providerFieldset.children];
  assert.ok(selectorHeader.contains(providerFieldset));
  assert.equal(providerLegend.tagName, 'LEGEND');
  assert.ok(providerLegend.classList.contains('sr-only'));
  assert.equal(providerLabel.tagName, 'SPAN');
  assert.equal(providerLabel.classList.contains('self-hosted-provider-label'), true);
  assert.notEqual(providerLabel.tagName, 'LEGEND');
  assert.equal(providerOptions.tagName, 'DIV');
  assert.ok(providerOptions.classList.contains('self-hosted-provider-options'));
  assert.match(selectorHeader.textContent, /接入方式选择：.*自有 HTTPS.*ngrok/);
  assert.match(selectorHeader.textContent, /等待切换/);
  assert.ok(tokenRow.firstElementChild.contains(tokenInput));
  assert.equal(document.querySelector('.self-hosted-credentials').contains(button('切换到 ngrok')), false);
  assert.ok([...document.querySelector('.self-hosted-credentials').querySelectorAll('button')].find(item => item.textContent === '保存 Token'));
  assert.equal(button('清除'), undefined);
  assert.equal(document.querySelector('.ngrok-result-card'), null);
  const pendingEndpoint = document.getElementById('ngrok-mcp-url');
  assert.equal(pendingEndpoint.value, '');
  assert.equal(pendingEndpoint.placeholder, '切换并连接后生成');
  assert.ok(document.querySelector('.ngrok-config-endpoint .remote-copy').disabled);
  assert.equal(button('测试连接'), undefined);
  assert.match(document.querySelector('.ngrok-local-target').textContent, /本地 MCP 目标.*Local Target.*Serena Core 内部端口.*自动路由/);
  assert.ok(document.querySelector('.ngrok-config-footer-actions').contains(button('切换到 ngrok')));
  assert.doesNotMatch(ngrokConfig.textContent, /5\/5|6\/6|PID|连接持续时间/);
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
  assert.match(document.body.textContent, /已保存/);
  assert.equal(document.querySelector('.self-hosted-current'), null);
  assert.match(document.querySelector('input[name="self-hosted-provider"][value="ngrok"]').closest('.self-hosted-provider-choice').textContent, /当前配置/);
  const tokenHeadingGroup = document.querySelector('.ngrok-token-heading-group');
  assert.match(tokenHeadingGroup.textContent, /ngrok Auth Token.*已保存/);
  assert.ok(document.querySelector('.self-hosted-credential-heading').contains(tokenHeadingGroup));
  assert.ok([...document.querySelector('.ngrok-token-row').querySelectorAll('button')].find(item => item.textContent === '更新 Token'));
  assert.equal(button('清除'), undefined);
  assert.equal(document.querySelector('.ngrok-result-card'), null);
  assert.equal(document.getElementById('ngrok-mcp-url').value, '');
  assert.ok(document.querySelector('.ngrok-config-footer-actions').contains(button('启动 ngrok')));
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

  controller.state = { ...controller.state, status: 'starting', active: true, publicContext: null };
  await render();
  assert.equal(document.querySelector('.ngrok-result-card'), null);
  assert.equal(document.getElementById('ngrok-mcp-url').value, '');
  assert.ok(document.querySelector('.ngrok-config-endpoint .remote-copy').disabled);
  assert.equal(button('测试连接'), undefined);

  controller.state = {
    ...controller.state, status: 'ready', active: true, authorizedClients: 1,
    publicContext: { publicOrigin: 'https://managed.example', mcpResource: 'https://managed.example/mcp', instanceId: 'managed-one' },
  };
  await render();
  const ngrokResult = document.querySelector('.ngrok-result-card');
  assert.ok(ngrokResult);
  assert.equal(ngrokResult.contains(document.querySelector('.self-hosted-credentials')), false);
  assert.equal(document.querySelector('.ngrok-config-card').contains(ngrokResult), false);
  assert.equal(document.getElementById('ngrok-mcp-url').value, 'https://managed.example/mcp');
  assert.match(document.querySelector('.remote-runtime-summary').textContent, /自建接入 · ngrok（当前生效）/);
  assert.match(document.querySelector('.ngrok-config-endpoint').textContent, /Public Endpoint.*已分配/);
  assert.match(document.querySelector('.ngrok-oauth-notice').textContent, /OAuth 2.0/);
  assert.doesNotMatch(document.querySelector('.remote-diagnostics').textContent, /5\/5|6\/6|正常|TLS 1.3/);
  assert.equal(document.getElementById('ngrok-auth-token').disabled, false);
  assert.match(document.querySelector('input[name="self-hosted-provider"][value="ngrok"]').closest('.self-hosted-provider-choice').textContent, /当前运行/);
  assert.ok(ngrokResult.contains(button('停止远程访问')));
  assert.ok(ngrokResult.contains(button('测试连接')));
  assert.equal(document.querySelector('.ngrok-config-card').contains(button('测试连接')), false);
  assert.ok(document.querySelector('.ngrok-config-endpoint .remote-copy'));
  assert.ok(button('测试连接').querySelector('svg.lucide-refresh-cw'));
  assert.ok(button('停止远程访问').querySelector('svg.lucide-square'));
  const liveReplacement = 'live-replacement-ngrok-token-must-stay-private';
  await act(async () => {
    Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value').set.call(tokenInput, liveReplacement);
    tokenInput.dispatchEvent(new window.Event('input', { bubbles: true }));
  });
  assert.doesNotMatch(document.body.textContent, new RegExp(liveReplacement));
  await act(async () => button('更新 Token').click());
  assert.deepEqual(calls.at(-1), ['save', liveReplacement]);
  await act(async () => root.render(createElement(RemoteAccessPage, { controller, port: 9120, allowLan: true, onSettings() {} })));
  assert.match(document.querySelector('.ngrok-oauth-notice').textContent, /局域网客户端仍需要 OAuth 授权/);
});

test('remote copy confirms only after clipboard success and self-hosted probe uses toast outcomes', async () => {
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
    assert.equal(document.querySelector('.remote-probe-feedback'), null);
    api.remoteProbe = async () => { throw new Error('HTTPS connect failed'); };
    await act(async () => button('测试连接').click());
    assert.equal(notices.at(-1)[0], 'error');
    assert.match(notices.at(-1)[1], /连接测试失败/);
    assert.match(controller.error, /HTTPS connect failed/);
    assert.equal(document.querySelector('.remote-probe-feedback'), null);
    assert.equal(document.querySelector('.remote-error'), null);
    assert.equal(button('测试连接').dataset.variant, 'default');
    assert.equal(button('停止远程访问').dataset.variant, 'destructive');
  } finally {
    toast.success = originalSuccess; toast.error = originalError;
    if (clipboard) Object.defineProperty(navigator, 'clipboard', clipboard); else delete navigator.clipboard;
  }
});

test('native approval escapes client claims, focuses deny, and binds decisions to request id', async () => {
  const { RemoteApprovalDialog } = await import('./RemoteApprovalDialog.tsx');
  const pending = { id: 'flow-1', clientName: '<script>untrusted</script>', clientIdHostname: 'client.example', redirectUri: 'https://client.example/callback', confirmationCode: '482731', expiresInSeconds: 100, scope: 'serena:mcp offline_access', refreshAllowed: true };
  const controller = { state: { pending: [pending] }, busy: '测试连接', approvalBusy: false, approvalError: '', approve: (id, allow) => api.remoteApprove(id, allow) };
  const decisions = [];
  api.remoteApprove = async (id, allow) => { decisions.push([id, allow]); };
  root = createRoot(document.getElementById('root'));
  await act(async () => root.render(createElement(RemoteApprovalDialog, { controller })));
  assert.equal(document.activeElement.textContent, '拒绝');
  const dialog = document.querySelector('[role="dialog"]');
  assert.match(dialog.textContent, /482731/);
  assert.match(dialog.textContent, /客户端身份域名/);
  assert.match(dialog.textContent, /client\.example/);
  assert.equal(dialog.querySelector('script'), null);
  await act(async () => [...dialog.querySelectorAll('button')].find(b => b.textContent === '允许连接').click());
  assert.deepEqual(decisions, [['flow-1', true]]);
  assert.match(document.querySelector('[role="dialog"]').textContent, /客户端可自动刷新/);
  controller.state = { pending: [{ ...pending, id: 'flow-2', clientIdHostname: null, expiresInSeconds: 0, scope: 'serena:mcp', refreshAllowed: false }] };
  await act(async () => root.render(createElement(RemoteApprovalDialog, { controller })));
  assert.equal(document.activeElement.textContent, '拒绝');
  assert.ok([...document.querySelectorAll('[role="dialog"] button')].find(b => b.textContent === '允许连接').disabled);
  await act(async () => document.activeElement.click());
  assert.deepEqual(decisions, [['flow-1', true], ['flow-2', false]]);
  assert.match(document.querySelector('[role="dialog"]').textContent, /不签发刷新令牌/);
  assert.doesNotMatch(document.querySelector('[role="dialog"]').textContent, /客户端身份域名/);
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
