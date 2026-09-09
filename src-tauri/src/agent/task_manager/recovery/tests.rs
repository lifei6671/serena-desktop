use super::*;
use crate::agent::{execution::CreateExecutionInput, store::StateStore};
use rusqlite::{Connection, params};
use serde_json::json;

fn run(f: impl std::future::Future<Output = ()>) {
    tokio::runtime::Runtime::new().unwrap().block_on(f);
}
async fn fixture(
    root: &std::path::Path,
    state: &str,
    dispatch: &str,
) -> (AgentTaskManager, String, Connection) {
    let store = StateStore::open(root.into()).await.unwrap();
    let manager = AgentTaskManager::new(store, "C:/not-a-provider.exe".into());
    let input: CreateExecutionInput = serde_json::from_value(json!({"agent_id":"a","request_key":"k","prompt":"p","execution_profile":{},"workspace_id":"w","canonical_workspace_root":root.to_str().unwrap(),"mode":"read_only"})).unwrap();
    let id = manager.create(input).await.unwrap().execution_id;
    let db = Connection::open(root.join("agent-state.db")).unwrap();
    db.execute(
        "UPDATE executions SET status=?1,dispatch_state=?2 WHERE id=?3",
        params![state, dispatch, id],
    )
    .unwrap();
    (manager, id, db)
}
fn original(db: &Connection, id: &str, safe: bool) {
    db.execute("INSERT INTO runtime_instances(id,owner_host_instance_id,state,created_at,updated_at,termination_evidence_state,termination_evidence_type,termination_evidence_at) VALUES ('R1','old-host',?1,1,1,?2,?3,?4)",params![if safe {"terminated"} else {"running"},if safe {"complete"} else {"unknown"},if safe {Some("job_active_processes_zero")} else {None},if safe {Some(10)} else {None}]).unwrap();
    db.execute(
        "UPDATE executions SET runtime_instance_id='R1' WHERE id=?1",
        [id],
    )
    .unwrap();
}
async fn retained(manager: &AgentTaskManager, id: &str) -> ExecutionRecord {
    let row = manager.store.execution(id.into()).await.unwrap().unwrap();
    assert!(
        manager
            .store
            .workspace_claim(row.canonical_workspace_root.clone())
            .await
            .unwrap()
            .is_some()
    );
    row
}

#[test]
fn pre_dispatch_crash_repeated_startup_waits_for_explicit_resume_without_side_effects() {
    run(async {
        let temp = tempfile::tempdir().unwrap();
        let (manager, id, db) = fixture(temp.path(), "dispatch_pending", "not_dispatched").await;
        let before = retained(&manager, &id).await;
        let claim_before = manager
            .store
            .workspace_claim(before.canonical_workspace_root.clone())
            .await
            .unwrap();
        for _ in 0..5 {
            assert!(matches!(
                &manager.recover_startup().await.unwrap()[0],
                RecoveryOutcome::PendingExplicitResume { .. }
            ));
            assert_eq!(before, retained(&manager, &id).await);
            assert_eq!(
                claim_before,
                manager
                    .store
                    .workspace_claim(before.canonical_workspace_root.clone())
                    .await
                    .unwrap()
            );
        }
        assert_eq!(
            db.query_row("SELECT count(*) FROM runtime_instances", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    });
}

#[test]
fn explicit_resume_rejects_invalid_states_bindings_and_claims_without_launch() {
    run(async {
        let cases = [
            ("dispatch_pending", "dispatching"),
            ("dispatch_pending", "dispatched"),
            ("dispatch_pending", "uncertain"),
            ("running", "dispatched"),
            ("cancel_requested", "dispatched"),
            ("cancelling", "dispatched"),
            ("finalizing", "dispatched"),
            ("reconciling", "uncertain"),
            ("unknown", "uncertain"),
            ("completed", "dispatched"),
            ("failed", "dispatched"),
            ("cancelled", "not_dispatched"),
            ("interrupted", "uncertain"),
        ];
        for (status, dispatch) in cases {
            let temp = tempfile::tempdir().unwrap();
            let (manager, id, db) = fixture(temp.path(), status, dispatch).await;
            let before = manager.store.execution(id.clone()).await.unwrap();
            let error = manager.resume_pending_execution(&id).await.unwrap_err();
            assert!(
                matches!(error,crate::agent::codex::provider::ExecutionFailure::State(ref s) if s=="PENDING_RESUME_REJECTED")
            );
            assert_eq!(before, manager.store.execution(id).await.unwrap());
            assert_eq!(
                db.query_row("SELECT count(*) FROM runtime_instances", [], |r| r
                    .get::<_, i64>(0))
                    .unwrap(),
                0
            );
        }
        for case in [
            "bound",
            "provider-terminal",
            "missing-claim",
            "wrong-owner",
            "wrong-root",
            "missing-execution",
        ] {
            let temp = tempfile::tempdir().unwrap();
            let (manager, id, db) =
                fixture(temp.path(), "dispatch_pending", "not_dispatched").await;
            match case {
                "bound" => original(&db, &id, false),
                "provider-terminal" => {
                    db.execute(
                        "UPDATE executions SET provider_terminal_status='completed'",
                        [],
                    )
                    .unwrap();
                }
                "missing-claim" => {
                    db.execute("DELETE FROM workspace_claims", []).unwrap();
                }
                "wrong-owner" => {
                    db.execute("INSERT INTO executions(id,agent_id,request_key,request_hash,prompt,execution_profile_json,workspace_id,canonical_workspace_root,provider,mode,status,created_at,updated_at) VALUES ('other','other','k','h','p','{}','w',?1,'codex','read_only','dispatch_pending',1,1)",[temp.path().to_str().unwrap()]).unwrap();
                    db.execute("UPDATE workspace_claims SET execution_id='other'", [])
                        .unwrap();
                }
                "wrong-root" => {
                    // Historical corrupt fixture only: normal composite FK forbids this row.
                    db.pragma_update(None, "foreign_keys", false).unwrap();
                    db.execute(
                        "UPDATE workspace_claims SET canonical_workspace_root='other-root'",
                        [],
                    )
                    .unwrap();
                }
                _ => {}
            }
            let before = manager.store.execution(id.clone()).await.unwrap();
            assert!(
                manager
                    .resume_pending_execution(if case == "missing-execution" {
                        "absent"
                    } else {
                        &id
                    })
                    .await
                    .is_err(),
                "{case}"
            );
            assert_eq!(before, manager.store.execution(id).await.unwrap());
            assert_eq!(
                db.query_row("SELECT count(*) FROM runtime_instances", [], |r| r
                    .get::<_, i64>(0))
                    .unwrap(),
                if case == "bound" { 1 } else { 0 }
            );
        }
    });
}

#[test]
fn explicit_resume_runtime_failure_retains_claim_and_rejects_retry() {
    run(async {
        let temp = tempfile::tempdir().unwrap();
        let (manager, id, db) = fixture(temp.path(), "dispatch_pending", "not_dispatched").await;
        // Guard succeeds, then the existing ManagedClient rejects the missing binary.
        assert!(matches!(
            manager.resume_pending_execution(&id).await,
            Err(crate::agent::codex::provider::ExecutionFailure::Runtime(_))
        ));
        let row = retained(&manager, &id).await;
        assert_eq!(row.status, "reconciling");
        assert_eq!(row.dispatch_state, "not_dispatched");
        assert!(manager.resume_pending_execution(&id).await.is_err());
        assert_eq!(row, retained(&manager, &id).await);
        assert_eq!(
            db.query_row("SELECT count(*) FROM runtime_instances", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    });
}

#[test]
fn multiple_claims_drive_recovery_not_unclaimed_status_rows() {
    run(async {
        let temp = tempfile::tempdir().unwrap();
        let (manager, _, db) = fixture(temp.path(), "dispatch_pending", "not_dispatched").await;
        for (id, status, claim) in [
            ("unknown", "unknown", true),
            ("legacy", "completed", true),
            ("unclaimed", "running", false),
        ] {
            db.execute("INSERT INTO executions(id,agent_id,request_key,request_hash,prompt,execution_profile_json,workspace_id,canonical_workspace_root,provider,mode,status,created_at,updated_at) VALUES (?1,?1,'k','h','p','{}','w',?1,'codex','read_only',?2,1,1)",params![id,status]).unwrap();
            if claim {
                db.execute(
                    "INSERT INTO workspace_claims VALUES (?1,?1,'exclusive_execution',1)",
                    [id],
                )
                .unwrap();
            }
        }
        let before = manager.store.execution("unclaimed".into()).await.unwrap();
        let report = manager.recover_startup().await.unwrap();
        assert_eq!(report.len(), 3);
        assert!(report.iter().any(
            |r| matches!(r,RecoveryOutcome::Unknown {execution_id,..} if execution_id=="unknown")
        ));
        assert!(report.iter().any(|r|matches!(r,RecoveryOutcome::Inconsistent {execution_id,..} if execution_id=="legacy")));
        assert_eq!(
            before,
            manager.store.execution("unclaimed".into()).await.unwrap()
        );
        assert_eq!(
            db.query_row("SELECT count(*) FROM workspace_claims", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            3
        );
    });
}

#[test]
fn same_session_destroyed_job_releases_but_missing_or_cross_session_retains_unknown() {
    run(async {
        for mode in ["same", "cross", "missing", "policy", "name"] {
            let temp = tempfile::tempdir().unwrap();
            let (manager, id, db) = fixture(temp.path(), "running", "dispatched").await;
            original(&db, &id, false);
            let session = runtime::current_session_id().unwrap();
            db.execute("UPDATE runtime_instances SET job_name='Local\\SerenaDesktop.Codex.R1',job_session_id=?1,job_creation_mode='proc_thread_attribute_job_list',job_handle_inheritable=0,job_kill_on_close=1,job_breakaway_allowed=0,job_policy_verified_at=1",[session]).unwrap();
            match mode {
                "cross" => {
                    db.execute(
                        "UPDATE runtime_instances SET job_session_id=job_session_id+1",
                        [],
                    )
                    .unwrap();
                }
                "missing" => {
                    db.execute("UPDATE runtime_instances SET job_session_id=NULL", [])
                        .unwrap();
                }
                "policy" => {
                    db.execute(
                        "UPDATE runtime_instances SET job_policy_verified_at=NULL",
                        [],
                    )
                    .unwrap();
                }
                "name" => {
                    db.execute("UPDATE runtime_instances SET job_name='wrong'", [])
                        .unwrap();
                }
                _ => {}
            }
            let report = manager.recover_startup().await.unwrap();
            if mode == "same" {
                assert!(matches!(&report[0], RecoveryOutcome::Interrupted { .. }));
                let r = manager.store.runtime("R1".into()).await.unwrap().unwrap();
                assert_eq!(
                    r.termination_evidence_type.as_deref(),
                    Some("managed_job_destroyed")
                );
            } else {
                assert!(matches!(&report[0], RecoveryOutcome::Unknown { .. }));
                assert_eq!(retained(&manager, &id).await.status, "unknown");
                assert_eq!(
                    manager
                        .store
                        .runtime("R1".into())
                        .await
                        .unwrap()
                        .unwrap()
                        .termination_evidence_state,
                    "unknown"
                );
            }
            assert_eq!(
                db.query_row("SELECT count(*) FROM runtime_instances", [], |r| r
                    .get::<_, i64>(0))
                    .unwrap(),
                1
            );
        }
    });
}

#[test]
fn claim_scan_handles_legacy_terminals_and_ignores_unclaimed_executions() {
    run(async {
        for complete in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let (manager, id, db) = fixture(temp.path(), "completed", "dispatched").await;
            if complete {
                db.execute("UPDATE executions SET release_evidence_state='complete',release_evidence_kind='runtime_terminated',release_evidence_json='{}'",[]).unwrap();
            }
            let report = manager.recover_startup().await.unwrap();
            if complete {
                assert!(matches!(&report[0], RecoveryOutcome::Released { .. }));
                assert!(manager.recover_startup().await.unwrap().is_empty());
            } else {
                assert!(matches!(
                    &report[0],
                    RecoveryOutcome::Inconsistent {
                        code: "WORKSPACE_CLAIM_INCONSISTENT",
                        ..
                    }
                ));
                assert_eq!(retained(&manager, &id).await.status, "completed");
            }
            assert_eq!(
                db.query_row("SELECT count(*) FROM runtime_instances", [], |r| r
                    .get::<_, i64>(0))
                    .unwrap(),
                0
            );
        }
    });
}

#[test]
fn dispatching_crash_becomes_uncertain_unknown_and_never_starts_r2() {
    run(async {
        for state in ["dispatch_pending", "running", "unknown"] {
            let temp = tempfile::tempdir().unwrap();
            let (manager, id, db) = fixture(temp.path(), state, "dispatching").await;
            original(&db, &id, false);
            db.execute(
                "UPDATE executions SET thread_id='THREAD',turn_id='TURN'",
                [],
            )
            .unwrap();
            assert!(matches!(
                &manager.recover_startup().await.unwrap()[0],
                RecoveryOutcome::Unknown { .. }
            ));
            let row = retained(&manager, &id).await;
            assert_eq!(row.status, "unknown");
            assert_eq!(row.dispatch_state, "uncertain");
            assert_eq!(row.runtime_instance_id.as_deref(), Some("R1"));
            assert!(matches!(
                &manager.recover_startup().await.unwrap()[0],
                RecoveryOutcome::Unknown { .. }
            ));
            assert_eq!(
                db.query_row("SELECT count(*) FROM runtime_instances", [], |r| r
                    .get::<_, i64>(0))
                    .unwrap(),
                1,
                "No R2 or R2 empty can authorize release"
            );
            let input: CreateExecutionInput=serde_json::from_value(json!({"agent_id":"a","request_key":"new","prompt":"p","execution_profile":{},"workspace_id":"w","canonical_workspace_root":temp.path().to_str().unwrap(),"mode":"read_only"})).unwrap();
            assert_eq!(manager.create(input).await.unwrap_err(), "AGENT_BUSY");
        }
    });
}

#[test]
fn historical_missing_runtime_is_unknown_not_the_pre_dispatch_semantic_gap() {
    run(async {
        let temp = tempfile::tempdir().unwrap();
        let (manager, id, db) = fixture(temp.path(), "running", "dispatched").await;
        assert!(matches!(
            &manager.recover_startup().await.unwrap()[0],
            RecoveryOutcome::Unknown { .. }
        ));
        let row = retained(&manager, &id).await;
        assert_eq!(row.status, "unknown");
        assert!(row.runtime_instance_id.is_none());
        assert_eq!(
            db.query_row("SELECT count(*) FROM runtime_instances", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    });
}

#[test]
fn new_job_evidence_resumes_unknown_but_never_maps_runtime_crash_to_cancelled() {
    run(async {
        let temp = tempfile::tempdir().unwrap();
        let (manager, id, db) = fixture(temp.path(), "unknown", "uncertain").await;
        original(&db, &id, false);
        db.execute("UPDATE executions SET interrupt_requested_at=2", [])
            .unwrap();
        assert!(matches!(
            &manager.recover_startup().await.unwrap()[0],
            RecoveryOutcome::Unknown { .. }
        ));
        // Explicit fixture of newly durable Job evidence, not elapsed time or R2 state.
        db.execute("UPDATE runtime_instances SET state='terminated',termination_evidence_state='complete',termination_evidence_type='job_active_processes_zero',termination_evidence_at=10",[]).unwrap();
        let report = manager.recover_startup().await.unwrap();
        let RecoveryOutcome::Interrupted { execution, .. } = &report[0] else {
            panic!("{report:?}")
        };
        assert_eq!(execution.status, "interrupted");
        assert_eq!(execution.result_completeness, "unknown");
        assert!(execution.final_result_json.is_none());
        assert_eq!(
            execution.release_evidence_kind.as_deref(),
            Some("runtime_terminated")
        );
        assert_eq!(execution.runtime_instance_id.as_deref(), Some("R1"));
        assert!(
            manager
                .store
                .workspace_claim(execution.canonical_workspace_root.clone())
                .await
                .unwrap()
                .is_none()
        );
    });
}

#[test]
fn runtime_finalization_rollback_preserves_every_result_release_field_and_claim() {
    run(async {
        for trigger in [
            "CREATE TRIGGER fault BEFORE UPDATE OF final_result_json ON executions BEGIN SELECT RAISE(ABORT,'before result'); END;",
            "CREATE TRIGGER fault BEFORE UPDATE OF status ON executions WHEN new.status='interrupted' BEGIN SELECT RAISE(ABORT,'before terminal'); END;",
            "CREATE TRIGGER fault BEFORE DELETE ON workspace_claims BEGIN SELECT RAISE(ABORT,'before delete'); END;",
            "CREATE TRIGGER fault AFTER DELETE ON workspace_claims BEGIN SELECT RAISE(ABORT,'after delete'); END;",
        ] {
            let temp = tempfile::tempdir().unwrap();
            let (manager, id, db) = fixture(temp.path(), "reconciling", "uncertain").await;
            original(&db, &id, true);
            db.execute_batch(trigger).unwrap();
            let before = retained(&manager, &id).await;
            assert!(manager.recover_startup().await.is_err());
            assert_eq!(retained(&manager, &id).await, before);
            let evidence: Option<i64> = db
                .query_row(
                    "SELECT runtime_termination_evidence_at FROM executions",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert!(evidence.is_none());
        }
    });
}

fn fixed_binary() -> PathBuf {
    PathBuf::from(
        r"C:\Users\lifei\AppData\Roaming\npm\node_modules\@openai\codex\node_modules\@openai\codex-win32-x64\vendor\x86_64-pc-windows-msvc\bin\codex.exe",
    )
}

#[test]
fn transaction_crash_child() {
    let Some(root) = std::env::var_os("TASK008_TX_ROOT") else {
        return;
    };
    run(async {
        let store = StateStore::open(PathBuf::from(root)).await.unwrap();
        let id = std::env::var("TASK008_TX_ID").unwrap();
        let row = store.execution(id.clone()).await.unwrap().unwrap();
        WorkspaceExecutionCoordinator { store }
            .finish_runtime_terminated(&id, row.revision, None)
            .await
            .unwrap();
        panic!("Crash point not reached");
    });
}

#[test]
fn process_crash_at_each_runtime_finalization_boundary_is_atomic() {
    use std::os::windows::process::CommandExt;
    for point in [
        "before_terminal",
        "after_terminal",
        "before_delete",
        "after_delete",
        "after_commit",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let mut id = String::new();
        run(async {
            let (_, created, db) = fixture(temp.path(), "reconciling", "uncertain").await;
            original(&db, &created, true);
            id = created;
        });
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "agent::task_manager::recovery::tests::transaction_crash_child",
                "--nocapture",
            ])
            .env("TASK008_TX_ROOT", temp.path())
            .env("TASK008_TX_ID", &id)
            .env("TASK002_CRASH_POINT", point)
            .creation_flags(0x08000000)
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(91),
            "{point}: {}",
            String::from_utf8_lossy(&output.stdout)
        );
        run(async {
            let store = StateStore::open(temp.path().into()).await.unwrap();
            let row = store.execution(id.clone()).await.unwrap().unwrap();
            let committed = point == "after_commit";
            assert_eq!(
                row.status,
                if committed {
                    "interrupted"
                } else {
                    "reconciling"
                }
            );
            assert_eq!(
                store
                    .workspace_claim(row.canonical_workspace_root)
                    .await
                    .unwrap()
                    .is_none(),
                committed
            );
            assert_eq!(
                row.release_evidence_state,
                if committed { "complete" } else { "incomplete" }
            );
            assert!(row.final_result_json.is_none());
            let db = Connection::open(temp.path().join("agent-state.db")).unwrap();
            let stamp: Option<i64> = db
                .query_row(
                    "SELECT runtime_termination_evidence_at FROM executions",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(stamp.is_some(), committed);
        });
    }
}

// Invoked only as an isolated child of the ignored system test below.
#[test]
fn crash_host() {
    let Some(root) = std::env::var_os("TASK008_CRASH_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    run(async {
        use crate::agent::{
            codex::protocol::{Notification, TurnStatus},
            execution::state::{DispatchState, Status},
        };
        let store = StateStore::open(root.join("store")).await.unwrap();
        let manager = AgentTaskManager::new(store.clone(), fixed_binary());
        let input:CreateExecutionInput=serde_json::from_value(json!({"agent_id":"crash-agent","request_key":"crash-key","prompt":"Reply exactly CRASH_RECOVERY_OK. Do not use tools or modify files.","execution_profile":{},"workspace_id":"isolated","canonical_workspace_root":root.join("workspace").to_str().unwrap(),"mode":"read_only"})).unwrap();
        let id = manager.create(input).await.unwrap().execution_id;
        let r1 = format!("crash-runtime-{id}");
        let managed = managed::connect(
            store.clone(),
            "crash-host".into(),
            r1.clone(),
            fixed_binary(),
            root.join("workspace"),
        )
        .await
        .unwrap();
        store
            .provider_event(
                id.clone(),
                Transition::Dispatch {
                    to: DispatchState::Dispatching,
                    runtime_id: Some(r1.clone()),
                },
                now(),
            )
            .await
            .unwrap();
        let thread = managed
            .client
            .thread_start(root.join("workspace").to_str().unwrap())
            .await
            .unwrap();
        let row = store.execution(id.clone()).await.unwrap().unwrap();
        store
            .bind_protocol_identity(
                id.clone(),
                row.revision,
                r1.clone(),
                thread.id.clone(),
                None,
                now(),
            )
            .await
            .unwrap();
        let (tx, rx) = tokio::sync::oneshot::channel();
        let (turn, flush) = tokio::join!(
            managed.client.turn_start_observed(
                &thread.id,
                &id,
                "Reply exactly CRASH_RECOVERY_OK. Do not use tools or modify files.",
                tx
            ),
            rx
        );
        flush.unwrap();
        store
            .provider_event(
                id.clone(),
                Transition::Dispatch {
                    to: DispatchState::Dispatched,
                    runtime_id: None,
                },
                now(),
            )
            .await
            .unwrap();
        let turn = turn.unwrap();
        let row = store.execution(id.clone()).await.unwrap().unwrap();
        store
            .bind_protocol_identity(
                id.clone(),
                row.revision,
                r1.clone(),
                thread.id.clone(),
                Some(turn.id.clone()),
                now(),
            )
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(120), async {
            loop {
                let event = managed.client.receive_event().await.unwrap();
                if let Notification::TurnCompleted {
                    thread_id,
                    turn: terminal,
                } = event.notification
                {
                    assert_eq!(thread_id, thread.id);
                    assert_eq!(terminal.id, turn.id);
                    assert_eq!(terminal.status, TurnStatus::Completed);
                    store
                        .provider_event(
                            id.clone(),
                            Transition::ProviderTerminal {
                                runtime_id: r1.clone(),
                                status: Status::Completed,
                            },
                            now(),
                        )
                        .await
                        .unwrap();
                    break;
                }
            }
        })
        .await
        .unwrap();
        // Crash between durable Provider terminal and result/finalization. No release occurs.
        let row = store.execution(id.clone()).await.unwrap().unwrap();
        assert_eq!(row.status, "finalizing");
        assert!(
            store
                .workspace_claim(row.canonical_workspace_root.clone())
                .await
                .unwrap()
                .is_some()
        );
        std::fs::write(
            root.join("host.ready"),
            serde_json::to_vec(&json!({"execution":id,"r1":r1,"thread":thread.id,"turn":turn.id}))
                .unwrap(),
        )
        .unwrap();
        std::future::pending::<()>().await;
        drop(managed);
    });
}

#[test]
#[ignore = "Isolated fixed codex-cli 0.153.4 Host crash and R1/R2 recovery; run alone"]
fn real_fixed_binary_crash_cross_runtime_recovery() {
    use std::{
        os::windows::process::CommandExt,
        process::{Command, Stdio},
    };
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    std::fs::create_dir(root.join("workspace")).unwrap();
    assert!(
        Command::new("git")
            .arg("init")
            .arg(root.join("workspace"))
            .output()
            .unwrap()
            .status
            .success()
    );
    let home = root.join("codex-home");
    std::fs::create_dir(&home).unwrap();
    std::fs::copy(
        PathBuf::from(std::env::var_os("USERPROFILE").unwrap()).join(".codex/auth.json"),
        home.join("auth.json"),
    )
    .unwrap();
    let evidence = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../docs/tasks/evidence/TASK-008/implementation-2026-09-09")
        .join(format!("real-crash-{}-{}", std::process::id(), now()));
    std::fs::create_dir_all(&evidence).unwrap();
    let mut host = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "agent::task_manager::recovery::tests::crash_host",
            "--nocapture",
        ])
        .env("TASK008_CRASH_ROOT", root)
        .env("CODEX_HOME", &home)
        .env("SERENA_CONTRACT_RAW_DIR", &evidence)
        .creation_flags(0x08000000)
        .stdout(Stdio::from(
            std::fs::File::create(evidence.join("host.log")).unwrap(),
        ))
        .stderr(Stdio::from(
            std::fs::File::create(evidence.join("host.stderr.log")).unwrap(),
        ))
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(150);
    while !root.join("host.ready").exists() {
        if let Some(exit) = host.try_wait().unwrap() {
            panic!("crash Host exited {exit}; evidence {}", evidence.display());
        }
        if std::time::Instant::now() >= deadline {
            host.kill().unwrap();
            host.wait().unwrap();
            panic!("Host readiness timed out");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let identity: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("host.ready")).unwrap()).unwrap();
    host.kill().unwrap();
    assert!(!host.wait().unwrap().success());
    struct Environment(Vec<(&'static str, Option<std::ffi::OsString>)>);
    impl Drop for Environment {
        fn drop(&mut self) {
            for (k, v) in &self.0 {
                unsafe {
                    match v {
                        Some(v) => std::env::set_var(k, v),
                        None => std::env::remove_var(k),
                    }
                }
            }
        }
    }
    let _environment = Environment(vec![
        ("CODEX_HOME", std::env::var_os("CODEX_HOME")),
        (
            "SERENA_CONTRACT_RAW_DIR",
            std::env::var_os("SERENA_CONTRACT_RAW_DIR"),
        ),
    ]);
    // Only this ignored test runs in this process; the child has already exited.
    unsafe {
        std::env::set_var("CODEX_HOME", &home);
        std::env::set_var("SERENA_CONTRACT_RAW_DIR", &evidence);
    }
    run(async {
        let store = StateStore::open(root.join("store")).await.unwrap();
        let id = identity["execution"].as_str().unwrap();
        let r1 = identity["r1"].as_str().unwrap();
        let before = store.execution(id.into()).await.unwrap().unwrap();
        let old = store.runtime(r1.into()).await.unwrap().unwrap();
        assert_eq!(old.termination_evidence_state, "unknown");
        assert!(
            store
                .workspace_claim(before.canonical_workspace_root.clone())
                .await
                .unwrap()
                .is_some()
        );
        std::fs::write(
            evidence.join("before.txt"),
            format!("{before:#?}\n{old:#?}"),
        )
        .unwrap();
        let report = AgentTaskManager::new(store.clone(), fixed_binary())
            .recover_startup()
            .await
            .unwrap();
        std::fs::write(evidence.join("outcome.txt"), format!("{report:#?}")).unwrap();
        let RecoveryOutcome::Interrupted {
            execution: row,
            result_diagnostic,
        } = &report[0]
        else {
            panic!("{report:?}")
        };
        assert!(result_diagnostic.is_none(), "{result_diagnostic:?}");
        assert_eq!(row.status, "interrupted");
        assert_eq!(row.runtime_instance_id.as_deref(), Some(r1));
        assert_eq!(row.provider_terminal_status.as_deref(), Some("completed"));
        assert_eq!(row.result_completeness, "complete");
        assert_eq!(row.release_evidence_state, "complete");
        let result: serde_json::Value =
            serde_json::from_str(row.final_result_json.as_ref().unwrap()).unwrap();
        assert_eq!(result["sourceRuntimeId"], r1);
        assert_eq!(result["threadId"], identity["thread"]);
        assert_eq!(result["turnId"], identity["turn"]);
        assert_eq!(result["historyMode"], "paginated");
        assert_eq!(result["terminalTurn"]["status"], "completed");
        let r2 = result["recoveredByRuntimeId"].as_str().unwrap();
        assert_ne!(r1, r2);
        assert!(
            store
                .workspace_claim(row.canonical_workspace_root.clone())
                .await
                .unwrap()
                .is_none()
        );
        for rid in [r1, r2] {
            let runtime = store.runtime(rid.into()).await.unwrap().unwrap();
            assert_eq!(runtime.termination_evidence_state, "complete");
            std::fs::write(
                evidence.join(format!("{rid}.job.txt")),
                format!("{runtime:#?}"),
            )
            .unwrap();
        }
        std::fs::write(
            evidence.join("result.json"),
            serde_json::to_vec_pretty(&result).unwrap(),
        )
        .unwrap();
        let db = Connection::open(root.join("store/agent-state.db")).unwrap();
        assert_eq!(
            db.query_row("SELECT count(*) FROM runtime_instances", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            2
        );
        let mut requests = Vec::new();
        for rid in [r1, r2] {
            let raw =
                std::fs::read_to_string(evidence.join(format!("{rid}.stdin.raw.jsonl"))).unwrap();
            for line in raw.lines() {
                let v: serde_json::Value = serde_json::from_str(line).unwrap();
                if rid == r2 {
                    assert!(!matches!(
                        v["method"].as_str(),
                        Some(
                            "thread/start"
                                | "turn/start"
                                | "thread/resume"
                                | "thread/backgroundTerminals/list"
                        )
                    ));
                }
                requests.push(v);
            }
        }
        assert_eq!(
            requests
                .iter()
                .filter(|v| v["method"] == "turn/start")
                .count(),
            1
        );
        std::fs::write(evidence.join("identity.json"),serde_json::to_vec_pretty(&json!({"execution":id,"sourceRuntimeId":r1,"recoveredByRuntimeId":r2,"thread":identity["thread"],"turn":identity["turn"],"business":"interrupted","provider":"completed","resultCompleteness":"complete","claim":"absent","turnStartCount":1,"runtimeCount":2,"jobs":"complete"})).unwrap()).unwrap();
    });
    let status = Command::new("git")
        .arg("-C")
        .arg(root.join("workspace"))
        .args(["status", "--porcelain"])
        .output()
        .unwrap();
    assert!(status.status.success());
    assert!(status.stdout.is_empty());
    std::fs::write(evidence.join("workspace-status.txt"), "clean").unwrap();
}
