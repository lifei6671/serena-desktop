import assert from "node:assert/strict";
import { readFileSync, existsSync } from "node:fs";
import { fileURLToPath, pathToFileURL } from "node:url";
import { registerHooks } from "node:module";
import path from "node:path";
import test, { afterEach } from "node:test";
import ts from "typescript";
import { JSDOM } from "jsdom";
import { toast } from "sonner";

registerHooks({
  resolve(specifier, context, next) {
    let target;
    if (specifier.startsWith("@/")) target = path.resolve("src", specifier.slice(2));
    else if (specifier.startsWith(".") && context.parentURL?.startsWith("file:")) {
      target = path.resolve(path.dirname(fileURLToPath(context.parentURL)), specifier);
    }
    if (target) for (const suffix of ["", ".ts", ".tsx"]) {
      if (/\.tsx?$/.test(target + suffix) && existsSync(target + suffix)) {
        return { url: pathToFileURL(target + suffix).href, shortCircuit: true };
      }
    }
    return next(specifier, context);
  },
  load(url, context, next) {
    if (url.startsWith("file:") && /\.tsx?$/.test(url)) {
      return {
        format: "module",
        shortCircuit: true,
        source: ts.transpileModule(readFileSync(fileURLToPath(url), "utf8"), {
          compilerOptions: {
            target: ts.ScriptTarget.ES2022,
            module: ts.ModuleKind.ESNext,
            jsx: ts.JsxEmit.ReactJSX,
          },
        }).outputText,
      };
    }
    return next(url, context);
  },
});

const dom = new JSDOM("<!doctype html><html><body><div id=\"root\"></div></body></html>", {
  url: "http://localhost/",
  pretendToBeVisual: true,
});
const invocations = [];
const eventCallbacks = new Map();
const unlistenedEvents = [];
let delayedEventListen = null;
dom.window.__TAURI_INTERNALS__ = {
  invoke: async (command, args) => {
    invocations.push([command, args]);
    if (command === "workspace_capability_observe") {
      return { workspaceId: args.workspaceId, providers: {} };
    }
    if (command === "workspace_capability_prepare") {
      return { operationId: "cap-op-default", readiness: "ready" };
    }
    if (command === "plugin:event|listen") {
      if (delayedEventListen) return new Promise(resolve => { delayedEventListen.resolve = () => resolve(args.handler); });
      return args.handler;
    }
    return "D:/picked-directory";
  },
  transformCallback(callback) {
    const id = eventCallbacks.size + 1;
    eventCallbacks.set(id, callback);
    return id;
  },
};
dom.window.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
  unregisterListener(event, eventId) {
    unlistenedEvents.push([event, eventId]);
    eventCallbacks.delete(eventId);
  },
};
Object.assign(globalThis, {
  window: dom.window,
  document: dom.window.document,
  HTMLElement: dom.window.HTMLElement,
  HTMLFormElement: dom.window.HTMLFormElement,
  HTMLInputElement: dom.window.HTMLInputElement,
  DocumentFragment: dom.window.DocumentFragment,
  NodeFilter: dom.window.NodeFilter,
  Element: dom.window.Element,
  Node: dom.window.Node,
  CustomEvent: dom.window.CustomEvent,
  MutationObserver: dom.window.MutationObserver,
  getComputedStyle: dom.window.getComputedStyle,
  requestAnimationFrame: dom.window.requestAnimationFrame.bind(dom.window),
  cancelAnimationFrame: dom.window.cancelAnimationFrame.bind(dom.window),
  IS_REACT_ACT_ENVIRONMENT: true,
});
globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} };
dom.window.HTMLElement.prototype.scrollIntoView = () => {};
const { createElement, act } = await import("react");
const { createRoot } = await import("react-dom/client");
const { api } = await import("./api.ts");
const { ProjectPanel } = await import("./ProjectPanel.tsx");
let root;

afterEach(async () => {
  if (root) await act(async () => root.unmount());
  root = null;
});

test("workspacePickDirectory invokes the frozen IPC name and returns its nullable path", async () => {
  invocations.length = 0;

  assert.equal(await api.workspacePickDirectory(), "D:/picked-directory");
  assert.deepEqual(invocations, [["workspace_pick_directory", {}]]);
});

test("workspace inspection, registration, Serena import, rename, remove, and reorder use their frozen IPC names and argument shapes", async () => {
  invocations.length = 0;

  await api.workspaceInspectDirectory("D:/picked-directory");
  await api.workspaceRegister("D:/canonical-directory", "Chosen name");
  await api.workspaceImportSerena();
  await api.workspaceRename("project-1", "Renamed project");
  await api.workspaceRemove("project-2");
  await api.workspaceReorder(["project-2", "project-1"]);
  await api.workspaceCapabilityObserve("project-3");
  await api.workspaceCapabilityPrepare("project-3", "third", "prepare");
  await api.workspaceCapabilityCancel("cap-op-3");

  assert.deepEqual(invocations, [
    ["workspace_inspect_directory", { root: "D:/picked-directory" }],
    ["workspace_register", { root: "D:/canonical-directory", name: "Chosen name" }],
    ["workspace_import_serena", {}],
    ["workspace_rename", { id: "project-1", name: "Renamed project" }],
    ["workspace_remove", { id: "project-2" }],
    ["workspace_reorder", { ids: ["project-2", "project-1"] }],
    ["workspace_capability_observe", { workspaceId: "project-3" }],
    ["workspace_capability_prepare", { workspaceId: "project-3", providerId: "third", actionId: "prepare" }],
    ["workspace_capability_cancel", { operationId: "cap-op-3" }],
  ]);
});

function workspaceState() {
  return {
    config: { workspaces: [], desktopSelectedWorkspaceId: null },
    desktopSelectedWorkspace: null,
    serverStatus: "stopped",
    activeInstallation: null,
    installation: null,
    git: { available: false, status: "missing", version: null },
  };
}

function workspaceController() {
  return {
    broker: {
      running: false,
      projects: [],
      activeWorkspace: null,
      codegraph: null,
      operation: null,
      projectSources: [],
      syncWarnings: [],
    },
    busy: "",
    perform: async (_label, action) => {
      try {
        await action();
      } catch {}
    },
  };
}

function workspacePanelProps(controller, state) {
  return {
    state,
    controller,
    onSettings() {},
    onRemote() {},
    onSerena() {},
    onSelectWorkspace: async () => {},
    onCopied() {},
  };
}

async function renderWorkspacePanel(controller = workspaceController(), state = workspaceState()) {
  root = createRoot(document.getElementById("root"));
  await act(async () => {
    root.render(createElement(ProjectPanel, workspacePanelProps(controller, state)));
  });
}

async function rerenderWorkspacePanel(controller, state) {
  await act(async () => {
    root.render(createElement(ProjectPanel, workspacePanelProps(controller, state)));
  });
}

const button = (label) => [...document.querySelectorAll("button")]
  .find((element) => element.textContent === label);

const managedWorkspaceState = (workspaces, selected = null) => ({
  ...workspaceState(),
  config: { workspaces, desktopSelectedWorkspaceId: selected?.id ?? null },
  desktopSelectedWorkspace: selected,
});

const managedWorkspaceRow = (id) => document.querySelector(`[data-workspace-id="${id}"]`);

const managedWorkspaceOrder = () => [...document.querySelectorAll(".workspace-management-row")]
  .map((row) => row.dataset.workspaceId);

const managedWorkspaceButton = (id, label) => [...managedWorkspaceRow(id).querySelectorAll("button")]
  .find((element) => element.textContent === label);

function capabilityHealth(workspaceId) {
  return {
    workspaceId,
    providers: {
      unavailable: {
        displayName: "Unavailable provider",
        installation: "not_installed",
        status: "unavailable",
        readiness: "unknown",
        runtimeState: "stopped",
        checkedAt: 1,
        stages: [{ id: "setup", displayName: "Setup", state: "unknown", requirement: "auto_preparable", messageCode: null }],
        actions: [],
      },
      prepared: {
        displayName: "Prepared provider",
        installation: "installed",
        status: "ready",
        readiness: "not_prepared",
        runtimeState: "ready",
        checkedAt: 2,
        stages: [{ id: "cache", displayName: "Cache", state: "stale", requirement: "optional", messageCode: "CACHE_STALE" }],
        actions: [],
      },
      third: {
        displayName: "Third provider",
        installation: "installed",
        status: "error",
        readiness: "error",
        runtimeState: "starting",
        checkedAt: 3,
        stages: [{ id: "index", displayName: "Index", state: "error", requirement: "required", messageCode: "INDEX_ERROR" }],
        actions: [{ id: "sync", displayName: "Sync third", authority: "local_human", execution: "provider_prepare" }],
      },
      stopping: {
        displayName: "Stopping provider",
        installation: "installed",
        status: "ready",
        readiness: "ready",
        runtimeState: "stopping",
        checkedAt: 4,
        stages: [],
        actions: [],
      },
      stoppedReady: {
        displayName: "Stopped ready provider",
        installation: "installed",
        status: "ready",
        readiness: "ready",
        runtimeState: "stopped",
        checkedAt: 5,
        stages: [],
        actions: [],
      },
      runtimeFailure: {
        displayName: "Runtime failure provider",
        installation: "installed",
        status: "error",
        readiness: "ready",
        runtimeState: "error",
        checkedAt: 6,
        stages: [],
        actions: [],
      },
    },
  };
}

test("capability health renders descriptor providers and three orthogonal dimensions without Provider ID branches", async () => {
  const original = api.workspaceCapabilityObserve;
  const workspace = { id: "health-workspace", name: "Health", root: "D:/health", generation: 7 };
  api.workspaceCapabilityObserve = async id => capabilityHealth(id);
  try {
    await renderWorkspacePanel(workspaceController(), managedWorkspaceState([workspace], workspace));
    await act(async () => { await Promise.resolve(); });
    const health = document.querySelector('[data-capability-workspace="health-workspace"]');
    assert.match(health.textContent, /Unavailable provider/);
    assert.match(health.textContent, /安装：未安装/);
    assert.match(health.textContent, /可用性：不可用/);
    assert.match(health.textContent, /准备：未准备/);
    assert.match(health.textContent, /运行：启动中/);
    assert.match(health.textContent, /运行：就绪/);
    assert.match(health.textContent, /运行：已停止/);
    assert.match(health.textContent, /运行：异常/);
    assert.match(health.textContent, /运行：停止中/);
    assert.match(health.textContent, /Cache：已过期/);
    assert.match(health.textContent, /Index：异常/);
    assert.match(health.textContent, /Third provider/);
    assert.doesNotMatch(document.querySelector('[data-capability-provider="third"]').textContent, /PID|port|D:\\health/i);
    assert.doesNotMatch(readFileSync("src/ProjectPanel.tsx", "utf8"), /providerId\s*===\s*["'](?:serena|codegraph)["']/i);
  } finally {
    api.workspaceCapabilityObserve = original;
  }
});

test("capability activity supplies cancellation identity, refreshes after terminal feedback, and unlistens on unmount", async () => {
  const original = {
    observe: api.workspaceCapabilityObserve,
    prepare: api.workspaceCapabilityPrepare,
    cancel: api.workspaceCapabilityCancel,
  };
  const workspace = { id: "activity-workspace", name: "Activity", root: "D:/activity", generation: 8 };
  const cancellations = [];
  let rejectPrepare;
  let observes = 0;
  api.workspaceCapabilityObserve = async id => {
    observes++;
    return capabilityHealth(id);
  };
  api.workspaceCapabilityPrepare = async () => new Promise((_resolve, reject) => { rejectPrepare = reject; });
  api.workspaceCapabilityCancel = async operationId => { cancellations.push(operationId); };
  try {
    await renderWorkspacePanel(workspaceController(), managedWorkspaceState([workspace], workspace));
    await act(async () => { await Promise.resolve(); });
    await act(async () => button("Sync third").click());
    assert.match(document.body.textContent, /正在执行/);
    const activityCallback = [...eventCallbacks.values()][0];
    await act(async () => activityCallback({ payload: {
      operationId: "cap-op-third",
      workspaceId: workspace.id,
      providerId: "third",
      actionId: "sync",
      stageCode: "preparing",
      state: "running",
      revision: 1,
      messageCode: "CAPABILITY_ACTION_RUNNING",
    } }));
    await act(async () => button("取消").click());
    assert.deepEqual(cancellations, ["cap-op-third"]);
    assert.equal(button("Sync third").disabled, true, "cancel request keeps the action flight exclusive until terminal feedback");
    rejectPrepare(new Error("cancelled"));
    await act(async () => { await Promise.resolve(); });
    assert.match(document.body.textContent, /已请求取消/);
    assert.ok(observes >= 2, "terminal prepare refreshes Health");
    await act(async () => root.unmount());
    root = null;
    assert.ok(unlistenedEvents.some(([event]) => event === "workspace-capability-activity"));
  } finally {
    api.workspaceCapabilityObserve = original.observe;
    api.workspaceCapabilityPrepare = original.prepare;
    api.workspaceCapabilityCancel = original.cancel;
  }
});

test("capability actions wait for the activity listener so the opaque cancellation identity cannot be missed", async () => {
  const original = api.workspaceCapabilityObserve;
  const workspace = { id: "listener-workspace", name: "Listener", root: "D:/listener", generation: 10 };
  api.workspaceCapabilityObserve = async id => capabilityHealth(id);
  delayedEventListen = {};
  try {
    await renderWorkspacePanel(workspaceController(), managedWorkspaceState([workspace], workspace));
    await act(async () => { await Promise.resolve(); });
    assert.equal(button("Sync third").disabled, true);
    await act(async () => delayedEventListen.resolve());
    assert.equal(button("Sync third").disabled, false);
  } finally {
    delayedEventListen = null;
    api.workspaceCapabilityObserve = original;
  }
});

test("capability action reports success and safe failure before refreshing the current workspace", async () => {
  const original = { observe: api.workspaceCapabilityObserve, prepare: api.workspaceCapabilityPrepare };
  const workspace = { id: "result-workspace", name: "Result", root: "D:/result", generation: 9 };
  const results = [
    { operationId: "cap-op-success", readiness: "ready" },
    new Error("provider details must stay private"),
  ];
  api.workspaceCapabilityObserve = async id => capabilityHealth(id);
  api.workspaceCapabilityPrepare = async () => {
    const result = results.shift();
    if (result instanceof Error) throw result;
    return result;
  };
  try {
    await renderWorkspacePanel(workspaceController(), managedWorkspaceState([workspace], workspace));
    await act(async () => { await Promise.resolve(); });
    await act(async () => button("Sync third").click());
    assert.match(document.body.textContent, /操作已完成/);
    await act(async () => button("Sync third").click());
    assert.match(document.body.textContent, /操作未完成/);
    assert.doesNotMatch(document.body.textContent, /provider details must stay private/);
  } finally {
    api.workspaceCapabilityObserve = original.observe;
    api.workspaceCapabilityPrepare = original.prepare;
  }
});

test("picker success inspects an ordinary directory, while cancel does not inspect or register", async () => {
  const original = {
    picker: api.workspacePickDirectory,
    inspect: api.workspaceInspectDirectory,
    register: api.workspaceRegister,
  };
  const effects = { inspect: [], register: [] };
  const outcomes = ["D:/ordinary-local-directory", null];
  api.workspacePickDirectory = async () => {
    const outcome = outcomes.shift();
    return outcome;
  };
  api.workspaceInspectDirectory = async root => {
    effects.inspect.push(root);
    return { canonicalRoot: "D:/canonical/ordinary-local-directory", folderBasename: "ordinary-local-directory" };
  };
  api.workspaceRegister = async (...args) => {
    effects.register.push(args);
    return { id: "new", name: args[1], root: args[0], generation: 1 };
  };
  try {
    await renderWorkspacePanel(workspaceController());
    await act(async () => button("添加项目").click());
    assert.deepEqual(effects.inspect, ["D:/ordinary-local-directory"]);
    assert.match(document.body.textContent, /D:\/canonical\/ordinary-local-directory/);
    assert.equal(document.querySelector("#workspace-register-name").value, "ordinary-local-directory");

    await act(async () => button("添加项目").click());
    assert.deepEqual(effects.inspect, ["D:/ordinary-local-directory"]);
    assert.equal(document.querySelector("#workspace-register-name").value, "ordinary-local-directory");
    assert.deepEqual(effects.register, []);
  } finally {
    api.workspacePickDirectory = original.picker;
    api.workspaceInspectDirectory = original.inspect;
    api.workspaceRegister = original.register;
  }
});

test("inspect failure keeps the previous candidate and registration failure does not clear it", async () => {
  const original = {
    picker: api.workspacePickDirectory,
    inspect: api.workspaceInspectDirectory,
    register: api.workspaceRegister,
  };
  const roots = ["D:/first", "D:/broken"];
  const registered = [];
  api.workspacePickDirectory = async () => roots.shift();
  api.workspaceInspectDirectory = async root => {
    if (root === "D:/broken") throw new Error("WORKSPACE_ROOT_NOT_FOUND");
    return { canonicalRoot: "D:/canonical/first", folderBasename: "first" };
  };
  api.workspaceRegister = async (...args) => {
    registered.push(args);
    throw new Error("WORKSPACE_ALREADY_REGISTERED");
  };
  try {
    await renderWorkspacePanel();
    await act(async () => button("添加项目").click());
    await act(async () => button("添加项目").click());
    assert.match(document.body.textContent, /D:\/canonical\/first/);

    await act(async () => button("登记项目").click());
    assert.deepEqual(registered, [["D:/canonical/first", "first"]]);
    assert.match(document.body.textContent, /待添加项目/);
    assert.equal(document.querySelector("#workspace-register-name").value, "first");
  } finally {
    api.workspacePickDirectory = original.picker;
    api.workspaceInspectDirectory = original.inspect;
    api.workspaceRegister = original.register;
  }
});

test("registration clears only the confirmed candidate and never selects or activates a workspace", async () => {
  const original = {
    picker: api.workspacePickDirectory,
    inspect: api.workspaceInspectDirectory,
    register: api.workspaceRegister,
    select: api.workspaceSelect,
    activate: api.activateProject,
  };
  const registered = [];
  const selected = [];
  const activated = [];
  api.workspacePickDirectory = async () => "D:/new-project";
  api.workspaceInspectDirectory = async () => ({ canonicalRoot: "D:/canonical/new-project", folderBasename: "new-project" });
  api.workspaceRegister = async (...args) => {
    registered.push(args);
    return { id: "new", name: args[1], root: args[0], generation: 1 };
  };
  api.workspaceSelect = async id => selected.push(id);
  api.activateProject = async id => activated.push(id);
  try {
    await renderWorkspacePanel();
    await act(async () => button("添加项目").click());
    const input = document.querySelector("#workspace-register-name");
    Object.getOwnPropertyDescriptor(dom.window.HTMLInputElement.prototype, "value").set.call(input, "  Custom project  ");
    await act(async () => input.dispatchEvent(new dom.window.Event("input", { bubbles: true })));
    await act(async () => button("登记项目").click());
    assert.deepEqual(registered, [["D:/canonical/new-project", "Custom project"]]);
    assert.equal(document.querySelector("#workspace-register-name"), null);
    assert.deepEqual(selected, []);
    assert.deepEqual(activated, []);
  } finally {
    api.workspacePickDirectory = original.picker;
    api.workspaceInspectDirectory = original.inspect;
    api.workspaceRegister = original.register;
    api.workspaceSelect = original.select;
    api.activateProject = original.activate;
  }
});

test("Serena import uses its explicit IPC action, keeps distinct feedback, and supports added and no-op results", async () => {
  const original = { importSerena: api.workspaceImportSerena, sync: api.syncProjects };
  const imported = [];
  const originalSuccess = toast.success;
  const successMessages = [];
  let legacySyncCalls = 0;
  api.workspaceImportSerena = async () => imported.shift();
  api.syncProjects = async () => {
    legacySyncCalls++;
    return 99;
  };
  toast.success = message => successMessages.push(message);
  imported.push(2, 0);
  try {
    await renderWorkspacePanel();
    await act(async () => button("从 Serena 导入").click());
    assert.match(button("导入中…").textContent, /导入中…/);
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 650)); });
    assert.match(button("已导入").textContent, /已导入/);
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 1600)); });
    await act(async () => button("从 Serena 导入").click());
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 650)); });
    assert.match(button("已导入").textContent, /已导入/);
    assert.equal(legacySyncCalls, 0);
    assert.deepEqual(successMessages, ["已导入 2 个项目", "没有新的 Serena 项目"]);
  } finally {
    api.workspaceImportSerena = original.importSerena;
    api.syncProjects = original.sync;
    toast.success = originalSuccess;
  }
});

test("project help describes explicit additive Serena import without startup synchronization", async () => {
  await renderWorkspacePanel();
  assert.match(document.body.textContent, /普通本地目录，不要求 Git，也不要求已有 \.serena/);
  await act(async () => button("Serena 导入说明").click());
  assert.match(document.body.textContent, /显式追加缺失项目/);
  assert.doesNotMatch(document.body.textContent, /启动时自动同步/);
});

test("workspace rename saves the trimmed name without selecting or activating the workspace", async () => {
  const original = {
    rename: api.workspaceRename,
    select: api.workspaceSelect,
    activate: api.activateProject,
  };
  const first = { id: "first", name: "First", root: "D:/first", generation: 1 };
  const second = { id: "second", name: "Second", root: "D:/second", generation: 2 };
  const renamed = [];
  const selected = [];
  const activated = [];
  api.workspaceRename = async (...args) => {
    renamed.push(args);
    return { ...first, name: args[1] };
  };
  api.workspaceSelect = async id => selected.push(id);
  api.activateProject = async id => activated.push(id);
  try {
    await renderWorkspacePanel(workspaceController(), managedWorkspaceState([first, second], first));
    await act(async () => button("更换工作区").click());
    const row = managedWorkspaceRow("first");
    await act(async () => [...row.querySelectorAll("button")].find(element => element.textContent === "重命名").click());
    const input = document.querySelector("#workspace-rename-first");
    Object.getOwnPropertyDescriptor(dom.window.HTMLInputElement.prototype, "value").set.call(input, "  Renamed first  ");
    await act(async () => input.dispatchEvent(new dom.window.Event("input", { bubbles: true })));
    await act(async () => button("保存").click());
    assert.deepEqual(renamed, [["first", "Renamed first"]]);
    assert.equal(document.querySelector("#workspace-rename-first"), null);
    assert.deepEqual(selected, []);
    assert.deepEqual(activated, []);
  } finally {
    api.workspaceRename = original.rename;
    api.workspaceSelect = original.select;
    api.activateProject = original.activate;
  }
});

test("rename failure retains the row, editor, and input instead of mutating local workspace state", async () => {
  const original = { rename: api.workspaceRename, error: toast.error };
  const first = { id: "first", name: "First", root: "D:/first", generation: 1 };
  const messages = [];
  const controller = workspaceController();
  controller.perform = async (_label, action) => {
    try {
      await action();
    } catch (reason) {
      toast.error(String(reason));
    }
  };
  api.workspaceRename = async () => {
    throw new Error("rename rejected");
  };
  toast.error = message => messages.push(message);
  try {
    await renderWorkspacePanel(controller, managedWorkspaceState([first], first));
    await act(async () => button("更换工作区").click());
    await act(async () => button("重命名").click());
    const input = document.querySelector("#workspace-rename-first");
    Object.getOwnPropertyDescriptor(dom.window.HTMLInputElement.prototype, "value").set.call(input, "Unsaved name");
    await act(async () => input.dispatchEvent(new dom.window.Event("input", { bubbles: true })));
    await act(async () => button("保存").click());
    assert.equal(document.querySelector("#workspace-rename-first").value, "Unsaved name");
    assert.match(managedWorkspaceRow("first").textContent, /保存/);
    assert.deepEqual(messages, ["Error: rename rejected"]);
  } finally {
    api.workspaceRename = original.rename;
    toast.error = original.error;
  }
});

test("duplicate names remain valid for rename while blank names do not send an IPC request", async () => {
  const original = api.workspaceRename;
  const first = { id: "first", name: "First", root: "D:/first", generation: 1 };
  const duplicate = { id: "duplicate", name: "Shared", root: "D:/duplicate", generation: 2 };
  const renamed = [];
  api.workspaceRename = async (...args) => {
    renamed.push(args);
    return { ...first, name: args[1] };
  };
  try {
    await renderWorkspacePanel(workspaceController(), managedWorkspaceState([first, duplicate], first));
    await act(async () => button("更换工作区").click());
    await act(async () => managedWorkspaceButton("first", "重命名").click());
    let input = document.querySelector("#workspace-rename-first");
    Object.getOwnPropertyDescriptor(dom.window.HTMLInputElement.prototype, "value").set.call(input, "Shared");
    await act(async () => input.dispatchEvent(new dom.window.Event("input", { bubbles: true })));
    await act(async () => button("保存").click());
    assert.deepEqual(renamed, [["first", "Shared"]]);

    await act(async () => managedWorkspaceButton("first", "重命名").click());
    input = document.querySelector("#workspace-rename-first");
    Object.getOwnPropertyDescriptor(dom.window.HTMLInputElement.prototype, "value").set.call(input, "   ");
    await act(async () => input.dispatchEvent(new dom.window.Event("input", { bubbles: true })));
    assert.equal(button("保存").disabled, true);
    await act(async () => button("保存").click());
    assert.deepEqual(renamed, [["first", "Shared"]]);
  } finally {
    api.workspaceRename = original;
  }
});

test("remove opens a truthful confirmation before its exact IPC action", async () => {
  const original = api.workspaceRemove;
  const first = { id: "first", name: "First", root: "D:/first", generation: 1 };
  let removeCalls = 0;
  api.workspaceRemove = async () => {
    removeCalls++;
    return first;
  };
  try {
    await renderWorkspacePanel(workspaceController(), managedWorkspaceState([first], first));
    await act(async () => button("更换工作区").click());
    await act(async () => [...managedWorkspaceRow("first").querySelectorAll("button")].find(element => element.textContent === "移除").click());
    const confirmation = document.querySelector('[role="alertdialog"]');
    assert.match(confirmation.textContent, /First/);
    assert.match(confirmation.textContent, /D:\/first/);
    assert.match(confirmation.textContent, /不会删除本地目录、源码、Git 仓库、\.serena 或 \.codegraph 内容/);
    assert.equal(removeCalls, 0);
  } finally {
    api.workspaceRemove = original;
  }
});

test("successful selected removal waits for authoritative refresh and does not select a fallback", async () => {
  const original = { remove: api.workspaceRemove, select: api.workspaceSelect };
  const first = { id: "first", name: "First", root: "D:/first", generation: 1 };
  const second = { id: "second", name: "Second", root: "D:/second", generation: 2 };
  const removed = [];
  const selected = [];
  api.workspaceRemove = async id => {
    removed.push(id);
    return first;
  };
  api.workspaceSelect = async id => selected.push(id);
  const controller = workspaceController();
  try {
    await renderWorkspacePanel(controller, managedWorkspaceState([first, second], first));
    await act(async () => button("更换工作区").click());
    await act(async () => [...managedWorkspaceRow("first").querySelectorAll("button")].find(element => element.textContent === "移除").click());
    await act(async () => button("确认移除").click());
    assert.deepEqual(removed, ["first"]);
    assert.ok(managedWorkspaceRow("first"), "old row remains until the parent refreshes its snapshot");

    await rerenderWorkspacePanel(controller, managedWorkspaceState([second], null));
    assert.equal(managedWorkspaceRow("first"), null);
    assert.match(document.querySelector(".workspace-summary").textContent, /尚未选择工作区/);
    assert.deepEqual(selected, []);
  } finally {
    api.workspaceRemove = original.remove;
    api.workspaceSelect = original.select;
  }
});

test("workspace-in-use removal failure keeps the row and confirmation without extra Agent or Provider actions", async () => {
  const original = {
    remove: api.workspaceRemove,
    agent: api.agent,
    activate: api.activateProject,
    error: toast.error,
  };
  const first = { id: "first", name: "First", root: "D:/first", generation: 1 };
  const messages = [];
  const calls = { agent: 0, activate: 0 };
  const controller = workspaceController();
  controller.perform = async (_label, action) => {
    try {
      await action();
    } catch (reason) {
      toast.error(String(reason));
    }
  };
  api.workspaceRemove = async () => {
    throw new Error("WORKSPACE_IN_USE");
  };
  api.agent = async () => {
    calls.agent++;
  };
  api.activateProject = async () => {
    calls.activate++;
  };
  toast.error = message => messages.push(message);
  try {
    await renderWorkspacePanel(controller, managedWorkspaceState([first], first));
    await act(async () => button("更换工作区").click());
    await act(async () => [...managedWorkspaceRow("first").querySelectorAll("button")].find(element => element.textContent === "移除").click());
    await act(async () => button("确认移除").click());
    assert.ok(managedWorkspaceRow("first"));
    assert.ok(document.querySelector('[role="alertdialog"]'));
    assert.deepEqual(calls, { agent: 0, activate: 0 });
    assert.deepEqual(messages, ["Error: 项目正在被 Agent 任务使用，当前不能移除"]);
  } finally {
    api.workspaceRemove = original.remove;
    api.agent = original.agent;
    api.activateProject = original.activate;
    toast.error = original.error;
  }
});

test("middle workspace moves up and down with complete adjacent ID permutations", async () => {
  const original = api.workspaceReorder;
  const first = { id: "A", name: "A", root: "D:/a", generation: 1 };
  const middle = { id: "B", name: "B", root: "D:/b", generation: 2 };
  const last = { id: "C", name: "C", root: "D:/c", generation: 3 };
  const reordered = [];
  api.workspaceReorder = async ids => {
    reordered.push(ids);
    return { registryRevision: 2, workspaces: [first, middle, last] };
  };
  try {
    await renderWorkspacePanel(workspaceController(), managedWorkspaceState([first, middle, last], middle));
    await act(async () => button("更换工作区").click());
    await act(async () => managedWorkspaceButton("B", "上移").click());
    await act(async () => managedWorkspaceButton("B", "下移").click());
    assert.deepEqual(reordered, [["B", "A", "C"], ["A", "C", "B"]]);
  } finally {
    api.workspaceReorder = original;
  }
});

test("first and last workspace ordering boundaries are disabled without IPC", async () => {
  const original = api.workspaceReorder;
  const first = { id: "A", name: "A", root: "D:/a", generation: 1 };
  const middle = { id: "B", name: "B", root: "D:/b", generation: 2 };
  const last = { id: "C", name: "C", root: "D:/c", generation: 3 };
  let calls = 0;
  api.workspaceReorder = async () => {
    calls++;
    return { registryRevision: 2, workspaces: [first, middle, last] };
  };
  try {
    await renderWorkspacePanel(workspaceController(), managedWorkspaceState([first, middle, last], middle));
    await act(async () => button("更换工作区").click());
    assert.equal(managedWorkspaceButton("A", "上移").disabled, true);
    assert.equal(managedWorkspaceButton("C", "下移").disabled, true);
    await act(async () => managedWorkspaceButton("A", "上移").click());
    await act(async () => managedWorkspaceButton("C", "下移").click());
    assert.equal(calls, 0);
  } finally {
    api.workspaceReorder = original;
  }
});

test("reorder keeps the old DOM order until the parent rerenders its authoritative snapshot", async () => {
  const original = api.workspaceReorder;
  const first = { id: "A", name: "A", root: "D:/a", generation: 1 };
  const middle = { id: "B", name: "B", root: "D:/b", generation: 2 };
  const last = { id: "C", name: "C", root: "D:/c", generation: 3 };
  const controller = workspaceController();
  api.workspaceReorder = async () => ({ registryRevision: 2, workspaces: [middle, first, last] });
  try {
    await renderWorkspacePanel(controller, managedWorkspaceState([first, middle, last], middle));
    await act(async () => button("更换工作区").click());
    await act(async () => managedWorkspaceButton("B", "上移").click());
    assert.deepEqual(managedWorkspaceOrder(), ["A", "B", "C"]);

    await rerenderWorkspacePanel(controller, managedWorkspaceState([middle, first, last], middle));
    assert.deepEqual(managedWorkspaceOrder(), ["B", "A", "C"]);
    assert.match(document.querySelector(".workspace-summary").textContent, /B/);
    assert.match(document.querySelector(".workspace-summary").textContent, /已选择/);
  } finally {
    api.workspaceReorder = original;
  }
});

test("reorder failure retains order and selection without Desktop, Agent, or Provider calls", async () => {
  const original = {
    reorder: api.workspaceReorder,
    select: api.workspaceSelect,
    activate: api.activateProject,
    agent: api.agent,
    error: toast.error,
  };
  const first = { id: "A", name: "A", root: "D:/a", generation: 1 };
  const middle = { id: "B", name: "B", root: "D:/b", generation: 2 };
  const last = { id: "C", name: "C", root: "D:/c", generation: 3 };
  const messages = [];
  const calls = { select: 0, activate: 0, agent: 0 };
  const controller = workspaceController();
  controller.perform = async (_label, action) => {
    try {
      await action();
    } catch (reason) {
      toast.error(String(reason));
    }
  };
  api.workspaceReorder = async () => {
    throw new Error("reorder rejected");
  };
  api.workspaceSelect = async () => { calls.select++; };
  api.activateProject = async () => { calls.activate++; };
  api.agent = async () => { calls.agent++; };
  toast.error = message => messages.push(message);
  try {
    await renderWorkspacePanel(controller, managedWorkspaceState([first, middle, last], middle));
    await act(async () => button("更换工作区").click());
    await act(async () => managedWorkspaceButton("B", "上移").click());
    assert.deepEqual(managedWorkspaceOrder(), ["A", "B", "C"]);
    assert.match(document.querySelector(".workspace-summary").textContent, /B/);
    assert.deepEqual(calls, { select: 0, activate: 0, agent: 0 });
    assert.deepEqual(messages, ["Error: reorder rejected"]);
  } finally {
    api.workspaceReorder = original.reorder;
    api.workspaceSelect = original.select;
    api.activateProject = original.activate;
    api.agent = original.agent;
    toast.error = original.error;
  }
});

test("project selection calls the Desktop selection action and never activates the Broker workspace", async () => {
  const original = { activate: api.activateProject, select: api.workspaceSelect };
  const selected = [];
  const activated = [];
  const first = { id: "A", name: "Desktop A", root: "D:/a", generation: 1 };
  const second = { id: "B", name: "Desktop B", root: "D:/b", generation: 2 };
  api.activateProject = async id => activated.push(id);
  api.workspaceSelect = async id => selected.push(id);
  const state = {
    config: { workspaces: [first, second], desktopSelectedWorkspaceId: first.id },
    desktopSelectedWorkspace: first,
    serverStatus: "stopped",
    activeInstallation: null,
    installation: null,
    git: { available: false, status: "missing", version: null },
  };
  const controller = {
    broker: { running: false, projects: [second], activeWorkspace: second, codegraph: null, operation: null, projectSources: [], syncWarnings: [] },
    busy: "",
    perform: async (_label, action) => action(),
  };
  try {
    root = createRoot(document.getElementById("root"));
    await act(async () => {
      root.render(createElement(ProjectPanel, {
        state,
        controller,
        onSettings() {},
        onRemote() {},
        onSerena() {},
        onSelectWorkspace: async id => api.workspaceSelect(id),
        onCopied() {},
      }));
    });
    await act(async () => [...document.querySelectorAll("button")].find(button => button.textContent === "更换工作区").click());
    await act(async () => document.querySelector("#project-select").click());
    const option = [...document.querySelectorAll('[role="option"]')].find(element => element.textContent.includes("Desktop B"));
    assert.ok(option, "Desktop selection option");
    await act(async () => option.dispatchEvent(new dom.window.MouseEvent("pointerdown", { bubbles: true })));
    await act(async () => option.click());
    await act(async () => [...document.querySelectorAll("button")].find(button => button.textContent === "选择此工作区").click());
    assert.deepEqual(selected, ["B"]);
    assert.deepEqual(activated, []);
  } finally {
    api.activateProject = original.activate;
    api.workspaceSelect = original.select;
  }
});
