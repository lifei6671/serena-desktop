# Phase 1B macOS App Bundle Baseline Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` (recommended) or `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让现有 `npm run tauri build` 在 macOS 生成带项目图标、可由 LaunchServices 启动且采用 ad-hoc 签名的 `Serena Desktop.app`，同时保持 Windows NSIS 契约不变。

**Architecture:** 保留 `tauri.conf.json` 作为跨平台基础与 Windows NSIS authority，新增 `tauri.macos.conf.json` 只覆盖 macOS `app` target、最低系统版本和 ad-hoc identity。一个跨平台 Node 契约测试读取两份配置，防止平台 target 再次混合；真实 macOS Gate 验证 `.app`、plist、图标、签名和 LaunchServices 启动。

**Tech Stack:** Tauri 2.11、JSON platform overlay、Node.js `node:test`、macOS `plutil` / `codesign` / `open`。

---

## 文件职责

- Create: `src-tauri/tauri.macos.conf.json` — macOS-only bundle target、最低系统版本和 ad-hoc 签名。
- Create: `scripts/macos-bundle-config.test.mjs` — 冻结 Windows/macOS 配置隔离与 icon 源文件契约。
- Modify: `package.json` — 把上述快速、跨平台配置测试加入标准 `npm test`。
- Modify: `docs/macos-porting-checklist.md` — 更新已经完成的 macOS App Bundle 基线事实，不误标 DMG、公证或最低版本真机验收。
- Modify: `.trellis/tasks/09-21-macos-p1b-app-bundle/prd.md` — 仅按真实 Gate 勾选验收。
- Modify: `.trellis/tasks/09-21-macos-p1b-app-bundle/implement.md` — 跟踪本计划步骤。
- Modify: `.trellis/workspace/codex/journal-1.md` — 记录真实命令、当前主机和延期边界。

## 实施约束

- 不修改 `src-tauri/tauri.conf.json` 的 Windows `targets: ["nsis"]`、WebView、NSIS 或资源字段。
- 不增加 `dmg`、`all`、Developer ID、公证、GitHub Actions 或 Release workflow。
- 不修改 Rust `main.rs` 或窗口生命周期；裸 Mach-O 不是用户入口，问题必须在 bundle 层解决。
- 不生成新图标；继续继承现有 `src-tauri/icons/icon.icns`。
- 不修改 Phase 2B、Provider、StateStore、Claim 或 Recovery。
- 所有新增函数和核心逻辑使用中文注释。
- 实现与 Trellis 收口分别提交；不推送远端。

## Task 1：用失败测试冻结平台配置契约

**Files:**

- Create: `scripts/macos-bundle-config.test.mjs`
- Create: `src-tauri/tauri.macos.conf.json`

- [ ] **Step 1：创建真实配置契约测试**

新增 `scripts/macos-bundle-config.test.mjs`：

```javascript
import assert from "node:assert/strict";
import { access, readFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

/** 读取仓库内 JSON 配置，解析失败应直接使契约测试失败。 */
async function readJson(relativePath) {
  return JSON.parse(await readFile(path.join(root, relativePath), "utf8"));
}

test("macOS app bundle overlay stays isolated from the Windows NSIS authority", async () => {
  const [base, macos] = await Promise.all([
    readJson("src-tauri/tauri.conf.json"),
    readJson("src-tauri/tauri.macos.conf.json"),
  ]);

  assert.deepEqual(base.bundle.targets, ["nsis"]);
  assert.deepEqual(macos.bundle.targets, ["app"]);
  assert.equal(macos.bundle.macOS.minimumSystemVersion, "12.0");
  assert.equal(macos.bundle.macOS.signingIdentity, "-");
  assert.equal(Object.hasOwn(macos.bundle, "windows"), false);
  assert.equal(macos.bundle.targets.includes("dmg"), false);
  assert.ok(base.bundle.icon.includes("icons/icon.icns"));
  await access(path.join(root, "src-tauri", "icons", "icon.icns"));
});
```

- [ ] **Step 2：运行测试并确认 RED 来自缺少 macOS overlay**

Run:

```bash
node --test scripts/macos-bundle-config.test.mjs
```

Expected: FAIL；`src-tauri/tauri.macos.conf.json` 不存在，错误为 `ENOENT`。基础 Windows 配置和 icon 文件读取不应失败。

- [ ] **Step 3：添加最小 macOS 平台配置**

新增 `src-tauri/tauri.macos.conf.json`：

```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "bundle": {
    "targets": ["app"],
    "macOS": {
      "minimumSystemVersion": "12.0",
      "signingIdentity": "-"
    }
  }
}
```

- [ ] **Step 4：验证配置测试转绿**

Run:

```bash
node --test scripts/macos-bundle-config.test.mjs
```

Expected: 1 passed、0 failed。

## Task 2：把配置回归加入标准前端 Gate

**Files:**

- Modify: `package.json`

- [ ] **Step 1：扩展现有 test script，不引入新 runner**

把：

```json
"test": "node --test src/*.test.mjs"
```

改为：

```json
"test": "node --test src/*.test.mjs scripts/macos-bundle-config.test.mjs"
```

- [ ] **Step 2：运行标准前端 Gate**

Run:

```bash
npm run lint
npm run build
npm test
```

Expected: 全部 exit 0；`npm test` 为原有 116 项加新增 1 项，共 117 passed、0 failed。

## Task 3：构建并验证真实 App Bundle

**Files:**

- Verify: `src-tauri/target/release/bundle/macos/Serena Desktop.app`

- [ ] **Step 1：使用用户原命令构建**

Run:

```bash
npm run tauri build
```

Expected: exit 0；输出包含 `Bundling Serena Desktop.app` 或等价 macOS bundle 阶段，并报告 `.app` 路径，不再只报告裸 `target/release/serena-desktop`。

- [ ] **Step 2：验证 bundle 类型、身份、最低版本和 icon**

Run:

```bash
MACOS_APP_BUNDLE="src-tauri/target/release/bundle/macos/Serena Desktop.app"
test -d "$MACOS_APP_BUNDLE"
test "$(plutil -extract CFBundlePackageType raw "$MACOS_APP_BUNDLE/Contents/Info.plist")" = "APPL"
test "$(plutil -extract CFBundleIdentifier raw "$MACOS_APP_BUNDLE/Contents/Info.plist")" = "io.github.lifei6671.serena-desktop"
test "$(plutil -extract LSMinimumSystemVersion raw "$MACOS_APP_BUNDLE/Contents/Info.plist")" = "12.0"
MACOS_ICON_FILE="$(plutil -extract CFBundleIconFile raw "$MACOS_APP_BUNDLE/Contents/Info.plist")"
test -n "$MACOS_ICON_FILE"
test -f "$MACOS_APP_BUNDLE/Contents/Resources/$MACOS_ICON_FILE"
file "$MACOS_APP_BUNDLE/Contents/Resources/$MACOS_ICON_FILE"
```

Expected: 所有 `test` exit 0；`file` 报告 Mac OS X icon。若 plist 返回不带 `.icns` 的逻辑名，只允许在实际 bundle 同名 `.icns` 存在时把验证解析为该文件，不修改产品图标命名来迎合测试。

- [ ] **Step 3：验证 ad-hoc 签名**

Run:

```bash
MACOS_APP_BUNDLE="src-tauri/target/release/bundle/macos/Serena Desktop.app"
codesign --verify --deep --strict "$MACOS_APP_BUNDLE"
codesign -dv --verbose=4 "$MACOS_APP_BUNDLE" 2>&1 | rg "Signature=adhoc"
```

Expected: 两条命令 exit 0；签名明确为 ad-hoc，不出现 Developer ID identity。

- [ ] **Step 4：通过 LaunchServices 启动并只清理该构建实例**

Run:

```bash
MACOS_APP_BUNDLE="$(pwd)/src-tauri/target/release/bundle/macos/Serena Desktop.app"
open -na "$MACOS_APP_BUNDLE"
MACOS_APP_PID=""
for _ in {1..50}; do
  MACOS_APP_PID="$(pgrep -nf "^$MACOS_APP_BUNDLE/Contents/MacOS/serena-desktop( |$)" || true)"
  if [ -n "$MACOS_APP_PID" ]; then break; fi
  sleep 0.1
done
test -n "$MACOS_APP_PID"
ps -p "$MACOS_APP_PID" -o command= | rg -F "$MACOS_APP_BUNDLE/Contents/MacOS/serena-desktop"
kill -TERM "$MACOS_APP_PID"
for _ in {1..50}; do
  if ! kill -0 "$MACOS_APP_PID" 2>/dev/null; then break; fi
  sleep 0.1
done
! kill -0 "$MACOS_APP_PID" 2>/dev/null
```

Expected: `open` 通过 LaunchServices 启动 `.app/Contents/MacOS/serena-desktop`，而不是 Terminal 启动裸 release 文件；只终止从该绝对 bundle 路径观测到的 PID，最终进程退出。人工同时确认 Finder/Dock 显示项目 icon；若 Finder 缓存仍显示旧图标，只记录缓存现象，不修改资源生成策略。

## Task 4：范围审计、文档与 Trellis 收口

**Files:**

- Modify: `docs/macos-porting-checklist.md`
- Modify: `.trellis/tasks/09-21-macos-p1b-app-bundle/prd.md`
- Modify: `.trellis/tasks/09-21-macos-p1b-app-bundle/implement.md`
- Modify: `.trellis/workspace/codex/journal-1.md`

- [ ] **Step 1：更新清单中的真实状态**

仅做以下文档变化：

- 在“当前基线”中说明 Windows 基础配置仍为 NSIS，macOS 已有独立 `app` overlay；CI/Release 仍只覆盖 Windows。
- 在“关键证据位置”增加 `src-tauri/tauri.macos.conf.json`。
- 勾选 Phase 5 的“创建 macOS 平台配置，避免直接将全局 targets 从 nsis 改成影响 Windows 的值”。
- 不勾选 DMG、Developer ID、公证、最低 macOS 12 真机、Finder/Dock/菜单栏完整视觉验收。

- [ ] **Step 2：运行完整相关 Gate**

Run:

```bash
npm run lint
npm run build
npm test
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo check --manifest-path src-tauri/Cargo.toml --locked
cargo test --manifest-path src-tauri/Cargo.toml --locked
git diff --exit-code HEAD -- src-tauri/tauri.conf.json .github/workflows scripts/verify-installer.mjs scripts/verify-uninstall-policy.mjs
git diff --check
python3 ./.trellis/scripts/task.py validate .trellis/tasks/09-21-macos-p1b-app-bundle
```

Expected: 全部 exit 0；Windows authority、workflow 和 installer verifier 无 diff；工作区只包含当前任务范围。

- [ ] **Step 3：更新验收记录但保持延期边界**

只在真实命令通过后勾选 `prd.md`。Journal 记录：

- 原始失败复现：`npm run tauri build` 只生成裸 Mach-O；
- 修复后 `.app`、plist、icon、ad-hoc 签名与 LaunchServices 结果；
- 当前测试主机架构和 macOS 版本；
- DMG、Developer ID、公证、CI 和最低 macOS 12 真机仍延期；
- 下一任务恢复为 Phase 2B StateStore/Recovery。

- [ ] **Step 4：提交前向用户展示两提交计划并确认**

```text
fix(macos): build app bundle with platform icon
chore(trellis): complete macos app bundle baseline
```

第一笔包含 `tauri.macos.conf.json`、配置测试、`package.json` 和清单更新；第二笔包含任务验收、归档和 session。均不推送远端。

- [ ] **Step 5：确认后提交、归档并记录 session**

按 Trellis Phase 3.4：先提交实现；再归档本任务，以实现提交 hash 记录 session，并创建独立 Trellis 收口提交。最后确认 `git status --short` 为空，随后开始 Phase 2B 设计。
