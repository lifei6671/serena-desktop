import { createReadStream } from "node:fs";
import { readdir, readFile, stat } from "node:fs/promises";
import { createHash } from "node:crypto";
import path from "node:path";
import { fileURLToPath } from "node:url";

const SCRIPT_DIRECTORY = path.dirname(fileURLToPath(import.meta.url));
const PROJECT_ROOT = path.resolve(SCRIPT_DIRECTORY, "..");

/** 创建稳定的 installer verification diagnostic。 */
function diagnostic(code, fields = {}) {
  return { code, ...fields };
}

/** 对 installer 文件以流式方式计算 SHA-256。 */
async function sha256(filePath) {
  const hash = createHash("sha256");
  for await (const chunk of createReadStream(filePath)) {
    hash.update(chunk);
  }
  return hash.digest("hex").toUpperCase();
}

/** 读取 Tauri authority，并只接受非空产品版本。 */
async function readAuthority(root) {
  try {
    const config = JSON.parse(await readFile(path.join(root, "src-tauri", "tauri.conf.json"), "utf8"));
    if (typeof config.version !== "string" || config.version.length === 0) {
      return { value: null, diagnostic: diagnostic("INSTALLER_CONFIG_INVALID", { reason: "tauri_version_missing" }) };
    }
    return { value: config.version, diagnostic: null };
  } catch {
    return { value: null, diagnostic: diagnostic("INSTALLER_CONFIG_INVALID", { reason: "tauri_config_unreadable" }) };
  }
}

/** 验证唯一、非空且符合当前 product version/x64 命名的 NSIS installer。 */
export async function verifyInstaller({ root = PROJECT_ROOT, directory } = {}) {
  const authority = await readAuthority(root);
  if (authority.diagnostic) {
    return { ok: false, diagnostics: [authority.diagnostic] };
  }
  const artifactDirectory = directory ?? path.join(root, "src-tauri", "target", "release", "bundle", "nsis");
  let entries;
  try {
    entries = await readdir(artifactDirectory, { withFileTypes: true });
  } catch {
    return {
      ok: false,
      diagnostics: [diagnostic("INSTALLER_ARTIFACT_MISSING", { directory: artifactDirectory })],
    };
  }
  const executables = entries.filter((entry) => entry.isFile() && entry.name.toLowerCase().endsWith(".exe"));
  if (executables.length === 0) {
    return {
      ok: false,
      diagnostics: [diagnostic("INSTALLER_ARTIFACT_MISSING", { directory: artifactDirectory })],
    };
  }
  if (executables.length !== 1) {
    return {
      ok: false,
      diagnostics: [diagnostic("INSTALLER_ARTIFACT_AMBIGUOUS", { count: executables.length })],
    };
  }
  const installer = executables[0];
  const expectedSuffix = `_${authority.value}_x64-setup.exe`;
  if (!installer.name.endsWith(expectedSuffix)) {
    return {
      ok: false,
      diagnostics: [diagnostic("INSTALLER_VERSION_OR_ARCH_INVALID", {
        expected_suffix: expectedSuffix,
        actual: installer.name,
      })],
    };
  }
  const artifactPath = path.join(artifactDirectory, installer.name);
  const metadata = await stat(artifactPath);
  if (!metadata.isFile() || metadata.size <= 0) {
    return {
      ok: false,
      diagnostics: [diagnostic("INSTALLER_ARTIFACT_INVALID", { reason: "regular_nonempty_file_required" })],
    };
  }
  return {
    ok: true,
    artifact: {
      path: artifactPath,
      filename: installer.name,
      size: metadata.size,
      sha256: await sha256(artifactPath),
      version: authority.value,
      architecture: "x64",
    },
    diagnostics: [],
  };
}

/** 将 verification result 输出为稳定 JSON，供 workflow log 与人工审计使用。 */
export function formatReport(result) {
  return JSON.stringify(result);
}

/** 执行默认 artifact 目录验证，失败时返回非零状态。 */
async function main() {
  const result = await verifyInstaller();
  console.log(formatReport(result));
  process.exitCode = result.ok ? 0 : 1;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  await main();
}
