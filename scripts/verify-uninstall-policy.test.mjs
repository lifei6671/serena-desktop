import assert from "node:assert/strict";
import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import { verifyUninstallPolicy } from "./verify-uninstall-policy.mjs";

/** 在临时配置中验证静态卸载政策，不修改真实 Tauri 配置。 */
async function withConfig(config, run) {
  const root = await mkdtemp(path.join(os.tmpdir(), "serena-uninstall-policy-"));
  try {
    await mkdir(path.join(root, "src-tauri"));
    await writeFile(path.join(root, "src-tauri", "tauri.conf.json"), JSON.stringify(config));
    await run(root);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
}

const CURRENT_USER_NSIS = { bundle: { active: true, targets: ["nsis"], windows: { nsis: { installMode: "currentUser" } } } };

test("default current-user NSIS configuration preserves the static policy", async () => {
  await withConfig(CURRENT_USER_NSIS, async (root) => {
    const result = await verifyUninstallPolicy({ root });
    assert.equal(result.ok, true);
    assert.equal(result.realUninstallDataPreservation, "NOT_RUN_P6_007");
  });
});

test("custom installer hooks are rejected because data deletion cannot be proven absent", async () => {
  const config = structuredClone(CURRENT_USER_NSIS);
  config.bundle.windows.nsis.installerHooks = "hooks.nsh";
  await withConfig(config, async (root) => {
    const result = await verifyUninstallPolicy({ root });
    assert.equal(result.ok, false);
    assert.equal(result.code, "UNINSTALL_POLICY_CUSTOM_UNINSTALL_UNVERIFIED");
  });
});

test("non-current-user NSIS configuration is rejected", async () => {
  const config = structuredClone(CURRENT_USER_NSIS);
  config.bundle.windows.nsis.installMode = "perMachine";
  await withConfig(config, async (root) => {
    const result = await verifyUninstallPolicy({ root });
    assert.equal(result.ok, false);
    assert.equal(result.code, "UNINSTALL_POLICY_CONFIG_INVALID");
  });
});
