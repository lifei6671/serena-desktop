import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

/** 按顶层 job 边界读取 CI，防止一个平台的命令误满足另一个平台的断言。 */
async function readJobs() {
  const workflow = await readFile(path.join(root, ".github", "workflows", "ci.yml"), "utf8");
  const body = workflow.slice(workflow.indexOf("\njobs:\n") + "\njobs:\n".length);
  const sections = [...body.matchAll(/^  ([a-z][a-z-]*):\s*$/gmu)];
  const jobs = Object.fromEntries(sections.map((match, index) => [
    match[1], body.slice(match.index, sections[index + 1]?.index ?? body.length),
  ]));
  return { workflow, jobs };
}

test("CI workflow runs the complete Windows and Apple Silicon quality gate without a tag", async () => {
  const { workflow, jobs } = await readJobs();

  assert.match(workflow, /^on:\s*\r?\n\s+workflow_dispatch:/mu);
  assert.match(workflow, /push:\s*\r?\n\s+branches:\s*\r?\n\s+- master/u);
  assert.match(workflow, /pull_request:\s*\r?\n\s+branches:\s*\r?\n\s+- master/u);
  assert.match(workflow, /permissions:\s*\r?\n\s+contents: read/u);
  assert.match(
    workflow,
    /concurrency:\s*\r?\n\s+group: ci-\$\{\{ github\.workflow \}\}-\$\{\{ github\.ref \}\}\s*\r?\n\s+cancel-in-progress: true/u,
  );
  assert.deepEqual(Object.keys(jobs), ["quality-windows", "quality-macos"]);
  assert.match(jobs["quality-windows"], /runs-on: windows-2022/u);
  assert.match(jobs["quality-windows"], /shell: pwsh/u);
  assert.match(jobs["quality-macos"], /runs-on: macos-14/u);
  assert.match(jobs["quality-macos"], /shell: bash/u);
  assert.match(jobs["quality-macos"], /run: test "\$\(uname -m\)" = arm64/u);
  assert.doesNotMatch(jobs["quality-windows"], /uname -m/u);

  for (const command of [
    "npm ci",
    "npm run lint",
    "npm run build",
    "npm test",
    "node --test scripts/release-workflow.test.mjs scripts/ci-workflow.test.mjs scripts/macos-bundle-config.test.mjs scripts/verify-macos-release.test.mjs",
    "cargo fmt --all -- --check",
    "cargo check --locked",
    "cargo clippy --locked --all-targets -- -D warnings",
    "cargo test --locked",
    "git diff --check",
  ]) {
    for (const [name, job] of Object.entries(jobs)) {
      assert.ok(job.includes(command), `${name} missing CI command: ${command}`);
    }
  }

  for (const [name, job] of Object.entries(jobs)) {
    for (const value of [
      "timeout-minutes: 90",
      "RUSTUP_TOOLCHAIN: stable",
      "uses: actions/checkout@v6",
      "persist-credentials: false",
      "uses: actions/setup-node@v6",
      "node-version: '22'",
      "cache: npm",
      "rustup toolchain install stable --profile minimal",
    ]) {
      assert.ok(job.includes(value), `${name} missing CI setup: ${value}`);
    }
    assert.equal((job.match(/working-directory: src-tauri/gu) ?? []).length, 4,
      `${name} must run every Rust quality gate from src-tauri`);
  }
  assert.match(jobs["quality-windows"], /run: node scripts\/verify-uninstall-policy\.mjs/u);
  assert.doesNotMatch(jobs["quality-macos"], /verify-uninstall-policy/u);
  assert.doesNotMatch(workflow, /matrix\.|strategy:/u);
  assert.doesNotMatch(workflow, /\btags\s*:/u);
  assert.doesNotMatch(workflow, /softprops\/action-gh-release/u);
  assert.doesNotMatch(workflow, /apply-release-version/u);
  assert.doesNotMatch(workflow, /check-version\.mjs\s+--tag/u);
  assert.doesNotMatch(workflow, /tauri(?:\.cmd)?\s+build/u);
  assert.doesNotMatch(workflow, /continue-on-error\s*:/u);
  assert.doesNotMatch(workflow, /\|\|\s*true/u);
  assert.doesNotMatch(workflow, /x86_64|universal|--no-bundle/u);
});
