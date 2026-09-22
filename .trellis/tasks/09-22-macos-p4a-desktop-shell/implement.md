# Phase 4A macOS Desktop Shell Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 macOS 使用固定 `/usr/bin/open` 并在无可见窗口的 Dock reopen 事件中恢复主窗口，同时保持 Windows、Linux、菜单栏和 shutdown 行为不变。

**Architecture:** `commands.rs` 用一个纯映射函数选择平台 opener，现有命令继续统一进入 `open_with_system`。`lib.rs` 用一个纯策略函数判断 macOS reopen 是否需要显示窗口，并在现有 `app.run` 事件匹配中复用 `tray::show_main_window`；不新增依赖、插件或平台抽象层。

**Tech Stack:** Rust 2024、Tauri 2.11、`std::process::Command`、内置 Rust 单元测试。

---

### Task 1: 平台系统 opener

**Files:**
- Modify: `src-tauri/src/commands.rs:444-484`
- Test: `src-tauri/src/commands.rs` 的现有 `tests` 模块

- [x] **Step 1: 写入失败测试**

在 `commands.rs` 的 `tests` 模块增加以下测试，先冻结三个目标平台的程序映射：

```rust
/// 系统 opener 必须按平台选择固定程序，macOS 不依赖 Finder 的 PATH。
#[test]
fn system_opener_program_is_platform_specific() {
    assert_eq!(system_opener_program("windows"), "explorer.exe");
    assert_eq!(system_opener_program("macos"), "/usr/bin/open");
    assert_eq!(system_opener_program("linux"), "xdg-open");
}
```

- [x] **Step 2: 运行测试并确认 RED**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --locked commands::tests::system_opener_program_is_platform_specific
```

Expected: 编译失败，提示找不到 `system_opener_program`。

- [x] **Step 3: 实现最小平台映射并复用现有 spawn**

在 `open_with_system` 前增加：

```rust
/// 返回当前桌面平台固定的系统 opener；目标始终作为独立 argv 传入。
fn system_opener_program(os: &str) -> &'static str {
    match os {
        "windows" => "explorer.exe",
        "macos" => "/usr/bin/open",
        _ => "xdg-open",
    }
}
```

将 `open_with_system` 收敛为一个 `Command`，Windows 仅保留 `CREATE_NO_WINDOW` 配置：

```rust
fn open_with_system(target: impl AsRef<Path>) -> Result<(), String> {
    let mut command = std::process::Command::new(system_opener_program(std::env::consts::OS));
    command
        .arg(target.as_ref())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("无法打开 {}：{error}", target.as_ref().display()))
}
```

- [x] **Step 4: 运行测试并确认 GREEN**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --locked commands::tests::system_opener_program_is_platform_specific
```

Expected: `1 passed; 0 failed`。

### Task 2: macOS Dock reopen

**Files:**
- Modify: `src-tauri/src/lib.rs:52-56,297-305`
- Test: `src-tauri/src/lib.rs:308` 的现有 `tests` 模块

- [x] **Step 1: 写入失败测试**

在 `lib.rs` 的 `tests` 模块增加：

```rust
/// Dock reopen 只在应用没有可见窗口时恢复主窗口。
#[cfg(target_os = "macos")]
#[test]
fn dock_reopen_only_restores_when_no_window_is_visible() {
    assert!(should_show_main_on_reopen(false));
    assert!(!should_show_main_on_reopen(true));
}
```

- [x] **Step 2: 运行测试并确认 RED**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --locked tests::dock_reopen_only_restores_when_no_window_is_visible
```

Expected: 编译失败，提示找不到 `should_show_main_on_reopen`。

- [x] **Step 3: 实现最小 reopen 策略与事件分支**

在 `is_autostart_launch` 后增加：

```rust
/// macOS Dock reopen 在没有可见窗口时需要恢复主窗口。
#[cfg(target_os = "macos")]
fn should_show_main_on_reopen(has_visible_windows: bool) -> bool {
    !has_visible_windows
}
```

将 `app.run` 回调改为完整匹配，同时保持 `ExitRequested` 原逻辑：

```rust
app.run(|app, event| match event {
    #[cfg(target_os = "macos")]
    RunEvent::Reopen {
        has_visible_windows,
        ..
    } => {
        if should_show_main_on_reopen(has_visible_windows) {
            tray::show_main_window(app);
        }
    }
    RunEvent::ExitRequested { api, .. } => {
        let shutdown = app.state::<ShutdownState>();
        if !shutdown.ready.load(Ordering::Acquire) {
            api.prevent_exit();
            request_exit(app);
        }
    }
    _ => {}
});
```

- [x] **Step 4: 运行测试并确认 GREEN**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --locked tests::dock_reopen_only_restores_when_no_window_is_visible
```

Expected: `1 passed; 0 failed`。

### Task 3: 范围验证与清单

**Files:**
- Modify: `docs/macos-porting-checklist.md:230-237`
- Modify: `.trellis/tasks/09-22-macos-p4a-desktop-shell/prd.md`
- Modify: `.trellis/tasks/09-22-macos-p4a-desktop-shell/implement.md`

- [x] **Step 1: 运行受影响测试**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --locked commands::tests::system_opener_program_is_platform_specific
cargo test --manifest-path src-tauri/Cargo.toml --locked tests::dock_reopen_only_restores_when_no_window_is_visible
```

Expected: 两条测试各 `1 passed; 0 failed`。

- [x] **Step 2: 运行完整质量 Gate**

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo check --manifest-path src-tauri/Cargo.toml --locked
cargo clippy --manifest-path src-tauri/Cargo.toml --locked --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --locked
git diff --check
```

Expected: 所有命令成功，Rust 测试 `0 failed`。

- [x] **Step 3: 更新证据但保留人工 Gate**

只勾选 `docs/macos-porting-checklist.md` 中 `/usr/bin/open` 自动化条目；Dock reopen、`Cmd+Q`、菜单栏点击和 single-instance 真人操作仍保持未完成。将本任务 PRD 与实施计划中已经完成的项目改为 `[x]`，记录完整测试计数。

- [ ] **Step 4: 准备 Trellis 提交计划**

完成检查后按项目工作流向用户提出一个逻辑提交：

```text
feat(macos): integrate desktop shell behavior
```

提交范围仅包含 `commands.rs`、`lib.rs`、迁移清单和本任务 Trellis 文档；不得推送远端。
