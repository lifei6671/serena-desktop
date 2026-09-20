import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

/** 普通构建读取产品清单，GitHub 发布构建以标签环境变量覆盖该版本。 */
function frontendVersion() {
  const manifest = JSON.parse(readFileSync(new URL("./package.json", import.meta.url), "utf8"));
  if (typeof manifest.version !== "string") {
    throw new Error("package.json must contain a product version");
  }
  return process.env.VITE_APP_VERSION ?? `v${manifest.version}`;
}

export default defineConfig({
  plugins: [react(), tailwindcss()],
  define: { __SERENA_DESKTOP_VERSION__: JSON.stringify(frontendVersion()) },
  resolve: { alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) } },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  envPrefix: ["VITE_", "TAURI_ENV_*"],
  build: {
    target:
      process.env.TAURI_ENV_PLATFORM === "windows" ? "chrome105" : "safari13",
    minify: process.env.TAURI_ENV_DEBUG ? false : "oxc",
    sourcemap: Boolean(process.env.TAURI_ENV_DEBUG),
  },
});
