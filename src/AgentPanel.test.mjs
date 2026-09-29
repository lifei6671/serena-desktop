import { providerSettingsFixture } from './configFixtures.mjs';
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
const { showExecutionIssueSection, showExecutionDiagnostic } = await import('./agentPresentation.ts');
const { activityLabel } = await import('./agentPresentation.ts');
const { toast } = await import('sonner');
const notifications = [];
toast.success = text => notifications.push(['success', text]);
toast.error = text => notifications.push(['error', text]);
let root;
const workspace = name => ({ id: name, name, root: `E:\\${name}` });
const row = (overrides = {}) => ({ executionId: 'old-E1', agentId: 'old-lineage', workspaceId: 'A', canonicalWorkspaceRoot: 'E:\\frozen-A', prompt: '原始任务 <literal>', status: 'unknown', attention: 'manual_resolution_required', revision: 'R1', resultAvailable: overrides.finalResult !== undefined && overrides.finalResult !== null, provider: { id: 'codex', displayName: 'Codex', version: null }, executionProfile: { model: null, reasoning: null }, effectiveExecutionProfile: null, providerSessionLabel: null, usage: { inputTokens: null, cachedInputTokens: null, cacheWriteInputTokens: null, outputTokens: null, reasoningTokens: null, totalTokens: null, modelContextWindow: null, completeness: 'unknown', usageRevision: 0, updatedAt: null }, progress: { phase: 'reconciling', summaryCode: 'execution.reconciling', activityPhase: null, toolCategory: null, lastActivityAt: null, activityAgeMs: null, silenceLevel: null }, nextAction: { action: 'manual_resolution' }, dispatchState: 'uncertain', threadId: null, threadName: null, turnId: null, providerTerminalStatus: null, errorCode: null, errorMessage: null, resultCompleteness: 'none', interruptRequested: false, interruptAcknowledged: false, interruptTimedOut: false, createdAt: 1000, updatedAt: 2000, completedAt: null,
  availableActions: { canCancel: false, canContinue: false, canResumePending: false }, ...overrides, provider: { id: 'codex', displayName: 'Codex', version: null, ...overrides.provider }, usage: { inputTokens: null, cachedInputTokens: null, cacheWriteInputTokens: null, outputTokens: null, reasoningTokens: null, totalTokens: null, modelContextWindow: null, completeness: 'unknown', usageRevision: 0, updatedAt: null, ...overrides.usage }, progress: { phase: 'reconciling', summaryCode: 'execution.reconciling', activityPhase: null, toolCategory: null, lastActivityAt: null, activityAgeMs: null, silenceLevel: null, ...overrides.progress } });
async function mount(rows, handler, props = {}, catalog = { providers: [], roleRouting: {} }, configurationHandler, defaultsHandler) {
  const calls = [];
  // 独立目录 fixture，避免任务行为测试依赖本机 Tauri 环境。
  api.agentProviderCatalog = typeof catalog === 'function' ? catalog : async () => structuredClone(catalog);
  api.agentProviderConfigurationCatalog = configurationHandler ?? (async providerId => ({ providerId, models: [{ id: 'default-model', name: 'Default Model', description: null, isDefault: true, hidden: false, reasoningOptions: [{ id: 'high', name: 'High', description: null }], defaultReasoning: 'high' }], currentModel: 'default-model', defaultModel: 'default-model', reasoningOptions: [], currentReasoning: 'high', defaultReasoning: 'high' }));
  api.agentProviderSetRoleDefaults = defaultsHandler ?? (async (role, providerId, defaults) => ({ ...roleSettings(), roleDefaults: { [role]: { [providerId]: structuredClone(defaults) } } }));
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
  const sidebarContainer = props.sidebarContainer ?? document.getElementById('task-nav-test') ?? navigationHost();
  const workspaces = [...new Map(rows.map(row => [row.canonicalWorkspaceRoot, { id: row.workspaceId, name: row.workspaceId, root: row.canonicalWorkspaceRoot }])).values()];
  root = createRoot(document.getElementById('root'));
  await act(async () => root.render(createElement(TooltipProvider, null, createElement(AgentPanel, { workspace: workspace('A'), workspaces, sidebarContainer, ...props }))));
  return calls;
}
function button(text, within = document) { return [...within.querySelectorAll('button')].find(b => b.textContent === text); }

/** Catalog fixture 只声明后端已有字段；协议始终缺失。 */
function catalogProvider(overrides = {}) {
  return { id: 'codex', displayName: 'Codex', version: '1.2.3', enabled: true, health: 'available',
    availableForNewExecution: true,
    capabilities: { canExecute: true, canContinue: false, canCancel: true, canRecover: false, activity: true, tokenUsage: false },
    ...overrides };
}

/** 读取可访问的字段名称与值，验证用户看到的独立状态维度。 */
function cardValues(card) {
  return Object.fromEntries([...card.querySelectorAll('dl > div')].map(field => [field.querySelector('dt').textContent, field.querySelector('dd').textContent]));
}

// 逐项验证正常 Idle、停用、健康失败与 draining，避免合并独立状态。
for (const scenario of [
  { name: 'idle available + stopped', provider: {}, rows: [], enabled: '已启用', health: '可用', runtime: '已停止', active: '0' },
  { name: 'disabled without active execution', provider: { enabled: false }, rows: [], enabled: '已停用', health: '可用', runtime: '已停止', active: '0' },
  { name: 'unavailable independently of enabled', provider: { health: 'unavailable' }, rows: [], enabled: '已启用', health: '不可用', runtime: '已停止', active: '0' },
  { name: 'disabled and unavailable remain separate', provider: { enabled: false, health: 'unavailable' }, rows: [], enabled: '已停用', health: '不可用', runtime: '已停止', active: '0' },
  { name: 'draining while execution remains active', provider: { enabled: false }, rows: [row({ status: 'running', attention: 'none' })], enabled: '正在停用', health: '可用', runtime: '运行中', active: '1' },
]) {
  test(`provider card: ${scenario.name}`, async () => {
    await mount(scenario.rows, undefined, {}, { providers: [catalogProvider(scenario.provider)], roleRouting: {} });
    const cards = document.querySelectorAll('.agent-provider-card');
    assert.equal(cards.length, 1);
    assert.equal(cards[0].querySelector('h3').textContent, 'Codex');
    assert.deepEqual(cardValues(cards[0]), { 接入: scenario.enabled, 可用性: scenario.health, 版本: '1.2.3', 协议: '—', Runtime: scenario.runtime, 活动任务: scenario.active });
    assert.equal(cards[0].querySelector('[role="alert"]'), null);
    if (scenario.health === '可用') assert.equal(cards[0].querySelectorAll('.tone-red').length, 0);
    assert.equal(document.querySelector('h1').textContent, 'Agent 管理');
    assertManagementSurface();
  });
}

test('provider cards dynamically render two providers and aggregate frozen row provider IDs', async () => {
  const rows = [
    row({ executionId: 'a', status: 'running', attention: 'none', provider: { id: 'codex', displayName: 'same name' } }),
    row({ executionId: 'b', status: 'finalizing', attention: 'none', provider: { id: 'codebuddy', displayName: 'same name' } }),
    row({ executionId: 'c', status: 'dispatch_pending', attention: 'pending_explicit_resume', provider: { id: 'codebuddy' } }),
    row({ executionId: 'd', status: 'completed', attention: 'none', provider: { id: 'codebuddy' } }),
    row({ executionId: 'e', status: 'running', attention: 'none', provider: { id: 'historical-other' } }),
  ];
  window.localStorage.setItem('agent-hidden-executions', JSON.stringify(['b']));
  await mount(rows, undefined, {}, { providers: [catalogProvider(), catalogProvider({ id: 'codebuddy', displayName: 'CodeBuddy', enabled: false })], roleRouting: { general: 'codex' } });
  const cards = document.querySelectorAll('.agent-provider-card');
  assert.deepEqual([...cards].map(card => card.querySelector('h3').textContent), ['Codex', 'CodeBuddy']);
  assert.equal(cardValues(cards[0]).活动任务, '1');
  assert.equal(cardValues(cards[1]).活动任务, '2');
  assert.equal(cardValues(cards[1]).接入, '正在停用');
  assert.equal(cards[1].querySelector('select,[role="combobox"]'), null);
  assert.match(document.querySelector('.agent-providers').textContent, /当前已加载/);
  assert.match(document.querySelector('.agent-providers').textContent, /不代表系统进程状态/);
});

test('unknown provider uses id and missing version/protocol fallbacks with the same card layout', async () => {
  const unknown = catalogProvider({ id: 'future-agent', displayName: null, version: null });
  const missing = catalogProvider({ id: 'new-provider' });
  delete missing.displayName;
  delete missing.version;
  await mount([], undefined, {}, { providers: [unknown, missing], roleRouting: {} });
  const cards = document.querySelectorAll('.agent-provider-card');
  assert.deepEqual([...cards].map(card => card.querySelector('h3').textContent), ['future-agent', 'new-provider']);
  for (const card of cards) {
    assert.equal(cardValues(card).版本, '—');
    assert.equal(cardValues(card).协议, '—');
    assert.equal(cardValues(card).Runtime, '已停止');
    assert.equal(card.querySelectorAll('dl > div').length, 6);
  }
});

test('catalog failure preserves sidebar and detail access without composer', async () => {
  await mount([row({ status: 'completed', attention: 'none' })], undefined, {}, async () => { throw new Error('catalog unavailable'); });
  assert.match(document.querySelector('.agent-providers [role="alert"]').textContent, /接入信息读取失败/);
  assertManagementSurface();
  await openTask();
  assert.ok(document.querySelector('details.agent-technical'));
});

test('provider presentation and card layout contain no provider ID special cases', () => {
  const presentation = readFileSync('src/agentPresentation.ts', 'utf8');
  const panel = readFileSync('src/AgentPanel.tsx', 'utf8');
  // 仅允许稳定诊断码及固定提示中的产品名称，其余源码仍禁止 Provider ID 特判。
  assert.doesNotMatch(presentation
    .replaceAll('"CODEBUDDY_ACP_INCOMPATIBLE"', '""')
    .replaceAll('请升级 CodeBuddy 或 SerenaDesktop 后重新检测。', ''), /codex|codebuddy/iu);
  assert.doesNotMatch(panel, /codex|codebuddy/iu);
  assert.match(panel, /catalog\?\.providers\.map\(provider/u);
  assert.match(readFileSync('src/api.ts', 'utf8'), /invoke<ProviderCatalogSnapshot>\("agent_provider_catalog_get"\)/u);
});
async function click(text, within) { const b = button(text, within); assert.ok(b, text); assert.equal(b.disabled, false, text); await act(async () => b.click()); }

/** 通过真实 shadcn Select 的键盘入口与 option 完成角色选择。 */
async function chooseRole(role, label) {
  dom.window.HTMLElement.prototype.scrollIntoView = () => {};
  const trigger = document.querySelector(`#agent-role-${role}`);
  assert.equal(trigger.disabled, false);
  await act(async () => trigger.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true })));
  const option = [...document.querySelectorAll('[role="option"]')].find(node => node.textContent === label);
  assert.ok(option, label);
  assert.notEqual(option.getAttribute('aria-disabled'), 'true');
  await act(async () => option.click());
}

/** 通过 aria-label 操作模型或推理 Select，避免测试绑定内部 DOM 层级。 */
async function chooseConfiguration(label, optionLabel) {
  dom.window.HTMLElement.prototype.scrollIntoView = () => {};
  const trigger = [...document.querySelectorAll('[role="combobox"]')].find(node => node.getAttribute('aria-label') === label);
  assert.ok(trigger, label);
  assert.equal(trigger.disabled, false);
  await act(async () => trigger.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true })));
  const option = [...document.querySelectorAll('[role="option"]')].find(node => node.textContent === optionLabel);
  assert.ok(option, optionLabel);
  assert.notEqual(option.getAttribute('aria-disabled'), 'true');
  await act(async () => option.click());
}

/** 等待配置目录 Promise 对应的 React state 提交。 */
async function flushConfigurationCatalog() {
  await act(async () => { await new Promise(resolve => setTimeout(resolve, 0)); });
}

/** 延迟 IPC/轮询 fixture，显式控制跨角色响应与旧快照的返回顺序。 */
function deferredRoleResponse() {
  let resolve, reject;
  const promise = new Promise((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
}

/** 返回完整后端 settings，测试不以 draft 充当提交结果。 */
function roleSettings(roleRouting = {}) {
  return { providers: {}, roleRouting: { development: null, testing: null, review: null, analysis: null, general: null, ...roleRouting } };
}

test('role editor sets enabled provider through local IPC and commits returned authoritative route', async () => {
  const calls = [];
  api.agentProviderSetRoleRoute = async (taskRole, providerId) => {
    calls.push({ taskRole, providerId });
    return roleSettings({ development: 'returned-provider' });
  };
  const executions = [row({ status: 'running', taskRole: 'analysis' })];
  const frozen = structuredClone(executions);
  const actions = await mount(executions, undefined, {}, { providers: [catalogProvider()], roleRouting: {} });
  const section = document.querySelector('.agent-role-routing');
  assert.equal(section.querySelectorAll('[role="combobox"]').length, 15);
  assert.ok(document.querySelector('.agent-providers').compareDocumentPosition(section) & Node.DOCUMENT_POSITION_FOLLOWING);
  assert.ok(section.compareDocumentPosition(document.querySelector('.agent-workspace-bar')) & Node.DOCUMENT_POSITION_FOLLOWING);
  await chooseRole('development', 'Codex');
  assert.deepEqual(calls, [{ taskRole: 'development', providerId: 'codex' }]);
  assert.match(document.querySelector('#agent-role-development').textContent, /returned-provider · 未注册/);
  assert.deepEqual(executions, frozen);
  assert.ok(actions.every(action => action.action === 'list'));
  assert.match(readFileSync('src/api.ts', 'utf8'), /invoke<AgentProviderSettings>\("agent_provider_set_role_route", \{ taskRole, providerId \}\)/);
});

test('role defaults remember independent model and reasoning values when provider changes', async () => {
  const catalog = {
    providers: [catalogProvider(), catalogProvider({ id: 'other', displayName: 'Other' })],
    roleRouting: { development: 'codex' },
    roleDefaults: { development: {
      codex: { model: 'codex-model', reasoning: 'high' },
      other: { model: 'other-model', reasoning: 'low' },
    } },
  };
  api.agentProviderSetRoleRoute = async (role, providerId) => ({ ...roleSettings({ [role]: providerId }), roleDefaults: catalog.roleDefaults });
  const configuration = async providerId => providerId === 'codex'
    ? { providerId, models: [{ id: 'codex-model', name: 'Codex Model', isDefault: true, hidden: false, reasoningOptions: [{ id: 'high', name: 'High' }], defaultReasoning: 'high' }], currentModel: 'codex-model', defaultModel: 'codex-model', reasoningOptions: [] }
    : { providerId, models: [{ id: 'other-model', name: 'Other Model', isDefault: true, hidden: false, reasoningOptions: [{ id: 'low', name: 'Low' }], defaultReasoning: 'low' }], currentModel: 'other-model', defaultModel: 'other-model', reasoningOptions: [] };
  await mount([], undefined, {}, catalog, configuration);
  await flushConfigurationCatalog();
  const model = () => [...document.querySelectorAll('[role="combobox"]')].find(node => node.getAttribute('aria-label') === '开发默认模型');
  const reasoning = () => [...document.querySelectorAll('[role="combobox"]')].find(node => node.getAttribute('aria-label') === '开发推理强度');
  assert.match(model().textContent, /Codex Model/);
  assert.match(reasoning().textContent, /High/);
  await chooseRole('development', 'Other');
  await flushConfigurationCatalog();
  assert.match(model().textContent, /Other Model/);
  assert.match(reasoning().textContent, /Low/);
  await chooseRole('development', 'Codex');
  assert.match(model().textContent, /Codex Model/);
  assert.match(reasoning().textContent, /High/);
});

test('model and reasoning Select mutations send the complete pair and commit authoritative settings', async () => {
  const calls = [];
  const catalog = { providers: [catalogProvider()], roleRouting: { development: 'codex' }, roleDefaults: { development: { codex: { model: 'model-a', reasoning: 'high' } } } };
  const configuration = async providerId => ({ providerId, models: [
    { id: 'model-a', name: 'Model A', isDefault: true, hidden: false, reasoningOptions: [{ id: 'high', name: 'High' }], defaultReasoning: 'high' },
    { id: 'model-b', name: 'Model B', isDefault: false, hidden: false, reasoningOptions: [{ id: 'high', name: 'High' }, { id: 'low', name: 'Low' }], defaultReasoning: 'low' },
  ], defaultModel: 'model-a', reasoningOptions: [] });
  const save = async (role, providerId, defaults) => {
    calls.push({ role, providerId, defaults: structuredClone(defaults) });
    const saved = calls.length === 1 ? { model: 'model-b', reasoning: 'low' } : { model: 'model-b', reasoning: 'high' };
    catalog.roleDefaults.development.codex = structuredClone(saved);
    return { ...roleSettings({ development: 'codex' }), roleDefaults: structuredClone(catalog.roleDefaults) };
  };
  await mount([], undefined, {}, catalog, configuration, save);
  await flushConfigurationCatalog();
  await chooseConfiguration('开发默认模型', 'Model B');
  assert.deepEqual(calls[0], { role: 'development', providerId: 'codex', defaults: { model: 'model-b', reasoning: 'high' } });
  const reasoning = () => [...document.querySelectorAll('[role="combobox"]')].find(node => node.getAttribute('aria-label') === '开发推理强度');
  assert.equal(reasoning().textContent, 'Low · Provider 默认');
  await chooseConfiguration('开发推理强度', 'High');
  assert.deepEqual(calls[1], { role: 'development', providerId: 'codex', defaults: { model: 'model-b', reasoning: 'high' } });
  assert.equal(reasoning().textContent, 'High');
});

test('pending defaults suppress duplicate mutation and failure restores the prior value', async () => {
  const mutation = deferredRoleResponse();
  const calls = [];
  const catalog = { providers: [catalogProvider()], roleRouting: { development: 'codex' }, roleDefaults: { development: { codex: { model: 'model-a', reasoning: 'high' } } } };
  const configuration = async providerId => ({ providerId, models: [{ id: 'model-a', name: 'Model A', isDefault: true, hidden: false, reasoningOptions: [{ id: 'high', name: 'High' }, { id: 'low', name: 'Low' }], defaultReasoning: 'high' }], defaultModel: 'model-a', reasoningOptions: [] });
  const save = (...args) => { calls.push(structuredClone(args)); return mutation.promise; };
  await mount([], undefined, {}, catalog, configuration, save);
  await flushConfigurationCatalog();
  await chooseConfiguration('开发推理强度', 'Low');
  const trigger = [...document.querySelectorAll('[role="combobox"]')].find(node => node.getAttribute('aria-label') === '开发推理强度');
  assert.equal(trigger.disabled, true);
  await act(async () => trigger.click());
  assert.equal(calls.length, 1);
  await act(async () => mutation.reject(new Error('persist failed')));
  assert.equal(trigger.textContent, 'High · Provider 默认');
  assert.match(notifications.at(-1)[1], /默认配置保存失败/);
});

test('changing model retains an invalid saved reasoning value until the user explicitly changes it', async () => {
  const calls = [];
  const catalog = { providers: [catalogProvider()], roleRouting: { development: 'codex' }, roleDefaults: { development: { codex: { model: 'model-a', reasoning: 'high' } } } };
  const configuration = async providerId => ({ providerId, models: [
    { id: 'model-a', name: 'Model A', isDefault: true, hidden: false, reasoningOptions: [{ id: 'high', name: 'High' }], defaultReasoning: 'high' },
    { id: 'model-b', name: 'Model B', isDefault: false, hidden: false, reasoningOptions: [{ id: 'low', name: 'Low' }], defaultReasoning: 'low' },
  ], defaultModel: 'model-a', reasoningOptions: [] });
  const save = async (role, providerId, defaults) => {
    calls.push({ role, providerId, defaults: structuredClone(defaults) });
    return { ...roleSettings({ development: 'codex' }), roleDefaults: { development: { codex: structuredClone(defaults) } } };
  };
  await mount([], undefined, {}, catalog, configuration, save);
  await flushConfigurationCatalog();
  await chooseConfiguration('开发默认模型', 'Model B');
  assert.deepEqual(calls, [{ role: 'development', providerId: 'codex', defaults: { model: 'model-b', reasoning: 'high' } }]);
  const reasoning = [...document.querySelectorAll('[role="combobox"]')].find(node => node.getAttribute('aria-label') === '开发推理强度');
  assert.equal(reasoning.textContent, 'high · 当前不可用');
});

test('unknown saved model and reasoning remain visible without automatic mutation', async () => {
  const calls = [];
  const catalog = { providers: [catalogProvider()], roleRouting: { development: 'codex' }, roleDefaults: { development: { codex: { model: 'retired-model', reasoning: 'ultra' } } } };
  await mount([], undefined, {}, catalog, async providerId => ({ providerId, models: [{ id: 'current-model', name: 'Current Model', isDefault: true, hidden: false, reasoningOptions: [{ id: 'high', name: 'High' }] }], defaultModel: 'current-model', reasoningOptions: [] }));
  api.agentProviderSetRoleDefaults = async (...args) => { calls.push(args); throw new Error('must not mutate'); };
  await flushConfigurationCatalog();
  const model = [...document.querySelectorAll('[role="combobox"]')].find(node => node.getAttribute('aria-label') === '开发默认模型');
  const reasoning = [...document.querySelectorAll('[role="combobox"]')].find(node => node.getAttribute('aria-label') === '开发推理强度');
  assert.equal(model.textContent, 'retired-model · 当前不可用');
  assert.equal(reasoning.textContent, 'ultra · 当前不可用');
  assert.deepEqual(calls, []);
});

test('model-specific empty reasoning list disables reasoning even when a global fallback exists', async () => {
  const catalog = { providers: [catalogProvider()], roleRouting: { development: 'codex' }, roleDefaults: {} };
  await mount([], undefined, {}, catalog, async providerId => ({ providerId, models: [{ id: 'plain', name: 'Plain', isDefault: true, hidden: false, reasoningOptions: [] }], defaultModel: 'plain', reasoningOptions: [{ id: 'high', name: 'High' }] }));
  await flushConfigurationCatalog();
  const reasoning = [...document.querySelectorAll('[role="combobox"]')].find(node => node.getAttribute('aria-label') === '开发推理强度');
  assert.equal(reasoning.textContent, '不支持');
  assert.equal(reasoning.disabled, true);
});

test('configuration catalog reports stable no-workspace and unavailable states without deleting defaults', async () => {
  let queries = 0;
  const saved = { model: 'saved-model', reasoning: 'saved-reasoning' };
  const catalog = { providers: [catalogProvider()], roleRouting: { development: 'codex' }, roleDefaults: { development: { codex: saved } } };
  await mount([], undefined, { workspace: null }, catalog, async () => { queries++; throw new Error('must not query'); });
  assert.equal(queries, 0);
  assert.match(document.querySelector('.agent-role-routing').textContent, /请选择工作区后读取 Provider 模型与推理目录/);
  for (const label of ['开发默认模型', '开发推理强度']) {
    const trigger = [...document.querySelectorAll('[role="combobox"]')].find(node => node.getAttribute('aria-label') === label);
    assert.equal(trigger.textContent, '需要工作区');
    assert.equal(trigger.disabled, true);
  }
  await act(async () => root.unmount()); root = null;
  await mount([], undefined, {}, catalog, async () => { queries++; throw new Error('catalog unavailable'); });
  await flushConfigurationCatalog();
  assert.equal(queries, 1);
  assert.match(document.querySelector('.agent-role-routing').textContent, /目录不可用，已保留设置/);
  assert.deepEqual(catalog.roleDefaults.development.codex, saved);
});

test('configuration catalog retries a rejected key once, preserves defaults, and caches success', async t => {
  const timers = [];
  const originalSetInterval = globalThis.setInterval;
  // 只手动推进 1.5 秒业务轮询，保留 JSDOM 自身的真实计时器。
  t.mock.method(globalThis, 'setInterval', (callback, delay, ...args) => {
    if (delay === 1500) { timers.push(callback); return originalSetInterval(() => {}, 60_000); }
    return originalSetInterval(callback, delay, ...args);
  });
  const first = deferredRoleResponse();
  const saved = { model: 'saved-model', reasoning: 'saved-reasoning' };
  const catalog = { providers: [catalogProvider()], roleRouting: { development: 'codex' }, roleDefaults: { development: { codex: saved } } };
  const available = { providerId: 'codex', models: [{ id: 'saved-model', name: 'Saved Model', isDefault: false, hidden: false, reasoningOptions: [{ id: 'saved-reasoning', name: 'Saved Reasoning' }], defaultReasoning: 'saved-reasoning' }], defaultModel: 'saved-model', reasoningOptions: [] };
  let queries = 0, writes = 0;
  await mount([], undefined, {}, catalog, async () => {
    queries++;
    return queries === 1 ? first.promise : structuredClone(available);
  }, async () => { writes++; throw new Error('retry must not save defaults'); });
  assert.equal(queries, 1);
  // 首次请求仍在进行时，即使目录轮询也不能为同一个 key 启动并发请求。
  await act(async () => timers[0]());
  assert.equal(queries, 1);
  await act(async () => first.reject(new Error('catalog unavailable')));
  await flushConfigurationCatalog();
  assert.match(document.querySelector('.agent-role-routing').textContent, /目录不可用，已保留设置/);
  assert.deepEqual(catalog.roleDefaults.development.codex, saved);
  assert.equal(writes, 0);

  // 下一次目录轮询重试同一 key；成功后 Select 恢复，后续轮询命中成功缓存。
  await act(async () => timers[0]());
  await flushConfigurationCatalog();
  const model = [...document.querySelectorAll('[role="combobox"]')].find(node => node.getAttribute('aria-label') === '开发默认模型');
  const reasoning = [...document.querySelectorAll('[role="combobox"]')].find(node => node.getAttribute('aria-label') === '开发推理强度');
  assert.equal(queries, 2);
  assert.equal(model.disabled, false);
  assert.equal(reasoning.disabled, false);
  assert.match(model.textContent, /Saved Model/);
  assert.match(reasoning.textContent, /Saved Reasoning/);
  assert.deepEqual(catalog.roleDefaults.development.codex, saved);
  assert.equal(writes, 0);
  await act(async () => timers[0]());
  await flushConfigurationCatalog();
  assert.equal(queries, 2);
});

test('long role configuration menus use one scroll viewport and hide inactive state options', async () => {
  const catalog = {
    providers: [catalogProvider({ id: 'codebuddy', displayName: 'CodeBuddy' })],
    roleRouting: { review: 'codebuddy' },
    roleDefaults: {},
  };
  const models = Array.from({ length: 16 }, (_, index) => ({
    id: `model-${index + 1}`,
    name: `Model ${index + 1}`,
    isDefault: index === 0,
    hidden: false,
    reasoningOptions: [{ id: 'high', name: 'High' }],
    defaultReasoning: 'high',
  }));
  await mount([], undefined, {}, catalog, async providerId => ({
    providerId,
    models,
    currentModel: 'model-1',
    defaultModel: 'model-1',
    reasoningOptions: [],
    currentReasoning: 'high',
    defaultReasoning: 'high',
  }));
  await flushConfigurationCatalog();
  const trigger = [...document.querySelectorAll('[role="combobox"]')]
    .find(node => node.getAttribute('aria-label') === '评审默认模型');
  assert.ok(trigger);
  await act(async () => trigger.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true })));
  const labels = [...document.querySelectorAll('[role="option"]')].map(node => node.textContent);
  assert.ok(labels.includes('跟随 Provider 默认'));
  assert.ok(labels.includes('Model 16'));
  for (const inactive of ['请先指定 Agent', '需要工作区', '正在加载…', '目录不可用']) {
    assert.equal(labels.includes(inactive), false, inactive);
  }
  const viewport = document.querySelector('[data-slot="select-viewport"]');
  const content = document.querySelector('[data-slot="select-content"]');
  assert.ok(viewport);
  assert.ok(content);
  assert.match(viewport.className, /max-h-72/);
  assert.match(viewport.className, /overflow-y-auto/);
  assert.match(viewport.className, /overscroll-contain/);
  assert.match(content.className, /overflow-hidden/);
  assert.doesNotMatch(content.className, /overflow-y-auto/);
});

test('role configuration CSS keeps narrow controls in the right column', () => {
  const css = readFileSync('src/styles.css', 'utf8');
  assert.match(css, /\.agent-role-row\s*\{[^}]*grid-template-columns:\s*minmax\(56px,\s*\.55fr\)\s+repeat\(3,\s*minmax\(150px,\s*1fr\)\)/);
  const narrowStart = css.indexOf('@media (max-width: 980px)');
  const narrowEnd = css.indexOf('.agent-providers h2', narrowStart);
  assert.notEqual(narrowStart, -1);
  assert.notEqual(narrowEnd, -1);
  const narrow = css.slice(narrowStart, narrowEnd);
  assert.match(narrow, /\.agent-role-row\s*\{[^}]*grid-template-columns:\s*minmax\(56px,\s*\.55fr\)\s+minmax\(150px,\s*1fr\)/);
  assert.match(narrow, /\.agent-role-row\s*>\s*label\s*\{[^}]*grid-column:\s*1/);
  assert.match(narrow, /\.agent-role-row\s*>\s*button,\s*\.agent-role-row\s*>\s*\[role="status"\]\s*\{[^}]*grid-column:\s*2/);
});

test('role editor clears persisted binding with explicit null and restores persisted catalog on remount', async () => {
  const catalog = { providers: [catalogProvider()], roleRouting: roleSettings({ development: 'codex', testing: 'codex', review: 'codex', analysis: 'codex', general: 'codex' }).roleRouting };
  const calls = [];
  api.agentProviderSetRoleRoute = async (role, value) => {
    calls.push([role, value]); catalog.roleRouting[role] = value;
    return roleSettings(catalog.roleRouting);
  };
  await mount([], undefined, {}, catalog);
  for (const role of Object.keys(catalog.roleRouting)) assert.equal(document.querySelector(`#agent-role-${role}`).textContent, 'Codex');
  await chooseRole('general', '未指定 Agent');
  assert.deepEqual(calls, [['general', null]]);
  assert.equal(document.querySelector('#agent-role-general').textContent, '未指定 Agent');
  await act(async () => root.unmount()); root = null;
  await mount([], undefined, {}, catalog);
  assert.equal(document.querySelector('#agent-role-general').textContent, '未指定 Agent');
  assert.equal(document.querySelector('#agent-role-testing').textContent, 'Codex');
  assert.equal(calls.length, 1);
});

test('role editor preserves disabled and unknown bindings without mutation and permits explicit replacement', async () => {
  const catalog = { providers: [catalogProvider({ id: 'none', displayName: 'Future Agent', enabled: false })], roleRouting: { development: 'none', testing: '__none__', review: 'historical' } };
  const calls = [];
  api.agentProviderSetRoleRoute = async (role, value) => {
    calls.push([role, value]);
    catalog.roleRouting[role] = value;
    return roleSettings(catalog.roleRouting);
  };
  await mount([], undefined, {}, catalog);
  assert.equal(document.querySelector('#agent-role-development').textContent, 'Future Agent · 已停用');
  assert.equal(document.querySelector('#agent-role-testing').textContent, '__none__ · 未注册');
  assert.equal(document.querySelector('#agent-role-review').textContent, 'historical · 未注册');
  assert.deepEqual(calls, []);
  await chooseRole('testing', '未指定 Agent');
  await chooseRole('review', 'Future Agent · 已停用');
  assert.deepEqual(calls, [['testing', null], ['review', 'none']]);
  assert.equal(document.querySelector('#agent-role-review').textContent, 'Future Agent · 已停用');
  assert.equal(document.querySelector('#agent-role-development').textContent, 'Future Agent · 已停用');
});
test('role save failure rolls back only its role while another role saves', async () => {
  const development = deferredRoleResponse(), testing = deferredRoleResponse();
  const calls = [];
  const catalog = { providers: [catalogProvider(), catalogProvider({ id: 'other', displayName: 'Other' })], roleRouting: { development: 'codex' } };
  api.agentProviderSetRoleRoute = (role, value) => { calls.push([role, value]); return role === 'development' ? development.promise : testing.promise; };
  await mount([], undefined, {}, catalog);
  await chooseRole('development', 'Other');
  assert.equal(document.querySelector('#agent-role-development').disabled, true);
  await chooseRole('testing', 'Other');
  assertManagementSurface();
  await act(async () => development.reject(new Error('persist failed')));
  assert.equal(document.querySelector('#agent-role-development').textContent, 'Codex');
  assert.equal(document.querySelector('#agent-role-testing').textContent, 'Other');
  assert.equal(document.querySelector('#agent-role-testing').disabled, true);
  assert.match(notifications.at(-1)[1], /开发角色保存失败/);
  catalog.roleRouting.testing = 'other';
  await act(async () => testing.resolve(roleSettings({ testing: 'other', development: 'other' })));
  assert.equal(document.querySelector('#agent-role-development').textContent, 'Codex');
  assert.equal(document.querySelector('#agent-role-testing').textContent, 'Other');
  assert.equal(calls.length, 2);
});

test('concurrent role responses arrive out of order without overwriting each other or cleared committed value', async () => {
  const development = deferredRoleResponse(), testing = deferredRoleResponse();
  api.agentProviderSetRoleRoute = role => role === 'development' ? development.promise : testing.promise;
  await mount([], undefined, {}, { providers: [catalogProvider()], roleRouting: { development: 'codex' } });
  await chooseRole('development', '未指定 Agent');
  await chooseRole('testing', 'Codex');
  await act(async () => testing.resolve(roleSettings({ development: 'codex', testing: 'codex' })));
  await act(async () => development.resolve(roleSettings()));
  assert.equal(document.querySelector('#agent-role-development').textContent, '未指定 Agent');
  assert.equal(document.querySelector('#agent-role-testing').textContent, 'Codex');
  api.agentProviderSetRoleRoute = async () => { throw new Error('persist failed'); };
  await chooseRole('development', 'Codex');
  assert.equal(document.querySelector('#agent-role-development').textContent, '未指定 Agent');
  assert.equal(document.querySelector('#agent-role-testing').textContent, 'Codex');
});

test('catalog polls cannot overwrite in-flight or newly committed role but a later fresh poll updates policy', async t => {
  const timers = [];
  const originalSetInterval = globalThis.setInterval;
  // 只控制业务轮询；JSDOM 的 requestAnimationFrame 也依赖 interval，必须保留其真实时钟。
  t.mock.method(globalThis, 'setInterval', (callback, delay, ...args) => {
    if (delay === 1500) { timers.push(callback); return originalSetInterval(() => {}, 60_000); }
    return originalSetInterval(callback, delay, ...args);
  });
  const mutation = deferredRoleResponse(), before = deferredRoleResponse(), during = deferredRoleResponse();
  const initial = { providers: [catalogProvider(), catalogProvider({ id: 'other', displayName: 'Other' })], roleRouting: { development: 'codex' } };
  let polls = 0;
  api.agentProviderSetRoleRoute = () => mutation.promise;
  await mount([], undefined, {}, () => {
    polls++;
    if (polls === 2) return before.promise;
    if (polls === 3) return during.promise;
    return Promise.resolve(initial);
  });
  await act(async () => timers[0]());
  await chooseRole('development', 'Other');
  await act(async () => before.resolve(initial));
  assert.equal(document.querySelector('#agent-role-development').textContent, 'Other');
  assert.equal(document.querySelector('#agent-role-development').disabled, true);
  await act(async () => timers[0]());
  await act(async () => mutation.resolve(roleSettings({ development: 'other' })));
  await act(async () => during.resolve(initial));
  assert.equal(document.querySelector('#agent-role-development').textContent, 'Other');
  assert.equal(document.querySelector('#agent-role-development').disabled, false);
  // 保存结束后才开始的新快照仍是 current policy，可反映另一次本地策略更新。
  initial.roleRouting.development = null;
  await act(async () => timers[0]());
  assert.equal(document.querySelector('#agent-role-development').textContent, '未指定 Agent');
  assert.equal(polls, 4);
});
// 通过真实轮询等待共享任务快照刷新。
async function refreshTasks() { await act(async () => { await new Promise(resolve => setTimeout(resolve, 1600)); }); }

/** 从工作区导航进入详情，不借用已移除的主区任务卡。 */
async function openTask() {
  const link = document.querySelector('.project-task-link');
  assert.ok(link);
  await act(async () => link.click());
}

/** 管理主区只保留管理能力，任何隐藏的 Composer 也不允许残留。 */
function assertManagementSurface() {
  for (const selector of ['#agent-prompt', '.agent-composer-section', '.agent-history', '.agent-filter-toolbar', '.agent-task-card', '.agent-load-more', '[aria-label="刷新任务列表"]', '[aria-label="筛选任务"]', '[aria-label="筛选工作区"]']) assert.equal(document.querySelector(selector), null, selector);
  assert.equal(button('开始新任务'), undefined);
  assert.ok(document.querySelector('.agent-providers'));
  assert.ok(document.querySelector('.agent-role-routing'));
  assert.ok(document.querySelector('.agent-workspace-bar'));
}
async function input(value, selector = '#agent-continuation') {
  await act(async () => {
    const field = document.querySelector(selector);
    Object.getOwnPropertyDescriptor(dom.window.HTMLTextAreaElement.prototype, 'value').set.call(field, value);
    field.dispatchEvent(new dom.window.Event('input', { bubbles: true }));
  });
}
afterEach(async () => { if (root) await act(async () => root.unmount()); root = null; agentRequests.pending = null; agentRequests.inFlight = false; notifications.length = 0; window.localStorage.clear(); document.getElementById("task-nav-test")?.remove(); });

test('management polling retains Provider facts on failure and recovers without a history section', async t => {
  const timers = [];
  const originalSetInterval = globalThis.setInterval;
  // 控制业务轮询，保留 JSDOM 布局相关时钟。
  t.mock.method(globalThis, 'setInterval', (callback, delay, ...args) => {
    if (delay === 1500) { timers.push(callback); return originalSetInterval(() => {}, 60_000); }
    return originalSetInterval(callback, delay, ...args);
  });
  const pending = row({ status: 'dispatch_pending', attention: 'pending_explicit_resume', availableActions: { canCancel: true, canResumePending: true, canContinue: false } });
  await mount([pending], undefined, { sidebarContainer: null }, { providers: [catalogProvider({ enabled: false })], roleRouting: {} });
  const card = document.querySelector('.agent-provider-card');
  assert.equal(cardValues(card).活动任务, '1');
  const history = api.agentHistory;
  api.agentHistory = async () => { throw new Error('history unavailable'); };
  await act(async () => timers[1]());
  assertManagementSurface();
  assert.match(document.querySelector('.agent-providers [role="alert"]').textContent, /history unavailable.*自动重试/);
  assert.equal(cardValues(card).活动任务, '1');
  assert.equal(button('取消任务', card).disabled, true);
  assert.equal(button('查看任务', card).disabled, false);
  api.agentHistory = history;
  await act(async () => timers[1]());
  assert.equal(document.querySelector('.agent-providers [role="alert"]'), null);
  assert.equal(button('取消任务', card).disabled, false);
  pending.status = 'cancelled'; pending.attention = 'none';
  pending.availableActions = { canCancel: false, canResumePending: false, canContinue: false };
  await act(async () => timers[1]());
  assert.equal(cardValues(card).活动任务, '0');
  assert.equal(button('查看任务', card), undefined);
});

test('pending cancel locks duplicate mutations until its request completes', async () => {
  const mutation = deferredRoleResponse();
  const pending = row({ attention: 'pending_explicit_resume', availableActions: { canCancel: true, canResumePending: true, canContinue: false } });
  const calls = await mount([pending], request => request.action === 'cancel' ? mutation.promise : undefined, {}, { providers: [catalogProvider({ enabled: false })], roleRouting: {} });
  const card = document.querySelector('.agent-provider-card');
  await click('取消任务', card);
  assert.equal(button('取消任务', card).disabled, true);
  await act(async () => button('取消任务', card).click());
  assert.equal(calls.filter(request => request.action === 'cancel').length, 1);
  await act(async () => mutation.resolve({ ok: true, data: pending }));
  assert.equal(button('取消任务', card).disabled, false);
});

test('unknown has only details, no retry or recovery; IDs and JSON stay out of list', async () => {
  await mount([row()]);
  assertManagementSurface();
  assert.equal(button('重试原请求'), undefined);
  assert.equal(document.querySelector('pre'), null);
  assert.ok(!document.querySelector('.project-task-link').textContent.includes('old-E1'));
  await openTask();
  assert.equal(document.querySelector('details.agent-technical').open, false);
  assert.match(document.querySelector('pre').textContent, /old-E1/);
  assert.equal(document.querySelector('literal'), null);
});

test('manual resolution stays in task details, requires confirmation, and uses Local IPC', async () => {
  await mount([row({ dispatchState: 'not_dispatched' })]);
  const localCalls = [];
  api.agentManualResolve = async (...args) => {
    localCalls.push(args);
    return row({ status: 'interrupted', attention: 'none', dispatchState: 'not_dispatched', completedAt: 3000 });
  };
  await openTask();
  await click('人工结束并释放工作区');
  const dialog = document.querySelector('[role="dialog"]');
  assert.match(dialog.textContent, /系统无法自动证明上一次 Runtime 的最终状态/);
  assert.match(dialog.textContent, /确认该执行不会继续修改工作区/);
  assert.equal(localCalls.length, 0);
  await click('确认结束并释放', dialog);
  assert.deepEqual(localCalls, [['old-E1', 'interrupt_and_release']]);
  assert.equal([...document.querySelectorAll('article button')].some(button => button.textContent === '人工结束并释放工作区'), false);
});

test('pending resume uses exact ID and only backend capability', async () => {
  const calls = await mount([row({ status: 'dispatch_pending', attention: 'pending_explicit_resume', availableActions: { canCancel: true, canContinue: false, canResumePending: true } })]);
  await openTask();
  assert.match(document.querySelector('.agent-detail').textContent, /等待恢复/);
  await click('恢复任务'); await click('取消任务');
  assert.deepEqual(calls.filter(c => ['cancel','resume_pending'].includes(c.action)), [{ action:'resume_pending', executionId:'old-E1' }, { action:'cancel', executionId:'old-E1' }]);
});





test('completed final answer is compact; detail pane exposes original prompt and technical output', async () => {
  await mount([row({ status: 'completed', attention: 'none', finalResult: { finalResult: [{ type:'agentMessage', phase:'commentary', text:'internal progress' }, { type:'agentMessage', phase:'final_answer', text:'DONE' }], huge: 'x'.repeat(10000) } })]);
  assert.ok(!document.querySelector('.project-task-link').textContent.includes('DONE'));
  assert.ok(!document.querySelector('.project-task-link').textContent.includes('internal progress'));
  assert.equal(document.querySelector('pre'), null);
  await openTask();
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
    await openTask();
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
  assert.ok(!document.querySelector('.project-task-link').textContent.includes('RESULT ONE'));
  await openTask();
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
  await openTask();
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
  await act(async () => document.querySelector('.project-task-delete').click());
  assert.equal(document.querySelectorAll('.project-task').length, 0);
  assertManagementSurface();
  await refreshTasks();
  assert.equal(document.querySelectorAll('.project-task').length, 0);
  assert.ok(calls.every(request => request.action === 'list'));
  await act(async () => root.unmount()); root = null;
  await mount([row()]);
  assert.equal(document.querySelectorAll('.project-task').length, 0);
});

test('continue uses independent detail pane input and original source across Workspace switch', async () => {
  const calls = await mount([row({ status:'completed', attention:'none', availableActions:{canCancel:false,canResumePending:false,canContinue:true} })]);
  await openTask();
  await act(async () => root.render(createElement(TooltipProvider, null, createElement(AgentPanel, { workspace: workspace('B') }))));
  assert.equal(button('继续任务').disabled, true);
  await input('continuation only', '#agent-continuation');
  await click('继续任务');
  const request = calls.find(c => c.action === 'continue');
  assert.equal(request.prompt, 'continuation only'); assert.equal(request.executionId, 'old-E1');
  assert.equal('workspaceId' in request, false); assert.equal('canonicalWorkspaceRoot' in request, false);
  assert.equal(document.querySelector('#agent-prompt'), null);
});


test('confirmed product errors do not expose transport retry and preserve execution reference', async () => {
  const completed = row({ status:'completed', attention:'none', errorCode:'HISTORICAL_ERROR', errorMessage:'先前的诊断' });
  await mount([row({ attention: 'pending_explicit_resume', availableActions: { canCancel: true, canContinue: false, canResumePending: false } })], r => {
    if (r.action === 'cancel') return { ok:false, error:{code:'AGENT_OPERATION_FAILED',message:'failure',executionId:'old-E1'} };
    if (r.action === 'observe') return { ok:true, data:completed };
  }, {}, { providers: [catalogProvider({ enabled: false })], roleRouting: {} });
  await click('取消任务', document.querySelector('.agent-provider-card'));
  assert.equal(button('重试原请求'), undefined); assert.equal(agentRequests.pending, null);
  assert.ok(button('查看相关任务')); assert.match(document.body.textContent, /操作未完成/);
  await click('查看相关任务');
  assert.match(document.querySelector('.agent-detail-header').textContent, /已完成/);
  assert.equal(document.querySelector('.agent-recovery-section'), null);
  assert.doesNotMatch(document.querySelector('.agent-detail').textContent, /操作未完成/);
});

test('confirmed product error opens a failed execution with its own diagnostic only', async () => {
  const failed = row({ status:'failed', attention:'none', errorCode:'AGENT_FAILED', errorMessage:'执行失败原因' });
  await mount([row({ attention: 'pending_explicit_resume', availableActions: { canCancel: true, canContinue: false, canResumePending: false } })], r => {
    if (r.action === 'cancel') return { ok:false, error:{code:'AGENT_OPERATION_FAILED',message:'failure',executionId:'old-E1'} };
    if (r.action === 'observe') return { ok:true, data:failed };
  }, {}, { providers: [catalogProvider({ enabled: false })], roleRouting: {} });
  await click('取消任务', document.querySelector('.agent-provider-card'));
  assert.match(document.querySelector('.agent-notice').textContent, /操作未完成/);
  await click('查看相关任务');
  assert.match(document.querySelector('.agent-recovery-section .agent-real-error').textContent, /AGENT_FAILED.*执行失败原因/);
  assert.doesNotMatch(document.querySelector('.agent-detail').textContent, /操作未完成/);
});


test('empty workspace provides real navigation without task creation', async () => {
  let navigated = false;
  await mount([], undefined, { workspace:null, onSelectWorkspace:() => { navigated = true; } });
  assert.match(document.body.textContent, /未选择可用工作区/); assertManagementSurface();
  await click('选择工作区'); assert.equal(navigated, true);
});

test('read failures show feedback and do not offer operation replay', async () => {
  await mount([row()], request => request.action === 'observe' ? Promise.reject(new Error('offline')) : undefined);
  await openTask(); assert.ok(button('重新加载详情')); assert.equal(button('重试原请求'), undefined);
  assert.ok(notifications.some(([kind]) => kind === 'error'));
});



test('continuation ambiguity stays in list feedback and retries its frozen independent input', async () => {
  const calls = await mount([row({status:'completed',attention:'none',availableActions:{canContinue:true,canCancel:false,canResumePending:false}})], request => request.action === 'continue' ? Promise.reject(new Error('transport')) : undefined);
  await openTask(); await input('frozen continuation', '#agent-continuation'); await click('继续任务');
  const detailPane = document.querySelector('[aria-label="任务详情"]');
  assert.equal(button('重试原请求', detailPane), undefined);
  assert.doesNotMatch(detailPane.textContent, /请求结果未确认/);
  await act(async () => root.unmount()); root = createRoot(document.getElementById('root'));
  await act(async () => root.render(createElement(TooltipProvider, null, createElement(AgentPanel, { workspace: workspace('A'), detailView: false }))));
  assert.ok(button('重试原请求'));
  await click('重试原请求');
  const requests = calls.filter(c => c.action === 'continue');
  assert.deepEqual(requests[0], requests[1]);
  assert.equal(requests[0].prompt, 'frozen continuation');
});

test('execution issue predicates ignore historical diagnostics without an issue state', () => {
  for (const status of ['completed', 'cancelled', 'running', 'dispatch_pending', 'finalizing']) {
    const execution = row({ status, attention:'none', errorCode:'OLD_ERROR', errorMessage:'历史诊断' });
    assert.equal(showExecutionIssueSection(execution), false, status);
    assert.equal(showExecutionDiagnostic(execution), false, status);
  }
  for (const overrides of [
    { status:'failed' }, { status:'unknown' }, { status:'reconciling' }, { status:'interrupted' },
    { status:'running', attention:'pending_explicit_resume' }, { status:'running', interruptTimedOut:true },
  ]) {
    const execution = row({ attention:'none', errorCode:'REAL_ERROR', ...overrides });
    assert.equal(showExecutionIssueSection(execution), true);
    assert.equal(showExecutionDiagnostic(execution), true);
  }
  assert.equal(showExecutionDiagnostic(row({ status:'failed', attention:'none' })), false);
});

test('completed historical diagnostic stays in raw JSON but not list or recovery UI', async () => {
  const completed = row({ status:'completed', attention:'none', completedAt:3000, errorCode:'OLD_ERROR', errorMessage:'历史诊断', finalResult:{ finalResult:[{ type:'agentMessage', phase:'final_answer', text:'任务已完成' }] } });
  await mount([completed]);
  assertManagementSurface();
  await openTask();
  assert.equal(document.querySelector('.agent-recovery-section'), null);
  assert.doesNotMatch(document.querySelector('.agent-detail').textContent, /操作未完成/);
  assert.match(document.querySelector('.agent-result-section').textContent, /任务已完成/);
  const raw = JSON.parse(document.querySelector('.agent-raw-json pre').textContent);
  assert.equal(raw.errorCode, 'OLD_ERROR');
  assert.equal(raw.errorMessage, '历史诊断');
});

test('failed execution retains its list error and detail diagnostic', async () => {
  await mount([row({ status:'failed', attention:'none', errorCode:'AGENT_TIMEOUT', errorMessage:'后端超时' })]);
  assertManagementSurface();
  await openTask();
  assert.match(document.querySelector('.agent-recovery-section .agent-real-error').textContent, /AGENT_TIMEOUT.*后端超时/);
});


test('task page has no in-page return control', async () => {
  await mount([row()]);
  await openTask();
  assert.equal(document.querySelector('.agent-page').classList.contains('agent-list-view'), false);
  assert.equal(document.querySelector('.agent-page').classList.contains('agent-detail-view'), true);
  assert.equal(document.querySelector('[aria-label="关闭任务详情"]'), null);
  assert.equal(document.querySelector('nav[aria-label="任务页面导航"]'), null);
  assert.equal(button('返回 Agent 任务'), undefined);
  assert.equal(document.querySelector('[aria-label="任务详情"]')?.getAttribute('aria-label'), '任务详情');
});





test('workspace display name stays consistent between history and details', async () => {
  await mount([row({canonicalWorkspaceRoot:'E:\\named-folder'})],undefined,{workspaces:[{id:'project-4',name:'实际工作区名',root:'E:\\named-folder'}]});
  assert.match(document.querySelector('.project-task-group').textContent,/实际工作区名/);
  await openTask();
  const location = document.querySelector('.agent-detail-location');
  assert.match(location.textContent,/执行位置.*实际工作区名.*E:\\named-folder/);
  assert.equal(location.querySelector('strong').textContent, '实际工作区名');
  assert.equal(location.querySelector('code').textContent, 'E:\\named-folder');
});

test('running detail keeps its blue pulse and left-aligned location', async () => {
  await mount([row({canonicalWorkspaceRoot:'E:\\named-folder', status:'running', attention:'none', progress:{phase:'running'}})], undefined, {workspaces:[{id:'project-4',name:'实际工作区名',root:'E:\\named-folder'}]});
  await openTask();
  for (const selector of ['.agent-detail-title .agent-status', '.agent-detail-live-grid .agent-status']) {
    const status = document.querySelector(selector);
    assert.equal(status.textContent, '执行中');
    assert.ok(status.querySelector('i.agent-task-pulse[aria-hidden="true"]'));
  }
  assert.ok(document.querySelector('.agent-detail-title .agent-status').classList.contains('tone-blue'));
  assert.ok(document.querySelector('.agent-detail-live-grid .agent-status').classList.contains('tone-blue'));
  const location = document.querySelector('.agent-detail-location');
  assert.equal(location.querySelector('strong').textContent, '实际工作区名');
  assert.equal(location.querySelector('code').textContent, 'E:\\named-folder');
  const styles = readFileSync('src/styles.css', 'utf8');
  assert.match(styles, /\.agent-detail-live-grid \.agent-status\.tone-blue \{ color: var\(--agent-blue\); \}/);
  assert.match(styles, /\.agent-detail-live-grid \.agent-status\.tone-green \{ color: var\(--signal-green\); \}/);
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
    const calls = await mount([execution]); await openTask();
    const page = document.querySelector('.agent-page');
    assert.ok(page.classList.contains('agent-detail-view')); assert.equal(page.classList.contains('agent-list-view'), false);
    assert.match(document.querySelector('.agent-detail-header').textContent, /真实线程标题.*执行中.*Provider A · v2\.0/);
    const infoCard = document.querySelector('.agent-detail-info-card');
    assert.match(infoCard.textContent, /执行状态.*执行中.*Provider.*Provider A · v2\.0.*当前活动正在测试.*最近活动暂无活动数据.*活跃状态暂无活动数据.*当前轮次总 Token.*12,531.*执行位置.*E:\\frozen-A/);
    assert.ok(infoCard.querySelector('.agent-status.tone-blue'));
    assert.doesNotMatch(infoCard.textContent, /legacy-session/);
    assert.equal(document.querySelector('.agent-usage-section'), null);
    assert.equal(document.querySelector('.agent-recovery-section'), null);
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
  await mount([historical]); await openTask();
  const detail = document.querySelector('.agent-detail');
  assert.match(detail.querySelector('.agent-detail-header').textContent, /引擎: legacy-provider/);
  assert.match(detail.querySelector('.agent-detail-info-card').textContent, /当前轮次总 Token.*—/);
  assert.doesNotMatch(detail.querySelector('.agent-detail-info-card').textContent, /legacy-session/);
  assert.equal(detail.querySelector('.agent-usage-section'), null);
});

test('Agent detail copies only a real result and continues with the original execution ID', async () => {
  const execution = row({ status: 'completed', attention: 'none', finalResult: { finalResult: [{ type: 'agentMessage', phase: 'final_answer', text: '真实结果' }] }, availableActions: { canCancel: false, canContinue: true, canResumePending: false } });
  let copiedText = '';
  const original = Object.getOwnPropertyDescriptor(navigator, 'clipboard');
  Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText: async text => { copiedText = text; } } });
  try {
    const calls = await mount([execution]); await openTask();
    assert.ok(document.querySelector('.agent-detail-info-card .agent-status.tone-green'));
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
  await openTask();
  assert.equal(document.querySelector('.agent-result-section'), null);
  const continuationSection = document.querySelector('.agent-continuation-section');
  assert.ok(continuationSection);
  assert.equal(continuationSection.parentElement, document.querySelector('.agent-detail-body'));
});

test('Agent detail duration labels follow active execution semantics rather than completedAt', async () => {
  const failed = row({ status: 'failed', attention: 'none', completedAt: null, progress: { phase: 'terminal', activityPhase: null, toolCategory: null, lastActivityAt: null, activityAgeMs: null, silenceLevel: null } });
  await mount([failed]); await openTask();
  assert.doesNotMatch(document.querySelector('.agent-detail-header').textContent, /已运行/);
  assert.match(document.querySelector('.agent-detail-header').textContent, /耗时/);
  assert.match(document.querySelector('.agent-detail-live-grid').textContent, /总耗时/);
  assert.doesNotMatch(document.querySelector('.agent-detail-live-grid').textContent, /已运行/);
  await act(async () => root.unmount()); root = null;

  const running = row({ status: 'running', attention: 'none', completedAt: null, progress: { phase: 'running', activityPhase: 'provider', toolCategory: null, lastActivityAt: null, activityAgeMs: null, silenceLevel: null } });
  await mount([running]); await openTask();
  assert.match(document.querySelector('.agent-detail-header').textContent, /已运行/);
  assert.match(document.querySelector('.agent-detail-live-grid').textContent, /已运行/);
  assert.doesNotMatch(document.querySelector('.agent-detail-live-grid').textContent, /总耗时/);
});


test('a failed continuation leaves the current execution detail free of operation feedback', async () => {
  await mount([row({status:'completed',attention:'none',availableActions:{canContinue:true,canCancel:false,canResumePending:false}})], r => {
    if (r.action === 'continue') return {ok:false,error:{code:'AGENT_OPERATION_FAILED',message:'handoff error',executionId:'new-E2'}};
    if (r.action === 'observe' && r.executionId === 'new-E2') return {ok:true,data:row({executionId:'new-E2',prompt:'new related task'})};
  });
  await openTask(); await input('next', '#agent-continuation'); await click('继续任务');
  assert.ok(document.querySelector('[aria-label="任务详情"]'));
  assert.match(document.querySelector('.agent-detail-body > section .agent-prose').textContent, /原始任务/);
  assert.equal(button('查看相关任务', document.querySelector('.agent-detail')), undefined);
  assert.doesNotMatch(document.querySelector('.agent-detail').textContent, /操作未完成|handoff error/);
});

test('list refresh cannot compete with an outstanding detail observation', async () => {
  let resolve;
  const calls = await mount([row()], r => r.action === 'observe' ? new Promise(done => {resolve = done;}) : undefined);
  await openTask();
  // Direct invocation also exercises the handler guard while the native modal blocks interaction.
  await refreshTasks();
  assert.equal(calls.filter(c => c.action === 'observe').length, 1);
  await act(async () => resolve({ok:true,data:row({prompt:'latest detail'})}));
  assert.match(document.querySelector('[aria-label="任务详情"] .agent-prose').textContent, /latest detail/);
});


test('status caches pending, successful and failed Codex probes until explicit refresh', async () => {
  const { default: App } = await import('./App.tsx');
  const originals = { getState: api.getState, broker: api.broker, codexVersion: api.codexVersion, detect: api.detect, openExternal: api.openExternal };
  const snapshot = { config: { agentProviders: providerSettingsFixture(), agentEnabled:false, broker:{enabled:false,port:9120,allowLan:false},workspaces:[],serenaPath:null,port:9121,dashboardEnabled:false,openDashboardOnLaunch:false,autoStartServer:false,minimizeToTray:false }, git:{available:true,status:'available',version:'test',path:null,error:null}, serverStatus:'stopped', installation:null, activeInstallation:null, managedRuntimePresent:false, managedProcessPresent:false, codegraphVersion:'test', dashboardEnabled:false, autostartEnabled:false, autostartError:null, lastError:null };
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
  assert.equal(document.querySelector('.agent-providers').closest('[hidden]') !== null,true);
  assert.equal(groups[1].querySelector('.project-task-link').getAttribute('aria-current'),'page');
  assert.deepEqual(calls.filter(c=>c.action==='observe').at(-1),{action:'observe',executionId:'b1',waitMs:0,includeResult:true});
  await act(async () => groups[0].querySelector('.project-task-link').click());
  assert.match(document.querySelector('.agent-detail').textContent,/项目 A 的任务/);
  assert.equal(calls.some(c=>['start','continue','cancel','resume_pending'].includes(c.action)),false);
});

test('workspace menu exposes icon actions that rename or remove only the selected workspace', async () => {
  const host = navigationHost(); const project = workspace('A');
  const renamed = []; const removed = [];
  await mount([], undefined, {
    workspaces: [project],
    sidebarContainer: host,
    onWorkspaceRename: async (...args) => { renamed.push(args); return true; },
    onWorkspaceRemove: async id => { removed.push(id); return true; },
  });
  assert.equal(host.querySelector('.project-task-navigation h2').textContent, '工作区');
  assert.match(readFileSync('src/styles.css', 'utf8'), /\.project-task-navigation h2 \{ position: sticky;/);
  const trigger = host.querySelector(`[aria-label="打开工作区菜单：${project.name}"]`);
  await act(async () => trigger.click());
  const menu = document.querySelector('[role="menu"]');
  assert.match(menu.textContent, /编辑.*删除/);
  assert.equal(host.contains(menu), false, '菜单通过 Portal 脱离侧栏滚动容器');
  assert.equal(menu.dataset.side, 'right');
  assert.ok(menu.querySelector('svg.lucide-pencil'));
  assert.ok(menu.querySelector('svg.lucide-trash-2'));
  assert.equal(host.querySelector('.project-task-header').dataset.menuOpen, 'true');
  const styles = readFileSync('src/styles.css', 'utf8');
  assert.match(styles, /\.project-workspace-more:hover, \.project-workspace-more\[aria-expanded="true"\] \{ background: transparent;/);
  assert.match(styles, /\.project-workspace-menu-content \{[^}]*z-index: 70;[^}]*min-width: 176px;/);
  assert.match(styles, /\.project-task-header\[data-menu-open="true"\] \{ background: var\(--project-hover\); \}/);
  await act(async () => menu.dispatchEvent(new dom.window.KeyboardEvent('keydown', { key: 'Escape', bubbles: true })));
  assert.equal(document.querySelector('[role="menu"]'), null, 'Escape 关闭 Portal 菜单');
  await act(async () => trigger.click());
  await click('编辑', document);
  const editDialog = document.querySelector('[role="dialog"]');
  const input = editDialog.querySelector('[aria-label="工作区名称"]');
  await act(async () => {
    Object.getOwnPropertyDescriptor(dom.window.HTMLInputElement.prototype, 'value').set.call(input, '重命名工作区');
    input.dispatchEvent(new dom.window.Event('input', { bubbles: true }));
  });
  await click('保存', editDialog);
  assert.deepEqual(renamed, [[project.id, '重命名工作区']]);
  await act(async () => trigger.click());
  await click('删除', document);
  const removeDialog = document.querySelector('[role="dialog"]');
  assert.match(removeDialog.textContent, /不会删除本地目录/);
  await click('删除', removeDialog);
  assert.deepEqual(removed, [project.id]);
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
  assertManagementSurface();
  assert.equal(host.querySelector('.project-task').dataset.selected, 'false');
  assert.equal(host.querySelector('[aria-current="page"]'), null);
  assert.equal(document.querySelector('.agent-detail'), null);
  assert.equal(document.querySelector('.agent-providers').closest('[hidden]'), null);
  await render(true);
  assert.equal(host.querySelector('.project-task').dataset.selected, 'true');
  assert.match(document.querySelector('.agent-detail').textContent, /原始任务/);
  assert.equal(button('返回 Agent 任务'), undefined);
  assert.equal(shownAgent, 0);
});

test('sidebar dates use local calendar boundaries and display today as local time', async () => {
  const originalNow = Date.now;
  Date.now = () => new Date(2026, 0, 1, 0, 5).getTime();
  try {
    const host = navigationHost(); const project = workspace('A');
    await mount([
      row({ executionId: 'today', canonicalWorkspaceRoot: project.root, updatedAt: new Date(2026, 0, 1, 0, 1).getTime() }),
      row({ executionId: 'yesterday', canonicalWorkspaceRoot: project.root, updatedAt: new Date(2025, 11, 31, 23, 59).getTime() }),
      row({ executionId: 'older', canonicalWorkspaceRoot: project.root, updatedAt: new Date(2025, 11, 30, 23, 59).getTime() }),
    ], undefined, { workspaces: [project], sidebarContainer: host });
    assert.deepEqual([...host.querySelectorAll('time')].map(item => item.textContent), ['00:01', '昨天', '2天前']);
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
  assert.equal(document.activeElement === host.querySelector('.project-task-heading'), true, 'deletion returns focus to workspace heading');
  assert.deepEqual(JSON.parse(window.localStorage.getItem('agent-hidden-executions')),['old-E1']);
  assert.equal(calls.some(c=>['cancel','resume_pending'].includes(c.action)),false);
});

test('sidebar tasks retain only titles and compact times while hover keeps Summary DTO facts', async () => {
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
  for (const item of visible) {
    assert.equal(item.querySelectorAll('.project-task-summary, .project-task-usage').length, 0);
  }
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
  for (const task of tasks) {
    assert.equal(task.querySelectorAll('.project-task-summary, .project-task-usage').length, 0);
  }
  await act(async () => tasks[0].querySelector('.project-task-link').focus());
  assert.match(document.querySelector('.project-task-preview').textContent, /Acme Worker · v2\.4\.1.*总 Token：0/);
  assert.equal(calls.every(call => call.action === 'list'), true);

  await openTask();
  const detail = document.querySelector('.agent-detail');
  assert.match(detail.querySelector('.agent-detail-info-card').textContent, /Provider.*Acme Worker · v2\.4\.1.*当前活动正在整理结果.*活跃状态一段时间没有新活动.*当前轮次总 Token.*0/);
  assert.doesNotMatch(detail.querySelector('.agent-detail-info-card').textContent, /续接会话 · S-42/);
  assert.ok(detail.querySelector('.agent-detail-info-card .agent-status.tone-blue'));
  assert.equal(detail.querySelector('.agent-usage-section'), null);
  assert.match(detail.querySelector('.agent-continuation-footer').textContent, /Acme Worker · v2\.4\.1/);
  assert.doesNotMatch(detail.textContent, /PRIVATE_REASONING|stdout|source code/iu);
  assert.equal(calls.filter(call => call.action === 'observe').length, 1);
  assert.equal(calls.filter(call => call.action !== 'observe').every(call => call.action === 'list'), true);
});

test('Agent detail prefers effective profile, then requested profile, then fixed Provider default text', async () => {
  const effective = row({ executionProfile: { model: 'requested-model', reasoning: 'medium' }, effectiveExecutionProfile: { model: 'effective-model', reasoning: 'xhigh' } });
  await mount([effective]); await openTask();
  const effectiveFields = Object.fromEntries([...document.querySelectorAll('.agent-detail-live-grid > div')].map(item => [item.querySelector('span').textContent, item.querySelector('strong,code').textContent]));
  assert.equal(effectiveFields['模型'], 'effective-model');
  assert.equal(effectiveFields['推理强度'], 'xhigh');
  assert.doesNotMatch(document.querySelector('.agent-detail-live-grid').textContent, /Provider 默认/);

  await act(async () => root.unmount()); root = null;
  await mount([row({ executionProfile: { model: null, reasoning: null }, effectiveExecutionProfile: { model: 'non-reasoning-model', reasoning: null } })]); await openTask();
  const partialEffectiveFields = Object.fromEntries([...document.querySelectorAll('.agent-detail-live-grid > div')].map(item => [item.querySelector('span').textContent, item.querySelector('strong,code').textContent]));
  assert.equal(partialEffectiveFields['模型'], 'non-reasoning-model');
  assert.equal(partialEffectiveFields['推理强度'], 'Provider 默认');

  await act(async () => root.unmount()); root = null;
  const changedCurrentDefaults = { providers: [catalogProvider()], roleRouting: { general: 'codex' }, roleDefaults: { general: { codex: { model: 'current-default', reasoning: 'low' } } } };
  await mount([row({ executionProfile: { model: 'historical-request', reasoning: null } })], undefined, {}, changedCurrentDefaults); await openTask();
  const requestedFields = Object.fromEntries([...document.querySelectorAll('.agent-detail-live-grid > div')].map(item => [item.querySelector('span').textContent, item.querySelector('strong,code').textContent]));
  assert.equal(requestedFields['模型'], 'historical-request');
  assert.equal(requestedFields['推理强度'], 'Provider 默认');

  await act(async () => root.unmount()); root = null;
  await mount([row()]); await openTask();
  const defaultFields = Object.fromEntries([...document.querySelectorAll('.agent-detail-live-grid > div')].map(item => [item.querySelector('span').textContent, item.querySelector('strong,code').textContent]));
  assert.equal(defaultFields['模型'], 'Provider 默认');
  assert.equal(defaultFields['推理强度'], 'Provider 默认');
});

test('CB9-002 keeps unsupported CodeBuddy Usage unknown without hiding lifecycle actions', async () => {
  const host = navigationHost(); const project = workspace('A');
  const complete = row({ executionId: 'cb9-codex-complete', canonicalWorkspaceRoot: project.root, prompt: 'Codex 完整统计', status: 'completed', attention: 'none', usage: { inputTokens: 8, outputTokens: 5, totalTokens: 13, completeness: 'complete' } });
  const partial = row({ executionId: 'cb9-codex-partial', canonicalWorkspaceRoot: project.root, prompt: 'Codex 部分统计', usage: { inputTokens: 8, outputTokens: null, totalTokens: 8, completeness: 'partial' } });
  const codebuddyRunning = row({ executionId: 'cb9-codebuddy-running', canonicalWorkspaceRoot: project.root, prompt: 'CodeBuddy 运行中', status: 'running', attention: 'none', provider: { id: 'codebuddy', displayName: 'CodeBuddy' }, usage: { totalTokens: null, completeness: 'unknown' }, availableActions: { canCancel: true, canContinue: false, canResumePending: false } });
  const codebuddyTerminal = row({ executionId: 'cb9-codebuddy-terminal', canonicalWorkspaceRoot: project.root, prompt: 'CodeBuddy 已完成', status: 'completed', attention: 'none', provider: { id: 'codebuddy', displayName: 'CodeBuddy' }, usage: { totalTokens: null, completeness: 'unknown' }, availableActions: { canCancel: false, canContinue: true, canResumePending: false } });
  const codebuddyCatalog = catalogProvider({
    id: 'codebuddy', displayName: 'CodeBuddy',
    capabilities: { canExecute: true, canContinue: true, canCancel: true, canRecover: true, activity: true, tokenUsage: false },
  });
  await mount([complete, partial, codebuddyRunning, codebuddyTerminal], undefined, { workspaces: [project], sidebarContainer: host }, { providers: [catalogProvider(), codebuddyCatalog], roleRouting: {} });

  const task = title => [...host.querySelectorAll('.project-task')].find(item => item.textContent.includes(title));
  await act(async () => task('Codex 完整统计').querySelector('.project-task-link').focus());
  assert.match(document.querySelector('.project-task-preview').textContent, /总 Token：13/);
  await act(async () => task('Codex 部分统计').querySelector('.project-task-link').focus());
  assert.match(document.querySelector('.project-task-preview').textContent, /总 Token：8 · 统计不完整/);
  await act(async () => task('CodeBuddy 运行中').querySelector('.project-task-link').focus());
  const preview = document.querySelector('.project-task-preview').textContent;
  assert.match(preview, /CodeBuddy.*总 Token：—/);
  assert.doesNotMatch(preview, /总 Token：0/);
  await act(async () => task('CodeBuddy 运行中').querySelector('.project-task-link').click());
  assert.ok(button('取消任务', document.querySelector('.agent-detail')), 'unknown Usage must not hide the backend-authorized Cancel action');

  await act(async () => task('CodeBuddy 已完成').querySelector('.project-task-link').click());
  const detail = document.querySelector('.agent-detail');
  assert.match(detail.querySelector('.agent-detail-info-card').textContent, /Provider.*CodeBuddy.*当前轮次总 Token.*—/);
  const tokenFact = [...detail.querySelectorAll('.agent-detail-live-grid > div')].find(item => item.querySelector('span')?.textContent === '当前轮次总 Token');
  assert.equal(tokenFact.querySelector('code').textContent, '—');
  assert.ok(button('继续任务', detail), 'unknown Usage must not hide the backend-authorized Continue action');
  assert.deepEqual([...document.querySelectorAll('.agent-provider-card h3')].map(node => node.textContent), ['Codex', 'CodeBuddy']);
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
  const label = more.querySelector('.project-task-more-label');
  assert.equal(more.textContent.trim(), '查看更多');
  assert.equal(more.getAttribute('aria-busy'), 'false');
  assert.equal(more.dataset.state, 'idle');
  assert.ok(more.querySelector('.project-task-more-icon-idle'));

  await act(async () => more.click());
  assert.equal(group.querySelector('.project-task-more'), more);
  assert.equal(more.querySelector('.project-task-more-label'), label);
  assert.equal(more.textContent.trim(), '正在加载…');
  assert.equal(more.getAttribute('aria-busy'), 'true');
  assert.equal(more.dataset.state, 'loading');
  assert.equal(more.disabled, true);
  assert.ok(more.querySelector('.project-task-more-icon-loading'));
  await act(async () => pending.resolve({ executions: tasks.slice(5), nextCursor: 'next-page' }));
  assert.equal(more.textContent.trim(), '查看更多');
  assert.equal(more.getAttribute('aria-busy'), 'false');
  assert.equal(more.dataset.state, 'idle');
  assert.equal(more.querySelector('.project-task-more-label'), label);

  pending = Promise.withResolvers();
  api.agentHistory = () => pending.promise;
  await act(async () => more.click());
  await act(async () => pending.reject(new Error('page unavailable')));
  assert.equal(group.querySelector('.project-task-more'), more);
  assert.equal(more.textContent.trim(), '重试');
  assert.equal(more.getAttribute('aria-busy'), 'false');
  assert.equal(more.dataset.state, 'retry');
  assert.equal(more.querySelector('.project-task-more-label'), label);
  assert.equal(more.disabled, false);
  assert.ok(more.querySelector('.project-task-more-icon-retry'));
  assert.match(group.querySelector('[role="status"]').textContent, /加载更多失败，请重试/);

  api.agentHistory = async () => ({ executions: [], nextCursor: 'remaining-page' });
  await act(async () => more.click());
  assert.equal(group.querySelector('[role="status"]'), null);
  assert.equal(more.textContent.trim(), '查看更多');
  assert.equal(more.dataset.state, 'idle');
  assert.equal(more.querySelector('.project-task-more-label'), label);
  const styles = readFileSync('src/styles.css', 'utf8');
  assert.match(styles, /\.project-task-more\[data-state="loading"\] \.project-task-more-icon-loading \{ animation: agent-control-spin/);
  assert.doesNotMatch(styles, /\.project-task-more-label\s*\{[^}]*animation:/);
  assert.doesNotMatch(styles, /\.project-task-more:disabled\s*\{[^}]*opacity:/);
  assert.match(styles, /@media \(prefers-reduced-motion: reduce\) \{\r?\n  \.project-task, \.project-task-header, \.project-task-more-icon-slot svg \{ transition: none; \}\r?\n  \.project-task-more\[data-state="loading"\] \.project-task-more-icon-loading \{ animation: none; \}\r?\n\}/);
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
    const label = more.querySelector('.project-task-more-label');
    let pending = Promise.withResolvers();
    api.agentHistory = () => pending.promise;
    assert.ok(refreshNow);
    await act(async () => refreshNow());
    assert.equal(host.querySelector('.project-task-more'), more);
    assert.equal(more.querySelector('.project-task-more-label'), label);
    assert.equal(more.disabled, true);
    assert.equal(more.textContent.trim(), '查看更多');
    assert.equal(more.dataset.state, 'idle');
    assert.doesNotMatch(readFileSync('src/styles.css', 'utf8'), /\.project-task-more:disabled\s*\{[^}]*opacity:/);
    await act(async () => more.click());
    assert.equal(more.getAttribute('aria-busy'), 'false');
    await act(async () => pending.reject(new Error('refresh unavailable')));
    assert.equal(more.disabled, false);
    assert.equal(more.textContent.trim(), '查看更多');
    assert.equal(more.dataset.state, 'idle');
    assert.equal(more.querySelector('.project-task-more-label'), label);
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
  await mount([row()]); await openTask();
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
  await mount([value]); await openTask();
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
  const styles = readFileSync('src/styles.css', 'utf8');
  assert.match(styles, /\.agent-markdown-table \{[^}]*inline-size: 100%;[^}]*overflow-x: auto;[^}]*overflow-y: hidden;/);
  assert.equal(JSON.parse(document.querySelector('[aria-label="原始执行数据"]').textContent).prompt,markdown);
});

test('markdown renders HTML literally and rejects executable links', async () => {
  await mount([row({prompt:'<script>alert(1)</script>\n\n<img src=x onerror=alert(1)>\n\n[危险](javascript:alert%281%29)'})]);
  await openTask();
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


for (const entry of ['sidebar']) test(`running task deletion requires confirmation from ${entry} without stopping provider`, async () => {
  const project = workspace('A'); const host = navigationHost();
  const calls = await mount([row({ status: 'running', attention: 'none', prompt: '正在执行的任务', canonicalWorkspaceRoot: project.root })], undefined, { workspaces: [project], sidebarContainer: host });
  const trigger = host.querySelector('.project-task-delete');
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



/** 启停 fixture 返回完整 settings，以检验只采纳目标 Provider。 */
function enabledSettings(providers, roleRouting = {}) {
  return { ...roleSettings(roleRouting), providers: Object.fromEntries(Object.entries(providers).map(([id, enabled]) => [id, { enabled }])) };
}

/** 通过实际 shadcn Switch 触发本地策略保存。 */
async function toggleProvider(name = 'Codex') {
  const control = document.querySelector(`[role="switch"][aria-label="启用 ${name}"]`);
  assert.ok(control);
  assert.equal(control.disabled, false);
  await act(async () => control.click());
}

// 两种 Product 投影都可单独提示 blocker，不从 dispatchState 或身份猜测。
for (const projection of [
  { attention: 'pending_explicit_resume', canResumePending: false },
  { attention: 'none', canResumePending: true },
]) {
  test(`disabled pending blocker uses existing view/cancel paths: ${projection.attention}`, async () => {
    const pending = row({ provider: { id: 'future', displayName: 'Future' }, status: 'dispatch_pending', attention: projection.attention,
      availableActions: { canCancel: true, canResumePending: projection.canResumePending, canContinue: false } });
    const actions = await mount([pending, row({ executionId: 'unrelated', attention: 'pending_explicit_resume' })], undefined, {},
      { providers: [catalogProvider({ id: 'future', displayName: 'Future', enabled: false })], roleRouting: { testing: 'future' } });
    const card = document.querySelector('.agent-provider-card');
    assert.match(card.textContent, /该 Provider 有待恢复任务仍占用 Workspace Claim。/);
    assert.equal(card.querySelectorAll('li').length, 1);
    assert.equal(document.querySelector('#agent-role-testing').textContent, 'Future · 已停用');
    assert.doesNotMatch(card.textContent, /Force Unlock|强制解锁/iu);
    await click('取消任务', card);
    assert.deepEqual(actions.filter(action => action.action === 'cancel'), [{ action: 'cancel', executionId: 'old-E1' }]);
    await click('查看任务', card);
    assert.ok(actions.some(action => action.action === 'observe' && action.executionId === 'old-E1' && action.includeResult));
    assert.ok(document.querySelector('details.agent-technical'));
    assert.equal(actions.some(action => action.action === 'resume_pending'), false);
  });
}

test('reenable pending Provider uses local IPC without resuming or rebinding', async () => {
  const calls = [];
  api.agentProviderSetEnabled = async (id, enabled) => { calls.push([id, enabled]); return enabledSettings({ future: enabled }, { testing: 'other' }); };
  const actions = await mount([row({ provider: { id: 'future' }, attention: 'pending_explicit_resume' })], undefined, {},
    { providers: [catalogProvider({ id: 'future', displayName: 'Future', enabled: false })], roleRouting: { testing: 'future' } });
  await click('重新启用 Provider');
  assert.deepEqual(calls, [['future', true]]);
  assert.equal(cardValues(document.querySelector('.agent-provider-card'))['接入'], '已启用');
  assert.doesNotMatch(document.querySelector('.agent-provider-card').textContent, /仍占用 Workspace Claim/);
  assert.equal(document.querySelector('#agent-role-testing').textContent, 'Future');
  assert.ok(actions.every(action => action.action === 'list'));
  assert.match(readFileSync('src/api.ts', 'utf8'), /invoke<AgentProviderSettings>\("agent_provider_set_enabled", \{ providerId, enabled \}\)/);
});

test('disable running Provider leaves execution untouched and shows draining and disabled binding', async () => {
  api.agentProviderSetEnabled = async () => enabledSettings({ codex: false });
  const execution = row({ status: 'running', attention: 'none' });
  const frozen = structuredClone(execution);
  const actions = await mount([execution], undefined, {}, { providers: [catalogProvider()], roleRouting: { development: 'codex' } });
  await toggleProvider();
  const values = cardValues(document.querySelector('.agent-provider-card'));
  assert.equal(values['接入'], '正在停用');
  assert.equal(values['Runtime'], '运行中');
  assert.equal(values['活动任务'], '1');
  assert.equal(document.querySelectorAll('.project-task').length, 1);
  assert.equal(document.querySelector('#agent-role-development').textContent, 'Codex · 已停用');
  assert.deepEqual(execution, frozen);
  assert.ok(actions.every(action => action.action === 'list'));
});

for (const enabled of [false, true]) {
  test(`Provider ${enabled ? 'disable' : 'enable'} failure rolls back without changing other policy`, async () => {
    const mutation = deferredRoleResponse();
    api.agentProviderSetEnabled = () => mutation.promise;
    await mount([], undefined, {}, { providers: [catalogProvider({ enabled })], roleRouting: { general: 'codex' } });
    await toggleProvider();
    const control = document.querySelector('[role="switch"]');
    assert.equal(control.getAttribute('aria-checked'), String(!enabled));
    assert.equal(control.disabled, true);
    await act(async () => mutation.reject(new Error('persist failed')));
    assert.equal(control.getAttribute('aria-checked'), String(enabled));
    assert.equal(control.disabled, false);
    assert.equal(document.querySelector('#agent-role-general').textContent, enabled ? 'Codex' : 'Codex · 已停用');
    assert.match(notifications.at(-1)[1], /启用状态保存失败，已恢复原状态/);
  });
}

test('two Provider mutations and role mutation commit independently out of order', async () => {
  const first = deferredRoleResponse(), second = deferredRoleResponse(), role = deferredRoleResponse();
  api.agentProviderSetEnabled = id => id === 'codex' ? first.promise : second.promise;
  api.agentProviderSetRoleRoute = () => role.promise;
  await mount([], undefined, {}, { providers: [catalogProvider(), catalogProvider({ id: 'other', displayName: 'Other', enabled: false })], roleRouting: { development: 'codex' } });
  await toggleProvider();
  await toggleProvider('Other');
  await chooseRole('development', 'Other');
  await act(async () => second.resolve(enabledSettings({ codex: true, other: true }, { development: 'codex' })));
  await act(async () => role.resolve(enabledSettings({ codex: true, other: false }, { development: 'other' })));
  assert.equal(document.querySelector('#agent-role-development').textContent, 'Other');
  await act(async () => first.resolve(enabledSettings({ codex: false, other: false }, { development: 'codex' })));
  assert.deepEqual([...document.querySelectorAll('[role="switch"]')].map(control => control.getAttribute('aria-checked')), ['false', 'true']);
  assert.equal(document.querySelector('#agent-role-development').textContent, 'Other');
});

test('catalog crossing enabled mutation cannot overwrite pending or committed state; fresh poll remains authoritative', async t => {
  const timers = [];
  const originalSetInterval = globalThis.setInterval;
  // 保留动画真实时钟，仅手动推进目录轮询。
  t.mock.method(globalThis, 'setInterval', (callback, delay, ...args) => {
    if (delay === 1500) { timers.push(callback); return originalSetInterval(() => {}, 60_000); }
    return originalSetInterval(callback, delay, ...args);
  });
  const mutation = deferredRoleResponse(), before = deferredRoleResponse(), during = deferredRoleResponse();
  const initial = { providers: [catalogProvider()], roleRouting: { development: 'codex' } };
  let polls = 0;
  api.agentProviderSetEnabled = () => mutation.promise;
  api.agentProviderSetRoleRoute = async () => roleSettings({ development: null });
  await mount([], undefined, {}, () => {
    polls++;
    if (polls === 2) return before.promise;
    if (polls === 3) return during.promise;
    return Promise.resolve(initial);
  });
  await act(async () => timers[0]());
  await toggleProvider();
  await chooseRole('development', '未指定 Agent');
  await act(async () => before.resolve(initial));
  const control = document.querySelector('[role="switch"]');
  assert.equal(control.getAttribute('aria-checked'), 'false');
  assert.equal(control.disabled, true);
  assert.equal(document.querySelector('#agent-role-development').textContent, '未指定 Agent');
  await act(async () => timers[0]());
  await act(async () => mutation.resolve(enabledSettings({ codex: false }, { development: 'codex' })));
  await act(async () => during.resolve({ ...initial, roleRouting: { development: null } }));
  assert.equal(control.getAttribute('aria-checked'), 'false');
  assert.equal(control.disabled, false);
  assert.equal(document.querySelector('#agent-role-development').textContent, '未指定 Agent');
  await act(async () => timers[0]());
  assert.equal(control.getAttribute('aria-checked'), 'true');
  assert.equal(document.querySelector('#agent-role-development').textContent, 'Codex');
  assert.equal(polls, 4);
});

// 任意产品版本与诊断 hash 都不改变稳定码的含义，未知 Provider 身份同样适用。
for (const version of ['2.153.0', '999.0.0-new', 'unknown-dev', null, '  ']) {
  test(`exact ACP incompatibility has fixed copy and no override: ${JSON.stringify(version)}`, async () => {
    await mount([], undefined, {}, { providers: [catalogProvider({ id: 'future', displayName: 'Future Agent', diagnosticCode: 'CODEBUDDY_ACP_INCOMPATIBLE', version, binaryHash: `arbitrary-${version}` })], roleRouting: {} });
    const card = document.querySelector('.agent-provider-card');
    assert.equal(card.querySelector('p.text-amber-700').textContent, 'Future Agent 的 ACP 协议或必需能力与当前 SerenaDesktop 不兼容。请升级 CodeBuddy 或 SerenaDesktop 后重新检测。');
    assert.equal(cardValues(card).版本, version?.trim() || '—');
    assert.doesNotMatch(card.textContent, /忽略版本检查|跳过协议检查|绕过|强制放行|仍然运行|Force Unlock|override|强制解锁/iu);
    assert.deepEqual([...card.querySelectorAll('button')].map(control => control.getAttribute('role')), ['switch']);
  });
}

// 旧码、近似码与普通 unavailable 均不能从身份、版本、hash 或自由文本推断兼容性。
for (const diagnosticCode of [undefined, null, 'CODEBUDDY_VERSION_UNSUPPORTED', 'AGENT_PROVIDER_UNAVAILABLE', 'CODEBUDDY_ACP_INCOMPATIBLE_EXTRA', 'codebuddy_acp_incompatible', ' CODEBUDDY_ACP_INCOMPATIBLE ']) {
  test(`non-exact diagnostic never infers ACP incompatibility: ${diagnosticCode}`, async () => {
    await mount([], undefined, {}, { providers: ['2.153.0', '999.0.0-new', 'unknown-dev', null, '  '].map((version, index) => catalogProvider({
      id: index === 0 ? 'codebuddy' : `future-${index}`, displayName: 'CodeBuddy', health: 'unavailable', version, diagnosticCode,
      binaryHash: `arbitrary-${version}`, errorMessage: 'CODEBUDDY_ACP_INCOMPATIBLE' })), roleRouting: {} });
    for (const card of document.querySelectorAll('.agent-provider-card')) {
      assert.equal(card.querySelector('p.text-amber-700'), null);
      assert.doesNotMatch(card.textContent, /ACP 协议|兼容性验证|受支持版本|未启用该版本/);
    }
  });
}

// 正常执行不展示诊断区时，明确的安全 Activity 仍须可见，且不依赖 Provider ID。
test('permission denied is a visible running activity without diagnostic', async () => {
  const value = row({ status: 'running', attention: 'none', dispatchState: 'dispatched',
    provider: { id: 'arbitrary', displayName: 'Worker' },
    progress: { phase: 'running', activityPhase: 'provider', toolCategory: null, summaryCode: 'provider.permission_denied' } });
  assert.equal(activityLabel(value), 'Provider 权限未获批准');
  await mount([value]); await openTask();
  assert.match(document.querySelector('.agent-detail-info-card').textContent, /当前活动Provider 权限未获批准/);
  assert.equal(showExecutionDiagnostic(value), false);
  assert.equal(activityLabel(row({progress: {summaryCode: 'provider.processing'}})), 'Agent 处理中');
  assert.equal(activityLabel(row({progress: {summaryCode: 'execution.finalizing'}})), '正在整理结果');
  assert.equal(activityLabel(row({progress: {summaryCode: 'execution.reconciling'}})), '正在恢复执行状态');
});
