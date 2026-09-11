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
    if (specifier === '@tauri-apps/api/event') return { url: 'data:text/javascript,export async function listen(name, handler) { globalThis.__remoteAuthorization = handler; return () => { delete globalThis.__remoteAuthorization; }; }', shortCircuit: true };
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
const { api } = await import('./api.ts');
const { useRemoteAccess } = await import('./useRemoteAccess.ts');
const { RemoteApprovalDialog } = await import('./RemoteApprovalDialog.tsx');
let root, controller;
let snapshots = [];
function Harness() {
  controller = useRemoteAccess();
  snapshots.push(controller.state);
  return createElement(RemoteApprovalDialog, { controller });
}
const snapshot = (overrides = {}) => ({ mode: 'quick_tunnel', status: 'ready', active: true, pending: [], publicContext: { publicOrigin: 'https://test.example', mcpResource: 'https://test.example/mcp' }, ...overrides });
const pending = { id: 'P1', clientName: 'Client', redirectUri: 'https://client.example/cb', confirmationCode: '123456', expiresInSeconds: 100, scope: 'serena:mcp', refreshAllowed: true };
async function mount() {
  root = createRoot(document.getElementById('root'));
  await act(async () => root.render(createElement(TooltipProvider, null, createElement(Harness))));
}
afterEach(async () => { if (root) await act(async () => root.unmount()); root = null; snapshots = []; });

test('native consent explains registered refresh capability without requiring offline_access', async () => {
  for (const refreshAllowed of [true, false]) {
    api.remoteState = async () => snapshot({ pending: [{ ...pending, refreshAllowed }] });
    let decisions = 0;
    api.remoteApprove = async () => { decisions++; };
    await mount();
    const text = document.querySelector('[role="dialog"]').textContent;
    assert.match(text, /serena:mcp/);
    assert.match(text, refreshAllowed ? /客户端可自动刷新/ : /不签发刷新令牌/);
    assert.equal(document.activeElement.textContent, '拒绝');
    assert.equal(decisions, 0);
    await act(async () => root.unmount()); root = null;
  }
});

test('probe and approval are independent; authorization events update the dialog during a remote mutation', async () => {
  for (const allow of [true, false]) {
    let current = snapshot();
    api.remoteState = async () => structuredClone(current);
    const probe = Promise.withResolvers(), approval = Promise.withResolvers();
    api.remoteProbe = () => probe.promise;
    const decisions = [];
    api.remoteApprove = async (id, value) => { decisions.push([id, value]); await approval.promise; current = snapshot(); };
    await mount();
    let operation;
    await act(async () => { operation = controller.operate('测试连接', api.remoteProbe); });
    assert.equal(controller.busy, '测试连接');
    current = snapshot({ pending: [pending] });
    await act(async () => globalThis.__remoteAuthorization({}));
    assert.ok(document.querySelector('[role="dialog"]'));
    assert.equal(document.activeElement.textContent, '拒绝');
    const button = [...document.querySelectorAll('button')].find(b => b.textContent === (allow ? '允许连接' : '拒绝'));
    assert.equal(button.disabled, false);
    await act(async () => button.click());
    assert.equal(controller.approvalBusy, true);
    assert.equal(controller.busy, '测试连接');
    assert.equal(await controller.approve('P1', !allow), false);
    assert.deepEqual(decisions, [['P1', allow]]);
    let remoteCalls = 0;
    assert.equal(await controller.operate('停止', async () => { remoteCalls++; }), false);
    assert.equal(remoteCalls, 0);
    await act(async () => approval.resolve());
    assert.equal(controller.approvalBusy, false);
    assert.equal(document.querySelector('[role="dialog"]'), null);
    await act(async () => { probe.resolve(); await operation; });
    assert.equal(controller.busy, '');
    await act(async () => root.unmount()); root = null;
  }
});

test('mutation refresh preserves the snapshot and read errors retain the last known state', async () => {
  const initial = snapshot();
  api.remoteState = async () => initial;
  await mount();
  snapshots = [];
  const read = Promise.withResolvers();
  api.remoteState = () => read.promise;
  let operation;
  await act(async () => { operation = controller.operate('停止', async () => {}); });
  assert.equal(controller.state, initial);
  assert.ok(snapshots.every(s => s !== null));
  const stopped = snapshot({ status: 'stopped', active: false, publicContext: null });
  await act(async () => { read.resolve(stopped); await operation; });
  assert.equal(controller.state, stopped);
  assert.ok(snapshots.every(s => s !== null));
  api.remoteState = async () => { throw new Error('read failed'); };
  await act(async () => globalThis.__remoteAuthorization({}));
  assert.equal(controller.state, stopped);
  assert.match(controller.error, /read failed/);
});

test('stale refresh cannot overwrite a newer snapshot or post-mutation state', async () => {
  api.remoteState = async () => snapshot();
  await mount();
  const stale = Promise.withResolvers();
  api.remoteState = () => stale.promise;
  await act(async () => globalThis.__remoteAuthorization({}));
  const newer = snapshot({ pending: [pending] });
  api.remoteState = async () => newer;
  await act(async () => globalThis.__remoteAuthorization({}));
  assert.equal(controller.state, newer);
  await act(async () => stale.resolve(snapshot({ status: 'stopped' })));
  assert.equal(controller.state, newer);
  api.remoteState = async () => snapshot({ status: 'error', publicContext: null });
  await act(async () => { assert.equal(await controller.operate('测试连接', async () => { throw new Error('probe failed'); }), false); });
  assert.equal(controller.state.status, 'error');
  assert.equal(controller.state.publicContext, null);
  assert.match(controller.error, /probe failed/);
});
