# Serena Desktop Owned-Process Shutdown Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:executing-plans` to implement this plan task-by-task. This task is tightly coupled around one shutdown state and must be executed inline; do not dispatch parallel write agents. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ensure every observable Serena Desktop exit path completes the existing bounded shutdown of all app-owned child processes before the host terminates.

**Architecture:** Add a small generic shutdown-step runner that attempts every existing owner and aggregates errors, then route macOS final `RunEvent::Exit` through the same idempotent shutdown state used by explicit exits. Preserve each subsystem's existing Process Group/Job Object ownership and avoid any global PID registry.

**Tech Stack:** Rust, Tauri 2.11.5, Tao 0.35.3, Tokio, existing macOS Process Group and Windows Job Object implementations.

---

## File map

- Modify `src-tauri/src/commands.rs`: run every owner shutdown step and aggregate failures without short-circuiting.
- Modify `src-tauri/src/lib.rs`: make explicit exit cleanup synchronous and add the macOS final-exit fallback with idempotent state.
- Modify `.trellis/tasks/09-22-macos-dock-quit-serena-cleanup/prd.md`: record final RED/GREEN, regression, and real `.app` evidence after verification.

No new source files, dependencies, configuration, watchdogs, PID registries, or platform abstractions are introduced.

### Task 1: Attempt every shutdown owner and aggregate failures

**Files:**

- Modify: `src-tauri/src/commands.rs:943`
- Test: `src-tauri/src/commands.rs` existing `tests` module

- [x] **Step 1: Write the failing shutdown-sequence test**

Add a test-only fixture helper inside the existing `commands.rs` test module and assert that an early failure does not skip later owners:

```rust
/// shutdown 必须尝试全部 owner，并在最后汇总稳定的 owner 诊断。
#[tokio::test]
async fn shutdown_steps_attempt_all_owners_and_aggregate_failures() {
    use std::sync::{Arc, Mutex};

    let calls = Arc::new(Mutex::new(Vec::new()));
    let step = |name: &'static str, result: Result<(), String>| {
        let calls = Arc::clone(&calls);
        Box::pin(async move {
            calls.lock().unwrap().push(name);
            result
        }) as ShutdownFuture<'_>
    };

    let error = run_shutdown_steps(vec![
        ("workspace capability", step("workspace capability", Err("capability failed".into()))),
        ("agent", step("agent", Ok(()))),
        ("broker", step("broker", Err("broker failed".into()))),
        ("serena", step("serena", Ok(()))),
    ])
    .await
    .unwrap_err();

    assert_eq!(
        calls.lock().unwrap().as_slice(),
        ["workspace capability", "agent", "broker", "serena"]
    );
    assert_eq!(
        error,
        "workspace capability: capability failed; broker: broker failed"
    );
}
```

- [x] **Step 2: Run the test and verify RED**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml shutdown_steps_attempt_all_owners_and_aggregate_failures
```

Expected: compilation fails because `ShutdownFuture` and `run_shutdown_steps` do not exist.

- [x] **Step 3: Implement the generic ordered runner**

Add the following private types/functions near `shutdown_impl` with Chinese comments:

```rust
type ShutdownFuture<'a> = std::pin::Pin<
    Box<dyn std::future::Future<Output = Result<(), String>> + 'a>,
>;

/// 顺序执行全部 owner 的正式 shutdown；失败只记录，不能跳过后续 owner。
async fn run_shutdown_steps(
    steps: Vec<(&'static str, ShutdownFuture<'_>)>,
) -> Result<(), String> {
    let mut errors = Vec::new();
    for (owner, step) in steps {
        if let Err(error) = step.await {
            errors.push(format!("{owner}: {error}"));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}
```

Refactor `shutdown_impl` so it cancels the active operation, holds the existing management lock, clones the required `Arc` owners, and calls:

```rust
run_shutdown_steps(vec![
    (
        "workspace capability",
        Box::pin(async move { capability_supervisor.shutdown_capability_runtimes().await }),
    ),
    (
        "agent",
        Box::pin(async move { product.shutdown().await }),
    ),
    (
        "broker",
        Box::pin(async move { shutdown_broker.shutdown().await }),
    ),
    (
        "serena",
        Box::pin(async move { serena_supervisor.stop() }),
    ),
])
.await
```

The production code must preserve the current owner order and must not use `?` between owner shutdown steps.

- [x] **Step 4: Run the focused tests and verify GREEN**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml shutdown_steps_attempt_all_owners_and_aggregate_failures
cargo test --manifest-path src-tauri/Cargo.toml commands::tests
```

Expected: both commands pass; the new test observes all four owner names in order and the exact aggregate error.

### Task 2: Route every observable macOS exit through shutdown once

**Files:**

- Modify: `src-tauri/src/lib.rs:38-104`
- Modify: `src-tauri/src/lib.rs:303-321`
- Test: `src-tauri/src/lib.rs` existing `tests` module

- [x] **Step 1: Write failing idempotency and retry tests**

Add these tests to the existing `lib.rs` test module:

```rust
/// shutdown 成功后最终 Exit 不得再次调用 owner。
#[test]
fn shutdown_once_marks_ready_and_skips_reentry() {
    let state = ShutdownState::default();
    let calls = std::cell::Cell::new(0);
    run_shutdown_once(&state, || {
        calls.set(calls.get() + 1);
        Ok(())
    })
    .unwrap();
    run_shutdown_once(&state, || {
        calls.set(calls.get() + 1);
        Ok(())
    })
    .unwrap();
    assert_eq!(calls.get(), 1);
    assert!(state.ready.load(Ordering::Acquire));
}

/// 可取消退出失败后必须允许用户再次触发完整 shutdown。
#[test]
fn shutdown_once_failure_allows_retry() {
    let state = ShutdownState::default();
    assert_eq!(
        run_shutdown_once(&state, || Err("fixture failure".into())),
        Err("fixture failure".into())
    );
    assert!(!state.started.load(Ordering::Acquire));
    run_shutdown_once(&state, || Ok(())).unwrap();
    assert!(state.ready.load(Ordering::Acquire));
}
```

- [x] **Step 2: Run the tests and verify RED**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml shutdown_once_
```

Expected: compilation fails because `run_shutdown_once` does not exist.

- [x] **Step 3: Implement the idempotent shutdown coordinator**

Add this helper beside `ShutdownState`:

```rust
/// 对所有可感知退出提供单一、可重试且成功后幂等的 shutdown gate。
fn run_shutdown_once(
    state: &ShutdownState,
    shutdown: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    if state.ready.load(Ordering::Acquire) {
        return Ok(());
    }
    if state.started.swap(true, Ordering::AcqRel) {
        return Err("退出清理正在进行".into());
    }
    match shutdown() {
        Ok(()) => {
            state.ready.store(true, Ordering::Release);
            Ok(())
        }
        Err(error) => {
            state.started.store(false, Ordering::Release);
            Err(error)
        }
    }
}
```

Refactor `request_exit` to call `run_shutdown_once` synchronously after hiding the window. On success call `app.exit(0)`; on failure append the existing shutdown log and restore the main window. Remove the `spawn_blocking` wrapper so the host cannot finish while cleanup is detached.

- [x] **Step 4: Add the macOS final-exit fallback**

Add an explicit macOS `RunEvent::Exit` arm before the catch-all arm:

```rust
#[cfg(target_os = "macos")]
RunEvent::Exit => {
    let shutdown = app.state::<ShutdownState>();
    if let Err(error) = run_shutdown_once(&shutdown, || commands::shutdown_impl(app)) {
        logs::append(
            &app.state::<std::sync::Arc<SupervisorState>>().paths.app_log,
            "shutdown",
            &error,
        );
    }
}
```

This fallback must remain macOS-only. The existing `ExitRequested` arm and Windows runtime behavior stay unchanged.

- [x] **Step 5: Run focused tests and verify GREEN**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml shutdown_once_
cargo test --manifest-path src-tauri/Cargo.toml should_show_main_on_reopen
```

Expected: idempotency、retry 与现有 macOS app lifecycle 测试均通过。

### Task 3: Verify source behavior and the real macOS app

**Files:**

- Modify after evidence is collected: `.trellis/tasks/09-22-macos-dock-quit-serena-cleanup/prd.md`

- [x] **Step 1: Remove the currently reproduced orphan by exact Process Group**

Resolve PID `9911` again before acting and require the exact managed command plus `PGID == PID`. Send `SIGTERM` only to that resolved group, wait for exit, and use `SIGKILL` only if the same group remains after the existing bounded grace. Confirm PID `9923` and port `9121` are gone. Never match or kill by the broad process name `serena`.

- [x] **Step 2: Run source-level verification**

Run:

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo check --manifest-path src-tauri/Cargo.toml --all-targets
cargo test --manifest-path src-tauri/Cargo.toml
npm run lint
npm run build
npm test
git diff --check
```

Expected: all commands exit `0`; Rust reports zero failed tests。当前机器只安装 `aarch64-apple-darwin` target，因此 Windows 保证通过 macOS-only `RunEvent::Exit` 条件编译边界、未修改 Windows Job Object 代码以及通用 shutdown runner 测试审查，不伪造本机 Windows cross-check 结果。

- [x] **Step 3: Build the release app**

Run:

```bash
npm run tauri build
```

Expected: exit `0` and `src-tauri/target/release/bundle/macos/Serena Desktop.app` exists. Ad-hoc signing and skipped notarization warnings remain acceptable for this local verification artifact.

- [x] **Step 4: Execute the Dock-equivalent integration gate**

Launch the built `.app` with LaunchServices, then record only processes whose command contains the app-managed runtime path under:

```text
/Users/lifeilin/Library/Application Support/io.github.lifei6671.serena-desktop/runtime/
```

Send the standard macOS Quit Apple Event:

```bash
osascript -e 'tell application id "io.github.lifei6671.serena-desktop" to quit'
```

Wait within the shutdown bounds, then assert:

- no `Serena Desktop` process remains;
- no previously recorded app-owned PID or Process Group member remains;
- no process using the managed `broker.yml` remains;
- ports `9121` and the configured Broker/remote listener ports are not held by the exited app;
- the application log contains shutdown completion rather than another orphan startup-only tail.

- [x] **Step 5: Record evidence and perform scope review**

Mark each satisfied PRD acceptance criterion and append the exact RED/GREEN, regression, build, PID/PGID, and port evidence. Confirm `git status --short` contains only the intended Rust files and current Trellis task artifacts.

- [ ] **Step 6: Commit through Trellis Phase 3.4**

After presenting a single batched commit plan and receiving confirmation, commit the coherent implementation and task evidence. Do not push. Archive the task and record the Trellis journal only after the work commits succeed.
