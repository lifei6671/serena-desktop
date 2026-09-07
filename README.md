# Serena Desktop

Serena Desktop 是 Windows 上的轻量级 Serena Manager，用于检测、安装、启动、停止、配置和后台托管 Serena MCP Server。

项目当前按 [V0.1 技术方案](docs/technical-design-v0.1.md) 实现。它只做 Serena Supervisor，不重新实现 Serena、不代理 MCP，也不管理项目、索引或 Memory。

## 功能

- 检测并显示 Serena 路径和版本；
- 安装、启动、停止和重启 Serena MCP Server；
- 配置本机端口、Dashboard 和启动行为；
- 在应用内打开 Serena Web Dashboard，也可转到系统浏览器；
- 设置自动保存，并可通过原生文件对话框选择 Serena 可执行文件；
- Windows 登录自启、关闭隐藏到托盘；
- 单实例运行；手动再次启动时唤醒现有窗口，登录自启保持隐藏；
- 后台检测和启动 Serena，退出时先隐藏窗口再完成后台清理；
- 保存 Serena 输出并打开日志目录。

## 开发

需要 Node.js `^20.19.0` 或 `>=22.12.0`、npm、Rust 和 Tauri 2 的 Windows 构建依赖。

```powershell
npm install
npm run tauri dev
```

验证与构建：

```powershell
npm run lint
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
npm run tauri build
```

当前 `tauri.conf.json` 关闭安装包 bundling，`npm run tauri build` 会生成单个 Windows 可执行文件：

```text
src-tauri/target/release/serena-desktop.exe
```

## 自动发布

`.github/workflows/release.yml` 监听所有 tag 的 push 事件，在 Windows x64 上安装依赖、运行前端 lint 和 Rust 测试、构建 Release 二进制，然后创建对应 tag 的 GitHub Release 并上传 `serena-desktop.exe`，自动生成发布说明。任何检查或构建失败都会阻止发布。

发布前先更新并提交项目版本号（`package.json`、`package-lock.json`、`src-tauri/Cargo.toml`、`src-tauri/Cargo.lock`、`src-tauri/tauri.conf.json`），再对包含工作流和待发布代码的提交打 tag 并推送，例如：

```powershell
git tag v0.1.0
git push origin v0.1.0
```

工作流使用自动提供的 `GITHUB_TOKEN` 和 `contents: write` 权限，不需要配置个人 Token。所有 tag 默认发布为正式 Release；tag 不会自动修改程序内版本号。重跑时会更新已有 Release 的同名附件（仓库启用不可变 Release 时，已发布附件不能覆盖，需使用新 tag）。
