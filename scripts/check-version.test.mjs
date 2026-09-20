import assert from "node:assert/strict";
import { mkdtemp, rm, mkdir, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import { checkVersion, formatReport, parseArguments } from "./check-version.mjs";

/** 在临时目录写入三份版本 source，绝不改动真实产品文件。 */
async function withFixture(versions, run) {
  const root = await mkdtemp(path.join(os.tmpdir(), "serena-version-gate-"));
  try {
    await mkdir(path.join(root, "src-tauri"));
    await writeFile(
      path.join(root, "src-tauri", "tauri.conf.json"),
      JSON.stringify({ version: versions.tauri }),
    );
    await writeFile(
      path.join(root, "src-tauri", "Cargo.toml"),
      versions.cargoToml ?? `[package]\nname = "fixture"\nversion = "${versions.cargo}"\n`,
    );
    await writeFile(
      path.join(root, "package.json"),
      versions.packageJson ?? JSON.stringify({ version: versions.npm }),
    );
    await run(root);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
}

/** 断言某个稳定 diagnostic code 被 Gate 返回。 */
function assertCode(result, code) {
  assert.ok(result.diagnostics.some((item) => item.code === code), `missing ${code}`);
}

test("matching files and a matching v tag pass", async () => {
  await withFixture({ tauri: "1.1.0", cargo: "1.1.0", npm: "1.1.0" }, async (root) => {
    const result = await checkVersion({ root, tag: "v1.1.0" });
    assert.equal(result.ok, true);
    assert.equal(result.diagnostics.length, 0);
  });
});

test("cargo mismatch fails with VERSION_MISMATCH", async () => {
  await withFixture({ tauri: "1.1.0", cargo: "0.3.0", npm: "1.1.0" }, async (root) => {
    const result = await checkVersion({ root });
    assert.equal(result.ok, false);
    assertCode(result, "VERSION_MISMATCH");
    assert.match(formatReport(result), /source=cargo expected=1\.1\.0 actual=0\.3\.0/u);
  });
});

test("npm mismatch fails with VERSION_MISMATCH", async () => {
  await withFixture({ tauri: "1.1.0", cargo: "1.1.0", npm: "0.1.0" }, async (root) => {
    const result = await checkVersion({ root });
    assert.equal(result.ok, false);
    assert.match(formatReport(result), /source=npm expected=1\.1\.0 actual=0\.1\.0/u);
  });
});

test("cargo and npm mismatches are reported together", async () => {
  await withFixture({ tauri: "1.1.0", cargo: "0.3.0", npm: "0.1.0" }, async (root) => {
    const result = await checkVersion({ root });
    assert.equal(result.ok, false);
    assert.equal(result.diagnostics.filter((item) => item.code === "VERSION_MISMATCH").length, 2);
    const report = formatReport(result);
    assert.match(report, /source=cargo/u);
    assert.match(report, /source=npm/u);
  });
});

test("valid but mismatched tag fails", async () => {
  await withFixture({ tauri: "1.1.0", cargo: "1.1.0", npm: "1.1.0" }, async (root) => {
    const result = await checkVersion({ root, tag: "v1.2.0" });
    assert.equal(result.ok, false);
    assertCode(result, "VERSION_TAG_MISMATCH");
  });
});

test("malformed tags fail closed", async () => {
  await withFixture({ tauri: "1.1.0", cargo: "1.1.0", npm: "1.1.0" }, async (root) => {
    for (const tag of ["1.1.0", "v1.1", "foo", "V1.1.0", "v1.1.0-beta"]) {
      const result = await checkVersion({ root, tag });
      assert.equal(result.ok, false, tag);
      assertCode(result, "VERSION_TAG_INVALID");
    }
  });
});

test("files-only mode passes without a tag when files match", async () => {
  await withFixture({ tauri: "1.1.0", cargo: "1.1.0", npm: "1.1.0" }, async (root) => {
    const result = await checkVersion({ root });
    assert.equal(result.ok, true);
    assert.equal(result.tag, undefined);
  });
});

test("malformed authority fails with VERSION_AUTHORITY_INVALID", async () => {
  await withFixture({ tauri: "", cargo: "1.1.0", npm: "1.1.0" }, async (root) => {
    const result = await checkVersion({ root });
    assert.equal(result.ok, false);
    assertCode(result, "VERSION_AUTHORITY_INVALID");
  });
});

test("missing or malformed source fails closed", async () => {
  await withFixture(
    {
      tauri: "1.1.0",
      cargoToml: "[package]\nname = \"fixture\"\n",
      packageJson: "{ invalid json",
    },
    async (root) => {
      const result = await checkVersion({ root });
      assert.equal(result.ok, false);
      assert.equal(result.diagnostics.filter((item) => item.code === "VERSION_SOURCE_INVALID").length, 2);
    },
  );
});

test("duplicate Cargo package version fails closed", async () => {
  await withFixture(
    {
      tauri: "1.1.0",
      cargoToml: "[package]\nname = \"fixture\"\nversion = \"1.1.0\"\nversion = \"1.1.0\"\n",
      npm: "1.1.0",
    },
    async (root) => {
      const result = await checkVersion({ root });
      assert.equal(result.ok, false);
      assertCode(result, "VERSION_SOURCE_INVALID");
      assert.match(formatReport(result), /source=cargo reason=package_version_duplicate/u);
    },
  );
});

test("CLI argument parser accepts only an optional explicit tag", () => {
  assert.deepEqual(parseArguments([]), { tag: undefined, diagnostic: null });
  assert.deepEqual(parseArguments(["--tag", "v1.1.0"]), { tag: "v1.1.0", diagnostic: null });
  assert.equal(parseArguments(["--tag"]).diagnostic.code, "VERSION_SOURCE_INVALID");
});
