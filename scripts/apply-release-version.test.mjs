import assert from "node:assert/strict";
import { mkdtemp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import { applyReleaseVersion, versionFromTag } from "./apply-release-version.mjs";

/** 建立彼此独立的产品清单 fixture，避免测试修改真实工作区。 */
async function withFixture(run) {
  const root = await mkdtemp(path.join(os.tmpdir(), "serena-release-version-"));
  try {
    await mkdir(path.join(root, "src-tauri"));
    await writeFile(path.join(root, "src-tauri", "tauri.conf.json"), JSON.stringify({ productName: "Serena Desktop", version: "1.1.0" }, null, 2));
    await writeFile(path.join(root, "src-tauri", "Cargo.toml"), "[package]\nname = \"fixture\"\nversion = \"1.1.0\"\nedition = \"2024\"\n\n[dependencies]\ntauri = \"2\"\n");
    await writeFile(path.join(root, "src-tauri", "Cargo.lock"), "version = 4\n\n[[package]]\nname = \"fixture-dependency\"\nversion = \"1.1.0\"\n\n[[package]]\nname = \"serena-desktop\"\nversion = \"1.1.0\"\n");
    await writeFile(path.join(root, "package.json"), JSON.stringify({ name: "fixture", version: "1.1.0" }, null, 2));
    await run(root);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
}

test("release tag synchronizes all build product manifests", async () => {
  await withFixture(async (root) => {
    const result = await applyReleaseVersion({ root, tag: "v1.2.3" });
    assert.equal(result.ok, true);
    assert.deepEqual(
      await Promise.all([
        readFile(path.join(root, "src-tauri", "tauri.conf.json"), "utf8"),
        readFile(path.join(root, "src-tauri", "Cargo.toml"), "utf8"),
        readFile(path.join(root, "src-tauri", "Cargo.lock"), "utf8"),
        readFile(path.join(root, "package.json"), "utf8"),
      ]),
      [
        '{\n  "productName": "Serena Desktop",\n  "version": "1.2.3"\n}\n',
        '[package]\nname = "fixture"\nversion = "1.2.3"\nedition = "2024"\n\n[dependencies]\ntauri = "2"\n',
        'version = 4\n\n[[package]]\nname = "fixture-dependency"\nversion = "1.1.0"\n\n[[package]]\nname = "serena-desktop"\nversion = "1.2.3"\n',
        '{\n  "name": "fixture",\n  "version": "1.2.3"\n}\n',
      ],
    );
  });
});

test("invalid release tag fails before modifying product manifests", async () => {
  await withFixture(async (root) => {
    await assert.rejects(() => applyReleaseVersion({ root, tag: "v1.2" }), /Release tag must be vX\.Y\.Z/u);
    assert.match(await readFile(path.join(root, "src-tauri", "Cargo.toml"), "utf8"), /version = "1\.1\.0"/u);
  });
});

test("release tag parser accepts only stable vX.Y.Z tags", () => {
  assert.equal(versionFromTag("v0.0.1"), "0.0.1");
  for (const tag of ["1.2.3", "v1.2", "v1.2.3-beta"]) {
    assert.throws(() => versionFromTag(tag), /Release tag must be vX\.Y\.Z/u);
  }
});
