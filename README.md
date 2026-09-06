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
