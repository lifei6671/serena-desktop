import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

/** 读取实际 workflow 文本，固定 P6-006 的关键发布契约。 */
async function readWorkflow() {
  return readFile(path.join(root, ".github", "workflows", "release.yml"), "utf8");
}

test("release workflow is tag-only and runs the full quality and version gates", async () => {
  const workflow = await readWorkflow();
  assert.match(workflow, /tags:\s*\r?\n\s*- 'v\*'/u);
  assert.match(workflow, /node-version: '22'/u);
  for (const command of [
    "npm ci",
    "npm run lint",
    "npm run build",
    "npm test",
    "node --test scripts/apply-release-version.test.mjs scripts/check-version.test.mjs scripts/release-workflow.test.mjs scripts/verify-installer.test.mjs scripts/verify-uninstall-policy.test.mjs",
    "cargo fmt --all -- --check",
    "cargo check --locked",
    "cargo clippy --locked --all-targets -- -D warnings",
    "cargo test --locked",
    "git diff --check",
    "node scripts/check-version.mjs --tag \"$env:RELEASE_TAG\"",
  ]) {
    assert.ok(workflow.includes(command), `missing release command: ${command}`);
  }
  assert.doesNotMatch(workflow, /continue-on-error\s*:/u);
  assert.doesNotMatch(workflow, /\|\|\s*true/u);
  assert.match(workflow, /VITE_APP_VERSION: \$\{\{ github\.ref_name \}\}/u);
  assert.match(workflow, /node scripts\/apply-release-version\.mjs --tag "\$env:RELEASE_TAG"/u);
});

test("release workflow builds and publishes only the verified NSIS installer", async () => {
  const workflow = await readWorkflow();
  assert.match(workflow, /npm run tauri -- build --ci -- --locked/u);
  assert.match(workflow, /node scripts\/verify-installer\.mjs/u);
  assert.match(workflow, /node scripts\/verify-uninstall-policy\.mjs/u);
  assert.match(workflow, /files: \$\{\{ steps\.installer\.outputs\.path \}\}/u);
  assert.doesNotMatch(workflow, /--no-bundle/u);
  assert.doesNotMatch(workflow, /target\/release\/serena-desktop\.exe/u);
});
