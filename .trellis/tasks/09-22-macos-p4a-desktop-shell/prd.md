# Phase 4A：macOS 桌面壳层行为

## Goal

让 macOS `.app` 使用原生系统打开命令，并在 Dock reopen 时恢复主窗口，同时保持现有 Windows、Linux、菜单栏与有界退出契约不变。

## Requirements

- `open_dashboard`、`open_log_directory` 和 `open_external_url` 在 macOS 上必须直接执行 `/usr/bin/open`，目标作为独立 argv 传入，不通过 shell。
- Windows 必须继续使用 `explorer.exe`；Linux 和其他非 macOS Unix 必须继续使用 `xdg-open`。
- macOS 收到 `RunEvent::Reopen` 且当前没有可见窗口时，必须复用 `tray::show_main_window` 显示、取消最小化并聚焦主窗口。
- 已有可见窗口时 Dock reopen 不额外改变窗口状态。
- 菜单栏图标左键继续直接显示主窗口，菜单项“退出”、`Cmd+Q` 和系统退出请求继续统一经过 `request_exit -> shutdown_impl`。
- 不引入 `tauri-plugin-shell`、新 capability、通用桌面平台抽象或 UI 改动。

## Acceptance Criteria

- [ ] 平台选择测试证明 macOS、Windows 和其他 Unix 分别绑定 `/usr/bin/open`、`explorer.exe` 和 `xdg-open`。
- [ ] reopen 策略测试证明仅在没有可见窗口时请求恢复主窗口。
- [ ] macOS 编译与 Rust 全量测试通过。
- [ ] 现有菜单栏左键行为和 shutdown 调用路径保持不变。
- [ ] `docs/macos-porting-checklist.md` 只勾选自动化证据已经覆盖的 Phase 4A 条目。

## Notes

- Keep `prd.md` focused on requirements, constraints, and acceptance criteria.
- Lightweight tasks can remain PRD-only.
- For complex tasks, add `design.md` for technical design and `implement.md` for execution planning before `task.py start`.
