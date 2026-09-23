import assert from "node:assert/strict";
import { mkdir, mkdtemp, rm, symlink, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import { hasExpectedVisibleEntries, isAdHocSignature, isArm64Only, isExpectedDmgName, verifyMacosRelease } from "./verify-macos-release.mjs";

/** 使用假系统工具在任意平台验证挂载、身份检查和清理逻辑。 */
async function withFixture(options, check) {
  const root = await mkdtemp(path.join(os.tmpdir(), "serena-dmg-test-"));
  const directory = path.join(root, "src-tauri", "target", "release", "bundle", "dmg");
  const calls = [];
  let mountPoint;
  try {
    await mkdir(directory, { recursive: true });
    await writeFile(path.join(root, "src-tauri", "tauri.conf.json"), JSON.stringify({ version: "1.1.0" }));
    for (const filename of options.files ?? ["Serena Desktop_1.1.0_aarch64.dmg"]) {
      await writeFile(path.join(directory, filename), options.empty ? "" : "dmg bytes");
    }
    const run = async (command, args) => {
      calls.push([command, ...args]);
      if (command.endsWith("hdiutil") && args[0] === "attach") {
        mountPoint = args[args.indexOf("-mountpoint") + 1];
        if (options.attachFails) throw new Error("transient attach stderr");
        const app = path.join(mountPoint, "Serena Desktop.app");
        await mkdir(path.join(app, "Contents", "MacOS"), { recursive: true });
        await writeFile(path.join(app, "Contents", "MacOS", "serena-desktop"), "binary");
        await symlink("/Applications", path.join(mountPoint, "Applications"));
        if (options.extraEntry) await writeFile(path.join(mountPoint, options.extraEntry), "extra");
        return "";
      }
      if (command.endsWith("hdiutil") && args[0] === "detach") {
        const mount = args.at(-1);
        if (args.includes("-force") ? options.forceDetachFails : options.normalDetachFails) {
          throw new Error("busy hdiutil stderr");
        }
        for (const entry of ["Serena Desktop.app", "Applications", options.extraEntry].filter(Boolean)) {
          await rm(path.join(mount, entry), { recursive: true, force: true });
        }
        if (options.mountDirectoryCleanupFails) await writeFile(path.join(mount, ".cleanup-marker"), "leftover");
        return "";
      }
      if (command.endsWith("PlistBuddy")) {
        const key = args[1].replace("Print :", "");
        return {
          CFBundleIdentifier: options.bundleIdentifier ?? "io.github.lifei6671.serena-desktop",
          CFBundleShortVersionString: options.version ?? "1.1.0",
          CFBundleExecutable: "serena-desktop",
        }[key];
      }
      if (command.endsWith("lipo")) return options.architecture ?? "arm64";
      if (command.endsWith("codesign") && args[0] === "-dv") {
        return options.signature ?? "CodeDirectory v=20500 flags=0x10002(adhoc,runtime)\nSignature=adhoc\n";
      }
      if (command.endsWith("codesign")) return "";
      throw new Error(`unexpected command: ${command}`);
    };
    await check({ root, directory, run, calls });
  } finally {
    // 假挂载只存在于测试文件系统；即使模拟两次卸载失败也要清掉 fixture。
    if (mountPoint) await rm(mountPoint, { recursive: true, force: true });
    await rm(root, { recursive: true, force: true });
  }
}

test("pure macOS release rules reject Intel, Universal, Developer ID and extra visible entries", () => {
  assert.equal(isExpectedDmgName("Serena Desktop_1.1.0_aarch64.dmg", "1.1.0"), true);
  assert.equal(isExpectedDmgName("Serena Desktop_1.1.0_x64.dmg", "1.1.0"), false);
  assert.equal(isArm64Only("arm64"), true);
  assert.equal(isArm64Only("x86_64 arm64"), false);
  assert.equal(isAdHocSignature("Signature=adhoc\n"), true);
  assert.equal(isAdHocSignature("Authority=Developer ID Application\nSignature=apple\n"), false);
  assert.equal(hasExpectedVisibleEntries(["Serena Desktop.app", "Applications", ".DS_Store"]), true);
  assert.equal(hasExpectedVisibleEntries(["Serena Desktop.app", "Applications", "readme.txt"]), false);
});

test("valid DMG returns stable artifact identity and detaches", async () => {
  await withFixture({}, async ({ root, directory, run, calls }) => {
    const result = await verifyMacosRelease({ root, directory, run });
    assert.equal(result.ok, true);
    assert.equal(result.artifact.architecture, "arm64");
    assert.equal(result.artifact.signature, "ad-hoc");
    assert.equal(result.artifact.sha256.length, 64);
    const detachCalls = calls.filter((call) => call[1] === "detach");
    assert.equal(detachCalls.length, 1);
    assert.equal(detachCalls[0].includes("-force"), false);
  });
});

test("invalid architecture or Developer ID fails and still detaches", async () => {
  for (const [options, code] of [
    [{ architecture: "arm64 x86_64" }, "DMG_ARCHITECTURE_INVALID"],
    [{ signature: "CodeDirectory v=20500 flags=0x10000(runtime)\nAuthority=Developer ID Application\nSignature=apple\n" }, "DMG_SIGNATURE_INVALID"],
  ]) {
    await withFixture(options, async ({ root, directory, run, calls }) => {
      const result = await verifyMacosRelease({ root, directory, run });
      assert.equal(result.ok, false);
      assert.equal(result.diagnostics[0].code, code);
      const detachCalls = calls.filter((call) => call[1] === "detach");
      assert.equal(detachCalls.length, 1);
      assert.equal(detachCalls[0].includes("-force"), false);
    });
  }
});

test("normal detach failure falls back to force exactly once", async () => {
  await withFixture({ normalDetachFails: true }, async ({ root, directory, run, calls }) => {
    const result = await verifyMacosRelease({ root, directory, run });
    assert.equal(result.ok, true);
    const detachCalls = calls.filter((call) => call[1] === "detach");
    assert.equal(detachCalls.length, 2);
    assert.equal(detachCalls[0].includes("-force"), false);
    assert.equal(detachCalls[1].includes("-force"), true);
  });
});

test("attach failure still attempts normal and force cleanup", async () => {
  await withFixture({ attachFails: true, normalDetachFails: true }, async ({ root, directory, run, calls }) => {
    const result = await verifyMacosRelease({ root, directory, run });
    assert.equal(result.ok, false);
    assert.deepEqual(result.diagnostics, [{ code: "DMG_ATTACH_FAILED" }]);
    const detachCalls = calls.filter((call) => call[1] === "detach");
    assert.equal(detachCalls.length, 2);
    assert.equal(detachCalls[1].includes("-force"), true);
  });
});

test("two detach failures return a stable cleanup code", async () => {
  await withFixture({ normalDetachFails: true, forceDetachFails: true }, async ({ root, directory, run, calls }) => {
    const result = await verifyMacosRelease({ root, directory, run });
    assert.equal(result.ok, false);
    assert.equal(result.diagnostics[0].code, "DMG_DETACH_FAILED");
    assert.equal(result.diagnostics.some((item) => item.code === "DMG_MOUNT_DIRECTORY_CLEANUP_FAILED"), true);
    assert.equal(calls.filter((call) => call[1] === "detach").length, 2);
    assert.equal(JSON.stringify(result).includes("busy hdiutil stderr"), false);
  });
});

test("mount directory cleanup failure does not hide verification failure", async () => {
  await withFixture({ architecture: "x86_64", mountDirectoryCleanupFails: true }, async ({ root, directory, run }) => {
    const result = await verifyMacosRelease({ root, directory, run });
    assert.equal(result.ok, false);
    assert.deepEqual(result.diagnostics, [
      { code: "DMG_ARCHITECTURE_INVALID" },
      { code: "DMG_MOUNT_DIRECTORY_CLEANUP_FAILED" },
    ]);
  });
});

test("multiple or empty DMGs fail before mounting", async () => {
  for (const [options, code] of [
    [{ files: ["Serena Desktop_1.1.0_aarch64.dmg", "other.dmg"] }, "DMG_COUNT_INVALID"],
    [{ empty: true }, "DMG_EMPTY"],
  ]) {
    await withFixture(options, async ({ root, directory, run, calls }) => {
      const result = await verifyMacosRelease({ root, directory, run });
      assert.equal(result.ok, false);
      assert.equal(result.diagnostics[0].code, code);
      assert.equal(calls.length, 0);
    });
  }
});
