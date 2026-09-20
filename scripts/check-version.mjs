import { readFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const SCRIPT_DIRECTORY = path.dirname(fileURLToPath(import.meta.url));
const PROJECT_ROOT = path.resolve(SCRIPT_DIRECTORY, "..");
const STABLE_VERSION = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/;

/** 以稳定 code 创建可供 CI 和测试消费的诊断。 */
function diagnostic(code, fields = {}) {
  return { code, ...fields };
}

/** 判断当前冻结版本格式是否为 SerenaDesktop 支持的稳定三段版本。 */
function isStableVersion(value) {
  return typeof value === "string" && STABLE_VERSION.test(value);
}

/** 读取文本，缺失或不可读时返回 fail-closed source diagnostic。 */
async function readSource(filePath, source) {
  try {
    return { text: await readFile(filePath, "utf8"), diagnostic: null };
  } catch (error) {
    return {
      text: null,
      diagnostic: diagnostic("VERSION_SOURCE_INVALID", {
        source,
        reason: `read_failed:${error.code ?? "unknown"}`,
      }),
    };
  }
}

/** 解析唯一权威的 Tauri JSON version 字段。 */
function parseAuthority(text) {
  try {
    const parsed = JSON.parse(text);
    if (!isStableVersion(parsed.version)) {
      return {
        value: null,
        diagnostic: diagnostic("VERSION_AUTHORITY_INVALID", {
          source: "tauri",
          reason: "version_must_be_stable_x_y_z",
        }),
      };
    }
    return { value: parsed.version, diagnostic: null };
  } catch {
    return {
      value: null,
      diagnostic: diagnostic("VERSION_AUTHORITY_INVALID", {
        source: "tauri",
        reason: "json_parse_failed",
      }),
    };
  }
}

/** 只读取 Cargo [package] section 中的 version，避免误读依赖版本。 */
function parseCargoPackageVersion(text) {
  let inPackage = false;
  let version = null;
  let versionCount = 0;
  for (const line of text.split(/\r?\n/u)) {
    const section = line.match(/^\s*\[([^\]]+)\]\s*(?:#.*)?$/u);
    if (section) {
      inPackage = section[1] === "package";
      continue;
    }
    if (!inPackage) {
      continue;
    }
    if (!/^\s*version\s*=/u.test(line)) {
      continue;
    }
    versionCount += 1;
    const match = line.match(/^\s*version\s*=\s*"([^"\r\n]*)"\s*(?:#.*)?$/u);
    if (!match) {
      return { value: null, reason: "package_version_malformed" };
    }
    version = match[1];
  }
  if (versionCount === 0) {
    return { value: null, reason: "package_version_missing" };
  }
  if (versionCount !== 1) {
    return { value: null, reason: "package_version_duplicate" };
  }
  return { value: version, reason: null };
}

/** 解析并验证一个从非权威 source 获取的稳定版本。 */
function validateSourceVersion(source, value, reason = "version_must_be_stable_x_y_z") {
  if (!isStableVersion(value)) {
    return {
      value: null,
      diagnostic: diagnostic("VERSION_SOURCE_INVALID", {
        source,
        reason,
      }),
    };
  }
  return { value, diagnostic: null };
}

/** 解析 package.json 顶层 version，避免误读依赖版本。 */
function parseNpmVersion(text) {
  try {
    return validateSourceVersion("npm", JSON.parse(text).version);
  } catch {
    return {
      value: null,
      diagnostic: diagnostic("VERSION_SOURCE_INVALID", {
        source: "npm",
        reason: "json_parse_failed",
      }),
    };
  }
}

/** 比较一个合法 source 与 authority，并保留全部不一致项。 */
function compareSource(diagnostics, source, authority, actual) {
  if (authority && actual && authority !== actual) {
    diagnostics.push(
      diagnostic("VERSION_MISMATCH", { source, expected: authority, actual }),
    );
  }
}

/** 验证可选 release tag 必须严格为 vX.Y.Z 且匹配 authority。 */
function validateTag(diagnostics, authority, tag) {
  if (tag === undefined) {
    return;
  }
  const match = typeof tag === "string" ? tag.match(/^v(.+)$/u) : null;
  if (!match || !isStableVersion(match[1])) {
    diagnostics.push(
      diagnostic("VERSION_TAG_INVALID", {
        actual: String(tag),
        reason: "tag_must_be_vX_Y_Z",
      }),
    );
  } else if (authority && match[1] !== authority) {
    diagnostics.push(
      diagnostic("VERSION_TAG_MISMATCH", {
        expected: `v${authority}`,
        actual: tag,
      }),
    );
  }
}

/** 在指定 root 检查产品版本；root 参数仅供隔离 fixture 测试使用。 */
export async function checkVersion({ root = PROJECT_ROOT, tag } = {}) {
  const diagnostics = [];
  const [tauriSource, cargoSource, npmSource] = await Promise.all([
    readSource(path.join(root, "src-tauri", "tauri.conf.json"), "tauri"),
    readSource(path.join(root, "src-tauri", "Cargo.toml"), "cargo"),
    readSource(path.join(root, "package.json"), "npm"),
  ]);

  if (tauriSource.diagnostic) diagnostics.push(tauriSource.diagnostic);
  if (cargoSource.diagnostic) diagnostics.push(cargoSource.diagnostic);
  if (npmSource.diagnostic) diagnostics.push(npmSource.diagnostic);

  const authority = tauriSource.text ? parseAuthority(tauriSource.text) : { value: null };
  const cargoVersion = cargoSource.text ? parseCargoPackageVersion(cargoSource.text) : null;
  const cargo = cargoVersion
    ? validateSourceVersion("cargo", cargoVersion.value, cargoVersion.reason ?? undefined)
    : { value: null };
  const npm = npmSource.text ? parseNpmVersion(npmSource.text) : { value: null };

  if (authority.diagnostic) diagnostics.push(authority.diagnostic);
  if (cargo.diagnostic) diagnostics.push(cargo.diagnostic);
  if (npm.diagnostic) diagnostics.push(npm.diagnostic);

  compareSource(diagnostics, "cargo", authority.value, cargo.value);
  compareSource(diagnostics, "npm", authority.value, npm.value);
  validateTag(diagnostics, authority.value, tag);

  return {
    ok: diagnostics.length === 0,
    authority: authority.value,
    cargo: cargo.value,
    npm: npm.value,
    tag,
    diagnostics,
  };
}

/** 把 diagnostic 转换为稳定且同时便于人读和机器读的一行文本。 */
function formatDiagnostic(item) {
  return [
    item.code,
    ...Object.entries(item)
      .filter(([key]) => key !== "code")
      .map(([key, value]) => `${key}=${value}`),
  ].join(" ");
}

/** 输出完整 Gate 结果，保证一次展示全部 mismatch。 */
export function formatReport(result) {
  const lines = [
    `Version consistency: ${result.ok ? "PASS" : "FAIL"}`,
    `authority: ${result.authority ?? "INVALID"}`,
    `cargo: ${result.cargo ?? "INVALID"}`,
    `npm: ${result.npm ?? "INVALID"}`,
  ];
  if (result.tag !== undefined) {
    lines.push(`tag: ${result.tag}`);
  }
  for (const item of result.diagnostics) {
    lines.push(formatDiagnostic(item));
  }
  return lines.join("\n");
}

/** 解析仅支持可选 --tag 的 CLI 参数，避免静默接受错误 release 输入。 */
export function parseArguments(args) {
  if (args.length === 0) {
    return { tag: undefined, diagnostic: null };
  }
  if (args.length === 2 && args[0] === "--tag") {
    return { tag: args[1], diagnostic: null };
  }
  return {
    tag: undefined,
    diagnostic: diagnostic("VERSION_SOURCE_INVALID", {
      source: "arguments",
      reason: "usage_check_version_mjs_optional_tag_vX_Y_Z",
    }),
  };
}

/** 执行 CLI 并以非零状态阻止不一致的 release。 */
async function main() {
  const args = parseArguments(process.argv.slice(2));
  if (args.diagnostic) {
    console.log(formatReport({ ok: false, authority: null, cargo: null, npm: null, diagnostics: [args.diagnostic] }));
    process.exitCode = 1;
    return;
  }
  const result = await checkVersion({ tag: args.tag });
  console.log(formatReport(result));
  process.exitCode = result.ok ? 0 : 1;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  await main();
}
