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

const button = (label) => [...document.querySelectorAll("button")]
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

test("project selector only presents selection controls, not workspace management", async () => {
  const first = { id: "first", name: "First", root: "D:/first", generation: 1 };
  const second = { id: "second", name: "Second", root: "D:/second", generation: 2 };
  const state = {
    ...workspaceState(),
    config: { workspaces: [first, second], desktopSelectedWorkspaceId: first.id },
    desktopSelectedWorkspace: first,
  };
  await renderWorkspacePanel(workspaceController(), state);
  await act(async () => button("更换工作区").click());
  const dialog = document.querySelector('[role="dialog"]');
  assert.match(dialog.textContent, /待操作项目/);
  assert.doesNotMatch(dialog.textContent, /项目管理|重命名|移除|上移|下移/);
  assert.equal(dialog.querySelector(".workspace-management"), null);
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
