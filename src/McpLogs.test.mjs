import { afterEach, test } from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { registerHooks } from 'node:module';
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
  },
});

const dom = new JSDOM('<!doctype html><html><body><div id="root"></div></body></html>', { url: 'http://localhost/', pretendToBeVisual: true });
Object.assign(globalThis, { window: dom.window, document: dom.window.document, HTMLElement: dom.window.HTMLElement, Element: dom.window.Element, Node: dom.window.Node, DocumentFragment: dom.window.DocumentFragment, CustomEvent: dom.window.CustomEvent, MutationObserver: dom.window.MutationObserver, HTMLInputElement: dom.window.HTMLInputElement, HTMLFormElement: dom.window.HTMLFormElement, Event: dom.window.Event, KeyboardEvent: dom.window.KeyboardEvent, MouseEvent: dom.window.MouseEvent, IS_REACT_ACT_ENVIRONMENT: true });
globalThis.getComputedStyle = dom.window.getComputedStyle.bind(dom.window);
HTMLElement.prototype.scrollIntoView ??= () => {};
globalThis.requestAnimationFrame = dom.window.requestAnimationFrame.bind(dom.window);
globalThis.cancelAnimationFrame = dom.window.cancelAnimationFrame.bind(dom.window);
const { createElement, act } = await import('react');
const { createRoot } = await import('react-dom/client');
const { McpLogs } = await import('./McpLogs.tsx');
const { api } = await import('./api.ts');
const { countNewLogLines, filterMcpLogs, parseMcpLogLine, reconcileMcpLogEntries } = await import('./mcpLogPresentation.ts');
let root;

afterEach(async () => { if (root) await act(async () => root.unmount()); root = null; });

test('MCP log parser and filters preserve raw fallback content', () => {
  const structured = parseMcpLogLine('WARN  2026-09-13 10:42:06.921 [TOOL] slow request', 0);
  const raw = parseMcpLogLine('legacy broker output', 1);
  assert.deepEqual(structured, { id: '0:WARN  2026-09-13 10:42:06.921 [TOOL] slow request', raw: 'WARN  2026-09-13 10:42:06.921 [TOOL] slow request', timestamp: '2026-09-13 10:42:06.921', level: 'WARN', source: 'TOOL', message: 'slow request', details: null });
  assert.deepEqual(raw, { id: '1:legacy broker output', raw: 'legacy broker output', timestamp: null, level: null, source: null, message: 'legacy broker output', details: null });
  assert.deepEqual(filterMcpLogs([structured, raw], { source: 'TOOL', level: 'WARN', query: 'request' }), [structured]);
  assert.deepEqual(filterMcpLogs([structured, raw], { source: '', level: '', query: 'legacy' }), [raw]);
  assert.equal(countNewLogLines(['a', 'b', 'c'], ['b', 'c', 'd']), 1);
});

test('MCP 日志解析会从列表消息中分离结构化诊断', () => {
  const raw = 'ERROR 2026-09-13 10:42:11.553 [TOOL] tools/call completed\t@serena-details={"kind":"tool_call","tool":"source_read_file","arguments":{"workspaceId":"W1","relative_path":"src/lib.rs"},"success":false,"errorCode":"SOURCE_NOT_FOUND","error":"SOURCE_NOT_FOUND: file does not exist"}';
  const entry = parseMcpLogLine(raw, 0);
  assert.equal(entry.message, 'tools/call completed');
  assert.deepEqual(entry.details, {
    kind: 'tool_call',
    tool: 'source_read_file',
    arguments: { workspaceId: 'W1', relative_path: 'src/lib.rs' },
    success: false,
    errorCode: 'SOURCE_NOT_FOUND',
    error: 'SOURCE_NOT_FOUND: file does not exist',
  });
  assert.equal(filterMcpLogs([entry], { source: '', level: '', query: 'source_read_file' }).length, 1);
});

test('刷新复用保留后缀日志的身份，并让重复原始行保持可区分', () => {
  let sequence = 0;
  const createId = raw => `${raw}:${sequence++}`;
  const initial = reconcileMcpLogEntries([], ['same', 'same', 'tail'], createId);
  const refreshed = reconcileMcpLogEntries(initial, ['same', 'tail', 'new'], createId);
  assert.deepEqual(initial.map(entry => entry.id), ['same:0', 'same:1', 'tail:2']);
  assert.deepEqual(refreshed.map(entry => entry.id), ['same:1', 'tail:2', 'new:3']);
});

test('日志筛选使用带明确当前值的 Radix 下拉框，并提供真实来源与标准级别', async () => {
  const original = api.mcpLogs;
  api.mcpLogs = async () => ['NOTICE 2026-09-13 10:42:01.124 [CUSTOM] custom event'];
  try {
    root = createRoot(document.getElementById('root'));
    await act(async () => root.render(createElement(McpLogs)));
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 0)); });
    const sourceTrigger = document.querySelector('[aria-label="按来源过滤"]');
    const levelTrigger = document.querySelector('[aria-label="按级别过滤"]');
    assert.equal(sourceTrigger.textContent, '来源：全部来源');
    assert.equal(levelTrigger.textContent, '级别：全部级别');
    assert.equal(document.querySelector('[aria-label="搜索"]').textContent, '搜索');
    await act(async () => sourceTrigger.click());
    assert.deepEqual([...document.querySelectorAll('[role="option"]')].map(item => item.textContent), ['全部来源', 'MCP', 'TOOL', 'CUSTOM']);
    await act(async () => window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })));
    await act(async () => levelTrigger.click());
    assert.deepEqual([...document.querySelectorAll('[role="option"]')].map(item => item.textContent).slice(-7), ['全部级别', 'TRACE', 'DEBUG', 'INFO', 'WARN', 'ERROR', 'NOTICE']);
  } finally {
    api.mcpLogs = original;
  }
});

test('日志搜索仅在按钮或 Enter 提交后应用，并可用空查询恢复全部', async () => {
  const original = api.mcpLogs;
  const first = 'INFO  2026-09-13 10:42:01.124 [MCP] first entry';
  const second = 'WARN  2026-09-13 10:42:02.124 [TOOL] second entry';
  api.mcpLogs = async () => [first, second];
  try {
    root = createRoot(document.getElementById('root'));
    await act(async () => root.render(createElement(McpLogs)));
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 0)); });
    const input = document.querySelector('[aria-label="搜索日志"]');
    const searchForm = input.closest('.log-search-form');
    const searchField = input.closest('.log-search-field');
    const searchButton = searchForm.querySelector('[aria-label="搜索"]');
    assert.ok(searchForm.contains(searchField));
    assert.ok(searchForm.contains(searchButton));
    assert.equal(searchField.contains(searchButton), false);
    assert.equal(searchButton.textContent, '搜索');
    const setInput = (value) => {
      const descriptor = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value');
      descriptor.set.call(input, value);
      input.dispatchEvent(new Event('input', { bubbles: true }));
    };
    await act(async () => setInput('second'));
    assert.equal(document.querySelectorAll('.log-entry').length, 2);
    await act(async () => document.querySelector('[aria-label="搜索"]').click());
    assert.equal(document.querySelectorAll('.log-entry').length, 1);
    assert.match(document.querySelector('.log-entry').textContent, /second entry/);
    await act(async () => setInput('first'));
    await act(async () => input.closest('form').dispatchEvent(new Event('submit', { bubbles: true, cancelable: true })));
    assert.equal(document.querySelectorAll('.log-entry').length, 1);
    assert.match(document.querySelector('.log-entry').textContent, /first entry/);
    await act(async () => setInput(''));
    await act(async () => document.querySelector('[aria-label="搜索"]').click());
    assert.equal(document.querySelectorAll('.log-entry').length, 2);
  } finally {
    api.mcpLogs = original;
  }
});

test('MCP log row opens a truthful detail panel and copies only its raw log', async () => {
  const original = { mcpLogs: api.mcpLogs, clearMcpLogs: api.clearMcpLogs, openLogs: api.openLogs };
  const originalSetTimeout = window.setTimeout;
  const line = 'ERROR 2026-09-13 10:42:11.553 [MCP] listener unavailable';
  const nextLine = 'INFO  2026-09-13 10:42:12.553 [TOOL] listener restored';
  const copied = [];
  let resetCopy;
  const navigatorDescriptor = Object.getOwnPropertyDescriptor(globalThis, 'navigator');
  Object.defineProperty(globalThis, 'navigator', { configurable: true, value: { clipboard: { writeText: async (value) => copied.push(value) } } });
  api.mcpLogs = async () => [line, nextLine];
  api.clearMcpLogs = async () => {};
  api.openLogs = async () => {};
  window.setTimeout = (callback, delay) => {
    if (delay === 1500) {
      resetCopy = callback;
      return 1;
    }
    return originalSetTimeout(callback, delay);
  };
  try {
    root = createRoot(document.getElementById('root'));
    await act(async () => root.render(createElement(McpLogs)));
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 0)); });
    const rows = [...document.querySelectorAll('.log-entry')];
    await act(async () => rows[0].click());
    assert.equal(rows[0].getAttribute('data-selected'), 'true');
    assert.equal(rows[1].getAttribute('data-selected'), null);
    const details = document.querySelector('[aria-label="日志详情"]');
    assert.match(details.textContent, /2026-09-13 10:42:11.553/);
    assert.match(details.textContent, /MCP/);
    assert.match(details.textContent, /listener unavailable/);
    assert.doesNotMatch(details.textContent, /Port:|Error Code:|Owner PID:/);
    await act(async () => [...details.querySelectorAll('button')].find(button => button.textContent === '复制日志').click());
    assert.deepEqual(copied, [line]);
    assert.ok([...details.querySelectorAll('button')].find(button => button.textContent === '已复制'));
    await act(async () => resetCopy());
    assert.ok([...details.querySelectorAll('button')].find(button => button.textContent === '复制日志'));
    await act(async () => rows[1].click());
    assert.equal(rows[0].getAttribute('data-selected'), null);
    assert.equal(rows[1].getAttribute('data-selected'), 'true');
    assert.match(document.querySelector('[aria-label="日志详情"]').textContent, /listener restored/);
  } finally {
    api.mcpLogs = original.mcpLogs;
    api.clearMcpLogs = original.clearMcpLogs;
    api.openLogs = original.openLogs;
    window.setTimeout = originalSetTimeout;
    if (navigatorDescriptor) Object.defineProperty(globalThis, 'navigator', navigatorDescriptor);
  }
});

test('工具和 HTTP 诊断只在日志详情中展示', async () => {
  const original = api.mcpLogs;
  const lines = [
    'ERROR 2026-09-13 10:42:11.553 [TOOL] tools/call completed\t@serena-details={"kind":"tool_call","tool":"source_read_file","arguments":{"workspaceId":"W1","relative_path":"src/lib.rs"},"success":false,"errorCode":"SOURCE_NOT_FOUND","error":"SOURCE_NOT_FOUND: file does not exist"}',
    'WARN  2026-09-13 10:42:12.553 [MCP] HTTP POST · 403 Forbidden\t@serena-details={"kind":"http_request","method":"POST","path":"/mcp","status":403,"peer":"127.0.0.1:50000"}',
  ];
  api.mcpLogs = async () => lines;
  try {
    root = createRoot(document.getElementById('root'));
    await act(async () => root.render(createElement(McpLogs)));
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 0)); });
    const rows = [...document.querySelectorAll('.log-entry')];
    assert.match(rows[0].textContent, /tools\/call completed/);
    assert.doesNotMatch(rows[0].textContent, /source_read_file|relative_path|SOURCE_NOT_FOUND/);
    await act(async () => rows[0].click());
    let details = document.querySelector('[aria-label="日志详情"]');
    assert.match(details.textContent, /工具调用详情/);
    assert.match(details.textContent, /source_read_file/);
    assert.match(details.textContent, /workspaceId/);
    assert.match(details.textContent, /SOURCE_NOT_FOUND/);
    await act(async () => rows[1].click());
    details = document.querySelector('[aria-label="日志详情"]');
    assert.match(details.textContent, /HTTP 请求详情/);
    assert.match(details.textContent, /POST/);
    assert.match(details.textContent, /\/mcp/);
    assert.match(details.textContent, /403/);
  } finally {
    api.mcpLogs = original;
  }
});

test('Ctrl/Cmd、Shift 与 Esc 支持可见顺序批量选择，并按原始内容顺序复制', async () => {
  const original = api.mcpLogs;
  const lines = [
    'INFO  2026-09-13 10:42:01.124 [MCP] first entry',
    'WARN  2026-09-13 10:42:02.124 [TOOL] second entry',
    'ERROR 2026-09-13 10:42:03.124 [MCP] third entry',
  ];
  const copied = [];
  const navigatorDescriptor = Object.getOwnPropertyDescriptor(globalThis, 'navigator');
  Object.defineProperty(globalThis, 'navigator', { configurable: true, value: { clipboard: { writeText: async (value) => copied.push(value) } } });
  api.mcpLogs = async () => lines;
  try {
    root = createRoot(document.getElementById('root'));
    await act(async () => root.render(createElement(McpLogs)));
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 0)); });
    const rows = [...document.querySelectorAll('.log-entry')];
    await act(async () => rows[0].dispatchEvent(new MouseEvent('click', { bubbles: true, ctrlKey: true })));
    await act(async () => rows[1].dispatchEvent(new MouseEvent('click', { bubbles: true, metaKey: true })));
    assert.match(document.querySelector('[aria-label="批量日志操作"]').textContent, /已选择 2 条/);
    assert.ok(document.querySelector('.log-batch-copy-button'));
    await act(async () => rows[2].dispatchEvent(new MouseEvent('click', { bubbles: true, shiftKey: true })));
    assert.match(document.querySelector('[aria-label="批量日志操作"]').textContent, /已选择 3 条/);
    assert.equal(document.querySelector('[aria-label="日志详情"]'), null);
    await act(async () => [...document.querySelectorAll('[aria-label="批量日志操作"] button')].find(button => /复制选中/.test(button.textContent)).click());
    assert.deepEqual(copied, [lines.join('\n')]);
    await act(async () => window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })));
    assert.equal(document.querySelector('[aria-label="批量日志操作"]'), null);
    await act(async () => rows[1].click());
    assert.match(document.querySelector('[aria-label="日志详情"]').textContent, /second entry/);
  } finally {
    api.mcpLogs = original;
    if (navigatorDescriptor) Object.defineProperty(globalThis, 'navigator', navigatorDescriptor);
  }
});

test('过滤与轮询刷新会剔除不可见或已不存在的批量选择', async () => {
  const original = api.mcpLogs;
  const originalSetTimeout = window.setTimeout;
  const originalClearTimeout = window.clearTimeout;
  const first = 'INFO  2026-09-13 10:42:01.124 [MCP] first entry';
  const second = 'WARN  2026-09-13 10:42:02.124 [TOOL] second entry';
  let lines = [first, second];
  let refresh;
  api.mcpLogs = async () => lines;
  window.setTimeout = (callback, delay) => {
    if (delay === 1000) {
      refresh = callback;
      return 1;
    }
    return originalSetTimeout(callback, delay);
  };
  window.clearTimeout = () => {};
  try {
    root = createRoot(document.getElementById('root'));
    await act(async () => root.render(createElement(McpLogs)));
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 0)); });
    let rows = [...document.querySelectorAll('.log-entry')];
    await act(async () => rows[0].dispatchEvent(new MouseEvent('click', { bubbles: true, ctrlKey: true })));
    await act(async () => rows[1].dispatchEvent(new MouseEvent('click', { bubbles: true, ctrlKey: true })));
    const input = document.querySelector('[aria-label="搜索日志"]');
    const descriptor = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value');
    await act(async () => {
      descriptor.set.call(input, 'second');
      input.dispatchEvent(new Event('input', { bubbles: true }));
      document.querySelector('[aria-label="搜索"]').click();
    });
    assert.match(document.querySelector('[aria-label="批量日志操作"]').textContent, /已选择 1 条/);
    await act(async () => {
      descriptor.set.call(input, '');
      input.dispatchEvent(new Event('input', { bubbles: true }));
      document.querySelector('[aria-label="搜索"]').click();
    });
    rows = [...document.querySelectorAll('.log-entry')];
    await act(async () => rows[1].dispatchEvent(new MouseEvent('click', { bubbles: true, ctrlKey: true })));
    lines = [first];
    await act(async () => refresh());
    assert.equal(document.querySelector('[aria-label="批量日志操作"]'), null);
  } finally {
    api.mcpLogs = original;
    window.setTimeout = originalSetTimeout;
    window.clearTimeout = originalClearTimeout;
  }
});

test('stopping follow shows new logs and resumes at the latest entry', async () => {
  const original = api.mcpLogs;
  const originalSetTimeout = window.setTimeout;
  const originalClearTimeout = window.clearTimeout;
  const first = 'INFO  2026-09-13 10:42:01.124 [MCP] first entry';
  const latest = 'WARN  2026-09-13 10:42:02.124 [TOOL] latest entry';
  let lines = [first];
  let refresh;
  api.mcpLogs = async () => lines;
  window.setTimeout = (callback, delay) => {
    if (delay === 1000) {
      refresh = callback;
      return 1;
    }
    return originalSetTimeout(callback, delay);
  };
  window.clearTimeout = () => {};
  try {
    root = createRoot(document.getElementById('root'));
    await act(async () => root.render(createElement(McpLogs)));
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 0)); });
    const stream = document.querySelector('.log-stream');
    Object.defineProperties(stream, {
      scrollHeight: { configurable: true, get: () => 1000 },
      clientHeight: { configurable: true, get: () => 300 },
    });
    stream.scrollTop = 0;
    await act(async () => stream.dispatchEvent(new Event('scroll', { bubbles: true })));
    assert.equal(document.querySelector('.log-follow-button').getAttribute('aria-pressed'), 'false');
    lines = [first, latest];
    await act(async () => refresh());
    assert.match(document.querySelector('.log-new-entries').textContent, /1 条新日志/);
    await act(async () => document.querySelector('.log-new-entries').click());
    assert.equal(document.querySelector('.log-follow-button').getAttribute('aria-pressed'), 'true');
    assert.equal(stream.scrollTop, 1000);
  } finally {
    api.mcpLogs = original;
    window.setTimeout = originalSetTimeout;
    window.clearTimeout = originalClearTimeout;
  }
});
