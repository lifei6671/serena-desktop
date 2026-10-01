import { test, afterEach } from 'node:test';
import assert from 'node:assert/strict';
import { registerHooks } from 'node:module';
import { readFileSync, existsSync } from 'node:fs';
import { fileURLToPath, pathToFileURL } from 'node:url';
import path from 'node:path';
import ts from 'typescript';
import { JSDOM } from 'jsdom';
import { providerSettingsFixture } from './configFixtures.mjs';

// 沿用现有测试的 TypeScript 加载方式，执行真实 Controller 和 API。
registerHooks({
  resolve(specifier, context, next) {
    if (specifier.startsWith('.') && context.parentURL?.startsWith('file:')) {
      const target = path.resolve(path.dirname(fileURLToPath(context.parentURL)), specifier);
      for (const suffix of ['', '.ts', '.tsx']) {
        if (/\.tsx?$/.test(target + suffix) && existsSync(target + suffix)) {
          return { url: pathToFileURL(target + suffix).href, shortCircuit: true };
        }
      }
    }
    return next(specifier, context);
  },
  load(url, context, next) {
    if (url.startsWith('file:') && /\.tsx?$/.test(url)) {
      return { format: 'module', shortCircuit: true, source: ts.transpileModule(
        readFileSync(fileURLToPath(url), 'utf8'),
        { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext } },
      ).outputText };
    }
    return next(url, context);
  },
});

const dom = new JSDOM('<!doctype html><div id="root"></div>', { url: 'http://localhost/' });
Object.assign(globalThis, { window: dom.window, document: dom.window.document, IS_REACT_ACT_ENVIRONMENT: true });
const { createElement, act } = await import('react');
const { createRoot } = await import('react-dom/client');
const { useAppController } = await import('./app/useAppController.ts');
const { api } = await import('./api.ts');
const originals = { ...api };
const intervals = new Map();
// 显式驱动轮询，不依赖真实时间；清理仍使用同一 Node timer。
window.setInterval = (callback, delay) => {
  const id = setInterval(() => {}, 60_000);
  intervals.set(id, { callback, delay });
  return id;
};
window.clearInterval = clearInterval;
let root;
let controller;
afterEach(async () => {
  if (root) await act(async () => root.unmount());
  root = null;
  for (const id of intervals.keys()) clearInterval(id);
  intervals.clear();
  Object.assign(api, originals);
  delete window.__TAURI_INTERNALS__;
});

/** 覆盖未来 Provider、未知路由以及显式空路由，不能被默认策略补回。 */
function futurePolicy() {
  return {
    providers: { 'future-acp': { enabled: true }, codex: { enabled: false } },
    roleRouting: { development: 'future-acp', testing: 'unregistered-provider', review: null, analysis: 'future-acp', general: null },
    roleDefaults: { testing: { 'unregistered-provider': { model: 'future-model', reasoning: null } } },
  };
}

/** 构造完整的配置快照，使局部保存可比较整个 wire payload。 */
function snapshot(policy = futurePolicy()) {
  return {
    config: {
      agentProviders: policy, agentEnabled: false,
      remoteAccess: { mode: 'mcp_only', quickTunnelDesiredRunning: false, selfHosted: { provider: 'custom_https', publicOrigin: null }, mcpOnly: { securityDeclaration: 'external_auth', publicOrigin: null } },
      remoteSourceWriteEnabled: false, remoteCommandExecutionEnabled: false,
      agentSuccessNotificationEnabled: true, agentFailureNotificationEnabled: true,
      agentSystemNotificationEnabled: true, agentSoundEnabled: true,
      broker: { enabled: false, port: 19120, allowLan: false }, workspaces: [],
      workspaceRegistryRevision: 1, desktopSelectedWorkspaceId: null, serenaPath: null,
      port: 19121, dashboardEnabled: true, openDashboardOnLaunch: false, autoStartServer: true, minimizeToTray: true,
    },
    desktopSelectedWorkspace: null, git: { status: 'missing', available: false, path: null, version: null, error: null },
    codegraphVersion: null, managedRuntimePresent: false, installation: null, activeInstallation: null,
    serverStatus: 'stopped', managedProcessPresent: false, activePort: 19121, endpoint: '', dashboardUrl: '',
    dashboardEnabled: true, logDirectory: '', autostartEnabled: false, autostartError: null, lastError: null,
  };
}

/** 挂载真实 hook，保留其初始化与刷新生命周期。 */
async function mount(getState = async () => snapshot()) {
  api.getState = getState;
  api.broker = async () => ({ syncWarnings: [], lastError: null });
  function Harness() {
    controller = useAppController(false);
    return null;
  }
  root = createRoot(document.getElementById('root'));
  await act(async () => root.render(createElement(Harness)));
}

test('initial placeholder and shared fixtures match Rust provider defaults; hydration does not overlay them', async () => {
  let resolveRead;
  await mount(() => new Promise(resolve => { resolveRead = resolve; }));
  assert.deepEqual(controller.draft.agentProviders, providerSettingsFixture());
  assert.deepEqual(Object.keys(controller.draft.agentProviders.roleRouting).sort(), ['analysis', 'development', 'general', 'review', 'testing']);
  const incoming = snapshot();
  await act(async () => resolveRead(incoming));
  assert.deepEqual(controller.draft, incoming.config);
  assert.equal(controller.draft.agentProviders.providers.codebuddy, undefined);
  assert.equal(controller.draft.agentProviders.roleRouting.review, null);
});

test('config API round-trip keeps unknown provider IDs and unknown/null routes with camelCase wire', async () => {
  const incoming = snapshot();
  const calls = [];
  window.__TAURI_INTERNALS__ = {
    // 模拟 IPC JSON 边界，不替换被测 api.saveConfig。
    invoke: async (command, args) => {
      calls.push({ command, args: args && JSON.parse(JSON.stringify(args)) });
      return JSON.parse(JSON.stringify(command === 'save_config' ? { ...incoming, config: args.config } : incoming));
    },
  };
  const read = await api.getState();
  const saved = await api.saveConfig(read.config);
  assert.deepEqual(saved.config, incoming.config);
  assert.deepEqual(calls, [{ command: 'get_app_state', args: {} }, { command: 'save_config', args: { config: incoming.config } }]);
  assert.equal(saved.config.agentProviders.roleRouting.testing, 'unregistered-provider');
  assert.equal(saved.config.agentProviders.roleRouting.review, null);
  assert.equal(saved.config.agentProviders.roleDefaults.testing['unregistered-provider'].model, 'future-model');
});

test('polling refresh synchronizes policy while preserving unrelated unsaved draft', async () => {
  let current = snapshot();
  await mount(async () => structuredClone(current));
  await act(async () => controller.setDraft(draft => ({ ...draft, port: 23456 })));
  current = snapshot({ providers: {}, roleRouting: { development: null, testing: null, review: null, analysis: null, general: null }, roleDefaults: {} });
  await act(async () => intervals.values().find(entry => entry.delay === 1500).callback());
  assert.deepEqual(controller.state.config.agentProviders, current.config.agentProviders);
  assert.deepEqual(controller.draft.agentProviders, current.config.agentProviders);
  assert.equal(controller.draft.port, 23456);
});

for (const method of ['saveFields', 'saveToggle']) {
  test(`${method} preserves policy on partial save and adopts newer returned policy without losing draft edits`, async () => {
    const incoming = snapshot();
    await mount(async () => structuredClone(incoming));
    await act(async () => controller.setDraft(draft => ({ ...draft, port: 23456, agentProviders: providerSettingsFixture() })));
    const newer = futurePolicy();
    newer.providers['future-acp'].enabled = false;
    newer.roleRouting.general = 'next-provider';
    let sent;
    let finishSave;
    api.saveConfig = config => {
      sent = JSON.parse(JSON.stringify(config));
      return new Promise(resolve => { finishSave = () => resolve({ ...incoming, config: { ...config, agentProviders: newer } }); });
    };
    let saving;
    await act(async () => { saving = controller[method]({ agentEnabled: true }, '保存完成'); });
    assert.deepEqual(sent, { ...incoming.config, agentEnabled: true });
    await act(async () => controller.setDraft(draft => ({ ...draft, serenaPath: 'unsaved-path' })));
    await act(async () => { finishSave(); await saving; });
    assert.deepEqual(controller.state.config.agentProviders, newer);
    assert.deepEqual(controller.draft.agentProviders, newer);
    assert.equal(controller.draft.port, 23456);
    assert.equal(controller.draft.serenaPath, 'unsaved-path');
    assert.equal(controller.draft.agentEnabled, true);
  });

  test(`${method} failure refresh adopts server policy and preserves unsaved ordinary fields`, async () => {
    let current = snapshot();
    await mount(async () => structuredClone(current));
    await act(async () => controller.setDraft(draft => ({ ...draft, port: 23456 })));
    current = snapshot(providerSettingsFixture());
    api.saveConfig = async () => { throw new Error('save failed'); };
    await act(async () => controller[method]({ agentEnabled: true }, '保存完成'));
    assert.deepEqual(controller.draft.agentProviders, current.config.agentProviders);
    assert.equal(controller.draft.agentEnabled, false);
    assert.equal(controller.draft.port, 23456);
  });
}

test('general action and autostart snapshots keep draft policy authoritative', async () => {
  await mount();
  await act(async () => controller.setDraft(draft => ({ ...draft, port: 23456 })));
  const changed = snapshot(providerSettingsFixture());
  await act(async () => controller.run('保存', async () => changed));
  assert.deepEqual(controller.draft.agentProviders, changed.config.agentProviders);
  api.setAutostart = async () => snapshot();
  await act(async () => controller.setAutostart(true));
  assert.deepEqual(controller.draft.agentProviders, futurePolicy());
  assert.equal(controller.draft.port, 23456);
});
