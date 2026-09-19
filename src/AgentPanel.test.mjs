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
const { AgentPanel } = await import('./AgentPanel.tsx');
const { api } = await import('./api.ts');
const { agentRequests } = await import('./agentRequests.ts');
const { toast } = await import('sonner');
const notifications = [];
toast.success = text => notifications.push(['success', text]);
toast.error = text => notifications.push(['error', text]);
let root;
const workspace = name => ({ id: name, name, root: `E:\\${name}` });
const row = (overrides = {}) => ({ executionId: 'old-E1', agentId: 'old-lineage', workspaceId: 'A', canonicalWorkspaceRoot: 'E:\\frozen-A', prompt: '原始任务 <literal>', status: 'unknown', attention: 'manual_resolution_required', revision: 'R1', resultAvailable: overrides.finalResult !== undefined && overrides.finalResult !== null, provider: { id: 'codex', displayName: 'Codex', version: null }, providerSessionLabel: null, usage: { inputTokens: null, cachedInputTokens: null, cacheWriteInputTokens: null, outputTokens: null, reasoningTokens: null, totalTokens: null, modelContextWindow: null, completeness: 'unknown', usageRevision: 0, updatedAt: null }, progress: { phase: 'reconciling', summaryCode: 'execution.reconciling', activityPhase: null, toolCategory: null, lastActivityAt: null, activityAgeMs: null, silenceLevel: null }, nextAction: { action: 'manual_resolution' }, dispatchState: 'uncertain', threadId: null, threadName: null, turnId: null, providerTerminalStatus: null, errorCode: null, errorMessage: null, resultCompleteness: 'none', interruptRequested: false, interruptAcknowledged: false, interruptTimedOut: false, createdAt: 1000, updatedAt: 2000, completedAt: null,
  availableActions: { canCancel: false, canContinue: false, canResumePending: false }, ...overrides, provider: { id: 'codex', displayName: 'Codex', version: null, ...overrides.provider }, usage: { inputTokens: null, cachedInputTokens: null, cacheWriteInputTokens: null, outputTokens: null, reasoningTokens: null, totalTokens: null, modelContextWindow: null, completeness: 'unknown', usageRevision: 0, updatedAt: null, ...overrides.usage }, progress: { phase: 'reconciling', summaryCode: 'execution.reconciling', activityPhase: null, toolCategory: null, lastActivityAt: null, activityAgeMs: null, silenceLevel: null, ...overrides.progress } });
async function mount(rows, handler, props = {}) {
  const calls = [];
  api.agent = async request => {
    calls.push(structuredClone(request));
    if (handler) { const result = handler(request); if (result !== undefined) return result; }
    if (request.action === 'list') return { ok: true, data: { executions: rows.map(row => { const summary = { ...row }; delete summary.finalResult; return summary; }) } };
    return { ok: true, data: structuredClone(rows.find(r => r.executionId === request.executionId) ?? row()) };
  };
  api.agentHistory = async (before = null, workspaceRoot = null) => {
    const response = await api.agent({action:'list',limit:5});
    if (!response.ok) throw new Error(`${response.error.code}: ${response.error.message}`);
    const all = response.data.executions.filter(row => !workspaceRoot || row.canonicalWorkspaceRoot === workspaceRoot);
    const start = before ? all.findIndex(row => row.executionId === before) + 1 : 0;
    const executions = all.slice(start, start + 5);
    return {executions, nextCursor: start + 5 < all.length ? executions.at(-1).executionId : null};
  };
  root = createRoot(document.getElementById('root'));
  await act(async () => root.render(createElement(TooltipProvider, null, createElement(AgentPanel, { workspace: workspace('A'), ...props }))));
  return calls;
}
function button(text, within = document) { return [...within.querySelectorAll('button')].find(b => b.textContent === text); }
async function click(text, within) { const b = button(text, within); assert.ok(b, text); assert.equal(b.disabled, false, text); await act(async () => b.click()); }
async function refreshTasks() { const b = document.querySelector('button[aria-label="刷新任务列表"]'); assert.ok(b, '刷新任务列表'); assert.equal(b.disabled, false, '刷新任务列表'); await act(async () => b.click()); }
async function input(value, selector = '#agent-prompt') {
  await act(async () => {
    const field = document.querySelector(selector);
    Object.getOwnPropertyDescriptor(dom.window.HTMLTextAreaElement.prototype, 'value').set.call(field, value);
    field.dispatchEvent(new dom.window.Event('input', { bubbles: true }));
  });
}
afterEach(async () => { if (root) await act(async () => root.unmount()); root = null; agentRequests.pending = null; agentRequests.inFlight = false; notifications.length = 0; window.localStorage.clear(); document.getElementById("task-nav-test")?.remove(); });

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
  await click('展开更多（5条）');
  await click('展开更多（5条）');
  assert.equal(document.querySelectorAll('article').length, 11);
  assert.equal(document.querySelectorAll('article button').length, 22);
  assert.match(document.body.textContent, /等待执行/); assert.match(document.body.textContent, /正在恢复执行状态/); assert.match(document.body.textContent, /需要处理/);
});

test('Agent list uses the local-runner card layout and filters only real status fields', async () => {
  dom.window.HTMLElement.prototype.scrollIntoView = () => {};
  await mount([
    row({ executionId: 'run', status: 'running', attention: 'none' }),
    row({ executionId: 'done', status: 'completed', attention: 'none', completedAt: 2000 }),
    row({ executionId: 'failed', status: 'failed', attention: 'none', errorCode: 'AGENT_TIMEOUT', errorMessage: '实际后端超时' }),
    row({ executionId: 'unknown', status: 'unknown', attention: 'none' }),
    row({ executionId: 'cancelled', status: 'cancelled', attention: 'none' }),
    row({ executionId: 'interrupted', status: 'interrupted', attention: 'none' }),
  ]);
  await click('展开更多（5条）');
  assert.match(document.querySelector('.agent-list-page-heading').textContent, /Agent 任务.*Codex Local Runner.*在当前工作区中创建、查看和管理本地 Agent 执行任务/);
  assert.match(document.querySelector('.agent-workspace-bar').textContent, /当前工作区:.*A.*E:\\A/);
  const composer = document.querySelector('.agent-composer-card');
  assert.ok(composer.querySelector('#agent-prompt'));
  assert.equal(composer.querySelector('.agent-composer-footer').parentElement, composer);
  assert.ok(document.querySelector('.agent-filter-toolbar'));
  const refresh = document.querySelector('button[aria-label="刷新任务列表"]');
  assert.ok(refresh); assert.equal(refresh.textContent, '');
  assert.equal(document.querySelectorAll('.agent-task-card').length, 6);
  assert.ok(document.querySelector('.agent-task-card .agent-task-title'));
  assert.ok(button('详情')); assert.ok(button('删除'));
  const styles = readFileSync('src/styles.css', 'utf8');
  assert.match(styles, /\.agent-task-actions \.agent-delete-action \{ color: var\(--signal-red\); \}/);
  assert.doesNotMatch(styles, /\.agent-task-actions \.agent-delete-action \{ color: #94a3b8; \}/);
  const selectStatus = async label => {
    await act(async () => document.querySelector('[aria-label="筛选任务"]').dispatchEvent(new dom.window.KeyboardEvent('keydown', { key: 'Enter', bubbles: true })));
    const option = [...document.querySelectorAll('[role="option"]')].find(node => node.textContent === label);
    assert.ok(option, label);
    await act(async () => option.click());
  };
  await selectStatus('失败');
  assert.equal(document.querySelectorAll('.agent-task-card').length, 2);
  assert.match(document.querySelector('.agent-task-error').textContent, /AGENT_TIMEOUT.*实际后端超时/);
  assert.doesNotMatch(document.body.textContent, /CODEX_RPC_TIMEOUT/);
  await selectStatus('已取消');
  assert.equal(document.querySelectorAll('.agent-task-card').length, 2);
  assert.match(document.body.textContent, /已取消.*已中断/);
});

test('refresh button keeps its element and reports the real pending state', async () => {
  await mount([row()]);
  const pending = Promise.withResolvers();
  api.agentHistory = () => pending.promise;
  const refresh = document.querySelector('button[aria-label="刷新任务列表"]');
  const icon = refresh.querySelector('.agent-refresh-icon');
  assert.equal(refresh.getAttribute('aria-busy'), 'false');
  assert.equal(refresh.dataset.state, 'idle');
  await act(async () => refresh.click());
  assert.equal(document.querySelector('button[aria-label="刷新任务列表"]'), refresh);
  assert.equal(refresh.querySelector('.agent-refresh-icon'), icon);
  assert.equal(refresh.getAttribute('aria-busy'), 'true');
  assert.equal(refresh.dataset.state, 'loading');
  assert.equal(refresh.disabled, true);
  await act(async () => pending.resolve({ executions: [row()], nextCursor: null }));
  assert.equal(document.querySelector('button[aria-label="刷新任务列表"]'), refresh);
  assert.equal(refresh.getAttribute('aria-busy'), 'false');
  assert.equal(refresh.dataset.state, 'idle');
});

test('load-more button keeps stable idle, loading, and retry icon states', async () => {
  await mount(Array.from({ length: 6 }, (_, index) => row({ executionId: `E${index}` })));
  const pending = Promise.withResolvers();
  api.agentHistory = () => pending.promise;
  const loadMore = button('展开更多（5条）');
  assert.equal(loadMore.getAttribute('aria-busy'), 'false');
  assert.equal(loadMore.dataset.state, 'idle');
  assert.ok(loadMore.querySelector('.agent-load-more-icon-idle'));
  await act(async () => loadMore.click());
  assert.equal(document.querySelector('.agent-load-more-button'), loadMore);
  assert.equal(loadMore.getAttribute('aria-busy'), 'true');
  assert.equal(loadMore.dataset.state, 'loading');
  assert.ok(loadMore.querySelector('.agent-load-more-icon-loading'));
  await act(async () => pending.reject(new Error('page unavailable')));
  const retry = button('重试加载更多');
  assert.equal(retry, loadMore);
  assert.equal(retry.getAttribute('aria-busy'), 'false');
  assert.equal(retry.dataset.state, 'retry');
  assert.ok(retry.querySelector('.agent-load-more-icon-retry'));
});

test('completed final answer is compact; detail pane exposes original prompt and technical output', async () => {
  await mount([row({ status: 'completed', attention: 'none', finalResult: { finalResult: [{ type:'agentMessage', phase:'commentary', text:'internal progress' }, { type:'agentMessage', phase:'final_answer', text:'DONE' }], huge: 'x'.repeat(10000) } })]);
  assert.ok(!document.querySelector('article').textContent.includes('DONE'));
  assert.ok(!document.querySelector('article').textContent.includes('internal progress'));
  assert.equal(document.querySelector('pre'), null);
  await click('详情');
  assert.match(document.querySelector('[aria-label="任务详情"]').textContent, /E:\\frozen-A/);
  assert.equal(document.querySelector('.agent-prose').textContent, '原始任务 <literal>');
  assert.ok(document.querySelector('.agent-technical pre').textContent.length > 10000);
});

test('all detail copy buttons transition only after clipboard success and recover after failure', async () => {
  const execution = row({ status: 'completed', attention: 'none', finalResult: { finalResult: [{ type: 'agentMessage', phase: 'final_answer', text: '真实结果' }] } });
  const writes = [];
  const original = Object.getOwnPropertyDescriptor(navigator, 'clipboard');
  Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText: text => new Promise((resolve, reject) => writes.push({ text, resolve, reject })) } });
  try {
    await mount([execution]);
    await click('详情');
    const cases = [
      [document.querySelector('.agent-task-content .agent-copy-text'), execution.prompt],
      [document.querySelector('.agent-result-section .agent-copy-text'), '真实结果'],
      [document.querySelector('.agent-technical-copy'), JSON.stringify(execution, null, 2)],
    ];
    for (const [copyButton, expected] of cases) {
      assert.ok(copyButton);
      assert.equal(copyButton.dataset.copyState, 'idle');
      assert.equal(copyButton.querySelectorAll('.agent-copy-icon-slot svg').length, 2);
      await act(async () => copyButton.click());
      assert.equal(copyButton.dataset.copyState, 'copying');
      assert.match(copyButton.textContent, /复制中/);
      assert.equal(writes.at(-1).text, expected);
      await act(async () => writes.at(-1).resolve());
      assert.equal(copyButton.dataset.copyState, 'copied');
      assert.equal(copyButton.textContent, '已复制');
      assert.equal(copyButton.disabled, false);
    }
    const failedButton = document.querySelector('.agent-task-content .agent-copy-text');
    await act(async () => failedButton.click());
    assert.equal(failedButton.dataset.copyState, 'copying');
    await act(async () => writes.at(-1).reject(new Error('clipboard denied')));
    assert.equal(failedButton.dataset.copyState, 'idle');
    assert.equal(failedButton.textContent, '复制内容');
    assert.ok(notifications.some(([kind, message]) => kind === 'error' && /clipboard denied/.test(message)));
  } finally {
    if (original) Object.defineProperty(navigator, 'clipboard', original);
    else delete navigator.clipboard;
  }
});

test('details explicitly fetch result, keep it for the same revision, and replace it for a new revision', async () => {
  const execution = row({ status:'completed', attention:'none', revision:'R1', resultAvailable:true, finalResult:{ finalResult:[{type:'agentMessage',phase:'final_answer',text:'RESULT ONE'}] } });
  const calls = await mount([execution]);
  assert.ok(!document.querySelector('article').textContent.includes('RESULT ONE'));
  await click('详情');
  assert.deepEqual(calls.find(c => c.action === 'observe'), {action:'observe',executionId:'old-E1',waitMs:0,includeResult:true});
  assert.match(document.querySelector('.agent-result').textContent, /RESULT ONE/);
  execution.updatedAt++;
  await act(async () => { await new Promise(resolve => setTimeout(resolve, 1600)); });
  assert.match(document.querySelector('.agent-result').textContent, /RESULT ONE/);
  assert.equal(calls.filter(c => c.action === 'observe').length, 1);
  execution.revision = 'R2';
  execution.finalResult = { finalResult:[{type:'agentMessage',phase:'final_answer',text:'RESULT TWO'}] };
  await act(async () => { await new Promise(resolve => setTimeout(resolve, 1600)); });
  assert.match(document.querySelector('.agent-result').textContent, /RESULT TWO/);
  assert.ok(!document.querySelector('.agent-result').textContent.includes('RESULT ONE'));
  assert.equal(calls.filter(c => c.action === 'observe').length, 2);
});

test('a new revision never retains stale result when fetching its body fails', async () => {
  const execution = row({ status:'completed', attention:'none', revision:'R1', resultAvailable:true, finalResult:{ finalResult:[{type:'agentMessage',text:'OLD RESULT'}] } });
  let fail = false;
  const calls = await mount([execution], request => {
    if (request.action === 'observe' && fail) throw new Error('result read failed');
  });
  await click('详情');
  execution.revision = 'R2'; fail = true;
  await act(async () => { await new Promise(resolve => setTimeout(resolve, 1600)); });
  assert.ok(!document.querySelector('[aria-label="任务详情"]').textContent.includes('OLD RESULT'));
  assert.match(document.querySelector('[aria-label="任务详情"]').textContent, /result read failed/);
  assert.equal(button('复制结果').disabled, true, 'placeholder results must not be copied');
  await act(async () => { await new Promise(resolve => setTimeout(resolve, 1600)); });
  assert.equal(calls.filter(c => c.action === 'observe').length, 2, 'no retry loop');
});

test('delete hides only the local list entry across refresh and remount', async () => {
  const calls = await mount([row()]);
  await click('删除');
  assert.equal(document.querySelectorAll('article').length, 0);
  assert.match(document.body.textContent, /暂无 Agent 任务/);
  await refreshTasks();
  assert.equal(document.querySelectorAll('article').length, 0);
  assert.ok(calls.every(request => request.action === 'list'));
  await act(async () => root.unmount()); root = null;
  await mount([row()]);
  assert.equal(document.querySelectorAll('article').length, 0);
});

test('continue uses independent detail pane input and original source across Workspace switch', async () => {
  const calls = await mount([row({ status:'completed', attention:'none', availableActions:{canCancel:false,canResumePending:false,canContinue:true} })]);
  await input('top-level new task');
  await act(async () => root.render(createElement(TooltipProvider, null, createElement(AgentPanel, { workspace: workspace('B') }))));
  await click('详情');
  assert.equal(button('继续任务').disabled, true);
  await input('continuation only', '#agent-continuation');
  await click('继续任务');
  const request = calls.find(c => c.action === 'continue');
  assert.equal(request.prompt, 'continuation only'); assert.equal(request.executionId, 'old-E1');
  assert.equal('workspaceId' in request, false); assert.equal('canonicalWorkspaceRoot' in request, false);
  assert.equal(document.querySelector('#agent-prompt').value, 'top-level new task');
});

test('only transport ambiguity offers exact retry, stable through input edits and remount', async () => {
  const calls = await mount([], request => { if (request.action === 'start') throw new Error('transport lost'); });
  await input('original prompt'); await click('开始新任务');
  assert.ok(button('重试原请求')); const frozen = agentRequests.pending;
  assert.equal(frozen.workspaceId, 'A');
  await act(async () => root.unmount()); root = createRoot(document.getElementById('root'));
  await act(async () => root.render(createElement(TooltipProvider, null, createElement(AgentPanel, { workspace: workspace('B') }))));
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



test('continuation ambiguity stays in the detail pane and retries its frozen independent input', async () => {
  const calls = await mount([row({status:'completed',attention:'none',availableActions:{canContinue:true,canCancel:false,canResumePending:false}})], request => request.action === 'continue' ? Promise.reject(new Error('transport')) : undefined);
  await click('详情'); await input('frozen continuation', '#agent-continuation'); await click('继续任务');
  const detailPane = document.querySelector('[aria-label="任务详情"]');
  assert.ok(button('重试原请求', detailPane));
  await click('重试原请求', detailPane);
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
  await refreshTasks(); await input('newly accepted'); await click('开始新任务');
  await act(async () => finishList({ok:true,data:{executions:[]}}));
  assert.match(document.querySelector('article').textContent, /newly accepted/);
});

test('task page has no in-page return control', async () => {
  await mount([row()]);
  await click('详情');
  assert.equal(document.querySelector('.agent-page').classList.contains('agent-list-view'), false);
  assert.equal(document.querySelector('.agent-page').classList.contains('agent-detail-view'), true);
  assert.equal(document.querySelector('[aria-label="关闭任务详情"]'), null);
  assert.equal(document.querySelector('nav[aria-label="任务页面导航"]'), null);
  assert.equal(button('返回 Agent 任务'), undefined);
  assert.equal(document.querySelector('[aria-label="任务详情"]')?.getAttribute('aria-label'), '任务详情');
});

test('history shows five then appends five, and preserves expanded rows on refresh', async () => {
  const rows = Array.from({length:11}, (_, n) => row({executionId:`E${n}`,prompt:`Task ${n}`,createdAt:new Date(2026,8,9,22,14).getTime(),completedAt:new Date(2026,8,9,22,16,18).getTime(),canonicalWorkspaceRoot:'E:\\A'}));
  await mount(rows);
  assert.equal(document.querySelectorAll('article').length, 5);
  assert.match(document.querySelector('.agent-meta').textContent, /A·2026-09-09 22:14·耗时 2分18秒/);
  await click('展开更多（5条）');
  assert.equal(document.querySelectorAll('article').length, 10);
  await refreshTasks();
  assert.equal(document.querySelectorAll('article').length, 10);
  await click('展开更多（5条）');
  assert.equal(document.querySelectorAll('article').length, 11);
  assert.equal(button('展开更多（5条）'), undefined);
});

test('history failure preserves exact cursor while running state polling continues', async () => {
  const originalSetInterval = globalThis.setInterval;
  let poll;
  globalThis.setInterval = callback => { poll = callback; return originalSetInterval(() => {}, 60_000); };
  const pollNow = () => act(async () => { poll(); for (let index = 0; index < 12; index++) await Promise.resolve(); });
  try {
  const history = Array.from({length:12},(_,n)=>row({executionId:'E'+n,status:'running',attention:'none'}));
  await mount(history, request => {
    if (request.action === 'start') {
      const created = row({executionId:'new',createdAt:3000,status:'running',attention:'none'});
      history.unshift(created);
      return {ok:true,data:created};
    }
  });
  await click('展开更多（5条）');
  const original = api.agentHistory;
  const cursors = [];
  api.agentHistory = async (cursor, root) => {
    cursors.push(cursor);
    if (cursor === 'E9') throw new Error('page failed');
    return original(cursor,root);
  };
  await click('展开更多（5条）');
  assert.equal(document.querySelectorAll('article').length,10);
  history[0].status = 'completed';
  assert.ok(poll);
  await pollNow();
  assert.ok(document.querySelector('article').textContent.includes('已完成'));
  assert.equal(document.querySelectorAll('article').length,10);
  assert.equal(cursors.filter(c=>c==='E9').length,1);
  await input('new task while page failed'); await click('开始新任务');
  assert.equal(document.querySelectorAll('article').length,11);
  history.find(row=>row.executionId==='E9').status='completed';
  await pollNow();
  assert.ok([...document.querySelectorAll('article')].at(-1).textContent.includes('已完成'));
  api.agentHistory = async (cursor, root) => { cursors.push(cursor); return original(cursor,root); };
  await click('重试加载更多');
  assert.equal(cursors.at(-1),'E9');
  assert.equal(document.querySelectorAll('article').length,13);
  } finally {
    globalThis.setInterval = originalSetInterval;
  }
});

test('workspace filter resets pagination and only reads that frozen workspace', async () => {
  await mount(Array.from({length:12},(_,n)=>row({executionId:`E${n}`,canonicalWorkspaceRoot:n < 6 ? 'E:\\A' : 'E:\\B'})), undefined, {workspaces:[workspace('A'),workspace('B')]});
  dom.window.HTMLElement.prototype.scrollIntoView = () => {};
  await act(async () => document.querySelector('[aria-label="筛选工作区"]').dispatchEvent(new dom.window.KeyboardEvent('keydown',{key:'Enter',bubbles:true})));
  const choice = [...document.querySelectorAll('[role="option"]')].find(node=>node.textContent==='B');
  assert.ok(choice);
  await act(async () => choice.click());
  assert.equal(document.querySelectorAll('article').length,5);
  assert.ok([...document.querySelectorAll('.agent-meta')].every(node=>node.textContent.startsWith('B')));
  await click('展开更多（5条）');
  assert.equal(document.querySelectorAll('article').length,6);
  assert.equal(button('展开更多（5条）'),undefined);
});

test('mutations preserve expanded history and never mix another workspace after refresh fails', async () => {
  const history = Array.from({length:26},(_,n)=>row({executionId:`E${n}`,canonicalWorkspaceRoot:'E:\\B',status:'running',attention:'none',availableActions:{canCancel:n===0,canContinue:false,canResumePending:false}}));
  await mount(history, request => request.action === 'start' ? {ok:true,data:row({executionId:'new-A',canonicalWorkspaceRoot:'E:\\A'})} : request.action === 'cancel' ? {ok:true,data:{...history[0],status:'cancelled'}} : undefined, {workspaces:[workspace('A'),workspace('B')]});
  dom.window.HTMLElement.prototype.scrollIntoView = () => {};
  await act(async () => document.querySelector('[aria-label="筛选工作区"]').dispatchEvent(new dom.window.KeyboardEvent('keydown',{key:'Enter',bubbles:true})));
  await act(async () => [...document.querySelectorAll('[role="option"]')].find(node=>node.textContent==='B').click());
  for(let n=0;n<4;n++) await click('展开更多（5条）');
  assert.equal(document.querySelectorAll('article').length,25);
  api.agentHistory = async () => { throw new Error('refresh unavailable'); };
  await click('取消任务');
  assert.equal(document.querySelectorAll('article').length,25);
  await input('create in active A'); await click('开始新任务');
  assert.equal(document.querySelectorAll('article').length,25);
  assert.ok([...document.querySelectorAll('.agent-meta')].every(node=>node.textContent.startsWith('B')));
});

test('workspace display name stays consistent between history and details', async () => {
  await mount([row({canonicalWorkspaceRoot:'E:\\named-folder'})],undefined,{workspaces:[{id:'project-4',name:'实际工作区名',root:'E:\\named-folder'}]});
  assert.match(document.querySelector('.agent-meta').textContent,/实际工作区名/);
  await click('详情');
  const location = document.querySelector('.agent-detail-location');
  assert.match(location.textContent,/执行位置.*实际工作区名.*E:\\named-folder/);
  assert.equal(location.querySelector('strong').textContent, '实际工作区名');
  assert.equal(location.querySelector('code').textContent, 'E:\\named-folder');
});

test('running detail shares blue pulse and keeps execution location left aligned', async () => {
  await mount([row({canonicalWorkspaceRoot:'E:\\named-folder', status:'running', attention:'none', progress:{phase:'running'}})], undefined, {workspaces:[{id:'project-4',name:'实际工作区名',root:'E:\\named-folder'}]});
  await click('详情');
  for (const selector of ['.agent-detail-title .agent-status', '.agent-detail-live-grid .agent-status']) {
    const status = document.querySelector(selector);
    assert.equal(status.textContent, '执行中');
    assert.ok(status.classList.contains('tone-blue'));
    assert.ok(status.querySelector('i.agent-task-pulse[aria-hidden="true"]'));
  }
  const location = document.querySelector('.agent-detail-location');
  assert.equal(location.querySelector('strong').textContent, '实际工作区名');
  assert.equal(location.querySelector('code').textContent, 'E:\\named-folder');
  const styles = readFileSync('src/styles.css', 'utf8');
  assert.match(styles, /\.agent-detail-live-grid \.agent-status\.tone-blue \{ color: var\(--agent-blue\); \}/);
  assert.match(styles, /\.agent-detail-location \{[^}]*justify-content: flex-start;/);
  assert.match(styles, /\.agent-detail-location > div \{[^}]*justify-content: flex-start;/);
  assert.match(styles, /@media \(prefers-reduced-motion: reduce\) \{\s*\.agent-page[^}]*\.agent-task-pulse \{ animation: none; \}/);
});

test('Agent detail presents only real execution fields and gates header actions by capability', async () => {
  const execution = row({
    threadName: '真实线程标题', status: 'running', attention: 'none', dispatchState: 'dispatched', threadId: 'thread-1', turnId: 'turn-1',
    provider: { id: 'provider-a', displayName: 'Provider A', version: '2.0' }, providerSessionLabel: 'legacy-session', providerTerminalStatus: 'running', resultCompleteness: 'partial', controlRevision: 'control-2', activityRevision: 'activity-3', nextAction: { action: 'observe', waitMs: 1000 },
    usage: { totalTokens: 12_531, inputTokens: 1, cachedInputTokens: 2, cacheWriteInputTokens: 3, outputTokens: 4, reasoningTokens: 5, modelContextWindow: null, completeness: 'partial', usageRevision: 9, updatedAt: 2_000 },
    errorCode: 'AGENT_REAL_ERROR', errorMessage: '后端实际错误', createdAt: 1_000, updatedAt: 2_000,
    progress: { phase: 'running', summaryCode: 'tool.test', activityPhase: 'tool', toolCategory: 'test', lastActivityAt: null, activityAgeMs: null, silenceLevel: null },
    availableActions: { canCancel: true, canContinue: false, canResumePending: true },
  });
  let copiedText = '';
  const original = Object.getOwnPropertyDescriptor(navigator, 'clipboard');
  Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText: async text => { copiedText = text; } } });
  try {
    const calls = await mount([execution]); await click('详情');
    const page = document.querySelector('.agent-page');
    assert.ok(page.classList.contains('agent-detail-view')); assert.equal(page.classList.contains('agent-list-view'), false);
    assert.match(document.querySelector('.agent-detail-header').textContent, /真实线程标题.*执行中.*Provider A · v2\.0/);
    assert.match(document.querySelector('.agent-detail-info-card').textContent, /Provider.*Provider A · v2\.0.*legacy-session.*当前活动正在测试.*最近活动暂无活动数据.*活跃状态暂无活动数据.*执行位置.*E:\\frozen-A/);
    assert.match(document.querySelector('.agent-usage-section').textContent, /Token 用量.*Total Tokens.*12,531.*统计不完整.*Input.*1.*Context Window.*—.*统计更新于/);
    assert.doesNotMatch(document.querySelector('.agent-usage-section').textContent, /15/);
    assert.match(document.querySelector('.agent-recovery-section').textContent, /AGENT_REAL_ERROR.*后端实际错误/);
    assert.doesNotMatch(document.querySelector('.agent-detail').textContent, /PID|CPU|RAM|Git branch/);
    await click('复制内容'); assert.equal(copiedText, execution.prompt);
    await click('恢复任务'); await click('取消任务');
    assert.deepEqual(calls.filter(call => call.action === 'resume_pending' || call.action === 'cancel'), [
      { action: 'resume_pending', executionId: execution.executionId }, { action: 'cancel', executionId: execution.executionId },
    ]);
    const technical = document.querySelector('.agent-technical'); assert.equal(technical.open, false);
    await act(async () => technical.querySelector('summary').click()); assert.equal(technical.open, true);
    assert.equal(technical.querySelector('.agent-raw-json').open, false);
    assert.match(technical.textContent, /Execution ID.*old-E1.*Thread ID.*thread-1.*Control Revision.*control-2.*Next Action.*observe/);
    await click('复制技术信息'); assert.deepEqual(JSON.parse(copiedText), execution);
  } finally { if (original) Object.defineProperty(navigator, 'clipboard', original); else delete navigator.clipboard; }
});

test('historical detail safely falls back to Provider ID and unknown usage projection', async () => {
  const historical = row({ provider: { id: 'legacy-provider', displayName: ' ', version: '' }, providerSessionLabel: null });
  await mount([historical]); await click('详情');
  const detail = document.querySelector('.agent-detail');
  assert.match(detail.querySelector('.agent-detail-header').textContent, /引擎: legacy-provider/);
  assert.doesNotMatch(detail.querySelector('.agent-detail-info-card').textContent, /legacy-session/);
  assert.match(detail.querySelector('.agent-usage-section').textContent, /Total Tokens.*—.*未知/);
});

test('Agent detail copies only a real result and continues with the original execution ID', async () => {
  const execution = row({ status: 'completed', attention: 'none', finalResult: { finalResult: [{ type: 'agentMessage', phase: 'final_answer', text: '真实结果' }] }, availableActions: { canCancel: false, canContinue: true, canResumePending: false } });
  let copiedText = '';
  const original = Object.getOwnPropertyDescriptor(navigator, 'clipboard');
  Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText: async text => { copiedText = text; } } });
  try {
    const calls = await mount([execution]); await click('详情');
    const resultSection = document.querySelector('.agent-result-section');
    const continuationSection = document.querySelector('.agent-continuation-section');
    assert.match(resultSection.textContent, /已完成 · 耗时/);
    assert.equal(resultSection.nextElementSibling, continuationSection);
    assert.equal(resultSection.parentElement, continuationSection.parentElement);
    assert.equal(resultSection.querySelector('.agent-continuation-card'), null);
    assert.ok(continuationSection.querySelector('.agent-continuation-card'));
    await click('复制结果'); assert.equal(copiedText, '真实结果');
    await input('继续真实任务', '#agent-continuation'); await click('继续任务');
    const continuation = calls.find(call => call.action === 'continue');
    assert.equal(continuation.executionId, 'old-E1'); assert.equal(continuation.prompt, '继续真实任务');
    assert.equal('workspaceId' in continuation, false); assert.equal('canonicalWorkspaceRoot' in continuation, false);
  } finally { if (original) Object.defineProperty(navigator, 'clipboard', original); else delete navigator.clipboard; }
});

test('continuation remains an independent section when no result is available', async () => {
  await mount([row({ status: 'running', attention: 'none', resultAvailable: false, finalResult: null, availableActions: { canCancel: false, canContinue: true, canResumePending: false } })]);
  await click('详情');
  assert.equal(document.querySelector('.agent-result-section'), null);
  const continuationSection = document.querySelector('.agent-continuation-section');
  assert.ok(continuationSection);
  assert.equal(continuationSection.parentElement, document.querySelector('.agent-detail-body'));
});

test('Agent detail duration labels follow active execution semantics rather than completedAt', async () => {
  const failed = row({ status: 'failed', attention: 'none', completedAt: null, progress: { phase: 'terminal', activityPhase: null, toolCategory: null, lastActivityAt: null, activityAgeMs: null, silenceLevel: null } });
  await mount([failed]); await click('详情');
  assert.doesNotMatch(document.querySelector('.agent-detail-header').textContent, /已运行/);
  assert.match(document.querySelector('.agent-detail-header').textContent, /耗时/);
  assert.match(document.querySelector('.agent-detail-live-grid').textContent, /总耗时/);
  assert.doesNotMatch(document.querySelector('.agent-detail-live-grid').textContent, /已运行/);
  await act(async () => root.unmount()); root = null;

  const running = row({ status: 'running', attention: 'none', completedAt: null, progress: { phase: 'running', activityPhase: 'provider', toolCategory: null, lastActivityAt: null, activityAgeMs: null, silenceLevel: null } });
  await mount([running]); await click('详情');
  assert.match(document.querySelector('.agent-detail-header').textContent, /已运行/);
  assert.match(document.querySelector('.agent-detail-live-grid').textContent, /已运行/);
  assert.doesNotMatch(document.querySelector('.agent-detail-live-grid').textContent, /总耗时/);
});

test('composer describes workspace execution without a permission selector', async () => {
  await mount([]);
  assert.match(document.querySelector('.agent-composer-footer').textContent, /Codex · 当前工作区 \(A\) · Local Runner Active/);
  assert.doesNotMatch(document.body.textContent, /只读执行/);
});

test('switching to a related execution from detail pane keeps its new details open', async () => {
  await mount([row({status:'completed',attention:'none',availableActions:{canContinue:true,canCancel:false,canResumePending:false}})], r => {
    if (r.action === 'continue') return {ok:false,error:{code:'AGENT_OPERATION_FAILED',message:'handoff error',executionId:'new-E2'}};
    if (r.action === 'observe' && r.executionId === 'new-E2') return {ok:true,data:row({executionId:'new-E2',prompt:'new related task'})};
  });
  await click('详情'); await input('next', '#agent-continuation'); await click('继续任务');
  await click('查看相关任务');
  await act(async () => new Promise(resolve => setTimeout(resolve, 10)));
  assert.ok(document.querySelector('[aria-label="任务详情"]'));
  assert.match(document.querySelector('.agent-detail-body > section .agent-prose').textContent, /new related task/);
});

test('list refresh cannot compete with an outstanding detail observation', async () => {
  let resolve;
  const calls = await mount([row()], r => r.action === 'observe' ? new Promise(done => {resolve = done;}) : undefined);
  await click('详情');
  // Direct invocation also exercises the handler guard while the native modal blocks interaction.
  await refreshTasks();
  assert.equal(calls.filter(c => c.action === 'list').length, 1);
  await act(async () => resolve({ok:true,data:row({prompt:'latest detail'})}));
  assert.match(document.querySelector('[aria-label="任务详情"] .agent-prose').textContent, /latest detail/);
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
    await act(async () => root.render(createElement(TooltipProvider, null, createElement(App))));
    await click('服务状态'); assert.equal(probes, 1);
    await click('首页'); await click('服务状态'); assert.equal(probes, 1);
    await act(async () => resolveProbe('codex-cli detected'));
    await click('首页'); await click('服务状态'); assert.equal(probes, 1);
    assert.match(document.body.textContent, /codex-cli detected/);
    await click('Serena GitHub ↗'); await click('CodeGraph GitHub ↗');
    assert.deepEqual(links, ['github', 'codegraph']);
    api.codexVersion = async () => { probes++; throw new Error('probe unavailable'); };
    await click('重新检测'); assert.equal(probes, 2);
    assert.match(document.body.textContent, /probe unavailable/);
    await click('首页'); await click('服务状态'); assert.equal(probes, 2);
    api.codexVersion = async () => { probes++; return 'codex-cli refreshed'; };
    await click('重新检测'); assert.equal(probes, 3);
    assert.match(document.body.textContent, /codex-cli refreshed/);
  } finally { Object.assign(api, originals); }
});

function navigationHost() {
  const host = document.createElement('aside'); host.id = 'task-nav-test'; document.body.append(host); return host;
}

test('sidebar uses official thread name consistently and falls back only for unnamed threads', async () => {
  const host = navigationHost();
  const project = workspace('A');
  const name = `0910 | 修复 | 会话标题 ${'长名称'.repeat(40)} <literal>`;
  const tasks = [row({ executionId: 'named', canonicalWorkspaceRoot: project.root, threadName: name }),
    ...[null, '', '   '].map((threadName, index) => row({ executionId: `unnamed-${index}`, canonicalWorkspaceRoot: project.root, threadName, prompt: `原始提示 ${index}` }))];
  await mount(tasks, undefined, { workspaces: [project], sidebarContainer: host });
  const links = host.querySelectorAll('.project-task-link');
  assert.equal(links[0].querySelector('.project-task-title').textContent, name);
  assert.equal(host.querySelector('.project-task-delete').getAttribute('aria-label'), `删除任务：${name}`);
  await act(async () => links[0].focus());
  assert.equal(document.querySelector('.project-task-preview strong').textContent, name);
  assert.equal(document.querySelector('.project-task-preview literal'), null);
  for (let i = 1; i < links.length; i++) assert.equal(links[i].querySelector('.project-task-title').textContent, `原始提示 ${i - 1}`);
});

test('recent tasks use the same official thread title as the sidebar with a legacy fallback', async () => {
  const name = '0911 | 修复 | 统一最近任务标题';
  const prompt = '这是一段不应作为最近任务主标题展示的完整任务描述';
  await mount([
    row({ executionId: 'named', threadName: name, prompt }),
    row({ executionId: 'legacy', threadName: '   ', prompt: '旧任务提示' }),
  ]);
  const titles = document.querySelectorAll('.agent-task-title h3');
  assert.equal(titles[0].textContent, name);
  assert.equal(titles[1].textContent, '旧任务提示');
  assert.ok(!document.querySelector('.agent-history').textContent.includes(prompt));
  await act(async () => titles[0].focus());
  assert.equal(document.querySelector('[role="tooltip"]').textContent, name);
});

test('project navigation groups tasks and opens a non-modal right content pane', async () => {
  const host = navigationHost(); let shown = 0;
  const projects = [workspace('A'), workspace('B')];
  const tasks = [row({executionId:'a1',canonicalWorkspaceRoot:projects[0].root,prompt:'项目 A 的任务'}), row({executionId:'b1',canonicalWorkspaceRoot:projects[1].root,prompt:'项目 B 的任务'})];
  const calls = await mount(tasks, undefined, {workspaces:projects,sidebarContainer:host,onShowTask:()=>shown++});
  const groups = host.querySelectorAll('.project-task-group');
  assert.equal(groups.length,2);
  assert.match(groups[0].textContent,/项目 A 的任务/); assert.ok(!groups[0].textContent.includes('项目 B 的任务'));
  await act(async () => groups[1].querySelector('.project-task-link').click());
  assert.equal(shown,1);
  assert.equal(document.querySelector('[role=dialog]'),null);
  assert.match(document.querySelector('.workspace')?.textContent ?? document.querySelector('.agent-detail').textContent,/项目 B 的任务/);
  assert.equal(document.querySelector('.agent-history').closest('[hidden]') !== null,true);
  assert.equal(groups[1].querySelector('.project-task-link').getAttribute('aria-current'),'page');
  assert.deepEqual(calls.filter(c=>c.action==='observe').at(-1),{action:'observe',executionId:'b1',waitMs:0,includeResult:true});
  await act(async () => groups[0].querySelector('.project-task-link').click());
  assert.match(document.querySelector('.agent-detail').textContent,/项目 A 的任务/);
  assert.equal(calls.some(c=>['start','continue','cancel','resume_pending'].includes(c.action)),false);
});

test('task detail view follows host navigation and has no in-page return', async () => {
  const host = navigationHost(); const project = workspace('A');
  let shownTask = 0; let shownAgent = 0;
  const props = { workspace: project, workspaces: [project], sidebarContainer: host, onShowTask: () => shownTask++, onShowAgent: () => shownAgent++, detailView: true };
  await mount([row({ canonicalWorkspaceRoot: project.root })], undefined, props);
  await act(async () => host.querySelector('.project-task-link').click());
  assert.equal(shownTask, 1);
  assert.equal(host.querySelector('.project-task').dataset.selected, 'true');
  const render = async detailView => act(async () => root.render(createElement(TooltipProvider, null, createElement(AgentPanel, { ...props, detailView }))));
  await render(false);
  assert.equal(host.querySelector('.project-task').dataset.selected, 'false');
  assert.equal(host.querySelector('[aria-current="page"]'), null);
  assert.equal(document.querySelector('.agent-detail'), null);
  assert.equal(document.querySelector('.agent-history').closest('[hidden]'), null);
  await render(true);
  assert.equal(host.querySelector('.project-task').dataset.selected, 'true');
  assert.match(document.querySelector('.agent-detail').textContent, /原始任务/);
  assert.equal(button('返回 Agent 任务'), undefined);
  assert.equal(shownAgent, 0);
});

test('sidebar dates use local calendar boundaries instead of elapsed 24 hours', async () => {
  const originalNow = Date.now;
  Date.now = () => new Date(2026, 0, 1, 0, 5).getTime();
  try {
    const host = navigationHost(); const project = workspace('A');
    await mount([
      row({ executionId: 'today', canonicalWorkspaceRoot: project.root, updatedAt: new Date(2026, 0, 1, 0, 1).getTime() }),
      row({ executionId: 'yesterday', canonicalWorkspaceRoot: project.root, updatedAt: new Date(2025, 11, 31, 23, 59).getTime() }),
      row({ executionId: 'older', canonicalWorkspaceRoot: project.root, updatedAt: new Date(2025, 11, 30, 23, 59).getTime() }),
    ], undefined, { workspaces: [project], sidebarContainer: host });
    assert.deepEqual([...host.querySelectorAll('time')].map(item => item.textContent), ['今天', '昨天', '2天前']);
  } finally { Date.now = originalNow; }
});

test('task focus shows basic information; only delete is offered and deletion stays local', async () => {
  const host = navigationHost(); const project=workspace('A');
  const calls = await mount([row({canonicalWorkspaceRoot:project.root,prompt:'检查项目构建'})], undefined, {workspaces:[project],sidebarContainer:host});
  const task = host.querySelector('.project-task');
  assert.equal(task.querySelectorAll('button').length,2);
  assert.equal(task.querySelector('.project-task-delete').textContent,'');
  await act(async () => task.querySelector('.project-task-link').focus());
  assert.match(document.querySelector('.project-task-preview').textContent,/检查项目构建/);
  assert.match(document.querySelector('.project-task-preview').textContent,/所属项目：A/);
  assert.match(document.querySelector('.project-task-preview').textContent,/更新于/);
  await act(async () => task.querySelector('.project-task-link').click());
  await act(async () => task.querySelector('.project-task-delete').click());
  assert.equal(host.querySelectorAll('.project-task').length,0);
  assert.equal(document.querySelector('.agent-detail'),null);
  await act(async () => { await new Promise(resolve => setTimeout(resolve, 40)); });
  assert.equal(document.activeElement, host.querySelector('.project-task-heading'));
  assert.deepEqual(JSON.parse(window.localStorage.getItem('agent-hidden-executions')),['old-E1']);
  assert.equal(calls.some(c=>['cancel','resume_pending'].includes(c.action)),false);
});

test('sidebar Provider and Token usage stay on Summary DTO without detail requests', async () => {
  const host = navigationHost(); const project = workspace('A');
  const tasks = [
    row({ executionId: 'complete', canonicalWorkspaceRoot: project.root, prompt: '完整统计', status: 'completed', attention: 'none', usage: { totalTokens: 12531, completeness: 'complete' } }),
    row({ executionId: 'partial', canonicalWorkspaceRoot: project.root, prompt: '部分统计', usage: { totalTokens: 12531, completeness: 'partial' } }),
    row({ executionId: 'unknown', canonicalWorkspaceRoot: project.root, prompt: '未知统计', usage: { totalTokens: null, completeness: 'unknown' } }),
    row({ executionId: 'zero', canonicalWorkspaceRoot: project.root, prompt: '零 Token', usage: { totalTokens: 0, completeness: 'complete' } }),
    row({ executionId: 'historical', canonicalWorkspaceRoot: project.root, prompt: '历史任务', usage: { totalTokens: null, completeness: 'unknown' } }),
    row({ executionId: 'custom', canonicalWorkspaceRoot: project.root, prompt: '自定义 Provider', provider: { id: 'custom-agent', displayName: ' ', version: null }, usage: { totalTokens: 8, completeness: 'complete' } }),
  ];
  const calls = await mount(tasks, undefined, { workspaces: [project], sidebarContainer: host });
  const originalHistory = api.agentHistory;
  let listQueries = 0;
  api.agentHistory = async (...args) => { listQueries++; return originalHistory(...args); };
  const group = host.querySelector('.project-task-group');
  const visible = [...group.querySelectorAll('.project-task')];
  assert.match(visible[0].textContent, /已完成 · Codex/);
  assert.match(visible[0].textContent, /总 Token：12,531/);
  assert.match(visible[1].textContent, /12,531 · 统计不完整/);
  assert.match(visible[2].textContent, /总 Token：—/);
  assert.match(visible[3].textContent, /总 Token：0/);
  assert.match(visible[4].textContent, /总 Token：—/);
  await act(async () => visible[0].querySelector('.project-task-link').focus());
  assert.match(document.querySelector('.project-task-preview').textContent, /完整统计/);
  assert.match(document.querySelector('.project-task-preview').textContent, /Codex/);
  assert.match(document.querySelector('.project-task-preview').textContent, /总 Token：12,531/);
  await act(async () => visible[1].querySelector('.project-task-link').focus());
  assert.match(document.querySelector('.project-task-preview').textContent, /部分统计/);
  assert.match(document.querySelector('.project-task-preview').textContent, /12,531 · 统计不完整/);
  await act(async () => visible[4].querySelector('.project-task-link').focus());
  assert.match(document.querySelector('.project-task-preview').textContent, /历史任务/);
  assert.match(document.querySelector('.project-task-preview').textContent, /总 Token：—/);
  assert.equal(listQueries, 0);
  assert.equal(calls.every(call => call.action === 'list'), true);
  await click('查看更多', group);
  assert.equal(listQueries, 1);
  const custom = group.querySelectorAll('.project-task')[5];
  assert.match(custom.textContent, /custom-agent/);
  await act(async () => custom.querySelector('.project-task-link').focus());
  assert.match(document.querySelector('.project-task-preview').textContent, /custom-agent/);
  assert.equal(listQueries, 1);
  assert.equal(calls.every(call => call.action === 'list'), true);
});

test('Phase 5 gate keeps Provider, Activity and Usage truthful across list, hover, detail and historical records', async () => {
  const host = navigationHost(); const project = workspace('A');
  // 同一冻结 fixture 同时覆盖新 Execution 的完整/部分/真实零与历史缺字段语义。
  const complete = row({
    executionId: 'p5-complete', canonicalWorkspaceRoot: project.root, prompt: '完整统计', status: 'finalizing', attention: 'none',
    provider: { id: 'acme-worker', displayName: 'Acme Worker', version: '2.4.1' }, providerSessionLabel: '续接会话 · S-42',
    usage: { inputTokens: 91, cachedInputTokens: 12, cacheWriteInputTokens: 7, outputTokens: 3, reasoningTokens: 5, totalTokens: 0, modelContextWindow: 128000, completeness: 'complete' },
    progress: { phase: 'reconciling', summaryCode: 'execution.finalizing', activityPhase: 'tool', toolCategory: 'command', lastActivityAt: null, activityAgeMs: null, silenceLevel: 'prolonged' },
    availableActions: { canCancel: false, canContinue: true, canResumePending: false },
  });
  const partial = row({ executionId: 'p5-partial', canonicalWorkspaceRoot: project.root, prompt: '部分统计', usage: { totalTokens: 12531, completeness: 'partial' } });
  const unknown = row({ executionId: 'p5-unknown', canonicalWorkspaceRoot: project.root, prompt: '未知统计', usage: { totalTokens: null, completeness: 'unknown' } });
  const historical = row({ executionId: 'p5-historical', canonicalWorkspaceRoot: project.root, prompt: '历史任务', provider: { id: 'legacy-provider', displayName: '', version: null }, usage: { totalTokens: null, completeness: 'unknown' } });
  const calls = await mount([complete, partial, unknown, historical], undefined, { workspaces: [project], sidebarContainer: host });
  const tasks = [...host.querySelectorAll('.project-task')];
  assert.match(tasks[0].textContent, /Acme Worker · v2\.4\.1.*总 Token：0/);
  assert.match(tasks[1].textContent, /12,531 · 统计不完整/);
  assert.match(tasks[2].textContent, /总 Token：—/);
  assert.match(tasks[3].textContent, /legacy-provider.*总 Token：—/);
  await act(async () => tasks[0].querySelector('.project-task-link').focus());
  assert.match(document.querySelector('.project-task-preview').textContent, /Acme Worker · v2\.4\.1.*总 Token：0/);
  assert.equal(calls.every(call => call.action === 'list'), true);

  await click('详情');
  const detail = document.querySelector('.agent-detail');
  assert.match(detail.querySelector('.agent-detail-info-card').textContent, /Provider.*Acme Worker · v2\.4\.1.*续接会话 · S-42.*当前活动正在整理结果.*活跃状态一段时间没有新活动/);
  assert.match(detail.querySelector('.agent-usage-section').textContent, /Total Tokens.*0.*完整.*Context Window.*128,000/);
  assert.doesNotMatch(detail.querySelector('.agent-usage-section').textContent, /118/);
  assert.match(detail.querySelector('.agent-continuation-footer').textContent, /Acme Worker · v2\.4\.1/);
  assert.doesNotMatch(detail.textContent, /PRIVATE_REASONING|stdout|source code/iu);
  assert.equal(calls.filter(call => call.action === 'observe').length, 1);
  assert.equal(calls.filter(call => call.action !== 'observe').every(call => call.action === 'list'), true);
});

test('project pagination and collapse are independent and older selected tasks keep updating', async () => {
  const host=navigationHost(); const projects=[workspace('A'),workspace('B')];
  const tasks=Array.from({length:12},(_,i)=>row({executionId:`task-${i}`,canonicalWorkspaceRoot:projects[i<7?0:1].root,prompt:`导航任务 ${i}`,revision:'R1'}));
  await mount(tasks,undefined,{workspaces:projects,sidebarContainer:host});
  const group=host.querySelectorAll('.project-task-group')[0];
  assert.equal(group.querySelectorAll('.project-task').length,5);
  await click('查看更多',group); assert.equal(group.querySelectorAll('.project-task').length,7);
  await act(async()=>group.querySelectorAll('.project-task-link')[6].click());
  tasks[6].revision='R2'; tasks[6].prompt='旧任务的新状态';
  await act(async()=>{await new Promise(resolve=>setTimeout(resolve,1600));});
  assert.match(document.querySelector('.agent-detail').textContent,/旧任务的新状态/);
  await act(async()=>group.querySelector('.project-task-heading').click());
  assert.equal(group.querySelector('ul'),null);
  assert.equal(host.querySelectorAll('.project-task-group')[1].querySelectorAll('.project-task').length,5);
});


test('deleting a sidebar task restores keyboard focus to its neighbor', async () => {
  const host=navigationHost(); const project=workspace('A');
  await mount([row({executionId:'first',canonicalWorkspaceRoot:project.root,prompt:'第一项'}),row({executionId:'second',canonicalWorkspaceRoot:project.root,prompt:'第二项'})],undefined,{workspaces:[project],sidebarContainer:host});
  await act(async () => host.querySelector('.project-task-delete').focus());
  await act(async () => host.querySelector('.project-task-delete').click());
  await act(async () => { await new Promise(resolve => setTimeout(resolve,40)); });
  assert.equal(document.activeElement,host.querySelector('.project-task-link'));
  assert.match(document.activeElement.textContent,/第二项/);
});


test('project order supports keyboard movement and persists across remount', async () => {
  const host = navigationHost(); const projects = [workspace('A'),workspace('B'),workspace('C')];
  await mount([], undefined, {workspaces:projects,sidebarContainer:host});
  const heading = host.querySelectorAll('.project-task-heading')[2];
  await act(async()=>heading.dispatchEvent(new window.KeyboardEvent('keydown',{key:'ArrowUp',altKey:true,bubbles:true})));
  assert.deepEqual([...host.querySelectorAll('.project-task-group')].map(e=>e.getAttribute('aria-label')),['A','C','B']);
  assert.deepEqual(JSON.parse(window.localStorage.getItem('agent-project-order')),projects.map(p=>p.root).toSpliced(1,2,projects[2].root,projects[1].root));
  await act(async()=>root.unmount());
  await mount([],undefined,{workspaces:[...projects,workspace('D')],sidebarContainer:host});
  assert.deepEqual([...host.querySelectorAll('.project-task-group')].map(e=>e.getAttribute('aria-label')),['A','C','B','D']);
});


test('project hints use keyboard accessible tooltips without native titles', async () => {
  const host = navigationHost();
  await mount([], undefined, {workspaces:[workspace('A')],sidebarContainer:host});
  const heading = host.querySelector('.project-task-heading');
  await act(async()=>heading.focus());
  assert.match(document.querySelector('[role="tooltip"]').textContent,/拖动可排序/);
  assert.ok(heading.getAttribute('aria-describedby'));
  assert.equal(host.querySelector('[title]'),null);
  await act(async()=>heading.dispatchEvent(new window.KeyboardEvent('keydown',{key:'Escape',bubbles:true})));
  assert.equal(document.querySelector('[role="tooltip"]'),null);
  assert.equal(heading.getAttribute('aria-expanded'),'true');
});

test('sidebar load-more button follows the real pending request and recovers to retry', async () => {
  const host = navigationHost(); const project = workspace('A');
  const tasks = Array.from({ length: 7 }, (_, index) => row({ executionId: `task-${index}`, canonicalWorkspaceRoot: project.root }));
  await mount(tasks, undefined, { workspaces: [project], sidebarContainer: host });
  const group = host.querySelector('.project-task-group');
  let pending = Promise.withResolvers();
  api.agentHistory = () => pending.promise;
  const more = group.querySelector('.project-task-more');
  assert.equal(more.textContent.trim(), '查看更多');
  assert.equal(more.getAttribute('aria-busy'), 'false');
  assert.equal(more.dataset.state, 'idle');
  assert.ok(more.querySelector('.project-task-more-icon-idle'));

  await act(async () => more.click());
  assert.equal(group.querySelector('.project-task-more'), more);
  assert.equal(more.textContent.trim(), '正在加载…');
  assert.equal(more.getAttribute('aria-busy'), 'true');
  assert.equal(more.dataset.state, 'loading');
  assert.equal(more.disabled, true);
  assert.ok(more.querySelector('.project-task-more-icon-loading'));
  await act(async () => pending.resolve({ executions: tasks.slice(5), nextCursor: 'next-page' }));
  assert.equal(more.textContent.trim(), '查看更多');
  assert.equal(more.getAttribute('aria-busy'), 'false');
  assert.equal(more.dataset.state, 'idle');

  pending = Promise.withResolvers();
  api.agentHistory = () => pending.promise;
  await act(async () => more.click());
  await act(async () => pending.reject(new Error('page unavailable')));
  assert.equal(group.querySelector('.project-task-more'), more);
  assert.equal(more.textContent.trim(), '重试');
  assert.equal(more.getAttribute('aria-busy'), 'false');
  assert.equal(more.dataset.state, 'retry');
  assert.equal(more.disabled, false);
  assert.ok(more.querySelector('.project-task-more-icon-retry'));
  assert.match(group.querySelector('[role="status"]').textContent, /加载更多失败，请重试/);

  api.agentHistory = async () => ({ executions: [], nextCursor: 'remaining-page' });
  await act(async () => more.click());
  assert.equal(group.querySelector('[role="status"]'), null);
  assert.equal(more.textContent.trim(), '查看更多');
  assert.equal(more.dataset.state, 'idle');
  const styles = readFileSync('src/styles.css', 'utf8');
  assert.match(styles, /\.project-task-more\[data-state="loading"\] \.project-task-more-icon-loading \{ animation: agent-control-spin/);
  assert.match(styles, /@media \(prefers-reduced-motion: reduce\) \{\r?\n  \.project-task, \.project-task-heading, \.project-task-more-icon-slot svg \{ transition: none; \}\r?\n  \.project-task-more\[data-state="loading"\] \.project-task-more-icon-loading, \.project-task-more-label \{ animation: none; \}\r?\n\}/);
});

test('sidebar load-more keeps pagination retry separate from its background refresh', async () => {
  const originalSetInterval = window.setInterval;
  const originalClearInterval = window.clearInterval;
  let refreshNow;
  window.setInterval = callback => { refreshNow = callback; return 1; };
  window.clearInterval = () => {};
  try {
    const host = navigationHost(); const project = workspace('A');
    const tasks = Array.from({ length: 6 }, (_, index) => row({ executionId: `task-${index}`, canonicalWorkspaceRoot: project.root }));
    await mount(tasks, undefined, { workspaces: [project], sidebarContainer: host });
    const more = host.querySelector('.project-task-more');
    let pending = Promise.withResolvers();
    api.agentHistory = () => pending.promise;
    assert.ok(refreshNow);
    await act(async () => refreshNow());
    assert.equal(more.disabled, true);
    assert.equal(more.dataset.state, 'idle');
    await act(async () => more.click());
    assert.equal(more.getAttribute('aria-busy'), 'false');
    await act(async () => pending.reject(new Error('refresh unavailable')));
    assert.equal(more.disabled, false);
    assert.equal(more.textContent.trim(), '查看更多');
    assert.equal(more.dataset.state, 'idle');
    assert.match(host.querySelector('[role="status"]').textContent, /任务加载失败/);

    pending = Promise.withResolvers();
    api.agentHistory = () => pending.promise;
    await act(async () => more.click());
    await act(async () => pending.reject(new Error('page unavailable')));
    assert.equal(more.textContent.trim(), '重试');
    assert.equal(more.dataset.state, 'retry');
    assert.match([...host.querySelectorAll('[role="status"]')].at(-1).textContent, /加载更多失败，请重试/);

    api.agentHistory = async () => ({ executions: tasks.slice(0, 5), nextCursor: 'task-4' });
    await act(async () => refreshNow());
    assert.equal(more.textContent.trim(), '重试');
    assert.equal(more.dataset.state, 'retry');
    assert.equal(host.querySelectorAll('[role="status"]').length, 1);
    assert.match(host.querySelector('[role="status"]').textContent, /加载更多失败，请重试/);
  } finally {
    if (root) await act(async () => root.unmount());
    root = null;
    window.setInterval = originalSetInterval;
    window.clearInterval = originalClearInterval;
  }
});

test('technical copy remains available without exposing JSON until technical information is opened', async () => {
  await mount([row()]); await click('详情');
  const details = document.querySelector('.agent-technical'); assert.equal(details.open, false);
  const original=Object.getOwnPropertyDescriptor(globalThis,'navigator');
  Object.defineProperty(globalThis,'navigator',{configurable:true,value:{clipboard:{writeText:async()=>{}}}});
  try {
    await act(async()=>document.querySelector('.agent-technical-copy').click());
    assert.equal(document.querySelector('.agent-technical-copy').dataset.copyState,'copied');
    assert.equal(details.open, false);
    await act(async()=>details.querySelector('summary').click());
    assert.equal(details.open, true);
    assert.ok(document.querySelector('[aria-label="原始执行数据"]'));
  } finally { if (original) Object.defineProperty(globalThis,'navigator',original); else delete globalThis.navigator; }
});


test('task and final result render GFM while technical data stays original', async () => {
  const markdown = '# 标题\n\n**重点**和 `inline`\n\n- 第一项\n- 第二项\n\n> 引用\n\n| 名称 | 状态 |\n| --- | --- |\n| 构建 | 通过 |\n\n- [x] 已完成\n\n```js\nconst value = 1;\n```\n\n[文档](https://example.com/docs)';
  const value=row({prompt:markdown,status:'completed',finalResult:{finalResult:[{type:'agentMessage',phase:'final_answer',text:markdown}]}});
  await mount([value]); await click('详情');
  const blocks=document.querySelectorAll('.agent-markdown');assert.equal(blocks.length,2);
  for(const block of blocks) {
    assert.equal(block.querySelector('h1').textContent,'标题');
    assert.equal(block.querySelector('strong').textContent,'重点');
    assert.equal(block.querySelectorAll('table tbody tr').length,1);
    assert.equal(block.querySelector('input[type="checkbox"]').checked,true);
    assert.equal(block.querySelector('input[type="checkbox"]').disabled,true);
    assert.match(block.querySelector('pre code').textContent,/const value = 1/);
    assert.equal(block.querySelector('a').target,'_blank');
  }
  assert.equal(JSON.parse(document.querySelector('[aria-label="原始执行数据"]').textContent).prompt,markdown);
});

test('markdown renders HTML literally and rejects executable links', async () => {
  await mount([row({prompt:'<script>alert(1)</script>\n\n<img src=x onerror=alert(1)>\n\n[危险](javascript:alert%281%29)'})]);
  await click('详情');
  const block=document.querySelector('.agent-markdown');
  assert.equal(block.querySelector('script, img, [onerror], a'),null);
  assert.match(block.textContent,/<script>alert/);
  assert.match(block.textContent,/危险/);
});


test('sidebar task markers distinguish processing, errors and inactive tasks', async () => {
  const host = navigationHost(); const project = workspace('A');
  const statuses = ['running', 'failed', 'unknown', 'completed', 'dispatch_pending'];
  await mount(statuses.map(status => row({ executionId: status, prompt: status, status,
    attention: status === 'unknown' ? 'manual_resolution_required' : 'none',
    canonicalWorkspaceRoot: project.root, progress: { phase: status === 'running' ? 'running' : 'pending' },
  })), undefined, { workspaces: [project], sidebarContainer: host });
  const items = [...host.querySelectorAll('.project-task-link')];
  assert.equal(items.length, 5);
  const styles = readFileSync('src/styles.css', 'utf8');
  assert.match(styles, /\.sidebar > nav\[aria-label="主导航"\] button/);
  assert.doesNotMatch(styles, /\.sidebar nav button/);
  assert.match(styles, /\.sidebar \.project-task-navigation \.project-task-link \{[^}]*padding: 6px 10px 6px 36px[^}]*color: #334155/);
  assert.equal(items[0].querySelector('svg[aria-label="执行中"]').classList.contains('animate-spin'), true);
  for (const index of [1, 2]) {
    assert.ok(items[index].querySelector('svg.tone-red'));
    assert.equal(items[index].querySelector('.animate-spin'), null);
  }
  for (const index of [3, 4]) assert.equal(items[index].querySelector('.project-task-state-icon'), null);
  await act(async () => items[0].focus());
  assert.equal(document.querySelector('.project-task-preview .agent-status').textContent, '执行中');
  assert.ok(document.querySelector('.project-task-preview .tone-blue'));
});


for (const entry of ['sidebar', 'history']) test(`running task deletion requires confirmation from ${entry} without stopping provider`, async () => {
  const project = workspace('A'); const host = navigationHost();
  const calls = await mount([row({ status: 'running', attention: 'none', prompt: '正在执行的任务', canonicalWorkspaceRoot: project.root })], undefined, { workspaces: [project], sidebarContainer: host });
  const trigger = entry === 'sidebar' ? host.querySelector('.project-task-delete') : document.querySelector('.agent-delete-action');
  await act(async () => { trigger.focus(); trigger.click(); });
  assert.ok(document.querySelector('[role="dialog"]'));
  assert.match(document.querySelector('[role="dialog"]').textContent, /不会停止 Agent/);
  assert.equal(window.localStorage.getItem('agent-hidden-executions'), null);
  await act(async () => [...document.querySelectorAll('[role="dialog"] button')].find(b => b.textContent === '保留任务').click());
  assert.equal(document.querySelector('[role="dialog"]'), null);
  assert.ok(host.querySelector('.project-task'));
  await act(async () => trigger.click());
  await act(async () => [...document.querySelectorAll('[role="dialog"] button')].find(b => b.textContent === '仅从列表删除').click());
  assert.deepEqual(JSON.parse(window.localStorage.getItem('agent-hidden-executions')), ['old-E1']);
  assert.equal(host.querySelector('.project-task'), null);
  assert.equal(document.querySelector('[role="dialog"]'), null);
  assert.equal(calls.some(c => ['cancel', 'resume_pending', 'start', 'continue'].includes(c.action)), false);
});


test('recent task title tooltip shows a bounded summary instead of the full prompt', async () => {
  const prompt = '检查任务内容。\n'.repeat(120);
  await mount([row({ prompt })]);
  const title = document.querySelector('.agent-task-title h3');
  await act(async () => title.focus());
  const tooltip = document.querySelector('[role="tooltip"]');
  assert.ok(tooltip);
  assert.equal(tooltip.textContent, title.textContent);
  assert.ok(Array.from(tooltip.textContent).length <= 101);
  assert.ok(!tooltip.textContent.includes('\n'));
  assert.notEqual(tooltip.textContent, prompt);
});
