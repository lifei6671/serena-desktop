use super::*;
use crate::agent::{
    codex::{
        macos_launcher::{self, MacosLaunchRequest},
        macos_runtime_store,
    },
    execution::{CreateExecutionInput, canonicalize_request},
    provider::port::ProviderReconcileKind,
    store::StateStore,
};
use rusqlite::params;
use serde_json::json;
use std::{
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

/// 等待 fixture 完成 signal disposition 与 leaf 建立。
fn wait_ready(marker: &Path) {
    let ready = PathBuf::from(format!("{}.ready", marker.display()));
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !ready.exists() {
        assert!(std::time::Instant::now() < deadline, "fixture ready 超时");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// 编译固定 child fixture，真实验证 Darwin identity 与 Process Group。
fn fixture(directory: &Path) -> PathBuf {
    let executable = directory.join("macos-recovery-child");
    let output = Command::new("rustc")
        .args(["--edition=2024", "--crate-name", "macos_recovery_child"])
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

/// 身份完全匹配时 recovery 必须形成 recovered group-empty evidence。
#[test]
fn identity_match_recovers_runtime_with_group_empty_evidence() {
    let directory = tempfile::tempdir().unwrap();
    let store =
        tauri::async_runtime::block_on(StateStore::open(directory.path().join("state"))).unwrap();
    let executable = fixture(directory.path());
    let marker = directory.path().join("leaf.pid");
    let request = MacosLaunchRequest {
        executable: executable.clone(),
        args: vec!["ignore-tree".into(), marker.as_os_str().to_owned()],
        current_dir: directory.path().to_owned(),
        runtime_instance_id: "recovery-match".into(),
    };
    let launched = macos_launcher::launch(&request).unwrap();
    wait_ready(&marker);
    macos_runtime_store::prepare(
        &store,
        "recovery-match",
        "old-host",
        &executable.to_string_lossy(),
        1,
    )
    .unwrap();
    macos_runtime_store::start(&store, "recovery-match", &launched.identity, 2).unwrap();
    macos_runtime_store::initialized(&store, "recovery-match", "1", "schema", 3).unwrap();
    let mut child = launched.child;
    let waiter = std::thread::spawn(move || child.process.wait().unwrap());

    let status = tauri::async_runtime::block_on(recover_runtime(
        &store,
        "recovery-match",
        Duration::from_millis(50),
        Duration::from_secs(2),
    ))
    .unwrap();
    assert_eq!(status, RecoveryStatus::Recovered);
    waiter.join().unwrap();
    let record = tauri::async_runtime::block_on(store.runtime("recovery-match".into()))
        .unwrap()
        .unwrap();
    assert_eq!(record.state, "terminated");
    assert_eq!(
        record.termination_evidence_type.as_deref(),
        Some("macos_recovered_process_group_empty")
    );
}

/// token 不匹配时不得向可能已复用 PID 的 Process Group 发送信号。
#[test]
fn identity_token_mismatch_is_unknown_without_signal() {
    let directory = tempfile::tempdir().unwrap();
    let store =
        tauri::async_runtime::block_on(StateStore::open(directory.path().join("state"))).unwrap();
    let executable = fixture(directory.path());
    let marker = directory.path().join("leaf.pid");
    let request = MacosLaunchRequest {
        executable: executable.clone(),
        args: vec!["ignore-tree".into(), marker.as_os_str().to_owned()],
        current_dir: directory.path().to_owned(),
        runtime_instance_id: "recovery-mismatch".into(),
    };
    let launched = macos_launcher::launch(&request).unwrap();
    wait_ready(&marker);
    let mut wrong = launched.identity.clone();
    wrong.start_token.microseconds = (wrong.start_token.microseconds + 1) % 1_000_000;
    macos_runtime_store::prepare(
        &store,
        "recovery-mismatch",
        "old-host",
        &executable.to_string_lossy(),
        1,
    )
    .unwrap();
    macos_runtime_store::start(&store, "recovery-mismatch", &wrong, 2).unwrap();

    let status = tauri::async_runtime::block_on(recover_runtime(
        &store,
        "recovery-mismatch",
        Duration::from_millis(20),
        Duration::from_millis(20),
    ))
    .unwrap();
    assert_eq!(status, RecoveryStatus::Unknown);
    assert!(
        !macos_launcher::process_group_members(launched.identity.pgid)
            .unwrap()
            .is_empty()
    );

    // 测试 teardown 不生成任何生产 evidence。
    unsafe { libc::killpg(launched.identity.pgid, libc::SIGKILL) };
    let mut child = launched.child;
    child.process.wait().unwrap();
}

/// TERM 后 leader 消失但组仍存在时必须停止，不能向残留组发送 SIGKILL。
#[test]
fn termination_leader_exit_with_live_group_stays_unknown_without_kill() {
    let directory = tempfile::tempdir().unwrap();
    let store =
        tauri::async_runtime::block_on(StateStore::open(directory.path().join("state"))).unwrap();
    let executable = fixture(directory.path());
    let marker = directory.path().join("leaf.pid");
    let request = MacosLaunchRequest {
        executable: executable.clone(),
        args: vec!["leader-term-exit".into(), marker.as_os_str().to_owned()],
        current_dir: directory.path().to_owned(),
        runtime_instance_id: "recovery-leader-exit".into(),
    };
    let launched = macos_launcher::launch(&request).unwrap();
    wait_ready(&marker);
    macos_runtime_store::prepare(
        &store,
        "recovery-leader-exit",
        "old-host",
        &executable.to_string_lossy(),
        1,
    )
    .unwrap();
    macos_runtime_store::start(&store, "recovery-leader-exit", &launched.identity, 2).unwrap();
    let pgid = launched.identity.pgid;
    let mut child = launched.child;
    let waiter = std::thread::spawn(move || child.process.wait().unwrap());

    let status = tauri::async_runtime::block_on(recover_runtime(
        &store,
        "recovery-leader-exit",
        Duration::from_secs(1),
        Duration::from_millis(20),
    ))
    .unwrap();
    assert_eq!(status, RecoveryStatus::Unknown);
    assert!(
        !macos_launcher::process_group_members(pgid)
            .unwrap()
            .is_empty()
    );

    // 仍存活说明 recovery 没有错误升级 KILL；随后只由测试 teardown 清理。
    unsafe { libc::killpg(pgid, libc::SIGKILL) };
    waiter.join().unwrap();
}

/// group 查询失败不是 group-empty evidence，且失败发生在信号阶段前时不得发信号。
#[test]
fn identity_group_observation_failure_is_unknown_without_signal() {
    let directory = tempfile::tempdir().unwrap();
    let store =
        tauri::async_runtime::block_on(StateStore::open(directory.path().join("state"))).unwrap();
    let executable = fixture(directory.path());
    let marker = directory.path().join("leaf.pid");
    let request = MacosLaunchRequest {
        executable: executable.clone(),
        args: vec!["ignore-tree".into(), marker.as_os_str().to_owned()],
        current_dir: directory.path().to_owned(),
        runtime_instance_id: "recovery-group-failure".into(),
    };
    let launched = macos_launcher::launch(&request).unwrap();
    wait_ready(&marker);
    macos_runtime_store::prepare(
        &store,
        "recovery-group-failure",
        "old-host",
        &executable.to_string_lossy(),
        1,
    )
    .unwrap();
    macos_runtime_store::start(&store, "recovery-group-failure", &launched.identity, 2).unwrap();
    let pgid = launched.identity.pgid;
    set_group_observation_failure(pgid, true);

    let status = tauri::async_runtime::block_on(recover_runtime(
        &store,
        "recovery-group-failure",
        Duration::from_millis(20),
        Duration::from_millis(20),
    ))
    .unwrap();
    set_group_observation_failure(pgid, false);
    assert_eq!(status, RecoveryStatus::Unknown);
    assert!(
        !macos_launcher::process_group_members(pgid)
            .unwrap()
            .is_empty()
    );

    unsafe { libc::killpg(pgid, libc::SIGKILL) };
    let mut child = launched.child;
    child.process.wait().unwrap();
}

/// recovered evidence 提交失败必须降级 unknown，不能继续 Claim release。
#[test]
fn termination_evidence_commit_failure_stays_unknown() {
    let directory = tempfile::tempdir().unwrap();
    let store =
        tauri::async_runtime::block_on(StateStore::open(directory.path().join("state"))).unwrap();
    let executable = fixture(directory.path());
    let marker = directory.path().join("leaf.pid");
    let request = MacosLaunchRequest {
        executable: executable.clone(),
        args: vec!["ignore-tree".into(), marker.as_os_str().to_owned()],
        current_dir: directory.path().to_owned(),
        runtime_instance_id: "recovery-commit-failure".into(),
    };
    let launched = macos_launcher::launch(&request).unwrap();
    wait_ready(&marker);
    macos_runtime_store::prepare(
        &store,
        "recovery-commit-failure",
        "old-host",
        &executable.to_string_lossy(),
        1,
    )
    .unwrap();
    macos_runtime_store::start(&store, "recovery-commit-failure", &launched.identity, 2).unwrap();
    store
        .write_blocking(|transaction| {
            transaction
                .execute_batch(
                    "CREATE TRIGGER reject_recovered_evidence
                     BEFORE UPDATE OF termination_evidence_state ON runtime_instances
                     WHEN NEW.termination_evidence_state='complete'
                     BEGIN SELECT RAISE(ABORT,'fixture evidence failure'); END;",
                )
                .map_err(|error| error.to_string())?;
            Ok(())
        })
        .unwrap();
    let mut child = launched.child;
    let waiter = std::thread::spawn(move || child.process.wait().unwrap());

    let status = tauri::async_runtime::block_on(recover_runtime(
        &store,
        "recovery-commit-failure",
        Duration::from_millis(50),
        Duration::from_secs(2),
    ))
    .unwrap();
    waiter.join().unwrap();
    assert_eq!(status, RecoveryStatus::Unknown);
    let record = tauri::async_runtime::block_on(store.runtime("recovery-commit-failure".into()))
        .unwrap()
        .unwrap();
    assert_eq!(record.state, "unknown");
    assert_eq!(record.termination_evidence_state, "unknown");
}

/// 构造带 durable Claim 的最小 Execution。
fn create_execution(store: &StateStore, id: &str, root: &str) {
    let input: CreateExecutionInput = serde_json::from_value(json!({
        "agent_id": format!("agent-{id}"),
        "request_key": format!("request-{id}"),
        "prompt": "fixture",
        "execution_profile": {},
        "workspace_id": "workspace",
        "canonical_workspace_root": root,
        "mode": "workspace_write"
    }))
    .unwrap();
    tauri::async_runtime::block_on(store.create_execution(
        id.into(),
        canonicalize_request(input).unwrap(),
        1,
    ))
    .unwrap();
}

/// 已有合法 macOS complete evidence 时 startup 必须幂等释放 Claim 并终结 Execution。
#[test]
fn startup_releases_claim_from_complete_macos_evidence() {
    let directory = tempfile::tempdir().unwrap();
    let store =
        tauri::async_runtime::block_on(StateStore::open(directory.path().join("state"))).unwrap();
    create_execution(&store, "execution", "/fixture/root");
    store
        .write_blocking(|transaction| {
            transaction
                .execute(
                    "INSERT INTO runtime_instances(
                        id,owner_host_instance_id,state,created_at,updated_at,
                        runtime_platform,containment_type,process_identity_scheme,
                        process_id,process_start_token,containment_process_group_id,
                        containment_session_id,containment_verified_at,stopped_at,
                        termination_evidence_type,termination_evidence_at,
                        termination_evidence_state)
                     VALUES('runtime','old-host','terminated',1,9,'macos',
                            'macos_process_group','darwin_proc_bsd_start_v1',70,
                            'darwin_proc_bsd_start_v1:1:2',70,70,2,9,
                            'macos_recovered_process_group_empty',9,'complete')",
                    [],
                )
                .map_err(|error| error.to_string())?;
            transaction
                .execute(
                    "UPDATE executions SET status='unknown',dispatch_state='uncertain',
                     runtime_instance_id='runtime' WHERE id='execution'",
                    [],
                )
                .map_err(|error| error.to_string())?;
            Ok(())
        })
        .unwrap();

    let summary = tauri::async_runtime::block_on(recover_startup(&store, "new-host")).unwrap();
    assert!(summary.items.iter().any(|item| {
        item.subject_id == "execution" && item.kind == ProviderReconcileKind::ExecutionInterrupted
    }));
    let execution = tauri::async_runtime::block_on(store.execution("execution".into()))
        .unwrap()
        .unwrap();
    assert_eq!(execution.status, "interrupted");
    assert!(
        tauri::async_runtime::block_on(store.workspace_claim("/fixture/root".into()))
            .unwrap()
            .is_none()
    );

    let second = tauri::async_runtime::block_on(recover_startup(&store, "new-host")).unwrap();
    assert!(second.items.is_empty());
}

/// startup 必须把真实运行中的 macOS group 收口为 recovered evidence，并原子释放其 Claim。
#[test]
fn startup_recovers_live_runtime_and_releases_claim() {
    let directory = tempfile::tempdir().unwrap();
    let store =
        tauri::async_runtime::block_on(StateStore::open(directory.path().join("state"))).unwrap();
    create_execution(&store, "startup-execution", "/fixture/startup");
    let executable = fixture(directory.path());
    let marker = directory.path().join("startup-leaf.pid");
    let request = MacosLaunchRequest {
        executable: executable.clone(),
        args: vec!["ignore-tree".into(), marker.as_os_str().to_owned()],
        current_dir: directory.path().to_owned(),
        runtime_instance_id: "startup-runtime".into(),
    };
    let launched = macos_launcher::launch(&request).unwrap();
    wait_ready(&marker);
    macos_runtime_store::prepare(
        &store,
        "startup-runtime",
        "old-host",
        &executable.to_string_lossy(),
        1,
    )
    .unwrap();
    macos_runtime_store::start(&store, "startup-runtime", &launched.identity, 2).unwrap();
    macos_runtime_store::initialized(&store, "startup-runtime", "1", "schema", 3).unwrap();
    store
        .write_blocking(|transaction| {
            transaction
                .execute(
                    "UPDATE executions SET status='running',dispatch_state='dispatched',
                        runtime_instance_id='startup-runtime' WHERE id='startup-execution'",
                    [],
                )
                .map_err(|error| error.to_string())?;
            Ok(())
        })
        .unwrap();
    let mut child = launched.child;
    let waiter = std::thread::spawn(move || child.process.wait().unwrap());

    let summary = tauri::async_runtime::block_on(recover_startup_with_timeouts(
        &store,
        "new-host",
        Duration::from_millis(50),
        Duration::from_secs(2),
    ))
    .unwrap();
    waiter.join().unwrap();
    let recovered_runtime =
        tauri::async_runtime::block_on(store.runtime("startup-runtime".into())).unwrap();
    assert!(
        summary.items.iter().any(|item| {
            item.subject_id == "startup-execution"
                && item.kind == ProviderReconcileKind::ExecutionInterrupted
        }),
        "{summary:?}; {recovered_runtime:?}"
    );
    let runtime = recovered_runtime.unwrap();
    assert_eq!(
        runtime.termination_evidence_type.as_deref(),
        Some("macos_recovered_process_group_empty")
    );
    let execution = tauri::async_runtime::block_on(store.execution("startup-execution".into()))
        .unwrap()
        .unwrap();
    assert_eq!(execution.status, "interrupted");
    assert!(
        tauri::async_runtime::block_on(store.workspace_claim("/fixture/startup".into()))
            .unwrap()
            .is_none()
    );
}

/// 缺失 Runtime 的 startup recovery 必须进入 unknown 并保留 Claim。
#[test]
fn startup_missing_runtime_retains_claim_as_unknown() {
    let directory = tempfile::tempdir().unwrap();
    let store =
        tauri::async_runtime::block_on(StateStore::open(directory.path().join("state"))).unwrap();
    create_execution(&store, "execution", "/fixture/missing");
    store
        .write_blocking(|transaction| {
            transaction
                .execute(
                    "UPDATE executions SET status='running',dispatch_state='dispatched'
                     WHERE id='execution'",
                    params![],
                )
                .map_err(|error| error.to_string())?;
            Ok(())
        })
        .unwrap();

    let summary = tauri::async_runtime::block_on(recover_startup(&store, "new-host")).unwrap();
    assert!(
        summary
            .items
            .iter()
            .any(|item| item.kind == ProviderReconcileKind::ExecutionUnknown)
    );
    let execution = tauri::async_runtime::block_on(store.execution("execution".into()))
        .unwrap()
        .unwrap();
    assert_eq!(execution.status, "unknown");
    assert!(
        tauri::async_runtime::block_on(store.workspace_claim("/fixture/missing".into()))
            .unwrap()
            .is_some()
    );
}

/// macOS startup 不得解释或改写历史 Windows orphan Runtime 的 Named Job 证据。
#[test]
fn startup_leaves_windows_orphan_runtime_unchanged() {
    let directory = tempfile::tempdir().unwrap();
    let store =
        tauri::async_runtime::block_on(StateStore::open(directory.path().join("state"))).unwrap();
    store
        .write_blocking(|transaction| {
            transaction
                .execute(
                    "INSERT INTO runtime_instances(
                        id,owner_host_instance_id,state,created_at,updated_at,
                        job_name,job_session_id,job_creation_mode,job_handle_inheritable,
                        job_kill_on_close,job_breakaway_allowed,job_policy_verified_at)
                     VALUES('windows-orphan','old-host','running',1,2,'Local\\fixture',1,
                            'proc_thread_attribute_job_list',0,1,0,2)",
                    [],
                )
                .map_err(|error| error.to_string())?;
            Ok(())
        })
        .unwrap();

    let summary = tauri::async_runtime::block_on(recover_startup(&store, "new-host")).unwrap();
    assert!(summary.items.iter().any(|item| {
        item.subject_id == "windows-orphan"
            && item.kind == ProviderReconcileKind::OrphanResourceUnknown
    }));
    let record = tauri::async_runtime::block_on(store.runtime("windows-orphan".into()))
        .unwrap()
        .unwrap();
    assert_eq!(record.state, "running");
    assert_eq!(record.runtime_platform, "windows");
}

/// Claim 删除失败必须回滚 finalization；清除故障后的下一次 startup 可幂等完成。
#[test]
fn startup_release_failure_retains_claim_and_retries_idempotently() {
    let directory = tempfile::tempdir().unwrap();
    let store =
        tauri::async_runtime::block_on(StateStore::open(directory.path().join("state"))).unwrap();
    create_execution(&store, "execution", "/fixture/retry");
    create_execution(&store, "following", "/fixture/following");
    store
        .write_blocking(|transaction| {
            transaction
                .execute_batch(
                    "INSERT INTO runtime_instances(
                        id,owner_host_instance_id,state,created_at,updated_at,
                        runtime_platform,containment_type,process_identity_scheme,
                        process_id,process_start_token,containment_process_group_id,
                        containment_session_id,containment_verified_at,stopped_at,
                        termination_evidence_type,termination_evidence_at,
                        termination_evidence_state)
                     VALUES('runtime','old-host','terminated',1,9,'macos',
                            'macos_process_group','darwin_proc_bsd_start_v1',70,
                            'darwin_proc_bsd_start_v1:1:2',70,70,2,9,
                            'macos_recovered_process_group_empty',9,'complete');
                     UPDATE executions SET status='unknown',dispatch_state='uncertain',
                        runtime_instance_id='runtime' WHERE id='execution';
                     UPDATE executions SET status='running',dispatch_state='dispatched'
                        WHERE id='following';
                     CREATE TRIGGER reject_claim_release BEFORE DELETE ON workspace_claims
                     BEGIN SELECT RAISE(ABORT,'fixture release failure'); END;",
                )
                .map_err(|error| error.to_string())?;
            Ok(())
        })
        .unwrap();

    let first = tauri::async_runtime::block_on(recover_startup(&store, "new-host")).unwrap();
    assert!(first.items.iter().any(|item| {
        item.subject_id == "execution"
            && item.kind == ProviderReconcileKind::ExecutionProviderFailure
    }));
    assert!(first.items.iter().any(|item| {
        item.subject_id == "following" && item.kind == ProviderReconcileKind::ExecutionUnknown
    }));
    assert!(
        tauri::async_runtime::block_on(store.workspace_claim("/fixture/retry".into()))
            .unwrap()
            .is_some()
    );
    let failed_execution = tauri::async_runtime::block_on(store.execution("execution".into()))
        .unwrap()
        .unwrap();
    assert_eq!(failed_execution.status, "reconciling");
    store
        .write_blocking(|transaction| {
            transaction
                .execute_batch("DROP TRIGGER reject_claim_release")
                .map_err(|error| error.to_string())?;
            Ok(())
        })
        .unwrap();
    let summary = tauri::async_runtime::block_on(recover_startup(&store, "new-host")).unwrap();
    assert!(
        summary
            .items
            .iter()
            .any(|item| item.kind == ProviderReconcileKind::ExecutionInterrupted),
        "{summary:?}"
    );
    assert!(
        tauri::async_runtime::block_on(store.workspace_claim("/fixture/retry".into()))
            .unwrap()
            .is_none()
    );
}
