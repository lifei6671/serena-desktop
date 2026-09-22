# Phase 4A macOS 桌面壳层设计

## 范围

本任务只处理系统 opener、Dock reopen 和既有退出路径的契约确认。通知/声音、LAN `Info.plist`、文件权限、LaunchAgent 真人验证和前端平台文案分别留给后续 Phase 4 子任务。

## 方案选择

采用最小原生分支方案：继续由 Rust `std::process::Command` 启动平台系统 opener，并直接在现有 `app.run` 事件闭包中处理 macOS reopen。

未采用以下方案：

- `tauri-plugin-shell`：需要新增依赖与 capability，但当前 opener 只有固定程序和受控目标参数，没有现实收益。
- 通用 DesktopPlatform trait：当前只有两个局部分支，新增 interface/factory 会扩大修改面。
- 一次完成全部 Phase 4：通知权限、声音、plist 和 UI 文案具有不同验证边界，不应与窗口生命周期混合提交。

## 系统 opener

`commands.rs` 保留唯一 `open_with_system` 入口，并把程序选择提取为无副作用的小函数，便于按目标平台编译和测试：

- Windows：`explorer.exe`
- macOS：`/usr/bin/open`
- 其他 Unix：`xdg-open`

目标继续使用单独的 `.arg(target)`，stdin/stdout/stderr 继续指向 null。spawn 失败沿用现有包含目标路径的中文错误，不增加 fallback，也不读取 shell 配置。

## Dock reopen 与退出

`lib.rs` 在现有 `app.run` 回调中增加 macOS `RunEvent::Reopen` 分支。事件的 `has_visible_windows` 为 `false` 时调用现有 `tray::show_main_window`；为 `true` 时保持现状。该决策由一个纯策略函数表达并单测，不创建新的窗口管理层。

已有行为保持不变：

- 菜单栏左键直接调用 `tray::show_main_window`。
- single-instance 非自启动二次启动继续调用同一函数。
- 菜单栏“退出”和 `RunEvent::ExitRequested` 继续调用 `request_exit`。
- `request_exit` 继续隐藏窗口、执行 `shutdown_impl`，只有 shutdown 成功后才 `exit(0)`；失败则恢复主窗口。

## 测试与验收

- 在 `commands.rs` 单测中验证当前构建目标选择 `/usr/bin/open`，同时用 cfg 分支冻结 Windows 和其他 Unix 的映射。
- 在 `lib.rs` 单测中验证 reopen 在 `has_visible_windows=false` 时返回显示决策，`true` 时不动作。
- 运行 targeted tests、`cargo fmt --check`、`cargo check --locked`、`cargo clippy --locked --all-targets -- -D warnings` 和 `cargo test --locked`。
- 自动化测试不能替代 Dock、`Cmd+Q` 和菜单栏点击的真人验收，因此清单中这些人工项目继续保持未完成。

## 风险边界

- `/usr/bin/open` 是 macOS 固定系统路径；不搜索 PATH，避免 Finder 环境差异。
- reopen 只恢复已有 `main` 窗口，不创建新窗口。
- 本任务不改变 Tauri capability、bundle、签名或前端视觉实现。
