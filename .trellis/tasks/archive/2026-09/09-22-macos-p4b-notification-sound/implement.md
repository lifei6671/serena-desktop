# Phase 4B macOS Notification Sound Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 macOS Agent 终态提示音调用系统首选警告音，同时保持系统通知与提示音两个独立开关及现有跨平台行为。

**Architecture:** 只在 `agent_notification.rs` 增加 macOS 条件编译的 AudioToolbox FFI；现有 `AgentNotificationPlan` 继续独立裁决通知和声音，通知 builder 不附加声音。真实可听性由默认忽略的人工测试验证，通知权限和点击激活继续由 `.app` 真机 Gate 验证。

**Tech Stack:** Rust 2024、Tauri 2、macOS AudioToolbox、Cargo test / fmt / check / Clippy、Trellis。

---

## 文件边界

- Modify: `src-tauri/src/agent_notification.rs` — macOS 原生提示音绑定、平台条件编译和可重复人工 Gate。
- Modify: `docs/macos-porting-checklist.md` — 记录 Phase 4B 自动化及真机证据，不提前勾选未验证的权限或点击项目。
- Modify: `.trellis/tasks/09-22-macos-p4b-notification-sound/prd.md` — 更新验收结果和限制。
- Modify: `.trellis/tasks/09-22-macos-p4b-notification-sound/implement.md` — 执行时逐项勾选本计划。

### Task 1：用 RED Gate 冻结 macOS 系统提示音契约

**Files:**
- Modify: `src-tauri/src/agent_notification.rs:145-210`
- Test: `src-tauri/src/agent_notification.rs:145-210`

- [x] **Step 1：先增加默认忽略的 macOS 可听性测试**

在 `tests` 模块末尾增加以下测试。它既冻结 SDK 的用户首选警告音 ID，也为真人听音保留可重复命令；默认忽略，完整测试不会每次发声。

```rust
    /// 人工 Gate：macOS 必须提交用户首选警告音，并留出实际播放时间。
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "需要真人确认系统首选警告音可听"]
    fn macos_user_preferred_alert_is_audible() {
        assert_eq!(
            std::hint::black_box(USER_PREFERRED_ALERT_SOUND_ID),
            0x0000_1000
        );
        play_system_sound().expect("macOS 用户首选警告音应可提交");
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
```

- [x] **Step 2：运行人工 Gate，确认 RED 来自缺少原生声音常量**

Run:

```bash
cd src-tauri
cargo test --locked agent_notification::tests::macos_user_preferred_alert_is_audible -- --exact --ignored --nocapture
```

Expected: 编译失败，错误指出 `USER_PREFERRED_ALERT_SOUND_ID` 不存在；不得通过修改断言绕过 RED。

- [x] **Step 3：保留 RED 证据，不单独提交不可编译状态**

记录错误摘要到本任务 `prd.md` 的 `Verification` 小节，然后直接进入 Task 2；避免留下不可编译提交。

### Task 2：接入 AudioToolbox 并保持平台边界

**Files:**
- Modify: `src-tauri/src/agent_notification.rs:125-138`
- Test: `src-tauri/src/agent_notification.rs:145-220`

- [x] **Step 1：增加 macOS 常量和最小 FFI 声明**

在 Windows `play_system_sound` 之前加入：

```rust
/// macOS SDK 定义的用户首选警告音 ID。
#[cfg(target_os = "macos")]
const USER_PREFERRED_ALERT_SOUND_ID: u32 = 0x0000_1000;

#[cfg(target_os = "macos")]
#[link(name = "AudioToolbox", kind = "framework")]
unsafe extern "C" {
    /// 提交系统警告音；空 completion block 表示调用方无需完成回调。
    #[link_name = "AudioServicesPlayAlertSoundWithCompletion"]
    fn audio_services_play_alert_sound_with_completion(
        system_sound_id: u32,
        completion_block: *const std::ffi::c_void,
    );
}
```

- [x] **Step 2：将非 Windows 静默分支拆成 macOS 原生实现和其他平台分支**

保留 Windows 函数原样，将当前 `#[cfg(not(windows))]` 函数替换为：

```rust
/// macOS 使用用户在系统设置中选择的警告音；提交调用本身没有错误返回值。
#[cfg(target_os = "macos")]
fn play_system_sound() -> Result<(), ()> {
    // SAFETY: 函数由当前目标 SDK 的 AudioToolbox 提供，声音 ID 来自同一 SDK，
    // completion block 明确允许为空且调用不转移任何 Rust 所有权。
    unsafe {
        audio_services_play_alert_sound_with_completion(
            USER_PREFERRED_ALERT_SOUND_ID,
            std::ptr::null(),
        );
    }
    Ok(())
}

/// 其他非 Windows、非 macOS 平台保持既有静默行为。
#[cfg(all(not(windows), not(target_os = "macos")))]
fn play_system_sound() -> Result<(), ()> {
    Ok(())
}
```

同时把 Windows 函数上方注释收窄为“Windows 使用轻量系统提示音”，不得给通知 builder 增加 `.sound(...)`。

- [x] **Step 3：运行 macOS 人工 Gate，确认 GREEN 和实际可听性**

Run:

```bash
cd src-tauri
cargo test --locked agent_notification::tests::macos_user_preferred_alert_is_audible -- --exact --ignored --nocapture
```

Expected: `1 passed; 0 failed`，并且机器播放一次当前用户首选警告音。命令通过但真人未听到时，只记录“调用通过、可听性未确认”，不得勾选声音真机 Gate。

- [x] **Step 4：验证两个能力开关仍彼此独立**

Run:

```bash
cd src-tauri
cargo test --locked agent_notification::tests::terminal_policy_respects_independent_capability_toggles -- --exact
```

Expected: `1 passed; 0 failed`；测试继续覆盖仅通知、仅声音和二者关闭。通知 builder 仍未设置声音，因此二者同时开启时只有 AudioToolbox 一处声音来源。

- [x] **Step 5：格式化并提交功能实现**

Run:

```bash
cd src-tauri
cargo fmt --all
cd ..
git diff --check
git add src-tauri/src/agent_notification.rs
git commit -m "feat(macos): play native agent alert sound"
```

Expected: 提交只包含 `agent_notification.rs`，不包含 Trellis 文档或无关文件。

### Task 3：运行质量 Gate 并记录真实证据

**Files:**
- Modify: `docs/macos-porting-checklist.md:230-246`
- Modify: `.trellis/tasks/09-22-macos-p4b-notification-sound/prd.md`
- Modify: `.trellis/tasks/09-22-macos-p4b-notification-sound/implement.md`

- [x] **Step 1：运行完整相关 Rust Gate**

Run:

```bash
cd src-tauri
cargo fmt --all -- --check
cargo check --locked
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

Expected: 四条命令均成功；完整测试中的人工声音测试显示为 ignored，其他测试无失败。若任何命令失败，先修复本任务引入的问题并重新运行受影响命令，不弱化 lint 或测试。

- [x] **Step 2：构建真实 macOS `.app`**

Run:

```bash
npm run tauri build
file "src-tauri/target/release/bundle/macos/Serena Desktop.app/Contents/MacOS/serena-desktop"
```

Expected: 构建成功，bundle 主程序包含 `arm64`。本步骤只生成本地候选，不签发 Developer ID、不公证、不创建 Release。

- [ ] **Step 3：执行通知与声音人工矩阵**

> 阻塞记录：2026-09-22 从新构建 `.app` 提交无修改短任务时，任务在派发前返回 `CODEX_COMPATIBILITY_BLOCKED`。本机两个 ARM64 候选为 `codex-cli 0.155.1`，当前精确白名单为 `0.153.4`，且本机没有保留 `0.153.4`。因此四种开关组合、通知权限和点击激活均保持 `未验证`；测试任务已取消，未删除。

Run:

```bash
open "src-tauri/target/release/bundle/macos/Serena Desktop.app"
```

在应用设置中依次验证以下组合，每次运行一个能够正常结束的短 Agent 任务：

| 系统通知 | 提示音 | 预期 |
|---|---|---|
| 关闭 | 开启 | 无通知；播放一次系统首选警告音 |
| 开启 | 关闭 | 出现无声通知 |
| 开启 | 开启 | 出现通知；只播放一次提示音 |
| 关闭 | 关闭 | 不产生通知或声音 |

再从通知中心点击已投递通知，记录应用是否激活主窗口；在系统设置中拒绝通知后重复一次并记录投递结果。该步骤需要真人观察，未观察的格子明确记录为 `未验证`，不得推断通过。

- [x] **Step 4：更新清单和任务证据**

在 `docs/macos-porting-checklist.md` 的 Phase 4 当前状态补充 AudioToolbox 实现及本次测试结果。只有 Step 3 的真人听音确认成功时，才把“为 macOS 实现真实声音提示”改为 `[x]`；通知权限和点击激活仅在对应矩阵真实通过时勾选。

在 `prd.md` 增加 `## Verification`，记录：

- RED 命令及缺少常量的错误；
- 人工声音 Gate 的命令、测试结果和是否实际听到；
- fmt/check/Clippy/完整测试的实际结果；
- `.app` 构建结果；
- 通知权限、点击激活及四种开关组合的真实状态或 `未验证`。

按实际结果勾选 PRD Acceptance Criteria；任何未完成的人工项保持 `[ ]`。

- [x] **Step 5：提交证据文档**

Run:

```bash
git diff --check
git add docs/macos-porting-checklist.md .trellis/tasks/09-22-macos-p4b-notification-sound/prd.md .trellis/tasks/09-22-macos-p4b-notification-sound/implement.md
git commit -m "docs(macos): record phase 4b sound evidence"
```

Expected: 文档只陈述实际执行结果，不把未执行的权限或点击测试写成通过。

### Task 4：最终审计与 Trellis 收口

**Files:**
- Review: `src-tauri/src/agent_notification.rs`
- Review: `docs/macos-porting-checklist.md`
- Review: `.trellis/tasks/09-22-macos-p4b-notification-sound/`

- [x] **Step 1：审计范围和提交历史**

Run:

```bash
git status --short --branch
git log --oneline --decorate -6
git diff --check HEAD~2..HEAD
```

Expected: 产品修改仅涉及 `agent_notification.rs`；其余修改是本任务和 Phase 4 清单文档；没有依赖、前端、LAN 或通知点击监听实现。

- [x] **Step 2：检查是否需要更新 Trellis spec**

本任务只增加单一平台 FFI，若未形成跨模块可复用规范，则明确记录“无需更新 spec”，不为一次性实现新增规范文件。

- [ ] **Step 3：归档任务并记录会话**

在所有自动化 Gate 通过、实际完成项已准确记录后运行：

```bash
python3 ./.trellis/scripts/task.py finish
python3 ./.trellis/scripts/task.py archive 09-22-macos-p4b-notification-sound
git add .trellis/tasks/09-21-macos-p4-desktop-integration/task.json .trellis/tasks/archive/2026-09/09-22-macos-p4b-notification-sound
git commit -m "chore(task): archive macos phase 4b notification sound"
phase4b_archive_commit=$(git rev-parse HEAD)
python3 ./.trellis/scripts/add_session.py --title "Phase 4B macOS 通知与真实提示音" --commit "$phase4b_archive_commit" --summary "接入 AudioToolbox 用户首选警告音，保持通知与声音独立开关，并记录通知权限及点击激活的真机 Gate。"
git add .trellis/workspace/codex
git commit -m "chore: record phase 4b notification session"
```

归档和 journal 修改分别进入聚焦的 `chore(task)` / `chore` 提交，不推送远端。
