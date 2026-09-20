import { readFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const SCRIPT_DIRECTORY = path.dirname(fileURLToPath(import.meta.url));
const PROJECT_ROOT = path.resolve(SCRIPT_DIRECTORY, "..");

/** 读取 bundle 配置，确认没有项目自定义的用户数据删除逻辑入口。 */
export async function verifyUninstallPolicy({ root = PROJECT_ROOT } = {}) {
  let config;
  try {
    config = JSON.parse(await readFile(path.join(root, "src-tauri", "tauri.conf.json"), "utf8"));
  } catch {
    return { ok: false, code: "UNINSTALL_POLICY_CONFIG_INVALID", reason: "tauri_config_unreadable" };
  }
  const nsis = config.bundle?.windows?.nsis;
  if (config.bundle?.active !== true || !Array.isArray(config.bundle?.targets) || config.bundle.targets.join(",") !== "nsis") {
    return { ok: false, code: "UNINSTALL_POLICY_CONFIG_INVALID", reason: "nsis_bundle_required" };
  }
  if (!nsis || nsis.installMode !== "currentUser") {
    return { ok: false, code: "UNINSTALL_POLICY_CONFIG_INVALID", reason: "current_user_nsis_required" };
  }
  if (nsis.template || nsis.installerHooks) {
    return { ok: false, code: "UNINSTALL_POLICY_CUSTOM_UNINSTALL_UNVERIFIED", reason: "template_or_installer_hooks_present" };
  }
  return {
    ok: true,
    installMode: nsis.installMode,
    customTemplate: false,
    installerHooks: false,
    realUninstallDataPreservation: "NOT_RUN_P6_007",
  };
}

/** 输出静态 policy 结果；真实卸载数据保留仍由 P6-007 验收。 */
async function main() {
  const result = await verifyUninstallPolicy();
  console.log(JSON.stringify(result));
  process.exitCode = result.ok ? 0 : 1;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  await main();
}
