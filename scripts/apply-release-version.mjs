import { readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { checkVersion, formatReport } from "./check-version.mjs";

const SCRIPT_DIRECTORY = path.dirname(fileURLToPath(import.meta.url));
const PROJECT_ROOT = path.resolve(SCRIPT_DIRECTORY, "..");
const STABLE_VERSION = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/;

/** 将发布标签转换为 Tauri、Cargo 与 npm 共用的稳定版本。 */
export function versionFromTag(tag) {
  const match = typeof tag === "string" ? tag.match(/^v(.+)$/u) : null;
  if (!match || !STABLE_VERSION.test(match[1])) {
    throw new Error(`Release tag must be vX.Y.Z, received: ${String(tag)}`);
  }
  return match[1];
}

/** 仅替换 Cargo [package] 区块的唯一 version，避免误改依赖版本。 */
export function replaceCargoPackageVersion(text, version) {
  let inPackage = false;
  let replacements = 0;
  const next = text.split(/\r?\n/u).map((line) => {
    const section = line.match(/^\s*\[([^\]]+)\]\s*(?:#.*)?$/u);
    if (section) {
      inPackage = section[1] === "package";
      return line;
    }
    if (!inPackage || !/^\s*version\s*=/u.test(line)) {
      return line;
    }
    if (!/^\s*version\s*=\s*"[^"\r\n]*"\s*(?:#.*)?$/u.test(line)) {
      throw new Error("Cargo package version is malformed");
    }
    replacements += 1;
    return `version = "${version}"`;
  });
  if (replacements !== 1) {
    throw new Error(`Cargo package version must occur once, found: ${replacements}`);
  }
  return next.join("\n");
}

/** 同步 Cargo.lock 中根产品包的版本，保证后续 --locked 检查仍可执行。 */
export function replaceCargoLockPackageVersion(text, version) {
  let replacements = 0;
  const sections = text.split(/(?=^\[\[package\]\]$)/mu);
  const next = sections.map((section) => {
    if (!/^\[\[package\]\]\r?\nname = "serena-desktop"$/mu.test(section)) {
      return section;
    }
    const updated = section.replace(/^version = "[^"\r\n]*"$/mu, () => {
      replacements += 1;
      return `version = "${version}"`;
    });
    return updated;
  });
  if (replacements !== 1) {
    throw new Error(`Cargo.lock product version must occur once, found: ${replacements}`);
  }
  return next.join("");
}

/** 更新 JSON 产品清单的顶层版本，同时保留机器可读的标准格式。 */
function replaceJsonVersion(text, source, version) {
  let document;
  try {
    document = JSON.parse(text);
  } catch {
    throw new Error(`${source} version source is not valid JSON`);
  }
  if (!document || typeof document !== "object" || Array.isArray(document)) {
    throw new Error(`${source} version source must be a JSON object`);
  }
  return `${JSON.stringify({ ...document, version }, null, 2)}\n`;
}

/** 在 CI checkout 中将已验证的发布标签同步给全部构建版本来源。 */
export async function applyReleaseVersion({ root = PROJECT_ROOT, tag } = {}) {
  const version = versionFromTag(tag);
  const baseline = await checkVersion({ root });
  if (!baseline.ok) {
    throw new Error(`Product manifests are inconsistent before release version injection:\n${formatReport(baseline)}`);
  }

  const paths = {
    tauri: path.join(root, "src-tauri", "tauri.conf.json"),
    cargo: path.join(root, "src-tauri", "Cargo.toml"),
    cargoLock: path.join(root, "src-tauri", "Cargo.lock"),
    npm: path.join(root, "package.json"),
  };
  const [tauri, cargo, cargoLock, npm] = await Promise.all([
    readFile(paths.tauri, "utf8"),
    readFile(paths.cargo, "utf8"),
    readFile(paths.cargoLock, "utf8"),
    readFile(paths.npm, "utf8"),
  ]);
  const updates = [
    [paths.tauri, replaceJsonVersion(tauri, "Tauri", version)],
    [paths.cargo, replaceCargoPackageVersion(cargo, version)],
    [paths.cargoLock, replaceCargoLockPackageVersion(cargoLock, version)],
    [paths.npm, replaceJsonVersion(npm, "npm", version)],
  ];
  await Promise.all(updates.map(([filePath, content]) => writeFile(filePath, content, "utf8")));

  const result = await checkVersion({ root, tag });
  if (!result.ok) {
    throw new Error(`Release version injection did not produce a consistent product version:\n${formatReport(result)}`);
  }
  return result;
}

/** 解析唯一支持的命令行参数，避免发布输入被静默忽略。 */
export function parseArguments(args) {
  if (args.length === 2 && args[0] === "--tag") {
    return { tag: args[1] };
  }
  throw new Error("Usage: node scripts/apply-release-version.mjs --tag vX.Y.Z");
}

/** 在 GitHub Actions 中执行标签版本同步，并以非零退出码阻止错误发布。 */
async function main() {
  try {
    const result = await applyReleaseVersion(parseArguments(process.argv.slice(2)));
    console.log(formatReport(result));
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  await main();
}
