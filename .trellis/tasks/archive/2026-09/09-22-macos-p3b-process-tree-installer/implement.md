# Phase 3B macOS uv 与进程树实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 完成 macOS Serena/Quick Tunnel 进程树所有权和固定摘要 uv 安装，并修复 Codex 终止升级竞态。

**Architecture:** 以私有 Darwin identity/process-group helper 支撑同步 Serena 与异步 cloudflared owner；平台安装路径在 `installer.rs` 内保持显式 `cfg` 分支。Windows authority 不改。

**Tech Stack:** Rust 2024、libc/libproc、Tokio process、reqwest blocking、flate2/tar、tempfile、Tauri 2。

---

### Task 1: Codex SIGKILL 身份门

**Files:**
- Modify: `src-tauri/src/agent/codex/macos_runtime.rs`

- [x] 将 `leader_exits_after_term_but_continuous_group_escalates` 改为期望 `CODEX_RUNTIME_TERMINATION_UNCONFIRMED`，并验证 stubborn descendant 仍存活。
- [x] 运行该单测，确认旧实现失败。
- [x] 删除 leader-missing 的 SIGKILL 许可，仅接受重新匹配完整 identity。
- [x] 运行 `cargo test --manifest-path src-tauri/Cargo.toml --locked agent::codex::macos_runtime::tests`。

### Task 2: Darwin 进程组所有权

**Files:**
- Create: `src-tauri/src/macos_process.rs`
- Modify: `src-tauri/src/lib.rs`

- [x] 先添加 fixture 与测试：`setsid` 后 identity 为 `PID=PGID=SID`；终止后 child reaped/group empty；另一个 process group 仍存活；leader 提前退出时拒绝对残留 group 升级。
- [x] 运行模块测试，确认缺少实现而失败。
- [x] 实现 `configure_command`、`Identity::capture/verify`、group signal/empty 和同步/异步等待所需最小 API，所有新函数与核心分支写中文注释。
- [x] 运行模块测试通过。

### Task 3: Serena 进程树接入

**Files:**
- Modify: `src-tauri/src/serena.rs`

- [x] 增加 macOS `ManagedProcess` identity 测试与 `run_with_timeout` 后代超时清理测试，并先确认失败。
- [x] 主 Serena spawn 前配置 `setsid`，spawn 后验证 identity；停止使用 `SIGTERM -> bounded grace -> SIGKILL` 并要求 group empty。
- [x] `run_with_timeout` 在 macOS 使用同一所有权边界，错误或超时收口完整 group。
- [x] 运行 `cargo test --manifest-path src-tauri/Cargo.toml --locked serena::tests`。

### Task 4: Quick Tunnel 进程树接入

**Files:**
- Create: `src-tauri/src/remote/process_macos.rs`
- Modify: `src-tauri/src/remote/process.rs`
- Modify: `src-tauri/src/remote/quick_tunnel.rs`

- [x] 扩展停止测试为带 descendant 的进程树，并增加另一个非目标 group 存活断言；先确认失败。
- [x] macOS `ManagedChild::spawn` 在 exec 前 `setsid` 并验证 identity，`start_kill/wait` 改为完整 group 收口。
- [x] 停止失败继续通过现有 `pending_child` 保留 owner。
- [x] 运行 `cargo test --manifest-path src-tauri/Cargo.toml --locked remote::quick_tunnel::tests`。

### Task 5: macOS 固定 uv 安装

**Files:**
- Modify: `src-tauri/src/installer.rs`
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/Cargo.lock`

- [x] 先添加 asset matrix、Finder PATH 候选、摘要拒绝与精确归档成员提取测试，确认旧实现失败。
- [x] Windows 保留 `winget`；macOS arm64 下载固定 0.12.17 归档，限制大小、验证摘要、精确提取并原子安装为 `0755`。
- [x] `find_uv` 优先返回应用托管 uv，并覆盖 PATH、用户目录和 Homebrew 固定位置。
- [x] 运行 `cargo test --manifest-path src-tauri/Cargo.toml --locked installer::tests`。

### Task 6: 收口验证与清单

**Files:**
- Modify: `docs/macos-porting-checklist.md`
- Modify: `.trellis/tasks/09-21-macos-p3-cli-process-tree/prd.md`
- Modify: `.trellis/tasks/09-22-macos-p3b-process-tree-installer/prd.md`

- [x] 运行 `cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check`。
- [x] 运行 `cargo clippy --manifest-path src-tauri/Cargo.toml --locked --all-targets -- -D warnings`。
- [x] 运行 `cargo test --manifest-path src-tauri/Cargo.toml --locked`。
- [x] 只勾选已有自动化证据覆盖的 Phase 3 条目；Finder/LaunchAgent/外置卷真人验收保持未完成。
