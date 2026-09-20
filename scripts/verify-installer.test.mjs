import assert from "node:assert/strict";
import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import { verifyInstaller } from "./verify-installer.mjs";

/** 使用临时 Tauri root 构建独立 installer fixture，绝不读取或修改真实 artifact。 */
async function withFixture(version, files, run) {
  const root = await mkdtemp(path.join(os.tmpdir(), "serena-installer-verify-"));
  const directory = path.join(root, "src-tauri", "target", "release", "bundle", "nsis");
  try {
    await mkdir(directory, { recursive: true });
    await writeFile(path.join(root, "src-tauri", "tauri.conf.json"), JSON.stringify({ version }));
    for (const [name, content] of files) {
      await writeFile(path.join(directory, name), content);
    }
    await run(root, directory);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
}

/** 断言验证结果含有指定稳定 diagnostic code。 */
function assertCode(result, code) {
  assert.ok(result.diagnostics.some((item) => item.code === code), `missing ${code}`);
}

test("one nonempty current-version x64 NSIS installer passes with SHA-256", async () => {
  await withFixture("1.1.0", [["Serena Desktop_1.1.0_x64-setup.exe", "installer"]], async (root, directory) => {
    const result = await verifyInstaller({ root, directory });
    assert.equal(result.ok, true);
    assert.equal(result.artifact.architecture, "x64");
    assert.equal(result.artifact.version, "1.1.0");
    assert.equal(result.artifact.sha256.length, 64);
  });
});

test("missing installer fails closed", async () => {
  await withFixture("1.1.0", [], async (root, directory) => {
    const result = await verifyInstaller({ root, directory });
    assert.equal(result.ok, false);
    assertCode(result, "INSTALLER_ARTIFACT_MISSING");
  });
});

test("multiple executable artifacts are rejected", async () => {
  await withFixture(
    "1.1.0",
    [["Serena Desktop_1.1.0_x64-setup.exe", "one"], ["other.exe", "two"]],
    async (root, directory) => {
      const result = await verifyInstaller({ root, directory });
      assert.equal(result.ok, false);
      assertCode(result, "INSTALLER_ARTIFACT_AMBIGUOUS");
    },
  );
});

test("wrong version or architecture filename is rejected", async () => {
  await withFixture("1.1.0", [["Serena Desktop_1.1.1_x64-setup.exe", "installer"]], async (root, directory) => {
    const result = await verifyInstaller({ root, directory });
    assert.equal(result.ok, false);
    assertCode(result, "INSTALLER_VERSION_OR_ARCH_INVALID");
  });
});
