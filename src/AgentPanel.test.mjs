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
globalThis.requestAnimationFrame = dom.window.requestAnimationFrame.bind(dom.window);
globalThis.cancelAnimationFrame = dom.window.cancelAnimationFrame.bind(dom.window);
// Use one timer registry for components that mix window and global timer calls.
dom.window.setInterval = globalThis.setInterval;
dom.window.clearInterval = globalThis.clearInterval;
Object.assign(globalThis, { window: dom.window, document: dom.window.document, HTMLElement: dom.window.HTMLElement, Element: dom.window.Element, Node: dom.window.Node, NodeFilter: dom.window.NodeFilter, CustomEvent: dom.window.CustomEvent, MutationObserver: dom.window.MutationObserver, HTMLInputElement: dom.window.HTMLInputElement, getComputedStyle: dom.window.getComputedStyle, IS_REACT_ACT_ENVIRONMENT: true });
for (const key of ["HTMLFormElement", "DocumentFragment", "HTMLSelectElement", "HTMLOptionElement", "Event", "KeyboardEvent", "MouseEvent"]) globalThis[key] = dom.window[key];
const { createElement, act } = await import('react');
const { createRoot } = await import('react-dom/client');
const { AgentPanel } = await import('./AgentPanel.tsx');
const { api } = await import('./api.ts');
const { agentRequests } = await import('./agentRequests.ts');
const { toast } = await import('sonner');
const notifications = [];
toast.success = text => notifications.push(['success', text]);
toast.error = text => notifications.push(['error', text]);
let root;
const workspace = name => ({ id: name, name, root: `E:\\${name}` });
const row = (overrides = {}) => ({ executionId: 'old-E1', agentId: 'old-lineage', workspaceId: 'A', canonicalWorkspaceRoot: 'E:\\frozen-A', prompt: '原始任务 <literal>', status: 'unknown', attention: 'manual_resolution_required', finalResult: null, dispatchState: 'uncertain', threadId: null, turnId: null, providerTerminalStatus: null, resultCompleteness: 'none', interruptRequested: false, interruptAcknowledged: false, interruptTimedOut: false, createdAt: 1000, updatedAt: 2000, completedAt: null,
  availableActions: { canCancel: false, canContinue: false, canResumePending: false }, ...overrides });
async function mount(rows, handler, props = {}) {
  const calls = [];
  api.agent = async request => {
    calls.push(structuredClone(request));
    if (handler) { const result = handler(request); if (result !== undefined) return result; }
    if (request.action === 'list') return { ok: true, data: { executions: rows } };
    return { ok: true, data: rows.find(r => r.executionId === request.executionId) ?? row() };
  };
  root = createRoot(document.getElementById('root'));
  await act(async () => root.render(createElement(AgentPanel, { workspace: workspace('A'), ...props })));
  return calls;
}
function button(text, within = document) { return [...within.querySelectorAll('button')].find(b => b.textContent === text); }
async function click(text, within) { const b = button(text, within); assert.ok(b, text); assert.equal(b.disabled, false, text); await act(async () => b.click()); }
async function input(value, selector = '#agent-prompt') {
  await act(async () => {
    const field = document.querySelector(selector);
    Object.getOwnPropertyDescriptor(dom.window.HTMLTextAreaElement.prototype, 'value').set.call(field, value);
    field.dispatchEvent(new dom.window.Event('input', { bubbles: true }));
  });
}
afterEach(async () => { if (root) await act(async () => root.unmount()); root = null; agentRequests.pending = null; agentRequests.inFlight = false; notifications.length = 0; window.localStorage.clear(); });

test('unknown has only details, no retry or recovery; IDs and JSON stay out of list', async () => {
  await mount([row()]);
  assert.match(document.body.textContent, /需要人工处理/);
  assert.deepEqual([...document.querySelectorAll('article button')].map(b => b.textContent), ['详情', '删除']);
  assert.equal(button('重试原请求'), undefined);
  assert.equal(document.querySelector('pre'), null);
  assert.ok(!document.querySelector('article').textContent.includes('old-E1'));
  await click('详情');
  assert.equal(document.querySelector('details.agent-technical').open, false);
  assert.match(document.querySelector('pre').textContent, /old-E1/);
  assert.equal(document.querySelector('literal'), null);
});

test('pending resume uses exact ID and only backend capability', async () => {
  const calls = await mount([row({ status: 'dispatch_pending', attention: 'pending_explicit_resume', availableActions: { canCancel: true, canContinue: false, canResumePending: true } })]);
  assert.match(document.body.textContent, /等待恢复/);
  await click('恢复任务'); await click('取消任务');
  assert.deepEqual(calls.filter(c => ['cancel','resume_pending'].includes(c.action)), [{ action:'resume_pending', executionId:'old-E1' }, { action:'cancel', executionId:'old-E1' }]);
});

test('all raw statuses remain presentation-only, no invented capability', async () => {
  const statuses = ['dispatch_pending','running','cancel_requested','cancelling','finalizing','reconciling','completed','failed','cancelled','interrupted','unknown'];
  await mount(statuses.map((status, i) => row({ status, attention: 'none', executionId: `E${i}` })));
  assert.equal(document.querySelectorAll('article').length, 11);
  assert.equal(document.querySelectorAll('article button').length, 22);
  assert.match(document.body.textContent, /等待执行/); assert.match(document.body.textContent, /正在恢复执行状态/); assert.match(document.body.textContent, /需要处理/);
});

test('completed final answer is compact; drawer exposes original prompt and technical output', async () => {
  await mount([row({ status: 'completed', attention: 'none', finalResult: { finalResult: [{ type:'agentMessage', phase:'commentary', text:'internal progress' }, { type:'agentMessage', phase:'final_answer', text:'DONE' }], huge: 'x'.repeat(10000) } })]);
  assert.match(document.querySelector('article').textContent, /DONE/);
  assert.ok(!document.querySelector('article').textContent.includes('internal progress'));
  assert.equal(document.querySelector('pre'), null);
  await click('详情');
  assert.match(document.querySelector('[role=dialog]').textContent, /E:\\frozen-A/);
  assert.equal(document.querySelector('.agent-prose').textContent, '原始任务 <literal>');
  assert.ok(document.querySelector('.agent-technical pre').textContent.length > 10000);
});

test('JSON icon copies the snapshot and provides success feedback', async () => {
  const execution = row();
  let copiedText;
  const original = Object.getOwnPropertyDescriptor(navigator, 'clipboard');
  Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText: async text => { copiedText = text; } } });
  try {
    await mount([execution]);
    await click('详情');
    const copyButton = document.querySelector('.agent-json button[aria-label="复制技术详情"]');
    assert.ok(copyButton);
    assert.equal(copyButton.textContent, '');
    await act(async () => copyButton.click());
    assert.deepEqual(JSON.parse(copiedText), execution);
    assert.equal(copyButton.dataset.copied, 'true');
    assert.equal(copyButton.getAttribute('aria-label'), '技术详情已复制');
    assert.equal(copyButton.disabled, true);
  } finally {
    if (original) Object.defineProperty(navigator, 'clipboard', original);
    else delete navigator.clipboard;
  }
});

test('delete hides only the local list entry across refresh and remount', async () => {
  const calls = await mount([row()]);
  await click('删除');
  assert.equal(document.querySelectorAll('article').length, 0);
  assert.match(document.body.textContent, /暂无 Agent 任务/);
  await click('刷新');
  assert.equal(document.querySelectorAll('article').length, 0);
  assert.ok(calls.every(request => request.action === 'list'));
  await act(async () => root.unmount()); root = null;
  await mount([row()]);
  assert.equal(document.querySelectorAll('article').length, 0);
});

test('continue uses independent drawer input and original source across Workspace switch', async () => {
  const calls = await mount([row({ status:'completed', attention:'none', availableActions:{canCancel:false,canResumePending:false,canContinue:true} })]);
  await input('top-level new task');
  await act(async () => root.render(createElement(AgentPanel, { workspace: workspace('B') })));
  await click('详情');
  assert.equal(button('继续对话').disabled, true);
  await input('continuation only', '#agent-continuation');
  await click('继续对话');
  const request = calls.find(c => c.action === 'continue');
  assert.equal(request.prompt, 'continuation only'); assert.equal(request.executionId, 'old-E1');
  assert.equal('workspaceId' in request, false); assert.equal('canonicalWorkspaceRoot' in request, false);
  assert.equal(document.querySelector('#agent-prompt').value, 'top-level new task');
});

test('only transport ambiguity offers exact retry, stable through input edits and remount', async () => {
  const calls = await mount([], request => { if (request.action === 'start') throw new Error('transport lost'); });
  await input('original prompt'); await click('开始新任务');
  assert.ok(button('重试原请求')); const frozen = agentRequests.pending;
  await act(async () => root.unmount()); root = createRoot(document.getElementById('root'));
  await act(async () => root.render(createElement(AgentPanel, { workspace: workspace('B') })));
  await input('edited prompt'); await click('重试原请求');
  assert.deepEqual(calls.filter(c => c.action === 'start'), [frozen, frozen]);
  assert.equal(frozen.prompt, 'original prompt');
});

test('confirmed product errors do not expose transport retry and preserve execution reference', async () => {
  await mount([], r => r.action === 'start' ? { ok:false, error:{code:'AGENT_OPERATION_FAILED',message:'failure',executionId:'old-E1'} } : undefined);
  await input('task'); await click('开始新任务');
  assert.equal(button('重试原请求'), undefined); assert.equal(agentRequests.pending, null);
  assert.ok(button('查看相关任务')); assert.match(document.body.textContent, /操作未完成/);
});

test('mutation lock prevents duplicate submit while request is outstanding', async () => {
  let resolve;
  const calls = await mount([], r => r.action === 'start' ? new Promise(done => { resolve = done; }) : undefined);
  await input('task'); await click('开始新任务');
  assert.equal(button('正在处理…').disabled, true);
  await act(async () => document.querySelector('.agent-composer').dispatchEvent(new dom.window.Event('submit', { bubbles:true, cancelable:true })));
  assert.equal(calls.filter(c => c.action === 'start').length, 1);
  await act(async () => resolve({ ok:true, data:row() }));
  assert.ok(notifications.some(([kind]) => kind === 'success'));
});

test('empty workspace provides real navigation and disables new task', async () => {
  let navigated = false;
  await mount([], undefined, { workspace:null, onSelectWorkspace:() => { navigated = true; } });
  assert.match(document.body.textContent, /未选择可用工作区/); assert.match(document.body.textContent, /暂无 Agent 任务/);
  await input('task'); assert.equal(button('开始新任务').disabled, true);
  await click('选择工作区'); assert.equal(navigated, true);
});

test('read failures show feedback and do not offer operation replay', async () => {
  await mount([row()], request => request.action === 'observe' ? Promise.reject(new Error('offline')) : undefined);
  await click('详情'); assert.ok(button('重新加载详情')); assert.equal(button('重试原请求'), undefined);
  assert.ok(notifications.some(([kind]) => kind === 'error'));
});



test('continuation ambiguity stays in the drawer and retries its frozen independent input', async () => {
  const calls = await mount([row({status:'completed',attention:'none',availableActions:{canContinue:true,canCancel:false,canResumePending:false}})], request => request.action === 'continue' ? Promise.reject(new Error('transport')) : undefined);
  await click('详情'); await input('frozen continuation', '#agent-continuation'); await click('继续对话');
  const drawer = document.querySelector('[role=dialog]');
  assert.ok(button('重试原请求', drawer));
  await click('重试原请求', drawer);
  const requests = calls.filter(c => c.action === 'continue');
  assert.deepEqual(requests[0], requests[1]);
  assert.equal(requests[0].prompt, 'frozen continuation');
});

test('late list response cannot overwrite a newly accepted operation', async () => {
  let finishList; let lists = 0;
  await mount([], request => {
    if (request.action === 'list' && ++lists === 2) return new Promise(resolve => {finishList = resolve;});
    if (request.action === 'start') return {ok:true,data:row({prompt:'newly accepted',executionId:'new'})};
  });
  await click('刷新'); await input('newly accepted'); await click('开始新任务');
  await act(async () => finishList({ok:true,data:{executions:[]}}));
  assert.match(document.querySelector('article').textContent, /newly accepted/);
});

test('closing details restores focus to its exact list entry', async () => {
  await mount([row()]); const trigger = button('详情');
  await click('详情');
  await act(async () => document.querySelector('[aria-label="关闭任务详情"]').click());
  await act(async () => new Promise(resolve => setTimeout(resolve, 10)));
  assert.equal(document.querySelector('[role=dialog]'), null);
  assert.equal(document.activeElement, trigger);
});

test('switching to a related execution from drawer keeps its new details open', async () => {
  await mount([row({status:'completed',attention:'none',availableActions:{canContinue:true,canCancel:false,canResumePending:false}})], r => {
    if (r.action === 'continue') return {ok:false,error:{code:'AGENT_OPERATION_FAILED',message:'handoff error',executionId:'new-E2'}};
    if (r.action === 'observe' && r.executionId === 'new-E2') return {ok:true,data:row({executionId:'new-E2',prompt:'new related task'})};
  });
  await click('详情'); await input('next', '#agent-continuation'); await click('继续对话');
  await click('查看相关任务');
  await act(async () => new Promise(resolve => setTimeout(resolve, 10)));
  assert.ok(document.querySelector('[role=dialog]'));
  assert.match(document.querySelector('.agent-drawer-body > section .agent-prose').textContent, /new related task/);
});

test('list refresh cannot compete with an outstanding detail observation', async () => {
  let resolve;
  const calls = await mount([row()], r => r.action === 'observe' ? new Promise(done => {resolve = done;}) : undefined);
  await click('详情');
  // Direct invocation also exercises the handler guard while the native modal blocks interaction.
  await click('刷新');
  assert.equal(calls.filter(c => c.action === 'list').length, 1);
  await act(async () => resolve({ok:true,data:row({prompt:'latest detail'})}));
  assert.match(document.querySelector('[role=dialog] .agent-prose').textContent, /latest detail/);
});


test('status caches pending, successful and failed Codex probes until explicit refresh', async () => {
  const { default: App } = await import('./App.tsx');
  const originals = { getState: api.getState, broker: api.broker, codexVersion: api.codexVersion, detect: api.detect, openExternal: api.openExternal };
  const snapshot = { config: { agentEnabled:false, broker:{enabled:false,port:9120,allowLan:false},workspaces:[],serenaPath:null,port:9121,dashboardEnabled:false,openDashboardOnLaunch:false,autoStartServer:false,minimizeToTray:false }, git:{available:true,status:'available',version:'test',path:null,error:null}, serverStatus:'stopped', installation:null, activeInstallation:null, managedRuntimePresent:false, managedProcessPresent:false, codegraphVersion:'test', dashboardEnabled:false, autostartEnabled:false, autostartError:null, lastError:null };
  let probes = 0; let resolveProbe;
  const links = [];
  api.getState = async () => snapshot;
  api.detect = async () => snapshot;
  api.broker = async () => ({ running:false,projects:[],syncWarnings:[],projectSources:[],activeWorkspace:null,codegraph:null,lastError:null });
  api.openExternal = async target => { links.push(target); };
  api.codexVersion = () => { probes++; return new Promise(resolve => { resolveProbe = resolve; }); };
  try {
    root = createRoot(document.getElementById('root'));
    await act(async () => root.render(createElement(App)));
    await click('状态'); assert.equal(probes, 1);
    await click('首页'); await click('状态'); assert.equal(probes, 1);
    await act(async () => resolveProbe('codex-cli detected'));
    await click('首页'); await click('状态'); assert.equal(probes, 1);
    assert.match(document.body.textContent, /codex-cli detected/);
    await click('Serena GitHub ↗'); await click('CodeGraph GitHub ↗');
    assert.deepEqual(links, ['github', 'codegraph']);
    api.codexVersion = async () => { probes++; throw new Error('probe unavailable'); };
    await click('重新检测'); assert.equal(probes, 2);
    assert.match(document.body.textContent, /probe unavailable/);
    await click('首页'); await click('状态'); assert.equal(probes, 2);
    api.codexVersion = async () => { probes++; return 'codex-cli refreshed'; };
    await click('重新检测'); assert.equal(probes, 3);
    assert.match(document.body.textContent, /codex-cli refreshed/);
  } finally { Object.assign(api, originals); }
});
