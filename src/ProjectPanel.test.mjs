import assert from "node:assert/strict";
import { readFileSync, existsSync } from "node:fs";
import { fileURLToPath, pathToFileURL } from "node:url";
import { registerHooks } from "node:module";
import path from "node:path";
import test, { afterEach } from "node:test";
import ts from "typescript";
import { JSDOM } from "jsdom";

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

test("directory selection is a local candidate and cancel or failure preserves it without project side effects", async () => {
  const original = {
    picker: api.workspacePickDirectory,
    sync: api.syncProjects,
    activate: api.activateProject,
  };
  const effects = { sync: 0, activate: 0 };
  const outcomes = ["D:/picked-directory", null, new Error("picker unavailable")];
  api.workspacePickDirectory = async () => {
    const outcome = outcomes.shift();
    if (outcome instanceof Error) throw outcome;
    return outcome;
  };
  api.syncProjects = async () => {
    effects.sync += 1;
    return 0;
  };
  api.activateProject = async () => {
    effects.activate += 1;
  };
  const state = {
    serverStatus: "stopped",
    activeInstallation: null,
    installation: null,
    git: { available: false, status: "missing", version: null },
  };
  const controller = {
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
        onCopied() {},
      }));
    });
    const picker = () => [...document.querySelectorAll("button")]
      .find((button) => button.textContent === "添加项目");

    await act(async () => picker().click());
    assert.match(document.body.textContent, /待登记目录：/);
    assert.match(document.body.textContent, /D:\/picked-directory/);

    await act(async () => picker().click());
    assert.match(document.body.textContent, /D:\/picked-directory/);

    await act(async () => picker().click());
    assert.match(document.body.textContent, /D:\/picked-directory/);
    assert.deepEqual(effects, { sync: 0, activate: 0 });
  } finally {
    api.workspacePickDirectory = original.picker;
    api.syncProjects = original.sync;
    api.activateProject = original.activate;
  }
});
