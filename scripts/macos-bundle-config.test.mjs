import assert from "node:assert/strict";
import { access, readFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

/** 读取仓库内 JSON 配置，解析失败应直接使契约测试失败。 */
async function readJson(relativePath) {
  return JSON.parse(await readFile(path.join(root, relativePath), "utf8"));
}

test("macOS app bundle overlay stays isolated from the Windows NSIS authority", async () => {
  // 同时读取两份配置，确保测试覆盖平台覆盖层与基础配置的合并边界。
  const [base, macos] = await Promise.all([
    readJson("src-tauri/tauri.conf.json"),
    readJson("src-tauri/tauri.macos.conf.json"),
  ]);

  assert.deepEqual(base.bundle.targets, ["nsis"]);
  assert.deepEqual(macos.bundle.targets, ["app"]);
  assert.equal(macos.bundle.macOS.minimumSystemVersion, "12.0");
  assert.equal(macos.bundle.macOS.signingIdentity, "-");
  assert.equal(Object.hasOwn(macos.bundle, "windows"), false);
  assert.equal(macos.bundle.targets.includes("dmg"), false);
  assert.ok(base.bundle.icon.includes("icons/icon.icns"));

  // 图标契约不仅检查配置项，还确认被引用的源文件真实存在。
  await access(path.join(root, "src-tauri", "icons", "icon.icns"));
});
