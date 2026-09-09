use super::*;
use rusqlite::Connection;
use std::{
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};
static IDS: AtomicU64 = AtomicU64::new(0);
fn block<T>(f: impl std::future::Future<Output = T>) -> T {
    tauri::async_runtime::block_on(f)
}
fn open(path: &Path) -> StateStore {
    block(StateStore::open(path.to_owned())).unwrap()
}
fn row(store: &StateStore, id: &str) -> RuntimeRecord {
    block(store.runtime(id.to_owned())).unwrap().unwrap()
}
fn request(exe: &Path, dir: &Path, mode: &str) -> LaunchRequest {
    LaunchRequest {
        executable: exe.to_owned(),
        current_dir: dir.to_owned(),
        args: vec![dir.as_os_str().to_owned(), mode.into()],
        runtime_instance_id: format!(
            "runtime-{}-{}",
            std::process::id(),
            IDS.fetch_add(1, Ordering::Relaxed)
        ),
    }
}
fn fixture(dir: &Path) -> PathBuf {
    let exe = dir.join("Runtime 测试 Child.exe");
    let result = Command::new("rustc")
        .args(["--edition=2024", "--crate-name", "runtime_child"])
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/runtime_child.rs"))
        .arg("-o")
        .arg(&exe)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    exe
}
fn wait_file(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !path.exists() {
        assert!(Instant::now() < deadline, "{}", path.display());
        std::thread::sleep(Duration::from_millis(2));
    }
}
fn observe_pid(path: &Path) -> OwnedHandle {
    wait_file(path);
    let pid = std::fs::read_to_string(path).unwrap().parse().unwrap();
    let raw = unsafe {
        OpenProcess(
            PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
            0,
            pid,
        )
    };
    assert!(!raw.is_null());
    unsafe { OwnedHandle::from_raw_handle(raw) }
}
fn unknown(store: &StateStore, id: &str) {
    let r = row(store, id);
    assert_eq!(r.state, "unknown");
    assert_eq!(r.termination_evidence_state, "unknown");
    assert!(r.termination_evidence_at.is_none());
    assert!(r.termination_evidence_type.is_none());
}
fn complete(store: &StateStore, id: &str, kind: &str) {
    let r = row(store, id);
    assert_eq!(r.state, "terminated");
    assert_eq!(r.termination_evidence_state, "complete");
    assert_eq!(r.termination_evidence_type.as_deref(), Some(kind));
    assert!(r.termination_evidence_at.is_some());
}

#[test]
fn tree_main_exit_is_not_evidence_and_explicit_termination_is_job_level() {
    let dir = tempfile::tempdir().unwrap();
    let exe = fixture(dir.path());
    let store = open(dir.path());
    let req = request(&exe, dir.path(), "main-exit");
    let id = req.runtime_instance_id.clone();
    let runtime = block(Runtime::create(
        store.clone(),
        "host".into(),
        req,
        Duration::from_secs(5),
    ))
    .unwrap();
    let leaf = observe_pid(&dir.path().join("leaf.pid"));
    assert_eq!(
        unsafe {
            WaitForSingleObject(
                runtime.child.as_ref().unwrap().process.as_raw_handle(),
                5000,
            )
        },
        WAIT_OBJECT_0
    );
    assert_eq!(
        unsafe { WaitForSingleObject(leaf.as_raw_handle(), 0) },
        WAIT_TIMEOUT
    );
    assert!(active_processes(runtime.job()).unwrap() > 0);
    let before = row(&store, &id);
    assert_eq!(before.state, "starting");
    assert_eq!(before.termination_evidence_state, "unknown");
    assert!(safe_policy(&before));
    assert!(before.codex_pid.is_some());
    assert_eq!(before.codex_process_start_token.as_ref().unwrap().len(), 16);
    block(runtime.terminate(Duration::from_secs(5))).unwrap();
    complete(&store, &id, "job_active_processes_zero");
    assert_eq!(
        unsafe { WaitForSingleObject(leaf.as_raw_handle(), 5000) },
        WAIT_OBJECT_0
    );
}

#[test]
fn identity_failure_and_pid_persist_failure_converge_created_ownership() {
    let dir = tempfile::tempdir().unwrap();
    let exe = fixture(dir.path());
    let store = open(dir.path());
    let req = request(&exe, dir.path(), "tree");
    let id = req.runtime_instance_id.clone();
    let result = Runtime::create_blocking(
        store.clone(),
        "host".into(),
        req,
        Duration::from_secs(5),
        Some(StartFault::Identity),
    );
    assert_eq!(result.unwrap_err().code, "CODEX_PROCESS_IDENTITY_FAILED");
    complete(&store, &id, "job_active_processes_zero");
    assert!(row(&store, &id).codex_pid.is_none());
    let c = Connection::open(dir.path().join("agent-state.db")).unwrap();
    c.execute_batch("CREATE TRIGGER fail_start BEFORE UPDATE OF state ON runtime_instances WHEN NEW.state='starting' BEGIN SELECT RAISE(ABORT,'injected PID persistence failure'); END;").unwrap();
    let req = request(&exe, dir.path(), "tree");
    let id = req.runtime_instance_id.clone();
    assert!(
        block(Runtime::create(
            store.clone(),
            "host".into(),
            req,
            Duration::from_secs(5)
        ))
        .is_err()
    );
    complete(&store, &id, "job_active_processes_zero");
    assert!(row(&store, &id).codex_pid.is_none());
}

#[test]
fn failed_evidence_commit_retains_unknown_and_owned_job_for_retry() {
    let dir = tempfile::tempdir().unwrap();
    let exe = fixture(dir.path());
    let store = open(dir.path());
    let req = request(&exe, dir.path(), "tree");
    let id = req.runtime_instance_id.clone();
    let runtime = block(Runtime::create(
        store.clone(),
        "host".into(),
        req,
        Duration::from_secs(5),
    ))
    .unwrap();
    let c = Connection::open(dir.path().join("agent-state.db")).unwrap();
    c.execute_batch("CREATE TRIGGER fail_evidence BEFORE UPDATE ON runtime_instances WHEN NEW.termination_evidence_state='complete' BEGIN SELECT RAISE(ABORT,'injected evidence persistence failure'); END;").unwrap();
    let error = block(runtime.terminate(Duration::from_secs(5))).unwrap_err();
    assert_eq!(error.code, "CODEX_RUNTIME_EVIDENCE_PERSIST_FAILED");
    assert!(
        error
            .message
            .contains("injected evidence persistence failure")
    );
    assert_error_fields(dir.path(), &id, &error);
    unknown(&store, &id);
    let retained = *error.runtime.unwrap();
    assert_eq!(active_processes(retained.job()).unwrap(), 0);
    c.execute_batch("DROP TRIGGER fail_evidence").unwrap();
    block(retained.terminate(Duration::from_secs(5))).unwrap();
    complete(&store, &id, "job_active_processes_zero");
    assert_eq!(error_fields(dir.path(), &id), (None, None));
}

fn error_fields(dir: &Path, id: &str) -> (Option<String>, Option<String>) {
    Connection::open(dir.join("agent-state.db"))
        .unwrap()
        .query_row(
            "SELECT last_error_code,last_error_message FROM runtime_instances WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap()
}
fn assert_error_fields(dir: &Path, id: &str, error: &RuntimeFailure) {
    assert_eq!(
        error_fields(dir, id),
        (Some(error.code.into()), Some(error.message.clone()))
    );
}

#[test]
fn bounded_poll_waits_for_zero_and_rejects_errors_and_timeout() {
    let mut calls = 0;
    poll_empty(Duration::from_secs(1), || {
        calls += 1;
        Ok(if calls < 4 { 1 } else { 0 })
    })
    .unwrap();
    assert_eq!(calls, 4);
    assert_eq!(
        poll_empty(Duration::ZERO, || Ok(1)).unwrap_err().code,
        "CODEX_RUNTIME_TERMINATION_TIMEOUT"
    );
    assert_eq!(
        poll_empty(Duration::from_secs(1), || Err(RuntimeError::new(
            "CODEX_JOB_QUERY_FAILED",
            "query denied"
        )))
        .unwrap_err()
        .message,
        "query denied"
    );
}

#[test]
fn delayed_job_empty_observation_timeout_and_query_failure_persist_correctly() {
    let dir = tempfile::tempdir().unwrap();
    let exe = fixture(dir.path());
    let store = open(dir.path());
    for case in ["delay", "timeout", "query_error"] {
        let req = request(&exe, dir.path(), "tree");
        let id = req.runtime_instance_id.clone();
        let runtime = block(Runtime::create(
            store.clone(),
            "host".into(),
            req,
            Duration::from_secs(5),
        ))
        .unwrap();
        let mut calls = 0;
        let result = runtime.terminate_with_query(
            if case == "timeout" {
                Duration::ZERO
            } else {
                Duration::from_secs(5)
            },
            |job| {
                calls += 1;
                // TerminateJobObject really ran. Model delayed visibility of zero:
                // never synthesize zero; the successful final observation is Win32.
                let record = row(&store, &id);
                assert_ne!(record.termination_evidence_state, "complete");
                match case {
                    "query_error" => Err(RuntimeError::new(
                        "CODEX_JOB_QUERY_FAILED",
                        "injected query failure",
                    )),
                    "timeout" => Ok(1),
                    _ if calls < 4 => Ok(1),
                    _ => active_processes(job),
                }
            },
        );
        if case == "delay" {
            result.unwrap();
            assert!(calls >= 4);
            complete(&store, &id, "job_active_processes_zero");
        } else {
            let error = result.unwrap_err();
            assert_eq!(
                error.code,
                if case == "timeout" {
                    "CODEX_RUNTIME_TERMINATION_TIMEOUT"
                } else {
                    "CODEX_JOB_QUERY_FAILED"
                }
            );
            assert_error_fields(dir.path(), &id, &error);
            let retained = *error.runtime.unwrap();
            unknown(&store, &id);
            block(retained.terminate(Duration::from_secs(5))).unwrap();
            assert_eq!(error_fields(dir.path(), &id), (None, None));
        }
    }
}

#[test]
fn named_active_job_is_opened_and_terminated_and_query_errors_stay_unknown() {
    let dir = tempfile::tempdir().unwrap();
    let exe = fixture(dir.path());
    let store = open(dir.path());
    let req = request(&exe, dir.path(), "tree");
    let id = req.runtime_instance_id.clone();
    let runtime = block(Runtime::create(
        store.clone(),
        "host".into(),
        req,
        Duration::from_secs(5),
    ))
    .unwrap();
    let leaf = observe_pid(&dir.path().join("leaf.pid"));
    let name = format!("Local\\SerenaDesktop.Codex.{id}");
    // Same test Host opens for recovery, then closes its original owner before
    // passing the sole remaining handle onward. Never transfer to another Host.
    let opened = open_job(&name).unwrap();
    drop(runtime);
    assert!(active_processes(opened.as_raw_handle()).unwrap() > 0);
    recover_blocking(
        store.clone(),
        id.clone(),
        Duration::from_secs(5),
        current_session_id(),
        |_| Ok(opened),
    )
    .unwrap();
    complete(&store, &id, "job_active_processes_zero");
    assert_eq!(
        unsafe { WaitForSingleObject(leaf.as_raw_handle(), 5000) },
        WAIT_OBJECT_0
    );
    let id = format!("query-error-{}", std::process::id());
    prepare(&store, &id);
    let raw = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
    assert!(!raw.is_null());
    let event = unsafe { OwnedHandle::from_raw_handle(raw) };
    let result = recover_blocking(
        store.clone(),
        id.clone(),
        Duration::ZERO,
        current_session_id(),
        |_| Ok(event),
    );
    let error = result.unwrap_err();
    assert_eq!(error.code, "CODEX_JOB_QUERY_FAILED");
    assert!(error.message.contains("Win32 6"));
    assert_error_fields(dir.path(), &id, &error);
    unknown(&store, &id);
}

fn prepare(store: &StateStore, id: &str) {
    store
        .prepare_runtime(
            id,
            "old-host",
            &format!("Local\\SerenaDesktop.Codex.{id}"),
            current_session_id().unwrap(),
            "C:\\fixture.exe",
            1,
        )
        .unwrap();
    store.verify_runtime_policy(id, 2).unwrap();
}
#[test]
fn recovery_missing_session_policy_and_open_errors_never_mint_evidence_or_release_claim() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    let c = Connection::open(dir.path().join("agent-state.db")).unwrap();
    let mutations = [
        "job_session_id=NULL",
        "job_session_id=job_session_id+1",
        "job_creation_mode=NULL",
        "job_handle_inheritable=NULL",
        "job_kill_on_close=NULL",
        "job_breakaway_allowed=NULL",
        "job_policy_verified_at=NULL",
        "job_name=NULL",
    ];
    for (i, mutation) in mutations.iter().enumerate() {
        let id = format!("invalid-{}-{i}", std::process::id());
        prepare(&store, &id);
        c.execute(
            &format!("UPDATE runtime_instances SET {mutation} WHERE id=?1"),
            [&id],
        )
        .unwrap();
        assert!(
            recover_blocking(
                store.clone(),
                id.clone(),
                Duration::ZERO,
                current_session_id(),
                |_| panic!("must not Open wrong namespace or unverified policy")
            )
            .is_err()
        );
        unknown(&store, &id);
    }
    let id = "session-api-error";
    prepare(&store, id);
    let error = recover_blocking(
        store.clone(),
        id.into(),
        Duration::ZERO,
        Err(RuntimeError::new(
            "CODEX_SESSION_ID_UNAVAILABLE",
            "session denied: Win32 5",
        )),
        |_| panic!("must not open"),
    )
    .unwrap_err();
    assert_eq!(error.code, "CODEX_SESSION_ID_UNAVAILABLE");
    assert_error_fields(dir.path(), id, &error);
    unknown(&store, id);
    for code in [ERROR_ACCESS_DENIED, ERROR_INVALID_HANDLE, ERROR_GEN_FAILURE] {
        let id = format!("open-error-{code}");
        prepare(&store, &id);
        let error = recover_blocking(
            store.clone(),
            id.clone(),
            Duration::ZERO,
            current_session_id(),
            |_| Err(code),
        )
        .unwrap_err();
        assert_eq!(error.code, "CODEX_JOB_OPEN_FAILED");
        assert!(error.message.contains(&format!("Win32 {code}")));
        assert_error_fields(dir.path(), &id, &error);
        unknown(&store, &id);
    }
    // A persisted unknown Runtime can still own an unresolved Execution/Claim.
    c.execute(
        "UPDATE runtime_instances SET job_session_id=NULL WHERE id=?1",
        [id],
    )
    .unwrap();
    c.execute("INSERT INTO executions (id,agent_id,request_key,request_hash,prompt,execution_profile_json,workspace_id,canonical_workspace_root,provider,mode,runtime_instance_id,status,created_at,updated_at) VALUES ('e','a','k','hash','p','{}','w','root','codex','workspace_write',?1,'unknown',1,1)",[id]).unwrap();
    c.execute(
        "INSERT INTO workspace_claims VALUES ('root','e','exclusive_execution',1)",
        [],
    )
    .unwrap();
    assert!(block(recover(store.clone(), id.into(), Duration::ZERO)).is_err());
    assert!(
        block(store.workspace_claim("root".into()))
            .unwrap()
            .is_some()
    );
}

#[test]
fn named_job_exists_empty_and_destroyed_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    let id = format!("empty-{}", std::process::id());
    prepare(&store, &id);
    let name: Vec<u16> = format!("Local\\SerenaDesktop.Codex.{id}")
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let raw = unsafe { CreateJobObjectW(std::ptr::null(), name.as_ptr()) };
    assert!(!raw.is_null());
    let job = unsafe { OwnedHandle::from_raw_handle(raw) };
    // Hand off sole ownership to recovery without duplicating the Handle.
    recover_blocking(
        store.clone(),
        id.clone(),
        Duration::from_secs(1),
        current_session_id(),
        |_| Ok(job),
    )
    .unwrap();
    complete(&store, &id, "job_active_processes_zero");
    let id = format!("destroyed-{}", std::process::id());
    prepare(&store, &id);
    block(recover(store.clone(), id.clone(), Duration::from_secs(1))).unwrap();
    complete(&store, &id, "managed_job_destroyed");
}

#[test]
fn pid_reuse_identity_is_diagnostic_only() {
    let mut created: FILETIME = unsafe { zeroed() };
    let mut a: FILETIME = unsafe { zeroed() };
    let mut b: FILETIME = unsafe { zeroed() };
    let mut c: FILETIME = unsafe { zeroed() };
    assert_ne!(
        unsafe { GetProcessTimes(GetCurrentProcess(), &mut created, &mut a, &mut b, &mut c) },
        0
    );
    let token = format!(
        "{:08x}{:08x}",
        created.dwHighDateTime, created.dwLowDateTime
    );
    assert_eq!(
        block(process_identity(std::process::id(), token)),
        ProcessIdentity::Same
    );
    assert_eq!(
        block(process_identity(
            std::process::id(),
            "0000000000000000".into()
        )),
        ProcessIdentity::Different
    );
    assert_eq!(
        block(process_identity(0, "0000000000000000".into())),
        ProcessIdentity::Unavailable
    );
}

#[test]
fn crash_host() {
    let Some(dir) = std::env::var_os("TASK004_HOST_DIR") else {
        return;
    };
    let dir = PathBuf::from(dir);
    let exe = PathBuf::from(std::env::var_os("TASK004_HOST_EXE").unwrap());
    let store = open(&dir);
    let mut req = request(&exe, &dir, "tree");
    req.runtime_instance_id = "crash-runtime".into();
    let runtime = block(Runtime::create(
        store,
        "host".into(),
        req,
        Duration::from_secs(5),
    ))
    .unwrap();
    wait_file(&dir.join("main.pid"));
    std::fs::write(dir.join("host.ready"), b"ready").unwrap();
    loop {
        std::thread::sleep(Duration::from_secs(1));
        std::hint::black_box(&runtime);
    }
}

#[test]
fn host_abrupt_exit_and_creation_crashes_recover_from_persisted_policy_only() {
    let root = tempfile::tempdir().unwrap();
    let exe = fixture(root.path());
    for point in [
        "row_created",
        "job_created",
        "before_create_process",
        "after_create_process",
        "before_pid_persist",
        "live_tree",
    ] {
        let dir = root.path().join(point);
        std::fs::create_dir(&dir).unwrap();
        let mut host = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "agent::codex::runtime::tests::crash_host",
                "--nocapture",
            ])
            .env("TASK004_HOST_DIR", &dir)
            .env("TASK004_HOST_EXE", &exe)
            .env("TASK004_CRASH_POINT", point)
            .creation_flags(CREATE_NO_WINDOW)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let observers = if point == "live_tree" {
            wait_file(&dir.join("host.ready"));
            let main = observe_pid(&dir.join("main.pid"));
            let leaf = observe_pid(&dir.join("leaf.pid"));
            host.kill().unwrap();
            Some((main, leaf))
        } else {
            None
        };
        assert!(!host.wait().unwrap().success());
        if let Some((main, leaf)) = observers {
            for p in [main, leaf] {
                assert_eq!(
                    unsafe { WaitForSingleObject(p.as_raw_handle(), 5000) },
                    WAIT_OBJECT_0
                );
            }
        }
        let store = open(&dir);
        let before = row(&store, "crash-runtime");
        assert_eq!(
            before.termination_evidence_state, "unknown",
            "kernel cleanup is not persisted proof"
        );
        let result = block(recover(
            store.clone(),
            "crash-runtime".into(),
            Duration::from_secs(5),
        ));
        if ["row_created", "job_created"].contains(&point) {
            assert!(result.is_err());
            unknown(&store, "crash-runtime");
        } else {
            result.unwrap();
            complete(&store, "crash-runtime", "managed_job_destroyed");
        }
        if point != "live_tree" {
            assert!(before.codex_pid.is_none());
        }
    }
    // Race real Host termination against the actual CreateProcessW call boundary.
    // A marker is emitted immediately before the API; the parent kills without
    // waiting for return. Scheduling can hit either side of the kernel call.
    for attempt in 0..12 {
        let dir = root.path().join(format!("during-{attempt}"));
        std::fs::create_dir(&dir).unwrap();
        let mut host = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "agent::codex::runtime::tests::crash_host",
                "--nocapture",
            ])
            .env("TASK004_HOST_DIR", &dir)
            .env("TASK004_HOST_EXE", &exe)
            .env("TASK004_CRASH_POINT", "during_create_process")
            .creation_flags(CREATE_NO_WINDOW)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        wait_file(&dir.join("create.entering"));
        if attempt % 3 != 0 {
            std::thread::sleep(Duration::from_millis(attempt % 3));
        }
        host.kill().unwrap();
        host.wait().unwrap();
        let store = open(&dir);
        assert_eq!(
            row(&store, "crash-runtime").termination_evidence_state,
            "unknown"
        );
        block(recover(
            store.clone(),
            "crash-runtime".into(),
            Duration::from_secs(5),
        ))
        .unwrap();
        complete(&store, "crash-runtime", "managed_job_destroyed");
    }
}
