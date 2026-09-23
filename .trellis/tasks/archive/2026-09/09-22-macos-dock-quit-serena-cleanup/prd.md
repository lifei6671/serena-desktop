# 收口宿主退出时的全部受管子进程

## Goal

Serena Desktop 能感知到自身退出时，必须在宿主结束前收口它启动并持有 ownership 的全部子孙进程，不遗留 Serena、CodeGraph、Codex Runtime 或远程隧道。

## Requirements

- macOS Dock 右键退出、`Cmd+Q`、系统注销等直接到达 `RunEvent::Exit` 的路径必须执行完整 shutdown。
- 托盘退出、窗口退出和错误页退出继续走同一 shutdown authority，不能形成第二套清理逻辑。
- shutdown 必须覆盖 Workspace Capability Runtime、Codex Agent Runtime、远程访问进程、MCP listener/workspace binding、主 Serena MCP broker 及其 dashboard tray 子进程。
- 一个 shutdown 阶段失败时仍必须尝试其余独立 owner，最终再汇总失败；不能因为前序错误跳过 Serena 或其他受管进程。
- shutdown 必须幂等；已完成的关闭流程不能在最终 `RunEvent::Exit` 中重复执行。
- 正常退出可以等待现有有界终止流程完成。无法运行任何清理代码的 `SIGKILL`、系统断电等不可感知终止不在保证范围内。
- 继续使用现有 Windows Job Object、macOS Process Group 和模块私有 ownership，不新增全局 PID 注册表、守护进程或第三方依赖。
- Windows 当前退出行为和运行时 ownership 契约保持不变。

## Acceptance Criteria

- [x] Dock 等价 Quit 事件触发完整 shutdown，Serena Desktop 主进程结束后不再遗留其 Serena broker 与 dashboard tray 子进程。
- [x] 正常退出后，Serena 端口、Broker listener 和远程访问相关监听均已关闭。
- [x] Workspace Capability、Codex Agent、Remote、Broker 和 Supervisor 的 shutdown 均被尝试，前序失败不会跳过后续 owner。
- [x] 已完成 shutdown 后收到最终 `RunEvent::Exit` 不会重复关闭 owner。
- [x] 新增自动化测试先失败后通过，覆盖退出决策、幂等和错误汇总；现有相关 Rust/前端回归通过。
- [x] 构建真实 macOS `.app` 后，用标准 Quit Apple Event 验证进程树和端口全部收口。
- [x] Windows 编译路径不受 macOS 退出事件补偿逻辑影响。

## Notes

- 现场复现：Serena Desktop 主进程已退出，但 Serena MCP broker PID `9911` 变为 `PPID=1`，其 dashboard tray 子进程 PID `9923` 仍存活，端口 `127.0.0.1:9121` 仍监听。
- 根因确认：Tauri 2.11.5 / Tao 0.35.3 在 macOS Dock Quit 时由 `applicationWillTerminate` 直接产生 `RunEvent::Exit`；当前应用只在 `ExitRequested` 调用 `request_exit`，而 `App::run` 最终使用 `process::exit`，不会依赖 Rust `Drop` 收口。

## Verification Evidence

- RED：`shutdown_steps_attempt_all_owners_and_aggregate_failures` 因缺少 `ShutdownFuture` / `run_shutdown_steps` 编译失败；`shutdown_once_` 因缺少 `run_shutdown_once` 编译失败。
- GREEN：新增 shutdown owner 顺序/错误汇总测试和两项 shutdown gate 幂等/重试测试通过；Dock reopen 既有测试通过。
- 回归：`cargo fmt --check`、`cargo check --all-targets`、完整 `cargo test`（`1076 passed; 0 failed; 21 ignored`）、`npm run lint`、`npm run build`、`npm test`（`117 passed; 0 failed`）和 `git diff --check` 均通过。
- 构建：`npm run tauri build` 成功生成 arm64 `Serena Desktop.app`；本地 ad-hoc 签名成功，未配置公证凭据的 warning 不影响本轮真机 Gate。
- 真机 Gate：LaunchServices 启动 app PID `15311`，其受管 Serena broker 为 PID/PGID `15494`、监听 `127.0.0.1:9121`；标准 Quit Apple Event 后两者均退出，受管 `broker.yml` 进程、`9120/9121` 监听和 cloudflared/ngrok 进程均无残留，应用日志记录 `Serena 已停止`。
- Windows：当前主机只安装 `aarch64-apple-darwin` target，未伪造 Windows 交叉编译结果；新增最终 `RunEvent::Exit` 分支受 `#[cfg(target_os = "macos")]` 限定，未修改 Windows Job Object 实现，通用 shutdown runner 由平台无关单元测试覆盖。
