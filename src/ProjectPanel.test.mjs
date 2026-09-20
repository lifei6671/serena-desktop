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
dom.window.__TAURI_INTERNALS__ = {
  invoke: async (command, args) => {
    invocations.push([command, args]);
    return "D:/picked-directory";
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

  assert.deepEqual(invocations, [
    ["workspace_inspect_directory", { root: "D:/picked-directory" }],
    ["workspace_register", { root: "D:/canonical-directory", name: "Chosen name" }],
    ["workspace_import_serena", {}],
    ["workspace_rename", { id: "project-1", name: "Renamed project" }],
    ["workspace_remove", { id: "project-2" }],
    ["workspace_reorder", { ids: ["project-2", "project-1"] }],
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
