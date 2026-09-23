import { createReadStream } from "node:fs";
import { createHash } from "node:crypto";
import { execFile as execFileCallback } from "node:child_process";
import { mkdtemp, readFile, readdir, readlink, rmdir, stat } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { promisify } from "node:util";
import { fileURLToPath } from "node:url";

const execFile = promisify(execFileCallback);
const PROJECT_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const BUNDLE_ID = "io.github.lifei6671.serena-desktop";

/** 运行 macOS 系统工具，保留 codesign 写到 stderr 的正常诊断输出。 */
async function runCommand(command, args) {
  const result = await execFile(command, args, { encoding: "utf8" });
  return `${result.stdout}${result.stderr}`;
}

/** 对 DMG 以流式方式计算 SHA-256。 */
async function sha256(filePath) {
  const hash = createHash("sha256");
  for await (const chunk of createReadStream(filePath)) hash.update(chunk);
  return hash.digest("hex").toUpperCase();
}

/** 只接受当前版本的 Apple Silicon DMG 文件名。 */
export function isExpectedDmgName(filename, version) {
  return filename === `Serena Desktop_${version}_aarch64.dmg`;
}

/** DMG 顶层只允许应用与指向系统 Applications 的安装入口。 */
export function hasExpectedVisibleEntries(names) {
  const visible = names.filter((name) => !name.startsWith(".")).sort();
  return visible.length === 2 && visible[0] === "Applications" && visible[1] === "Serena Desktop.app";
}

/** 主程序必须恰好包含 arm64 slice，拒绝 Intel 和 Universal。 */
export function isArm64Only(architectures) {
  const slices = architectures.trim().split(/\s+/u);
  return slices.length === 1 && slices[0] === "arm64";
}

/** codesign 显示的签名类型必须明确为 ad-hoc。 */
export function isAdHocSignature(details) {
  return /^Signature=adhoc\s*$/mu.test(details);
}

/** 验证挂载卷内应用的可见安装结构和代码身份。 */
async function verifyMountedApp(mountPoint, version, run, readLink) {
  const entries = await readdir(mountPoint);
  if (!hasExpectedVisibleEntries(entries)) throw new Error("DMG_CONTENTS_INVALID");
  const applications = path.join(mountPoint, "Applications");
  if ((await readLink(applications)) !== "/Applications") throw new Error("DMG_APPLICATIONS_LINK_INVALID");

  const app = path.join(mountPoint, "Serena Desktop.app");
  if (!(await stat(app)).isDirectory()) throw new Error("DMG_APP_INVALID");
  const plist = path.join(app, "Contents", "Info.plist");
  const plistValue = async (key) => (await run("/usr/libexec/PlistBuddy", ["-c", `Print :${key}`, plist])).trim();
  const [bundleIdentifier, actualVersion, executableName] = await Promise.all([
    plistValue("CFBundleIdentifier"),
    plistValue("CFBundleShortVersionString"),
    plistValue("CFBundleExecutable"),
  ]);
  if (bundleIdentifier !== BUNDLE_ID) throw new Error("DMG_BUNDLE_ID_INVALID");
  if (actualVersion !== version) throw new Error("DMG_VERSION_INVALID");
  if (executableName !== "serena-desktop") throw new Error("DMG_EXECUTABLE_INVALID");

  const executable = path.join(app, "Contents", "MacOS", executableName);
  if (!(await stat(executable)).isFile()) throw new Error("DMG_EXECUTABLE_INVALID");
  const architecture = (await run("/usr/bin/lipo", ["-archs", executable])).trim();
  if (!isArm64Only(architecture)) throw new Error("DMG_ARCHITECTURE_INVALID");
  await run("/usr/bin/codesign", ["--verify", "--deep", "--strict", app]);
  const signatureDetails = await run("/usr/bin/codesign", ["-dv", "--verbose=4", app]);
  if (!isAdHocSignature(signatureDetails)) throw new Error("DMG_SIGNATURE_INVALID");
  if (!/^CodeDirectory .*flags=[^\r\n]*\([^)]*\bruntime\b[^)]*\)/mu.test(signatureDetails)) {
    throw new Error("DMG_HARDENED_RUNTIME_MISSING");
  }
  return { architecture: "arm64", bundleIdentifier, signature: "ad-hoc" };
}

/** 检查唯一 DMG，挂载后在成功和失败路径都卸载。 */
export async function verifyMacosRelease({ root = PROJECT_ROOT, directory, run = runCommand, readLink = readlink } = {}) {
  try {
    if (process.platform !== "darwin" && run === runCommand) throw new Error("MACOS_REQUIRED");
    const config = JSON.parse(await readFile(path.join(root, "src-tauri", "tauri.conf.json"), "utf8"));
    const version = config.version;
    if (typeof version !== "string" || !/^\d+\.\d+\.\d+$/u.test(version)) throw new Error("TAURI_VERSION_INVALID");
    const dmgDirectory = directory ?? path.join(root, "src-tauri", "target", "release", "bundle", "dmg");
    const entries = await readdir(dmgDirectory, { withFileTypes: true });
    const dmgs = entries.filter((entry) => entry.isFile() && entry.name.toLowerCase().endsWith(".dmg"));
    if (dmgs.length !== 1) throw new Error("DMG_COUNT_INVALID");
    const filename = dmgs[0].name;
    if (!isExpectedDmgName(filename, version)) throw new Error("DMG_FILENAME_INVALID");
    const artifactPath = path.resolve(dmgDirectory, filename);
    const metadata = await stat(artifactPath);
    if (!metadata.isFile() || metadata.size <= 0) throw new Error("DMG_EMPTY");
    const checksum = await sha256(artifactPath);

    const mountPoint = await mkdtemp(path.join(os.tmpdir(), "serena-dmg-verify-"));
    let identity;
    let verificationError;
    try {
      try {
        await run("/usr/bin/hdiutil", ["attach", "-readonly", "-nobrowse", "-quiet", "-mountpoint", mountPoint, artifactPath]);
      } catch {
        throw new Error("DMG_ATTACH_FAILED");
      }
      identity = await verifyMountedApp(mountPoint, version, run, readLink);
    } catch (error) {
      verificationError = error;
    }

    const cleanupDiagnostics = [];
    try {
      // attach 失败也可能留下挂载；普通 detach 失败时只追加一次 force 兜底。
      await run("/usr/bin/hdiutil", ["detach", "-quiet", mountPoint]);
    } catch {
      try {
        await run("/usr/bin/hdiutil", ["detach", "-force", "-quiet", mountPoint]);
      } catch {
        cleanupDiagnostics.push({ code: "DMG_DETACH_FAILED" });
      }
    }
    try {
      await rmdir(mountPoint);
    } catch {
      cleanupDiagnostics.push({ code: "DMG_MOUNT_DIRECTORY_CLEANUP_FAILED" });
    }

    if (verificationError || cleanupDiagnostics.length > 0) {
      return {
        ok: false,
        diagnostics: [
          ...(verificationError ? [{ code: verificationError instanceof Error ? verificationError.message : String(verificationError) }] : []),
          ...cleanupDiagnostics,
        ],
      };
    }
    return {
      ok: true,
      artifact: { path: artifactPath, filename, size: metadata.size, sha256: checksum, version, ...identity },
      diagnostics: [],
    };
  } catch (error) {
    return { ok: false, diagnostics: [{ code: error instanceof Error ? error.message : String(error) }] };
  }
}

/** CLI 只输出一行稳定 JSON，供 Actions 和人工审计读取。 */
async function main() {
  const result = await verifyMacosRelease();
  console.log(JSON.stringify(result));
  process.exitCode = result.ok ? 0 : 1;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) await main();
