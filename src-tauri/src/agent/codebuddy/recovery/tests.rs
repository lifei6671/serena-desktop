use super::*;
use crate::agent::{
    codebuddy::{
        discovery::ResolvedLaunchSpec,
        store::Ownership,
        windows_launcher::{self, LaunchRequest, UncCurrentDirectoryPolicy},
    },
    execution::CreateExecutionInput,
    task_manager::AgentTaskManager,
};
use rusqlite::{Connection, params};
use std::{
    io::{BufRead, BufReader},
    mem::{size_of, zeroed},
    os::windows::{
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
        process::CommandExt,
    },
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Foundation::*,
    System::{JobObjects::*, Threading::*},
};

/// 当前 schema 的真实 Store/Claim fixture，SQL 仅用于测试损坏 durable 输入。
async fn fixture(
    private: bool,
) -> (
    tempfile::TempDir,
    StateStore,
    AgentTaskManager,
    String,
    String,
    Connection,
) {
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::open(dir.path().into()).await.unwrap();
    let manager = AgentTaskManager::new(store.clone(), "must-not-launch.exe".into());
    let input:CreateExecutionInput=serde_json::from_value(serde_json::json!({"agent_id":"a","request_key":"k","prompt":"p","execution_profile":{},"workspace_id":"w","canonical_workspace_root":dir.path().to_str().unwrap(),"mode":"read_only"})).unwrap();
    let id = manager.create(input).await.unwrap().execution_id;
    let runtime = format!(
        "cb-recovery-{}",
        crate::agent::codebuddy::store::new_conversation_id().unwrap()
    );
    store
        .prepare_codebuddy_runtime(
            runtime.clone(),
            "old-host".into(),
            windows::session().unwrap(),
            "fake.exe".into(),
            1,
        )
        .await
        .unwrap();
    store
        .update_codebuddy_runtime(runtime.clone(), CodeBuddyRuntimeUpdate::PolicyVerified, 2)
        .await
        .unwrap();
    store
        .update_codebuddy_runtime(
            runtime.clone(),
            CodeBuddyRuntimeUpdate::ProcessStarted {
                pid: 0,
                start_token: "diagnostic-only".into(),
            },
            3,
        )
        .await
        .unwrap();
    store
        .update_codebuddy_runtime(runtime.clone(), CodeBuddyRuntimeUpdate::Initialized, 4)
        .await
        .unwrap();
    let db = Connection::open(dir.path().join("agent-state.db")).unwrap();
    db.execute("UPDATE executions SET provider='codebuddy',runtime_instance_id=?2,status='running',dispatch_state='dispatched' WHERE id=?1",params![id,runtime]).unwrap();
    if private {
        store
            .create_codebuddy_state(
                id.clone(),
                Ownership {
                    execution_revision: 0,
                    runtime_instance_id: Some(runtime.clone()),
                },
            )
            .await
            .unwrap();
    }
    (dir, store, manager, id, runtime, db)
}

/// Native empty Job 具有相同安全 policy，用于 exact-zero 和故障路径。
fn job(id: &str) -> OwnedHandle {
    let name: Vec<u16> = format!("Local\\SerenaDesktop.CodeBuddy.{id}")
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let raw = unsafe { CreateJobObjectW(null(), name.as_ptr()) };
    assert!(!raw.is_null());
    let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
    let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    assert_ne!(
        unsafe {
            SetInformationJobObject(
                raw,
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        },
        0
    );
    handle
}

/// Claim 和 unknown 是故障路径的外部可观察结果。
async fn assert_unknown(store: &StateStore, id: &str) {
    let row = store.execution(id.into()).await.unwrap().unwrap();
    assert_eq!(row.status, "unknown");
    assert!(
        store
            .workspace_claim(row.canonical_workspace_root)
            .await
            .unwrap()
            .is_some()
    );
}

/// 真实空 Job 与 verified missing Job 产生不同的 durable evidence；重复恢复幂等。
#[tokio::test]
async fn native_zero_destroyed_and_idempotence() {
    for exists in [false, true] {
        let (_dir, store, _manager, id, runtime, _db) = fixture(true).await;
        let handle = exists.then(|| job(&runtime));
        let report = startup(&store, "new-host").await.unwrap();
        assert_eq!(
            report.items,
            [ProviderReconcileItem {
                subject_id: id.clone(),
                kind: Kind::ExecutionInterrupted
            }]
        );
        let evidence = store.runtime(runtime.clone()).await.unwrap().unwrap();
        assert_eq!(evidence.state, "terminated");
        assert_eq!(
            evidence.termination_evidence_type.as_deref(),
            Some(if exists {
                "job_active_processes_zero"
            } else {
                "managed_job_destroyed"
            })
        );
        recover(&store, runtime.clone(), Duration::ZERO)
            .await
            .unwrap();
        assert_eq!(store.runtime(runtime).await.unwrap().unwrap(), evidence);
        let row = store.execution(id).await.unwrap().unwrap();
        assert_eq!(row.result_completeness, "unknown");
        assert!(row.final_result_json.is_none());
        assert!(row.thread_id.is_none());
        assert!(row.turn_id.is_none());
        assert!(
            store
                .workspace_claim(row.canonical_workspace_root)
                .await
                .unwrap()
                .is_none()
        );
        drop(handle);
    }
}

/// 持久化 identity/session/policy 缺失或冲突不能升级 PID disappearance。
#[tokio::test]
async fn invalid_identity_retains_claim() {
    for assignment in [
        "job_session_id=999999",
        "job_name='wrong'",
        "job_creation_mode=NULL",
        "job_handle_inheritable=1",
        "job_kill_on_close=0",
        "job_breakaway_allowed=1",
        "job_policy_verified_at=NULL",
        "provider='codex'",
    ] {
        let (_dir, store, _manager, id, runtime, db) = fixture(true).await;
        db.execute_batch("PRAGMA ignore_check_constraints=ON;")
            .unwrap();
        db.execute(
            &format!("UPDATE runtime_instances SET {assignment} WHERE id=?1"),
            [&runtime],
        )
        .unwrap();
        let report = startup(&store, "new-host").await.unwrap();
        assert!(matches!(
            report.items[0].kind,
            Kind::ExecutionUnknown | Kind::ExecutionInconsistent
        ));
        assert_unknown(&store, &id).await;
        assert_ne!(
            store
                .runtime(runtime)
                .await
                .unwrap()
                .unwrap()
                .termination_evidence_state,
            "complete"
        );
    }
}

/// private 缺失可停止 generic-owned Job 但不释放；已有 R1 冲突则不进行 OS mutation。
#[tokio::test]
async fn private_missing_and_conflict_fail_closed() {
    for private in [false, true] {
        let (_dir, store, _manager, id, runtime, db) = fixture(private).await;
        if private {
            db.execute("UPDATE codebuddy_execution_state SET runtime_instance_id=NULL WHERE execution_id=?1",[&id]).unwrap();
        }
        let report = startup(&store, "new-host").await.unwrap();
        assert_eq!(report.items[0].kind, Kind::ExecutionUnknown);
        assert_unknown(&store, &id).await;
        assert_eq!(
            store
                .runtime(runtime)
                .await
                .unwrap()
                .unwrap()
                .termination_evidence_state
                == "complete",
            !private
        );
    }
}

/// 无原 Runtime 的 execution 不能由 private identity 补造。
#[tokio::test]
async fn missing_runtime_retains_claim_and_orphan_is_recovered() {
    let (_dir, store, _manager, id, runtime, db) = fixture(false).await;
    db.execute_batch("DROP TRIGGER prevent_execution_runtime_rebind;")
        .unwrap();
    db.execute(
        "UPDATE executions SET runtime_instance_id=NULL WHERE id=?1",
        [&id],
    )
    .unwrap();
    let report = startup(&store, "new-host").await.unwrap();
    assert!(report.items.contains(&ProviderReconcileItem {
        subject_id: id.clone(),
        kind: Kind::ExecutionUnknown
    }));
    assert!(report.items.contains(&ProviderReconcileItem {
        subject_id: runtime,
        kind: Kind::OrphanResourceRecovered
    }));
    assert_unknown(&store, &id).await;
}

/// orphan 缺失 policy 仍为 unknown；Codex 所属 orphan 不被扫描或修改。
#[tokio::test]
async fn orphan_unknown_and_codex_isolation() {
    let (_dir, store, manager, id, runtime, db) = fixture(false).await;
    db.execute_batch("DROP TRIGGER prevent_execution_runtime_rebind;")
        .unwrap();
    db.execute(
        "UPDATE executions SET runtime_instance_id=NULL WHERE id=?1",
        [&id],
    )
    .unwrap();
    db.execute(
        "UPDATE runtime_instances SET job_policy_verified_at=NULL WHERE id=?1",
        [&runtime],
    )
    .unwrap();
    let report = startup(&store, "new-host").await.unwrap();
    assert!(
        report
            .items
            .iter()
            .any(|item| item.subject_id == runtime && item.kind == Kind::OrphanResourceUnknown)
    );
    let before = store.runtime(runtime.clone()).await.unwrap().unwrap();
    assert!(manager.recover_startup().await.unwrap().is_empty());
    assert_eq!(store.runtime(runtime).await.unwrap().unwrap(), before);
}

/// sealed evidence 提交时再核对原 snapshot；owner 变化与 DB 写失败都不能提交 complete。
#[tokio::test]
async fn durable_evidence_commit_failure_and_snapshot_conflict() {
    for mutate in [false, true] {
        let (_dir, store, _manager, id, runtime, db) = fixture(true).await;
        if mutate {
            let proof = observe(
                store.runtime(runtime.clone()).await.unwrap().unwrap(),
                Duration::ZERO,
            )
            .unwrap();
            db.execute(
                "UPDATE runtime_instances SET owner_host_instance_id='different-owner' WHERE id=?1",
                [&runtime],
            )
            .unwrap();
            assert!(store.complete_codebuddy_runtime(proof).await.is_err());
        } else {
            db.execute_batch("CREATE TRIGGER fail_complete BEFORE UPDATE ON runtime_instances WHEN NEW.termination_evidence_state='complete' BEGIN SELECT RAISE(ABORT,'injected persistence failure'); END;").unwrap();
            assert_eq!(
                startup(&store, "new-host").await.unwrap().items[0].kind,
                Kind::ExecutionUnknown
            );
            assert_unknown(&store, &id).await;
            assert_eq!(
                store.runtime(runtime.clone()).await.unwrap().unwrap().state,
                "unknown"
            );
        }
        assert_ne!(
            store
                .runtime(runtime)
                .await
                .unwrap()
                .unwrap()
                .termination_evidence_state,
            "complete"
        );
    }
}

/// 另一 Provider 的 complete evidence 不可借用为 CodeBuddy 幂等成功。
#[tokio::test]
async fn other_provider_complete_is_rejected() {
    let (_dir, store, _manager, _id, runtime, db) = fixture(true).await;
    recover(&store, runtime.clone(), Duration::ZERO)
        .await
        .unwrap();
    db.execute(
        "UPDATE runtime_instances SET provider='codex' WHERE id=?1",
        [&runtime],
    )
    .unwrap();
    assert!(recover(&store, runtime, Duration::ZERO).await.is_err());
}

/// 测试真正 first-runnable Job 内 parent 退出后 descendant 仍活着，恢复以整个 Job exact zero 为准。
#[tokio::test]
async fn native_tree_main_pid_gone_still_requires_job_zero() {
    let (dir, store, _manager, id, runtime, _db) = fixture(true).await;
    let executable = dir.path().join("recovery-child.exe");
    let output = std::process::Command::new("rustc")
        .args(["--edition=2024", "--crate-name", "codebuddy_recovery_child"])
        .arg(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/codebuddy_recovery_child.rs"),
        )
        .arg("-o")
        .arg(&executable)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let request = LaunchRequest::from_resolved(
        &ResolvedLaunchSpec {
            executable: executable.clone(),
            args: vec!["--acp".into()],
            path_projection: vec![dir.path().into()],
        },
        &crate::config::canonicalize_workspace_root(dir.path()).unwrap(),
        UncCurrentDirectoryPolicy::Supported,
        runtime.clone(),
    )
    .unwrap();
    let child = windows_launcher::launch(&request).unwrap().child;
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let stdout = child.stdout;
    let reader = std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        for _ in 0..2 {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            ready_tx.send(line).unwrap();
        }
    });
    for _ in 0..2 {
        assert!(
            !ready_rx
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .is_empty()
        );
    }
    reader.join().unwrap();
    assert_eq!(
        unsafe { WaitForSingleObject(child.process.as_raw_handle(), 5000) },
        WAIT_OBJECT_0
    );
    let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { zeroed() };
    assert_ne!(
        unsafe {
            QueryInformationJobObject(
                child.job.as_raw_handle(),
                JobObjectBasicAccountingInformation,
                (&mut info as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                null_mut(),
            )
        },
        0
    );
    assert!(
        info.ActiveProcesses > 0,
        "主 PID 已退出，但 descendant 必须仍在 Job 中"
    );
    assert_ne!(
        store
            .runtime(runtime.clone())
            .await
            .unwrap()
            .unwrap()
            .termination_evidence_state,
        "complete"
    );
    assert_eq!(
        startup(&store, "new-host").await.unwrap().items[0].kind,
        Kind::ExecutionInterrupted
    );
    assert_ne!(
        unsafe {
            QueryInformationJobObject(
                child.job.as_raw_handle(),
                JobObjectBasicAccountingInformation,
                (&mut info as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                null_mut(),
            )
        },
        0
    );
    assert_eq!(info.ActiveProcesses, 0);
    assert_eq!(
        store
            .runtime(runtime)
            .await
            .unwrap()
            .unwrap()
            .termination_evidence_type
            .as_deref(),
        Some("job_active_processes_zero")
    );
    assert_eq!(
        store.execution(id).await.unwrap().unwrap().status,
        "interrupted"
    );
}

/// 注入在真实 Win32 observer 的具体失败边界，Claim outcome 通过完整 startup 验证。
#[tokio::test]
async fn os_failure_matrix_keeps_unknown_and_claim() {
    use super::windows::Fault;
    // 局部别名仅描述故障观察函数，保留所有生产错误分类测试。
    type Observer = fn(RuntimeRecord, Duration) -> Result<TerminationEvidence, String>;
    let observers: [Observer; 8] = [
        |r, t| injected(r, t, Fault::Session),
        |r, t| injected(r, t, Fault::AccessDenied),
        |r, t| injected(r, t, Fault::Open),
        |r, t| injected(r, t, Fault::Query),
        |r, t| injected(r, t, Fault::Policy),
        |r, t| injected(r, t, Fault::PolicyQuery),
        |r, t| injected(r, t, Fault::Terminate),
        |r, _| injected(r, Duration::ZERO, Fault::Timeout),
    ];
    for observer in observers {
        let (_dir, store, _manager, id, runtime, _db) = fixture(true).await;
        let _job = job(&runtime);
        let report = TEST_OBSERVER
            .scope(observer, startup(&store, "new-host"))
            .await
            .unwrap();
        assert_eq!(report.items[0].kind, Kind::ExecutionUnknown);
        assert_unknown(&store, &id).await;
        let row = store.runtime(runtime).await.unwrap().unwrap();
        assert_eq!(row.state, "unknown");
        assert_ne!(row.termination_evidence_state, "complete");
    }
}

/// blocking 线程故障作用域在 observation 后清除，不影响后续测试。
fn injected(
    r: RuntimeRecord,
    t: Duration,
    fault: windows::Fault,
) -> Result<TerminationEvidence, String> {
    windows::FAULT.set(Some(fault));
    let result = observe(r, t);
    windows::FAULT.set(None);
    result
}

/// persisted policy 正确但 live policy 错误也不能生成 evidence。
#[tokio::test]
async fn native_live_policy_mismatch_is_unknown() {
    let (_dir, store, _manager, id, runtime, _db) = fixture(true).await;
    let handle = job(&runtime);
    let policy: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
    assert_ne!(
        unsafe {
            SetInformationJobObject(
                handle.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                (&policy as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        },
        0
    );
    assert_eq!(
        startup(&store, "new-host").await.unwrap().items[0].kind,
        Kind::ExecutionUnknown
    );
    assert_unknown(&store, &id).await;
}

/// 无效 Runtime id 在 prepare 和恢复入口均被拒绝，不能注入 Job namespace。
#[tokio::test]
async fn invalid_runtime_ids_never_form_evidence() {
    let (_dir, store, _manager, _id, runtime, _db) = fixture(true).await;
    for invalid in ["", "a\\b", "a/b", "a\0b"] {
        assert!(
            store
                .prepare_codebuddy_runtime(
                    invalid.into(),
                    "host".into(),
                    windows::session().unwrap(),
                    "fake.exe".into(),
                    1
                )
                .await
                .is_err()
        );
        let mut row = store.runtime(runtime.clone()).await.unwrap().unwrap();
        row.id = invalid.into();
        assert!(observe(row, Duration::ZERO).is_err());
    }
}

/// 全局 Claim 分类保留 pending/terminal inconsistent 语义，绝不补造 Runtime。
#[tokio::test]
async fn generic_claim_classification_is_preserved() {
    for (status, dispatch, expected) in [
        (
            "dispatch_pending",
            "not_dispatched",
            Kind::ExecutionPendingExplicitResume,
        ),
        ("interrupted", "dispatched", Kind::ExecutionInconsistent),
    ] {
        let (_dir, store, _manager, id, _runtime, db) = fixture(false).await;
        db.execute_batch("DROP TRIGGER prevent_execution_runtime_rebind;")
            .unwrap();
        db.execute("UPDATE executions SET runtime_instance_id=NULL,status=?2,dispatch_state=?3 WHERE id=?1",params![id,status,dispatch]).unwrap();
        let report = startup(&store, "new-host").await.unwrap();
        assert_eq!(
            report.items[0],
            ProviderReconcileItem {
                subject_id: id.clone(),
                kind: expected
            }
        );
        let row = store.execution(id).await.unwrap().unwrap();
        assert!(
            store
                .workspace_claim(row.canonical_workspace_root)
                .await
                .unwrap()
                .is_some()
        );
    }
}

/// 真正 TaskManager Registry 路径：disabled + missing CLI 及 refresh 后仍恢复历史 Claim。
#[tokio::test]
async fn disabled_missing_cli_refresh_retains_recovery_authority() {
    use crate::agent::{
        codebuddy::{TEST_DISCOVERY, discovery::DiscoveryError},
        provider::{ProviderId, registry::ProviderHealth},
    };
    let (dir, store, manager, id, _runtime, db) = fixture(true).await;
    // 模拟 Host restart：关闭旧 Store/Manager/connection，再从磁盘恢复 authority。
    drop((db, manager, store));
    let store = StateStore::open(dir.path().into()).await.unwrap();
    let mut manager = AgentTaskManager::new(store.clone(), "must-not-launch.exe".into());
    manager.set_provider_enabled_for_test("codebuddy", false);
    TEST_DISCOVERY
        .scope(Err(DiscoveryError::not_found(true)), async {
            let provider_id = ProviderId::new("codebuddy".into()).unwrap();
            assert_eq!(
                manager
                    .refresh_provider_health(provider_id.clone())
                    .await
                    .unwrap(),
                ProviderHealth::Unavailable
            );
            let registry = manager.registry().unwrap();
            let provider = registry.get_registered(&provider_id).unwrap();
            assert!(provider.capabilities().can_recover);
            assert!(provider.capabilities().can_execute);
            assert!(registry.get(&provider_id).is_err());
            drop(registry);
            let report = manager.reconcile_startup().await.unwrap();
            assert!(report.contains(&ProviderReconcileItem {
                subject_id: id.clone(),
                kind: Kind::ExecutionInterrupted
            }));
            assert_eq!(
                manager.registry().unwrap().health(&provider_id).unwrap(),
                ProviderHealth::Unavailable
            );
            assert_eq!(
                store.execution(id).await.unwrap().unwrap().status,
                "interrupted"
            );
        })
        .await;
}

/// generic execution 指向不存在的原 Runtime 时不允许释放 Claim。
#[tokio::test]
async fn missing_runtime_row_is_unknown() {
    let (_dir, store, _manager, id, runtime, db) = fixture(false).await;
    db.execute_batch("PRAGMA foreign_keys=OFF;").unwrap();
    db.execute("DELETE FROM runtime_instances WHERE id=?1", [runtime])
        .unwrap();
    assert_eq!(
        startup(&store, "new-host").await.unwrap().items[0].kind,
        Kind::ExecutionUnknown
    );
    assert_unknown(&store, &id).await;
}

/// 已有 generic complete release 可幂等清理历史 Claim，不要求 private Session。
#[tokio::test]
async fn already_released_claim_keeps_generic_outcome() {
    let (_dir, store, _manager, id, _runtime, db) = fixture(true).await;
    startup(&store, "new-host").await.unwrap();
    let row = store.execution(id.clone()).await.unwrap().unwrap();
    db.execute("INSERT INTO workspace_claims(canonical_workspace_root,execution_id,claim_type,acquired_at) VALUES (?1,?2,'exclusive_execution',1)",params![row.canonical_workspace_root,id]).unwrap();
    assert_eq!(
        startup(&store, "new-host").await.unwrap().items,
        [ProviderReconcileItem {
            subject_id: id,
            kind: Kind::ExecutionReleased
        }]
    );
    assert!(
        store
            .workspace_claim(row.canonical_workspace_root)
            .await
            .unwrap()
            .is_none()
    );
}

/// typed 生命周期拒绝逆序调用及跨 Provider 写入，原记录逐字段不变。
#[tokio::test]
async fn typed_runtime_updates_reject_invalid_transition_and_other_provider() {
    let (_dir, store, _manager, _id, runtime, db) = fixture(true).await;
    let before = store.runtime(runtime.clone()).await.unwrap().unwrap();
    assert!(
        store
            .update_codebuddy_runtime(runtime.clone(), CodeBuddyRuntimeUpdate::PolicyVerified, 5)
            .await
            .is_err()
    );
    assert!(
        store
            .update_codebuddy_runtime(runtime.clone(), CodeBuddyRuntimeUpdate::Initialized, 5)
            .await
            .is_err()
    );
    assert_eq!(
        store.runtime(runtime.clone()).await.unwrap().unwrap(),
        before
    );
    db.execute(
        "UPDATE runtime_instances SET provider='codex' WHERE id=?1",
        [&runtime],
    )
    .unwrap();
    let before = store.runtime(runtime.clone()).await.unwrap().unwrap();
    for update in [
        CodeBuddyRuntimeUpdate::PolicyVerified,
        CodeBuddyRuntimeUpdate::ProcessStarted {
            pid: 99,
            start_token: "diagnostic".into(),
        },
        CodeBuddyRuntimeUpdate::Initialized,
        CodeBuddyRuntimeUpdate::Terminating,
        CodeBuddyRuntimeUpdate::Unknown,
    ] {
        assert!(
            store
                .update_codebuddy_runtime(runtime.clone(), update, 6)
                .await
                .is_err()
        );
        assert_eq!(
            store.runtime(runtime.clone()).await.unwrap().unwrap(),
            before
        );
    }
}

/// 构建只接受 initialize/session/load 的 native peer；所有请求证据写在 Workspace 外。
fn build_result_recovery_peer(directory: &std::path::Path) -> std::path::PathBuf {
    use std::os::windows::process::CommandExt;
    let deps = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_owned();
    let library = std::fs::read_dir(&deps)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("libserde_json-")
                && entry
                    .path()
                    .extension()
                    .is_some_and(|value| value == "rlib")
        })
        .max_by_key(|entry| entry.metadata().unwrap().modified().unwrap())
        .expect("built serde_json")
        .path();
    let binary = directory.join("result-recovery-peer.exe");
    let output = std::process::Command::new("rustc")
        .args([
            "--edition=2024",
            "--crate-name",
            "codebuddy_result_recovery_child",
        ])
        .arg(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/codebuddy_result_recovery_child.rs"),
        )
        .arg("-L")
        .arg(format!("dependency={}", deps.display()))
        .arg("--extern")
        .arg(format!("serde_json={}", library.display()))
        .arg("-o")
        .arg(&binary)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    binary
}

/// 单个 crash window 使用独立真实 SQLite、Workspace、R1 与 native R2 peer。
async fn result_recovery_fixture(
    binary: &std::path::Path,
    window: &str,
    mode: &str,
) -> (
    tempfile::TempDir,
    tempfile::TempDir,
    StateStore,
    String,
    super::super::discovery::DiscoveryResult,
) {
    use crate::agent::{
        codebuddy::store::{CodeBuddyStore, Mutation, Ownership},
        execution::state::{ResultCompleteness, Status, Transition},
        execution::{CreateExecutionInput, canonicalize_request},
    };
    let control = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let store = StateStore::open(control.path().into()).await.unwrap();
    let root = crate::config::canonicalize_workspace_root(workspace.path()).unwrap();
    let input: CreateExecutionInput = serde_json::from_value(serde_json::json!({
        "agent_id":"a","request_key":"k","prompt":"fixed crash fixture",
        "execution_profile":{},"workspace_id":"w","canonical_workspace_root":root,
        "mode":"workspace_write","provider":"codebuddy"
    }))
    .unwrap();
    store
        .create_execution("e".into(), canonicalize_request(input).unwrap(), 1)
        .await
        .unwrap();
    let r1 = "cb8-r1".to_owned();
    store
        .prepare_codebuddy_runtime(
            r1.clone(),
            "old-host".into(),
            windows::session().unwrap(),
            "old-codebuddy.exe".into(),
            2,
        )
        .await
        .unwrap();
    store
        .update_codebuddy_runtime(r1.clone(), CodeBuddyRuntimeUpdate::PolicyVerified, 3)
        .await
        .unwrap();
    store
        .update_codebuddy_runtime(
            r1.clone(),
            CodeBuddyRuntimeUpdate::ProcessStarted {
                pid: 0,
                start_token: "diagnostic-only".into(),
            },
            4,
        )
        .await
        .unwrap();
    store
        .update_codebuddy_runtime(r1.clone(), CodeBuddyRuntimeUpdate::Initialized, 5)
        .await
        .unwrap();
    let db = Connection::open(control.path().join("agent-state.db")).unwrap();
    let (status, dispatch) = match window {
        "prepared" => ("dispatch_pending", "not_dispatched"),
        "preflush" => ("dispatch_pending", "dispatching"),
        _ => ("running", "dispatched"),
    };
    db.execute(
        "UPDATE executions SET runtime_instance_id=?2,status=?3,dispatch_state=?4 WHERE id=?1",
        params!["e", r1, status, dispatch],
    )
    .unwrap();
    let private_store = CodeBuddyStore(store.clone());
    let ownership = Ownership {
        execution_revision: 0,
        runtime_instance_id: Some(r1.clone()),
    };
    let mut private = private_store
        .create("e".into(), ownership.clone())
        .await
        .unwrap();
    private = private_store
        .mutate(
            "e".into(),
            ownership.clone(),
            private.revision,
            Mutation::NegotiatedProtocol(1),
        )
        .await
        .unwrap();
    private = private_store
        .mutate(
            "e".into(),
            ownership.clone(),
            private.revision,
            Mutation::ExactSession("exact-session".into()),
        )
        .await
        .unwrap();
    if window != "prepared" {
        private = private_store
            .mutate(
                "e".into(),
                ownership.clone(),
                private.revision,
                Mutation::MarkSent { rpc_id: None },
            )
            .await
            .unwrap();
    }
    if matches!(window, "terminal" | "staged") {
        private = private_store
            .mutate(
                "e".into(),
                ownership,
                private.revision,
                Mutation::ObserveTerminal {
                    session_id: "exact-session".into(),
                    conversation_request_id: private.conversation_request_id.clone(),
                    stop_reason: agent_client_protocol::schema::v1::StopReason::EndTurn,
                    observed_at: 6,
                },
            )
            .await
            .unwrap();
    }
    if window == "staged" {
        store
            .provider_event(
                "e".into(),
                Transition::ProviderTerminalResult {
                    runtime_id: r1.clone(),
                    status: Status::Completed,
                    result: Some(serde_json::json!({"text":"original exact"})),
                    completeness: ResultCompleteness::Complete,
                },
                7,
            )
            .await
            .unwrap();
    }
    std::fs::write(control.path().join("mode"), mode).unwrap();
    std::fs::write(
        control.path().join("conversation"),
        &private.conversation_request_id,
    )
    .unwrap();
    let peer = control.path().join("peer.exe");
    std::fs::copy(binary, &peer).unwrap();
    let discovery = super::super::discovery::DiscoveryResult::direct_for_test(peer);
    (control, workspace, store, r1, discovery)
}

/// 读取 fake peer 原始 method 序列；文件不存在表示 R2 从未启动。
fn recovery_methods(control: &std::path::Path) -> Vec<String> {
    let path = control.join("requests.jsonl");
    if !path.exists() {
        return Vec::new();
    }
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|line| {
            serde_json::from_str::<serde_json::Value>(line).unwrap()["method"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect()
}

/// 通过 CB8-003 的 typed mutation 形成 same-runtime continuation partial provenance。
async fn mark_same_runtime_continuation_partial(
    store: &StateStore,
    r1: &str,
) -> crate::agent::codebuddy::store::PrivateState {
    use crate::agent::codebuddy::store::{CodeBuddyStore, Mutation, RecoveryState};
    let row = store.execution("e".into()).await.unwrap().unwrap();
    let ownership = Ownership {
        execution_revision: row.revision,
        runtime_instance_id: Some(r1.to_owned()),
    };
    let private_store = CodeBuddyStore(store.clone());
    let mut private = store.read_codebuddy_state("e".into()).await.unwrap();
    private = private_store
        .mutate(
            "e".into(),
            ownership.clone(),
            private.revision,
            Mutation::BeginContinuationLoad {
                session_id: "exact-session".into(),
                recovery_runtime_instance_id: r1.to_owned(),
            },
        )
        .await
        .unwrap();
    private = private_store
        .mutate(
            "e".into(),
            ownership,
            private.revision,
            Mutation::FinishContinuationLoad,
        )
        .await
        .unwrap();
    assert_eq!(private.recovery_state, RecoveryState::Partial);
    assert_eq!(private.recovery_runtime_instance_id.as_deref(), Some(r1));
    assert_eq!(private.runtime_instance_id.as_deref(), Some(r1));
    private
}

/// CB8-003 same-runtime partial 只描述 child continuation；crash 后必须新建 external R2。
#[tokio::test]
async fn continued_child_same_runtime_partial_crash_starts_external_result_recovery() {
    fn fail_r2(runtime: RuntimeRecord, timeout: Duration) -> Result<TerminationEvidence, String> {
        if runtime.id.starts_with("codebuddy-recovery-") {
            Err("injected external R2 evidence failure".into())
        } else {
            observe(runtime, timeout)
        }
    }

    let build_dir = tempfile::tempdir().unwrap();
    let binary = build_result_recovery_peer(build_dir.path());

    // exact replay：R1 recovery proof 后覆盖 same-runtime provenance，并只创建一个 external R2。
    let (control, _workspace, store, r1, discovery) =
        result_recovery_fixture(&binary, "sent", "exact").await;
    mark_same_runtime_continuation_partial(&store, &r1).await;
    assert!(!approved_runtime(&store, &r1).await.unwrap());
    let report = startup_with_launch(&store, "new-host", Some(&discovery.launch_spec))
        .await
        .unwrap();
    assert_eq!(report.items[0].kind, Kind::ExecutionInterrupted);
    let row = store.execution("e".into()).await.unwrap().unwrap();
    assert_eq!(row.status, "interrupted");
    assert_eq!(row.result_completeness, "partial");
    assert_eq!(row.runtime_instance_id.as_deref(), Some(r1.as_str()));
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(row.final_result_json.as_deref().unwrap())
            .unwrap(),
        serde_json::json!({"text":"recovered partial"})
    );
    let private = store.read_codebuddy_state("e".into()).await.unwrap();
    let r2 = private.recovery_runtime_instance_id.clone().unwrap();
    assert_ne!(r2, r1);
    assert_eq!(
        private.recovery_state,
        crate::agent::codebuddy::store::RecoveryState::Partial
    );
    assert!(approved_runtime(&store, &r1).await.unwrap());
    assert!(approved_runtime(&store, &r2).await.unwrap());
    assert_eq!(
        recovery_methods(control.path()),
        ["initialize", "session/load"]
    );
    assert!(!control.path().join("forbidden-method").exists());
    assert!(
        store
            .workspace_claim(row.canonical_workspace_root.clone())
            .await
            .unwrap()
            .is_none()
    );
    let database = Connection::open(control.path().join("agent-state.db")).unwrap();
    assert_eq!(
        database
            .query_row(
                "SELECT count(*) FROM execution_runtime_attempts WHERE execution_id='e'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        1
    );
    assert_eq!(
        database
            .query_row(
                "SELECT runtime_instance_id FROM execution_runtime_attempts WHERE execution_id='e'",
                [],
                |row| row.get::<_, String>(0)
            )
            .unwrap(),
        r2
    );
    let before = (
        row.revision,
        recovery_methods(control.path()),
        database
            .query_row(
                "SELECT count(*) FROM execution_runtime_attempts WHERE execution_id='e'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
    );
    drop(database);
    let second = startup_with_launch(&store, "new-host", Some(&discovery.launch_spec))
        .await
        .unwrap();
    assert!(second.items.is_empty());
    assert_eq!(
        store.execution("e".into()).await.unwrap().unwrap().revision,
        before.0
    );
    assert_eq!(recovery_methods(control.path()), before.1);
    assert_eq!(
        Connection::open(control.path().join("agent-state.db"))
            .unwrap()
            .query_row(
                "SELECT count(*) FROM execution_runtime_attempts WHERE execution_id='e'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        before.2
    );

    // external R2 evidence失败：same-runtime partial 不能授权 release，Claim 必须保留。
    let (control, _workspace, store, r1, discovery) =
        result_recovery_fixture(&binary, "sent", "exact").await;
    mark_same_runtime_continuation_partial(&store, &r1).await;
    let report = TEST_OBSERVER
        .scope(
            fail_r2,
            startup_with_launch(&store, "new-host", Some(&discovery.launch_spec)),
        )
        .await
        .unwrap();
    assert_eq!(report.items[0].kind, Kind::ExecutionUnknown);
    assert_unknown(&store, "e").await;
    let row = store.execution("e".into()).await.unwrap().unwrap();
    assert_eq!(row.runtime_instance_id.as_deref(), Some(r1.as_str()));
    let private = store.read_codebuddy_state("e".into()).await.unwrap();
    let r2 = private.recovery_runtime_instance_id.as_deref().unwrap();
    assert_ne!(r2, r1);
    assert_eq!(
        private.recovery_state,
        crate::agent::codebuddy::store::RecoveryState::Inspecting
    );
    assert!(approved_runtime(&store, &r1).await.unwrap());
    assert!(!approved_runtime(&store, r2).await.unwrap());
    assert_eq!(
        recovery_methods(control.path()),
        ["initialize", "session/load"]
    );
    assert_eq!(
        Connection::open(control.path().join("agent-state.db"))
            .unwrap()
            .query_row(
                "SELECT count(*) FROM execution_runtime_attempts WHERE execution_id='e'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        1
    );
}

/// A-D crash matrix：真实 pre-prompt generic 状态也必须先证明 R1，partial 不冒充 complete。
#[tokio::test]
async fn crash_result_recovery_matrix_and_restart_twice_are_safe() {
    use sha2::{Digest, Sha256};
    let build_dir = tempfile::tempdir().unwrap();
    let binary = build_result_recovery_peer(build_dir.path());
    for (window, mode, expected_status, expected_completeness, r2_started) in [
        ("prepared", "empty", "interrupted", "unknown", false),
        ("preflush", "empty", "interrupted", "unknown", true),
        ("sent", "empty", "interrupted", "unknown", true),
        ("sent", "exact", "interrupted", "partial", true),
        ("terminal", "exact", "interrupted", "partial", true),
        ("staged", "empty", "completed", "complete", false),
    ] {
        let (control, workspace, store, r1, discovery) =
            result_recovery_fixture(&binary, window, mode).await;
        let marker = workspace.path().join("marker.txt");
        std::fs::write(&marker, b"CB8_SIDE_EFFECT\n").unwrap();
        let marker_before = Sha256::digest(std::fs::read(&marker).unwrap());
        let report = startup_with_launch(&store, "new-host", Some(&discovery.launch_spec))
            .await
            .unwrap();
        assert_eq!(
            report.items[0].kind,
            if expected_status == "interrupted" {
                Kind::ExecutionInterrupted
            } else {
                Kind::ExecutionReleased
            },
            "{window}/{mode}"
        );
        let row = store.execution("e".into()).await.unwrap().unwrap();
        assert_eq!(row.status, expected_status, "{window}/{mode}");
        assert_eq!(
            row.result_completeness, expected_completeness,
            "{window}/{mode}"
        );
        assert_eq!(row.runtime_instance_id.as_deref(), Some(r1.as_str()));
        assert_eq!(row.release_evidence_state, "complete");
        assert!(
            store
                .workspace_claim(row.canonical_workspace_root.clone())
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(
            Sha256::digest(std::fs::read(&marker).unwrap()),
            marker_before,
            "R2 must not modify the side-effect marker"
        );
        let private = store.read_codebuddy_state("e".into()).await.unwrap();
        if r2_started {
            let r2 = private.recovery_runtime_instance_id.as_deref().unwrap();
            assert_ne!(r2, r1);
            assert!(approved_runtime(&store, r2).await.unwrap());
            assert_eq!(
                recovery_methods(control.path()),
                ["initialize", "session/load"]
            );
        } else {
            assert!(recovery_methods(control.path()).is_empty());
        }
        if expected_completeness == "partial" {
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(
                    row.final_result_json.as_deref().unwrap()
                )
                .unwrap(),
                serde_json::json!({"text":"recovered partial"})
            );
        }
        if expected_completeness == "complete" {
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(
                    row.final_result_json.as_deref().unwrap()
                )
                .unwrap(),
                serde_json::json!({"text":"original exact"})
            );
        }
        let before = (
            row.revision,
            recovery_methods(control.path()),
            Connection::open(control.path().join("agent-state.db"))
                .unwrap()
                .query_row(
                    "SELECT count(*) FROM execution_runtime_attempts",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
        );
        let second = startup_with_launch(&store, "new-host", Some(&discovery.launch_spec))
            .await
            .unwrap();
        assert!(second.items.is_empty(), "{window}/{mode}");
        let after = store.execution("e".into()).await.unwrap().unwrap();
        assert_eq!(after.revision, before.0);
        assert_eq!(recovery_methods(control.path()), before.1);
        assert_eq!(
            Connection::open(control.path().join("agent-state.db"))
                .unwrap()
                .query_row(
                    "SELECT count(*) FROM execution_runtime_attempts",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            before.2
        );
    }
}

/// R1 proof 缺失时不得创建 R2；R2 load/identity 失败但安全终止时释放为 interrupted unknown。
#[tokio::test]
async fn evidence_and_replay_failure_matrix_fails_closed() {
    let build_dir = tempfile::tempdir().unwrap();
    let binary = build_result_recovery_peer(build_dir.path());
    let (control, _workspace, store, _r1, discovery) =
        result_recovery_fixture(&binary, "sent", "exact").await;
    Connection::open(control.path().join("agent-state.db"))
        .unwrap()
        .execute(
            "UPDATE runtime_instances SET job_policy_verified_at=NULL WHERE id='cb8-r1'",
            [],
        )
        .unwrap();
    let report = startup_with_launch(&store, "new-host", Some(&discovery.launch_spec))
        .await
        .unwrap();
    assert_eq!(report.items[0].kind, Kind::ExecutionUnknown);
    assert!(recovery_methods(control.path()).is_empty());
    assert_unknown(&store, "e").await;
    let before = store.execution("e".into()).await.unwrap().unwrap();
    let before_attempts: i64 = Connection::open(control.path().join("agent-state.db"))
        .unwrap()
        .query_row(
            "SELECT count(*) FROM execution_runtime_attempts",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let second = startup_with_launch(&store, "new-host", Some(&discovery.launch_spec))
        .await
        .unwrap();
    assert_eq!(second.items[0].kind, Kind::ExecutionUnknown);
    assert_unknown(&store, "e").await;
    assert!(recovery_methods(control.path()).is_empty());
    assert_eq!(
        store.execution("e".into()).await.unwrap().unwrap().revision,
        before.revision
    );
    assert_eq!(
        Connection::open(control.path().join("agent-state.db"))
            .unwrap()
            .query_row(
                "SELECT count(*) FROM execution_runtime_attempts",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        before_attempts
    );

    for mode in [
        "empty",
        "foreign",
        "wrong-session",
        "load-error",
        "load-mismatch",
        "no-capability",
    ] {
        let (control, _workspace, store, r1, discovery) =
            result_recovery_fixture(&binary, "sent", mode).await;
        let report = startup_with_launch(&store, "new-host", Some(&discovery.launch_spec))
            .await
            .unwrap();
        assert_eq!(report.items[0].kind, Kind::ExecutionInterrupted, "{mode}");
        let row = store.execution("e".into()).await.unwrap().unwrap();
        assert_eq!(row.status, "interrupted", "{mode}");
        assert_eq!(row.result_completeness, "unknown", "{mode}");
        assert!(row.final_result_json.is_none(), "{mode}");
        let private = store.read_codebuddy_state("e".into()).await.unwrap();
        let r2 = private.recovery_runtime_instance_id.as_deref().unwrap();
        assert_ne!(r2, r1);
        assert!(approved_runtime(&store, r2).await.unwrap());
        assert!(
            recovery_methods(control.path())
                .iter()
                .all(|method| matches!(method.as_str(), "initialize" | "session/load"))
        );
    }
}

/// 注入 R2 Job proof 失败时 Claim 必须保留；下一次取得 approved evidence 后只收敛原 R2。
#[tokio::test]
async fn r2_termination_failure_retains_claim_then_resumes_without_new_runtime() {
    fn fail_r2(runtime: RuntimeRecord, timeout: Duration) -> Result<TerminationEvidence, String> {
        if runtime.id.starts_with("codebuddy-recovery-") {
            Err("injected R2 evidence failure".into())
        } else {
            observe(runtime, timeout)
        }
    }
    let build_dir = tempfile::tempdir().unwrap();
    let binary = build_result_recovery_peer(build_dir.path());
    let (control, _workspace, store, r1, discovery) =
        result_recovery_fixture(&binary, "sent", "exact").await;
    let first = TEST_OBSERVER
        .scope(
            fail_r2,
            startup_with_launch(&store, "new-host", Some(&discovery.launch_spec)),
        )
        .await
        .unwrap();
    assert_eq!(first.items[0].kind, Kind::ExecutionUnknown);
    assert_unknown(&store, "e").await;
    let private = store.read_codebuddy_state("e".into()).await.unwrap();
    let r2 = private.recovery_runtime_instance_id.clone().unwrap();
    assert_ne!(r2, r1);
    assert_eq!(
        private.recovery_state,
        crate::agent::codebuddy::store::RecoveryState::Inspecting
    );
    assert!(!approved_runtime(&store, &r2).await.unwrap());
    let before_methods = recovery_methods(control.path());
    let before_attempts: i64 = Connection::open(control.path().join("agent-state.db"))
        .unwrap()
        .query_row(
            "SELECT count(*) FROM execution_runtime_attempts",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let second = startup_with_launch(&store, "new-host", Some(&discovery.launch_spec))
        .await
        .unwrap();
    assert_eq!(second.items[0].kind, Kind::ExecutionInterrupted);
    let row = store.execution("e".into()).await.unwrap().unwrap();
    assert_eq!(row.status, "interrupted");
    assert_eq!(row.result_completeness, "unknown");
    assert!(approved_runtime(&store, &r2).await.unwrap());
    assert_eq!(recovery_methods(control.path()), before_methods);
    assert_eq!(
        Connection::open(control.path().join("agent-state.db"))
            .unwrap()
            .query_row(
                "SELECT count(*) FROM execution_runtime_attempts",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        before_attempts
    );
}

/// private provenance 损坏时，下一 Host 的 orphan scan 必须独立收敛 R2，但不得绕过原 Claim。
#[tokio::test]
async fn orphan_r2_is_recovered_without_releasing_unknown_execution() {
    fn fail_r2(runtime: RuntimeRecord, timeout: Duration) -> Result<TerminationEvidence, String> {
        if runtime.id.starts_with("codebuddy-recovery-") {
            Err("injected R2 evidence failure".into())
        } else {
            observe(runtime, timeout)
        }
    }
    let build_dir = tempfile::tempdir().unwrap();
    let binary = build_result_recovery_peer(build_dir.path());
    let (control, _workspace, store, _r1, discovery) =
        result_recovery_fixture(&binary, "sent", "exact").await;
    TEST_OBSERVER
        .scope(
            fail_r2,
            startup_with_launch(&store, "first-host", Some(&discovery.launch_spec)),
        )
        .await
        .unwrap();
    let private = store.read_codebuddy_state("e".into()).await.unwrap();
    let r2 = private.recovery_runtime_instance_id.clone().unwrap();
    let before_methods = recovery_methods(control.path());
    let before_attempts: i64 = Connection::open(control.path().join("agent-state.db"))
        .unwrap()
        .query_row(
            "SELECT count(*) FROM execution_runtime_attempts",
            [],
            |row| row.get(0),
        )
        .unwrap();
    // 模拟 private provenance 无法读取；R2 只能由既有 provider orphan runtime scan 收敛。
    Connection::open(control.path().join("agent-state.db"))
        .unwrap()
        .execute(
            "DELETE FROM codebuddy_execution_state WHERE execution_id='e'",
            [],
        )
        .unwrap();
    let report = startup_with_launch(&store, "second-host", Some(&discovery.launch_spec))
        .await
        .unwrap();
    assert!(
        report
            .items
            .iter()
            .any(|item| item.subject_id == "e" && item.kind == Kind::ExecutionUnknown)
    );
    assert!(
        report
            .items
            .iter()
            .any(|item| { item.subject_id == r2 && item.kind == Kind::OrphanResourceRecovered })
    );
    assert_unknown(&store, "e").await;
    assert!(approved_runtime(&store, &r2).await.unwrap());
    assert_eq!(recovery_methods(control.path()), before_methods);
    assert_eq!(
        Connection::open(control.path().join("agent-state.db"))
            .unwrap()
            .query_row(
                "SELECT count(*) FROM execution_runtime_attempts",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        before_attempts
    );
    let revision = store.execution("e".into()).await.unwrap().unwrap().revision;
    let second = startup_with_launch(&store, "second-host", Some(&discovery.launch_spec))
        .await
        .unwrap();
    assert_eq!(second.items.len(), 1);
    assert_eq!(second.items[0].kind, Kind::ExecutionUnknown);
    assert_eq!(
        store.execution("e".into()).await.unwrap().unwrap().revision,
        revision
    );
    assert_eq!(recovery_methods(control.path()), before_methods);
}
