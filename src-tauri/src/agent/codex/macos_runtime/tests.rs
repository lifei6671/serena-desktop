use super::*;
use crate::agent::codex::macos_launcher::{MacosLaunchRequest, process_group_members};
use std::{
    io::Write,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

static RUNTIME_IDS: AtomicU64 = AtomicU64::new(1);

/// 在测试失败或 panic 时强制清理 fixture Process Group 并回收直接 child。
struct FixtureCleanup {
    leader_pid: libc::pid_t,
    pgid: libc::pid_t,
    armed: bool,
}

impl FixtureCleanup {
    /// 从 Runtime 记录清理所需的直接 child 与 Process Group 身份。
    fn armed(runtime: &MacosRuntime) -> Self {
        Self {
            leader_pid: runtime.identity.pid,
            pgid: runtime.identity.pgid,
            armed: true,
        }
    }

    /// 正常 shutdown 已完成时停用兜底清理。
    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for FixtureCleanup {
    /// 兜底路径只用于测试 teardown，不生成 Runtime 终止证据。
    fn drop(&mut self) {
        if !self.armed {
            return;
        }

        // SAFETY: 测试只向刚创建且持续持有 ownership 的 fixture Process Group 发送 SIGKILL。
        unsafe { libc::killpg(self.pgid, libc::SIGKILL) };
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let mut status = 0;
            // SAFETY: leader_pid 来自当前测试创建的直接 child，WNOHANG 保证 Drop 不无界阻塞。
            let reaped = unsafe { libc::waitpid(self.leader_pid, &mut status, libc::WNOHANG) };
            let group_empty = process_group_members(self.pgid)
                .map(|members| members.is_empty())
                .unwrap_or(false);
            if (reaped == self.leader_pid || reaped == -1) && group_empty {
                break;
            }
            if Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

/// 直接使用 rustc 编译固定 fixture，避免测试经过 shell command string。
fn fixture(directory: &Path) -> PathBuf {
    let executable = directory.join("macos-runtime-child");
    let output = Command::new("rustc")
        .args(["--edition=2024", "--crate-name", "macos_runtime_child"])
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/macos_runtime_child.rs"))
        .arg("-o")
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    executable
}

/// 创建由当前测试持续持有 TempDir 与 ownership 的真实 Runtime fixture。
fn runtime_fixture(mode: &str) -> (tempfile::TempDir, MacosRuntime, PathBuf, PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let executable = fixture(directory.path());
    let marker = directory.path().join("leaf.pid");
    let ready = PathBuf::from(format!("{}.ready", marker.display()));
    let store = tauri::async_runtime::block_on(crate::agent::store::StateStore::open(
        directory.path().join("state"),
    ))
    .unwrap();
    let runtime = MacosRuntime::create(
        store,
        "test-host".into(),
        MacosLaunchRequest {
            executable,
            args: vec![mode.into(), marker.as_os_str().to_owned()],
            current_dir: directory.path().to_owned(),
            runtime_instance_id: format!(
                "macos-runtime-{}-{}",
                std::process::id(),
                RUNTIME_IDS.fetch_add(1, Ordering::Relaxed),
            ),
        },
    )
    .unwrap();
    let _: &dyn Write = &runtime.child.stdin;
    (directory, runtime, marker, ready)
}

/// create 成功后必须持久化 launcher 已验证的完整 macOS 身份组。
#[test]
fn store_identity_is_persisted_after_create() {
    let (_directory, runtime, _marker, _ready) = runtime_fixture("tree");
    let mut cleanup = FixtureCleanup::armed(&runtime);
    let record = tauri::async_runtime::block_on(runtime.store.runtime(runtime.id.clone()))
        .unwrap()
        .unwrap();
    assert_eq!(record.state, "starting");
    assert_eq!(record.runtime_platform, "macos");
    assert_eq!(record.codex_pid, Some(runtime.identity.pid as u32));
    assert_eq!(
        record.containment_process_group_id,
        Some(i64::from(runtime.identity.pgid))
    );
    assert_eq!(
        record.containment_session_id,
        Some(i64::from(runtime.identity.sid))
    );
    assert_eq!(
        record.codex_process_start_token.as_deref(),
        Some(runtime.identity.start_token.encode().as_str())
    );
    cleanup_fixture(runtime);
    cleanup.disarm();
}

/// spawn 后 start 写入失败时必须把仍可 shutdown 的 Runtime ownership 交还调用方。
#[test]
fn store_start_failure_retains_runtime_ownership() {
    let directory = tempfile::tempdir().unwrap();
    let executable = fixture(directory.path());
    let marker = directory.path().join("leaf.pid");
    let store = tauri::async_runtime::block_on(crate::agent::store::StateStore::open(
        directory.path().join("state"),
    ))
    .unwrap();
    store
        .write_blocking(|transaction| {
            transaction
                .execute_batch(
                    "CREATE TRIGGER reject_macos_start BEFORE UPDATE OF state ON runtime_instances
                     WHEN NEW.state='starting'
                     BEGIN SELECT RAISE(ABORT,'fixture start failure'); END;",
                )
                .map_err(|error| error.to_string())?;
            Ok(())
        })
        .unwrap();
    let failure = MacosRuntime::create(
        store.clone(),
        "test-host".into(),
        MacosLaunchRequest {
            executable,
            args: vec!["ignore-tree".into(), marker.as_os_str().to_owned()],
            current_dir: directory.path().to_owned(),
            runtime_instance_id: "store-start-failure".into(),
        },
    )
    .unwrap_err();
    assert_eq!(failure.code, "CODEX_RUNTIME_STORE_FAILED");
    let runtime = *failure.runtime.expect("spawn 后失败必须保留 Runtime");
    assert!(
        !process_group_members(runtime.identity.pgid)
            .unwrap()
            .is_empty()
    );
    let pgid = runtime.identity.pgid;
    let evidence = runtime
        .shutdown(Duration::from_millis(50), Duration::from_secs(2))
        .unwrap();
    assert_eq!(evidence.pgid, pgid);
    assert!(process_group_members(pgid).unwrap().is_empty());
    let record = tauri::async_runtime::block_on(store.runtime("store-start-failure".into()))
        .unwrap()
        .unwrap();
    assert_eq!(record.state, "unknown");
    assert_eq!(record.termination_evidence_state, "unknown");
}

/// terminating 写入失败时必须在发出任何信号前停止，并保留 Runtime ownership。
#[test]
fn store_terminating_failure_is_unknown_without_signal() {
    let (_directory, runtime, _marker, ready) = runtime_fixture("tree");
    let mut cleanup = FixtureCleanup::armed(&runtime);
    wait_file(&ready);
    let store = runtime.store.clone();
    let runtime_id = runtime.id.clone();
    let pgid = runtime.identity.pgid;
    store
        .write_blocking(|transaction| {
            transaction
                .execute_batch(
                    "CREATE TRIGGER reject_macos_terminating
                     BEFORE UPDATE OF state ON runtime_instances
                     WHEN NEW.state='terminating'
                     BEGIN SELECT RAISE(ABORT,'fixture terminating failure'); END;",
                )
                .map_err(|error| error.to_string())?;
            Ok(())
        })
        .unwrap();

    let failure = runtime
        .shutdown(Duration::from_millis(20), Duration::from_millis(20))
        .unwrap_err();
    assert_eq!(failure.code, "CODEX_RUNTIME_STORE_FAILED");
    assert!(!process_group_members(pgid).unwrap().is_empty());
    let record = tauri::async_runtime::block_on(store.runtime(runtime_id))
        .unwrap()
        .unwrap();
    assert_eq!(record.state, "unknown");
    assert_eq!(record.termination_evidence_state, "unknown");

    cleanup_fixture(*failure.runtime);
    cleanup.disarm();
}

/// 在固定五秒 deadline 内等待 fixture ready 文件。
fn wait_file(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !path.exists() {
        assert!(
            Instant::now() < deadline,
            "等待 fixture 文件超时: {}",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// 在两秒内通过 try_wait 回收直接 child，不把 leader 退出等同于组为空。
fn wait_direct_child_exit(runtime: &mut MacosRuntime) {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if runtime.child.process.try_wait().unwrap().is_some() {
            break;
        }
        assert!(Instant::now() < deadline, "直接 child 未按期退出");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// 专用测试清理：强制收口 fixture，不生成或伪造生产终止证据。
fn cleanup_fixture(mut runtime: MacosRuntime) {
    let pgid = runtime.identity.pgid;
    let _ = signal_group(pgid, libc::SIGKILL);
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut direct_child_reaped = false;
    loop {
        direct_child_reaped |= runtime.child.process.try_wait().unwrap().is_some();
        let group_empty = process_group_members(pgid)
            .map(|members| members.is_empty())
            .unwrap_or(false);
        if direct_child_reaped && group_empty {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "测试 fixture 直接 child 或 Process Group 未按期退出"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// unknown failure 必须使用冻结设计中的稳定错误码并保留 Runtime ownership。
#[test]
fn unknown_uses_frozen_unconfirmed_code() {
    let (_directory, runtime, _marker, ready) = runtime_fixture("tree");
    let cleanup = FixtureCleanup::armed(&runtime);
    wait_file(&ready);

    let failure = runtime.unknown("测试 unknown 失败");
    assert_eq!(failure.code, "CODEX_RUNTIME_TERMINATION_UNCONFIRMED");

    // 先释放 failure 中的 child handle，再由 guard 强制收口并回收 fixture。
    drop(failure);
    drop(cleanup);
}

/// 响应 SIGTERM 的 leader 与 leaf 必须在 grace 内退出并形成完整证据。
#[test]
fn shutdown_reaps_term_responsive_child_and_group() {
    let (_directory, runtime, _marker, ready) = runtime_fixture("tree");
    let mut cleanup = FixtureCleanup::armed(&runtime);
    wait_file(&ready);
    let store = runtime.store.clone();
    let expected_id = runtime.id.clone();
    let expected_pid = runtime.identity.pid;
    let expected_token = runtime.identity.start_token.clone();
    let evidence = runtime
        .shutdown(Duration::from_secs(2), Duration::from_secs(2))
        .unwrap();
    assert_eq!(evidence.runtime_id, expected_id);
    assert_eq!(evidence.leader_pid, expected_pid);
    assert_eq!(evidence.pgid, expected_pid);
    assert_eq!(evidence.process_start_token, expected_token);
    assert!(evidence.observed_at > 0);
    assert!(evidence.direct_child_reaped);
    assert!(evidence.host_continuous_ownership);
    assert!(process_group_members(evidence.pgid).unwrap().is_empty());
    let record = tauri::async_runtime::block_on(store.runtime(expected_id))
        .unwrap()
        .unwrap();
    assert_eq!(record.state, "terminated");
    assert_eq!(
        record.termination_evidence_type.as_deref(),
        Some("macos_live_process_group_empty")
    );
    cleanup.disarm();
}

/// direct child 正常退出且 group 已空时，live Host 可提交 group-empty evidence。
#[test]
fn shutdown_records_normal_exit_group_empty_evidence() {
    let (_directory, mut runtime, _marker, ready) = runtime_fixture("leader-only-exit");
    let mut cleanup = FixtureCleanup::armed(&runtime);
    wait_file(&ready);
    let store = runtime.store.clone();
    let runtime_id = runtime.id.clone();
    runtime.child.stdin.write_all(b"x").unwrap();
    wait_direct_child_exit(&mut runtime);

    let evidence = runtime
        .shutdown(Duration::from_millis(20), Duration::from_millis(20))
        .unwrap();
    assert!(process_group_members(evidence.pgid).unwrap().is_empty());
    let record = tauri::async_runtime::block_on(store.runtime(runtime_id))
        .unwrap()
        .unwrap();
    assert_eq!(record.state, "terminated");
    assert_eq!(
        record.termination_evidence_type.as_deref(),
        Some("macos_live_process_group_empty")
    );
    cleanup.disarm();
}

/// group 已空但 complete evidence 提交失败时必须保持 unknown，不能形成持久化完成事实。
#[test]
fn store_complete_failure_retains_unknown_runtime() {
    let (_directory, runtime, _marker, ready) = runtime_fixture("tree");
    let cleanup = FixtureCleanup::armed(&runtime);
    wait_file(&ready);
    let store = runtime.store.clone();
    let runtime_id = runtime.id.clone();
    let pgid = runtime.identity.pgid;
    store
        .write_blocking(|transaction| {
            transaction
                .execute_batch(
                    "CREATE TRIGGER reject_live_evidence
                     BEFORE UPDATE OF termination_evidence_state ON runtime_instances
                     WHEN NEW.termination_evidence_state='complete'
                     BEGIN SELECT RAISE(ABORT,'fixture live evidence failure'); END;",
                )
                .map_err(|error| error.to_string())?;
            Ok(())
        })
        .unwrap();

    let failure = runtime
        .shutdown(Duration::from_secs(2), Duration::from_secs(2))
        .unwrap_err();
    assert_eq!(failure.code, "CODEX_RUNTIME_STORE_FAILED");
    assert!(process_group_members(pgid).unwrap().is_empty());
    let record = tauri::async_runtime::block_on(store.runtime(runtime_id))
        .unwrap()
        .unwrap();
    assert_eq!(record.state, "unknown");
    assert_eq!(record.termination_evidence_state, "unknown");
    drop(failure);
    drop(cleanup);
}

/// 忽略 SIGTERM 的 Process Group 必须等待完整 grace 后升级 SIGKILL。
#[test]
fn shutdown_escalates_to_sigkill_for_ignoring_group() {
    let (_directory, runtime, _marker, ready) = runtime_fixture("ignore-tree");
    let mut cleanup = FixtureCleanup::armed(&runtime);
    wait_file(&ready);
    let started = Instant::now();
    let evidence = runtime
        .shutdown(Duration::from_millis(100), Duration::from_secs(2))
        .unwrap();
    assert!(started.elapsed() >= Duration::from_millis(100));
    assert!(process_group_members(evidence.pgid).unwrap().is_empty());
    cleanup.disarm();
}

/// 创建时启动令牌被篡改后必须 fail closed，且不得向原组发送任何信号。
#[test]
fn token_mismatch_returns_unknown_and_retains_runtime() {
    let (_directory, mut runtime, _marker, ready) = runtime_fixture("ignore-tree");
    let mut cleanup = FixtureCleanup::armed(&runtime);
    wait_file(&ready);
    let mut mismatched = runtime.identity.clone();
    mismatched.start_token.microseconds += 1;
    runtime.replace_identity_for_test(mismatched);
    let pgid = runtime.identity.pgid;

    let failure = runtime
        .shutdown(Duration::from_millis(20), Duration::from_millis(20))
        .unwrap_err();
    assert_eq!(failure.code, "CODEX_RUNTIME_TERMINATION_UNCONFIRMED");
    assert_eq!(failure.runtime.identity.pgid, pgid);
    assert!(!process_group_members(pgid).unwrap().is_empty());

    cleanup_fixture(*failure.runtime);
    cleanup.disarm();
}

/// 首次 shutdown 前 leader 已退出而组仍存活时，必须返回 unknown 且不向组发信号。
#[test]
fn missing_leader_with_live_group_is_unknown_without_signalling_group() {
    let (_directory, mut runtime, _marker, ready) = runtime_fixture("leader-exit");
    let mut cleanup = FixtureCleanup::armed(&runtime);
    wait_file(&ready);
    runtime.child.stdin.write_all(b"x").unwrap();
    wait_direct_child_exit(&mut runtime);
    let pgid = runtime.identity.pgid;

    let failure = runtime
        .shutdown(Duration::from_millis(20), Duration::from_millis(20))
        .unwrap_err();
    assert_eq!(failure.code, "CODEX_RUNTIME_TERMINATION_UNCONFIRMED");
    assert_eq!(failure.runtime.identity.pgid, pgid);
    assert!(!process_group_members(pgid).unwrap().is_empty());

    cleanup_fixture(*failure.runtime);
    cleanup.disarm();
}

/// leader 在 SIGTERM 后退出但组持续非空时，live-host shutdown 必须在 grace 后升级收口。
#[test]
fn leader_exits_after_term_but_continuous_group_escalates() {
    let (_directory, runtime, _marker, ready) = runtime_fixture("leader-term-exit");
    let mut cleanup = FixtureCleanup::armed(&runtime);
    wait_file(&ready);
    let started = Instant::now();

    let evidence = runtime
        .shutdown(Duration::from_millis(100), Duration::from_secs(2))
        .unwrap();
    assert!(started.elapsed() >= Duration::from_millis(100));
    assert!(evidence.direct_child_reaped);
    assert!(process_group_members(evidence.pgid).unwrap().is_empty());
    cleanup.disarm();
}

/// launcher 创建返回后立即 shutdown 也必须保留同一 PGID 证据并清空该组。
#[test]
fn shutdown_immediately_after_launch_leaves_no_process_group() {
    let (_directory, runtime, _marker, _ready) = runtime_fixture("tree");
    let mut cleanup = FixtureCleanup::armed(&runtime);
    let pgid = runtime.identity.pgid;

    let evidence = runtime
        .shutdown(Duration::from_secs(1), Duration::from_secs(2))
        .unwrap();
    assert_eq!(evidence.pgid, pgid);
    assert!(process_group_members(pgid).unwrap().is_empty());
    cleanup.disarm();
}
