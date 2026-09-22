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

- [ ] Dock 等价 Quit 事件触发完整 shutdown，Serena Desktop 主进程结束后不再遗留其 Serena broker 与 dashboard tray 子进程。
- [ ] 正常退出后，Serena 端口、Broker listener 和远程访问相关监听均已关闭。
- [ ] Workspace Capability、Codex Agent、Remote、Broker 和 Supervisor 的 shutdown 均被尝试，前序失败不会跳过后续 owner。
- [ ] 已完成 shutdown 后收到最终 `RunEvent::Exit` 不会重复关闭 owner。
- [ ] 新增自动化测试先失败后通过，覆盖退出决策、幂等和错误汇总；现有相关 Rust/前端回归通过。
- [ ] 构建真实 macOS `.app` 后，用标准 Quit Apple Event 验证进程树和端口全部收口。
- [ ] Windows 编译路径不受 macOS 退出事件补偿逻辑影响。

## Notes

- 现场复现：Serena Desktop 主进程已退出，但 Serena MCP broker PID `9911` 变为 `PPID=1`，其 dashboard tray 子进程 PID `9923` 仍存活，端口 `127.0.0.1:9121` 仍监听。
- 根因确认：Tauri 2.11.5 / Tao 0.35.3 在 macOS Dock Quit 时由 `applicationWillTerminate` 直接产生 `RunEvent::Exit`；当前应用只在 `ExitRequested` 调用 `request_exit`，而 `App::run` 最终使用 `process::exit`，不会依赖 Rust `Drop` 收口。
