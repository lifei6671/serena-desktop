# 标签构建版本调研

- 当前 `.github/workflows/release.yml` 只在构建后运行 `check-version.mjs`，要求仓库内的 Tauri、Cargo 和 npm 版本预先等于标签；它不会将标签写入构建输入。
- Tauri 的产品版本同时来自 `src-tauri/tauri.conf.json` 与 Rust crate 的 `Cargo.toml`；根产品包版本也记录在 `Cargo.lock`，否则现有 `cargo --locked` 检查会拒绝构建。因此仅向 Vite 注入环境变量不足以改变安装包和运行时版本。
- 现有发布契约仅接受稳定的 `vX.Y.Z` 标签。同步脚本应在 CI checkout 中于 `npm ci` 后、质量检查和编译前更新三个产品清单；该工作目录不提交回 Git。
- 前端由 Vite 编译。Vite `define` 可以在普通构建中读取 `package.json` 版本、在发布构建中读取 `VITE_APP_VERSION`，避免运行时 IPC 和异步展示。
