# Phase 3B：macOS uv 与进程树

## Goal

修复 Codex 终止竞态，为 macOS Serena 与 cloudflared 建立独立进程组所有权，并提供固定版本和摘要的 arm64 uv 安装。

## Requirements

- Codex macOS Runtime 在升级到 `SIGKILL` 前必须重新证明原 leader identity；leader 已消失时保持 `unknown`，不得仅凭此前 process-group 非空观测继续 `killpg`。
- Serena 主服务、Serena 安装命令和 Quick Tunnel cloudflared 在 macOS 上必须在 `exec` 前创建独立 Session/Process Group，并只向已验证的应用所有进程组发信号。
- 停止路径必须等待直接 child 被回收且 process group 为空；身份或退出证据不足时返回明确错误并保留 owner，不误杀其他进程。
- Windows Job Object、`taskkill` 和 `winget` 路径保持现有行为；Linux 现有行为不因本阶段扩展为新的平台承诺。
- macOS Apple Silicon 缺少 uv 时，下载固定 `uv 0.12.17` 的 `uv-aarch64-apple-darwin.tar.gz`，验证固定 SHA-256 `85f00cbdc6dd3e97eba4c31b4d014375a9fdfe8f570023b84e5102fc3456896b` 后原子安装到应用 runtime 目录。
- macOS uv 发现覆盖当前 PATH、`~/.local/bin`、`/opt/homebrew/bin`、`/usr/local/bin` 和应用托管路径；不读取 `LOCALAPPDATA`，不调用 `winget`，不执行远端 shell installer。
- 保留固定 `SERENA_PACKAGE`、`SERENA_PYTHON`、`UV_TOOL_DIR`、`UV_TOOL_BIN_DIR` 与安装后 Serena capability 验证。

## Acceptance Criteria

- [x] Codex leader 在 grace 期间退出但 group 仍存在时不再发送 `SIGKILL`，返回 unconfirmed 并保留 Runtime owner。
- [x] macOS Serena 与 cloudflared 的受管 child 以 `PID=PGID=SID` 启动，停止后直接 child 已回收且原 process group 为空。
- [x] 停止一个受管进程组不会影响另一个受管组或用户自行启动的同名进程。
- [x] Serena 安装命令超时会收口其完整 macOS process group。
- [x] macOS uv 资产选择、摘要校验、归档成员选择、权限与原子持久化均有自动化测试。
- [x] Windows 原有安装和进程生命周期代码保持条件编译边界，macOS 构建、clippy 与相关测试通过。

## Verification

- `cargo check --manifest-path src-tauri/Cargo.toml --locked`
- `cargo clippy --manifest-path src-tauri/Cargo.toml --locked --all-targets -- -D warnings`
- `cargo test --manifest-path src-tauri/Cargo.toml --locked`：`1069 passed; 0 failed; 19 ignored`
- `cargo test --manifest-path src-tauri/Cargo.toml --locked official_macos_uv_install_matches_pinned_version -- --ignored --exact`：官方固定资产下载、摘要、提取和 `uv 0.12.17` 运行验证通过

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.
