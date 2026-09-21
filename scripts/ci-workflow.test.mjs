import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

/** 读取实际 CI workflow 文本，固定正式发布前的 Windows 质量 Gate。 */
async function readWorkflow() {
  return readFile(path.join(root, ".github", "workflows", "ci.yml"), "utf8");
}

test("CI workflow runs the complete Windows quality gate without a tag", async () => {
  const workflow = await readWorkflow();

  assert.match(workflow, /^on:\s*\r?\n\s+workflow_dispatch:/mu);
  assert.match(workflow, /push:\s*\r?\n\s+branches:\s*\r?\n\s+- master/u);
  assert.match(workflow, /pull_request:\s*\r?\n\s+branches:\s*\r?\n\s+- master/u);
  assert.match(workflow, /permissions:\s*\r?\n\s+contents: read/u);
  assert.match(
    workflow,
    /concurrency:\s*\r?\n\s+group: ci-\$\{\{ github\.workflow \}\}-\$\{\{ github\.ref \}\}\s*\r?\n\s+cancel-in-progress: true/u,
  );
  assert.match(workflow, /runs-on: windows-2022/u);
  assert.match(workflow, /timeout-minutes: 45/u);
  assert.match(workflow, /shell: pwsh/u);
  assert.match(workflow, /RUSTUP_TOOLCHAIN: stable/u);
  assert.match(workflow, /uses: actions\/checkout@v6/u);
  assert.match(workflow, /persist-credentials: false/u);
  assert.match(workflow, /uses: actions\/setup-node@v6/u);
  assert.match(workflow, /node-version: '22'/u);
  assert.match(workflow, /cache: npm/u);
  assert.match(workflow, /rustup toolchain install stable --profile minimal/u);

  for (const command of [
    "npm ci",
    "npm run lint",
    "npm run build",
    "npm test",
    "node --test scripts/release-workflow.test.mjs scripts/ci-workflow.test.mjs",
    "cargo fmt --all -- --check",
    "cargo check --locked",
    "cargo clippy --locked --all-targets -- -D warnings",
    "cargo test --locked",
    "git diff --check",
    "node scripts/verify-uninstall-policy.mjs",
  ]) {
    assert.ok(workflow.includes(command), `missing CI command: ${command}`);
  }

  assert.equal(
    (workflow.match(/working-directory: src-tauri/gu) ?? []).length,
    4,
    "every Rust quality gate must run from src-tauri",
  );
  assert.doesNotMatch(workflow, /\btags\s*:/u);
  assert.doesNotMatch(workflow, /softprops\/action-gh-release/u);
  assert.doesNotMatch(workflow, /apply-release-version/u);
  assert.doesNotMatch(workflow, /check-version\.mjs\s+--tag/u);
  assert.doesNotMatch(workflow, /tauri(?:\.cmd)?\s+build/u);
  assert.doesNotMatch(workflow, /continue-on-error\s*:/u);
  assert.doesNotMatch(workflow, /\|\|\s*true/u);
});
