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
