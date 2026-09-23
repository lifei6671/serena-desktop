import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

/** 按顶层 job 边界读取 workflow，避免某个平台的步骤满足另一个平台的断言。 */
async function readJobs() {
  const workflow = await readFile(path.join(root, ".github", "workflows", "release.yml"), "utf8");
  const sections = [...workflow.matchAll(/^  (release-windows|release-macos|publish):\s*$/gmu)];
  const jobs = Object.fromEntries(sections.map((match, index) => [
    match[1], workflow.slice(match.index, sections[index + 1]?.index ?? workflow.length),
  ]));
  return { workflow, jobs };
}

test("release is tag-only with two complete platform gates and one publish job", async () => {
  const { workflow, jobs } = await readJobs();
  assert.match(workflow, /^on:\s*\r?\n\s+push:\s*\r?\n\s+tags:\s*\r?\n\s+- 'v\*'/mu);
  assert.match(workflow, /^permissions:\s*\r?\n  contents: read\s*$/mu);
  assert.match(jobs.publish, /^    permissions:\s*\r?\n      contents: write\s*$/mu);
  assert.doesNotMatch(jobs["release-windows"], /contents: write/u);
  assert.doesNotMatch(jobs["release-macos"], /contents: write/u);
  assert.equal((workflow.match(/contents: write/gu) ?? []).length, 1);
  assert.equal((workflow.match(/softprops\/action-gh-release@v3/gu) ?? []).length, 1);
  assert.match(jobs["release-windows"], /runs-on: windows-2022/u);
  assert.match(jobs["release-macos"], /runs-on: macos-14/u);
  assert.match(jobs.publish, /needs: \[release-windows, release-macos\]/u);

  const common = [
    "npm ci", "npm run lint", "npm run build", "npm test",
    "node --test scripts/apply-release-version.test.mjs scripts/check-version.test.mjs scripts/ci-workflow.test.mjs scripts/release-workflow.test.mjs scripts/macos-bundle-config.test.mjs scripts/verify-installer.test.mjs scripts/verify-macos-release.test.mjs scripts/verify-uninstall-policy.test.mjs",
    "cargo fmt --all -- --check", "cargo check --locked",
    "cargo clippy --locked --all-targets -- -D warnings", "cargo test --locked",
    "git diff --check", "actions/setup-node@v6", "node-version: '22'",
    "VITE_APP_VERSION: ${{ github.ref_name }}", "persist-credentials: false",
  ];
  for (const name of ["release-windows", "release-macos"]) {
    const job = jobs[name];
    for (const value of common) assert.ok(job.includes(value), `${name} missing ${value}`);
    assert.equal((job.match(/working-directory: src-tauri/gu) ?? []).length, 4);
    assert.equal((job.match(/node scripts\/check-version\.mjs --tag/gu) ?? []).length, 1);
    assert.equal((job.match(/node scripts\/apply-release-version\.mjs --tag/gu) ?? []).length, 1);
  }
  assert.match(jobs["release-windows"], /node scripts\/verify-uninstall-policy\.mjs/u);
  assert.doesNotMatch(jobs["release-macos"], /run: node scripts\/verify-uninstall-policy\.mjs/u);
  assert.match(jobs["release-macos"], /run: test "\$\(uname -m\)" = arm64/u);
  assert.doesNotMatch(workflow, /continue-on-error\s*:|\|\|\s*true|--no-bundle|x86_64|universal/u);
});

test("release uploads only verified NSIS and DMG to one publisher", async () => {
  const { workflow, jobs } = await readJobs();
  const windows = jobs["release-windows"];
  const macos = jobs["release-macos"];
  const publish = jobs.publish;
  assert.match(windows, /run: \.\\node_modules\\\.bin\\tauri\.cmd build --ci -- --locked/u);
  assert.match(windows, /node scripts\/verify-installer\.mjs/u);
  assert.match(windows, /name: verified-windows-nsis/u);
  assert.match(windows, /path: \$\{\{ steps\.installer\.outputs\.path \}\}/u);
  assert.match(macos, /run: \.\/node_modules\/\.bin\/tauri build --ci -- --locked/u);
  assert.match(macos, /node scripts\/verify-macos-release\.mjs/u);
  assert.match(macos, /name: verified-macos-dmg/u);
  assert.match(macos, /path: \$\{\{ steps\.dmg\.outputs\.path \}\}/u);
  assert.match(publish, /actions\/download-artifact@v4/gu);
  assert.match(publish, /release-assets\/windows\/\*-setup\.exe/u);
  assert.match(publish, /release-assets\/macos\/\*\.dmg/u);
  assert.match(publish, /fail_on_unmatched_files: true/u);
  assert.match(publish, /generate_release_notes: true/u);
  assert.match(publish, /test "\$\{#windows\[@\]\}" -eq 1/u);
  assert.match(publish, /test "\$\{#macos\[@\]\}" -eq 1/u);
  assert.doesNotMatch(publish, /\.app(?:\s|$)|serena-desktop\.exe|bare/u);
  assert.equal((workflow.match(/sha256:/gu) ?? []).length, 2);
  assert.equal((workflow.match(/- filename:/gu) ?? []).length, 2);
  assert.equal((workflow.match(/- size:/gu) ?? []).length, 2);
});
