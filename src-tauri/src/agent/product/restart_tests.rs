//! Restart means closing the first service/store and reopening the same SQLite file.
use super::*;
use crate::agent::{store::transactions::ClaimRecovery, task_manager::recovery::RecoveryOutcome};
use rusqlite::{Connection, params};

async fn pending(store: &StateStore, root: &std::path::Path, id: &str) {
    store
        .product_create_fresh(
            id.into(),
            id.into(),
            "K".into(),
            "original prompt".into(),
            "W".into(),
            w(root, "W"),
            1,
        )
        .await
        .unwrap();
}
async fn initialize(root: &std::path::Path) -> (AgentProductService, Vec<RecoveryOutcome>) {
    let store = StateStore::open(root.into()).await.unwrap();
    TEST_DISCOVERY
        .scope(
            Err("BACKEND_UNAVAILABLE: restart fixture".into()),
            AgentProductService::initialize(store),
        )
        .await
        .unwrap()
}
fn count(db: &Connection, table: &str) -> i64 {
    db.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

#[tokio::test]
async fn rt01_unbound_runtime_attempt_restart_is_unknown_without_replay() {
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::open(dir.path().into()).await.unwrap();
    pending(&store, dir.path(), "E1").await;
    let before = store.execution("E1".into()).await.unwrap().unwrap();
    let db = Connection::open(dir.path().join("agent-state.db")).unwrap();
    db.execute("INSERT INTO runtime_instances(id,owner_host_instance_id,state,created_at,updated_at) VALUES ('runtime-E1','old-host','running',1,1)", []).unwrap();
    drop(db);
    drop(store);

    let store = StateStore::open(dir.path().into()).await.unwrap();
    assert_eq!(
        store.recover_claims(now()).await.unwrap(),
        vec![ClaimRecovery::Pending {
            execution_id: "E1".into()
        }]
    );
    let pre_recovery = AgentProductService::new(store.clone());
    assert!(
        !pre_recovery
            .observe("E1".into(), false)
            .await
            .unwrap()
            .available_actions
            .can_resume_pending
    );
    assert!(
        matches!(pre_recovery.manager.resume_pending_execution("E1").await,
        Err(crate::agent::codex::provider::ExecutionFailure::State(ref e)) if e == "PENDING_RESUME_REJECTED")
    );
    drop(pre_recovery);
    drop(store);

    let (service, report) = initialize(dir.path()).await;
    assert!(
        matches!(&report[0], RecoveryOutcome::Unknown { execution_id, .. } if execution_id == "E1")
    );
    let view = service.observe("E1".into(), false).await.unwrap();
    assert_eq!(view.status, "unknown");
    assert_eq!(view.attention, "manual_resolution_required");
    assert!(!view.available_actions.can_resume_pending);
    assert!(!view.available_actions.can_cancel);
    let row = service.store.execution("E1".into()).await.unwrap().unwrap();
    assert_eq!(row.id, before.id);
    assert_eq!(row.prompt, before.prompt);
    assert_eq!(row.request_hash, before.request_hash);
    assert_eq!(row.dispatch_state, "not_dispatched");
    assert!(row.runtime_instance_id.is_none());
    assert!(row.thread_id.is_none() && row.turn_id.is_none());
    assert!(
        service
            .store
            .workspace_claim(row.canonical_workspace_root)
            .await
            .unwrap()
            .is_some()
    );
    let db = Connection::open(dir.path().join("agent-state.db")).unwrap();
    assert_eq!(count(&db, "runtime_instances"), 1);
    assert_eq!(count(&db, "executions"), 1);
    assert_eq!(count(&db, "workspace_claims"), 1);
    assert_eq!(
        service
            .store
            .runtime("runtime-E1".into())
            .await
            .unwrap()
            .unwrap()
            .state,
        "unknown" // Orphan recovery observes missing Job policy; no fabricated termination.
    );
}

#[tokio::test]
async fn rt02_clean_pending_restart_preserves_identity_and_explicit_actions() {
    let dir = tempfile::tempdir().unwrap();
    let (first, _) = initialize(dir.path()).await;
    pending(&first.store, dir.path(), "E1").await;
    let before = first.store.execution("E1".into()).await.unwrap().unwrap();
    let revision = first.observe("E1".into(), false).await.unwrap().revision;
    drop(first);
    let (service, report) = initialize(dir.path()).await;
    assert!(
        matches!(&report[0], RecoveryOutcome::PendingExplicitResume { execution_id } if execution_id == "E1")
    );
    assert_eq!(
        before,
        service.store.execution("E1".into()).await.unwrap().unwrap()
    );
    let view = service.observe("E1".into(), false).await.unwrap();
    assert_eq!(view.revision, revision);
    assert_eq!(view.prompt, "original prompt");
    assert_eq!(view.workspace_id, "W");
    assert_eq!(
        view.canonical_workspace_root,
        before.canonical_workspace_root
    );
    assert_eq!(view.attention, "pending_explicit_resume");
    assert!(view.available_actions.can_resume_pending && view.available_actions.can_cancel);
    assert!(matches!(view.next_action, Some(NextAction::ResumePending)));
    let permit = service
        .store
        .guard_pending_dispatch("E1".into())
        .await
        .unwrap();
    drop(permit);
    assert!(
        service
            .store
            .runtime("runtime-E1".into())
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn rt03_lost_start_receipt_exact_retry_after_restart_returns_original_execution() {
    let dir = tempfile::tempdir().unwrap();
    let (first, _) = initialize(dir.path()).await;
    let request = json!({"action":"start","agentId":"A","requestKey":"K","workspaceId":"W","prompt":"original prompt"});
    // The transport loses this receipt after the durable create; the caller only retains A/K/W/P.
    let lost = first.operation(request.clone(), w(dir.path(), "W")).await;
    assert_eq!(lost["error"]["code"], "BACKEND_UNAVAILABLE");
    let id = lost["error"]["executionId"].as_str().unwrap().to_owned();
    let before = first.store.execution(id.clone()).await.unwrap().unwrap();
    drop(first);
    let (service, _) = initialize(dir.path()).await;
    for _ in 0..2 {
        let retry = service.operation(request.clone(), w(dir.path(), "W")).await;
        assert_eq!(retry["ok"], true, "{retry}");
        assert_eq!(retry["data"]["executionId"], id);
        assert_eq!(retry["data"]["attention"], "pending_explicit_resume");
        assert_eq!(retry["control"]["providerInvoked"], false);
        assert_eq!(
            before,
            service.store.execution(id.clone()).await.unwrap().unwrap()
        );
    }
    let db = Connection::open(dir.path().join("agent-state.db")).unwrap();
    assert_eq!(count(&db, "executions"), 1);
    assert_eq!(count(&db, "workspace_claims"), 1);
    assert_eq!(count(&db, "runtime_instances"), 0);
}

#[tokio::test]
async fn rt04_dispatched_and_uncertain_restart_remain_visible_and_fail_closed() {
    for (status, dispatch) in [
        ("dispatch_pending", "dispatching"),
        ("running", "dispatched"),
        ("cancel_requested", "dispatched"),
        ("cancelling", "dispatched"),
        ("reconciling", "uncertain"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let store = StateStore::open(dir.path().into()).await.unwrap();
        pending(&store, dir.path(), "E1").await;
        let db = Connection::open(dir.path().join("agent-state.db")).unwrap();
        // A persisted original Runtime without sufficient Job evidence must remain fail-closed.
        db.execute("INSERT INTO runtime_instances(id,owner_host_instance_id,state,created_at,updated_at) VALUES ('runtime-E1','old-host','running',1,1)", []).unwrap();
        db.execute("UPDATE executions SET status=?1,dispatch_state=?2,runtime_instance_id='runtime-E1',thread_id='THREAD',turn_id='TURN' WHERE id='E1'", params![status,dispatch]).unwrap();
        let before = store.execution("E1".into()).await.unwrap().unwrap();
        drop(db);
        drop(store);
        let (service, report) = initialize(dir.path()).await;
        assert!(
            matches!(&report[0], RecoveryOutcome::Unknown { .. }),
            "{status}: {report:?}"
        );
        let response = service
            .operation(
                json!({"action":"observe","executionId":"E1","waitMs":0}),
                None,
            )
            .await;
        assert_eq!(response["ok"], true);
        assert_eq!(response["data"]["status"], "unknown");
        assert_eq!(response["data"]["threadId"], "THREAD");
        assert_eq!(response["data"]["turnId"], "TURN");
        assert_eq!(
            response["data"]["availableActions"]["canResumePending"],
            false
        );
        let listed = service.operation(json!({"action":"list"}), None).await;
        assert_eq!(listed["data"]["executions"][0]["executionId"], "E1");
        let row = service.store.execution("E1".into()).await.unwrap().unwrap();
        assert_eq!(row.runtime_instance_id, before.runtime_instance_id);
        assert_eq!(row.request_hash, before.request_hash);
        assert_eq!(
            row.dispatch_state,
            if dispatch == "dispatching" {
                "uncertain"
            } else {
                dispatch
            }
        );
        assert!(
            service
                .store
                .workspace_claim(row.canonical_workspace_root)
                .await
                .unwrap()
                .is_some()
        );
        let db = Connection::open(dir.path().join("agent-state.db")).unwrap();
        assert_eq!(count(&db, "runtime_instances"), 1);
        assert_eq!(count(&db, "executions"), 1);
    }
}

#[tokio::test]
async fn rt06_startup_list_history_and_observe_synchronize_durable_records() {
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::open(dir.path().into()).await.unwrap();
    for id in ["pending", "reconciling", "unknown", "completed"] {
        pending(&store, &dir.path().join(id), id).await;
    }
    let db = Connection::open(dir.path().join("agent-state.db")).unwrap();
    // Historical fixture, including a fully committed terminal record without a Claim.
    db.execute("UPDATE executions SET status=id,dispatch_state='uncertain' WHERE id IN ('reconciling','unknown')", []).unwrap();
    db.execute("UPDATE executions SET status='completed',dispatch_state='dispatched',result_completeness='complete',final_result_json='{}',release_evidence_state='complete',release_evidence_kind='same_runtime_cleanup',release_evidence_json='{}' WHERE id='completed'", []).unwrap();
    db.execute(
        "DELETE FROM workspace_claims WHERE execution_id='completed'",
        [],
    )
    .unwrap();
    drop(db);
    drop(store);
    let (service, _) = initialize(dir.path()).await;
    let listed = service.operation(json!({"action":"list"}), None).await;
    let rows = listed["data"]["executions"].as_array().unwrap();
    assert_eq!(rows.len(), 4);
    let history = service.history_page(None, None).await.unwrap(); // The agent_history Tauri source.
    assert_eq!(history.executions.len(), 4);
    for id in ["pending", "reconciling", "unknown", "completed"] {
        let observed = service
            .operation(
                json!({"action":"observe","executionId":id,"waitMs":0}),
                None,
            )
            .await;
        assert_eq!(observed["ok"], true);
        assert_eq!(observed["data"]["executionId"], id);
        assert_eq!(
            observed["data"]["status"],
            match id {
                "pending" => "dispatch_pending",
                "completed" => "completed",
                _ => "unknown",
            }
        );
        assert!(
            rows.iter()
                .any(|r| r["executionId"] == id && r["revision"] == observed["data"]["revision"])
        );
        assert!(
            history
                .executions
                .iter()
                .any(|r| r.execution_id == id && r.revision == observed["data"]["revision"])
        );
    }
}

#[tokio::test]
async fn rt05_finalizing_restart_retains_terminal_and_recovers_result_under_v04() {
    use crate::agent::{
        codex::app_server::recovery::RecoveryScope, coordinator::WorkspaceExecutionCoordinator,
    };
    for terminal in ["completed", "failed", "interrupted"] {
        let dir = tempfile::tempdir().unwrap();
        let store = StateStore::open(dir.path().into()).await.unwrap();
        pending(&store, dir.path(), "E1").await;
        let db = Connection::open(dir.path().join("agent-state.db")).unwrap();
        // Snapshot at Host exit: original Job termination proof and terminal are durable;
        // final result remains in exact Provider history until atomic finalization.
        db.execute("INSERT INTO runtime_instances(id,owner_host_instance_id,state,created_at,updated_at,termination_evidence_state,termination_evidence_type,termination_evidence_at) VALUES ('R1','old-host','terminated',1,1,'complete','job_active_processes_zero',10)", []).unwrap();
        db.execute("UPDATE executions SET status='finalizing',dispatch_state='dispatched',runtime_instance_id='R1',thread_id='THREAD',turn_id='TURN',provider_terminal_status=?1,provider_terminal_evidence_runtime_instance_id='R1',provider_terminal_evidence_at=9 WHERE id='E1'", [terminal]).unwrap();
        let before = store.execution("E1".into()).await.unwrap().unwrap();
        drop(db);
        drop(store);
        let (service, report) = initialize(dir.path()).await;
        assert!(matches!(&report[0], RecoveryOutcome::RuntimeFailure { .. }));
        let row = service.store.execution("E1".into()).await.unwrap().unwrap();
        assert_eq!(row.status, "unknown");
        assert_eq!(
            row.provider_terminal_status,
            before.provider_terminal_status
        );
        assert_eq!(
            row.provider_terminal_evidence_at,
            before.provider_terminal_evidence_at
        );
        assert_eq!(row.thread_id, before.thread_id);
        assert_eq!(row.turn_id, before.turn_id);
        assert_eq!(row.runtime_instance_id, before.runtime_instance_id);
        assert!(
            service
                .store
                .workspace_claim(row.canonical_workspace_root.clone())
                .await
                .unwrap()
                .is_some()
        );
        assert!(row.final_result_json.is_none());

        // Explicit local recovery after bounded unavailable result recovery.
        service.store.provider_event("E1".into(), crate::agent::execution::state::Transition::ResumeRecovery(
            crate::agent::execution::state::RecoveryBasis::LocalResolve { diagnostic: "fixture result recovery".into() }),
            crate::agent::coordinator::now()).await.unwrap();
        let row = service.store.execution("E1".into()).await.unwrap().unwrap();

        // Drive the existing sealed Cross-Runtime recovery + coordinator boundary
        // with fake history. No start/continue/cleanup RPC is permitted on R2.
        let db = Connection::open(dir.path().join("agent-state.db")).unwrap();
        db.execute("INSERT INTO runtime_instances(id,owner_host_instance_id,state,created_at,updated_at) VALUES ('R2','new-host','running',11,11)", []).unwrap();
        let (wire, server) = tokio::io::duplex(32 * 1024);
        let (read, write) = tokio::io::split(wire);
        let client = Client::product_test_transport("R2".into(), read, write, tokio::io::empty());
        let fake = tokio::spawn(async move {
            let mut server = BufReader::new(server);
            for (method, response) in [
                (
                    "initialize",
                    json!({"userAgent":"fake","codexHome":"isolated","platformFamily":"windows","platformOs":"windows"}),
                ),
                ("initialized", Value::Null),
                (
                    "thread/read",
                    json!({"thread":{"id":"THREAD","turns":[],"historyMode":"paginated"}}),
                ),
                (
                    "thread/turns/list",
                    json!({"data":[{"id":"TURN","status":terminal,"items":[],"itemsView":"summary"}],"nextCursor":null}),
                ),
                (
                    "thread/items/list",
                    json!({"data":[{"turnId":"TURN","item":{"id":"result","type":"agentMessage","phase":"final_answer","text":"DURABLE_PROVIDER_RESULT"}}],"nextCursor":null}),
                ),
            ] {
                let mut line = String::new();
                server.read_line(&mut line).await.unwrap();
                let request: Value = serde_json::from_str(&line).unwrap();
                assert_eq!(request["method"], method, "no Provider replay");
                if method == "initialized" {
                    continue;
                }
                if method == "thread/read" {
                    assert_eq!(request["params"]["includeTurns"], false);
                }
                if method == "thread/items/list" {
                    assert_eq!(request["params"]["turnId"], "TURN");
                }
                server
                    .write_all(
                        &crate::agent::codex::protocol::encode(
                            &json!({"id":request["id"],"result":response}),
                        )
                        .unwrap(),
                    )
                    .await
                    .unwrap();
            }
            let mut extra = String::new();
            assert_eq!(
                server.read_line(&mut extra).await.unwrap(),
                0,
                "unexpected Provider replay: {extra}"
            );
        });
        client.initialize().await.unwrap();
        let scope = RecoveryScope::after_termination_for_execution(&service.store, "E1", "R2")
            .await
            .unwrap();
        let result = client.recover_result(scope).await.unwrap();
        let finished = WorkspaceExecutionCoordinator {
            store: service.store.clone(),
        }
        .finish_runtime_terminated("E1", row.revision, Some(result))
        .await
        .unwrap();
        // V0.4 Runtime termination recovery is interrupted, independently of Provider terminal.
        // Restoring completed/failed here would be a Material Contract Difference.
        assert_eq!(finished.status, "interrupted");
        assert_eq!(finished.provider_terminal_status.as_deref(), Some(terminal));
        assert_eq!(finished.runtime_instance_id.as_deref(), Some("R1"));
        assert_eq!(finished.result_completeness, "complete");
        assert_eq!(finished.release_evidence_state, "complete");
        assert!(
            finished
                .final_result_json
                .as_deref()
                .unwrap()
                .contains("DURABLE_PROVIDER_RESULT")
        );
        assert!(
            service
                .store
                .workspace_claim(row.canonical_workspace_root)
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(count(&db, "executions"), 1);
        assert_eq!(count(&db, "runtime_instances"), 2);
        drop(client);
        fake.await.unwrap();
        drop(db);
        drop(service);
        let (service, report) = initialize(dir.path()).await;
        assert!(report.iter().all(|outcome| matches!(outcome, RecoveryOutcome::OrphanRuntime { .. })));
        assert_ne!(service.store.runtime("R2".into()).await.unwrap().unwrap().state, "running");
        assert_eq!(
            service.store.execution("E1".into()).await.unwrap().unwrap(),
            finished
        );
        let view = service.observe("E1".into(), true).await.unwrap();
        assert!(view.result_available && view.final_result.is_some());
    }
}

#[tokio::test]
async fn explicit_random_runtime_attempt_prebind_crash_is_never_resumable() {
    for persisted_runtime in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let store = StateStore::open(dir.path().into()).await.unwrap();
        pending(&store, dir.path(), "E1").await;
        store.reserve_runtime_attempt("E1".into(), "R123-independent".into(), 2).await.unwrap();
        if persisted_runtime {
            Connection::open(dir.path().join("agent-state.db")).unwrap().execute(
                "INSERT INTO runtime_instances(id,owner_host_instance_id,state,created_at,updated_at) VALUES ('R123-independent','old-host','starting',2,2)", []).unwrap();
        }
        assert!(store.execution("E1".into()).await.unwrap().unwrap().runtime_instance_id.is_none());
        assert!(store.has_runtime_attempt("E1".into()).await.unwrap());
        assert_eq!(store.guard_pending_dispatch("E1".into()).await.err().as_deref(), Some("PENDING_RESUME_REJECTED"));
        let service = AgentProductService::new(store.clone());
        assert!(!service.observe("E1".into(),false).await.unwrap().available_actions.can_resume_pending);
        assert!(!service.checked_operation(json!({"action":"resume_pending","executionId":"E1"}),None).await["ok"].as_bool().unwrap());
        drop(service); drop(store);
        let (service, report) = initialize(dir.path()).await;
        assert!(!report.iter().any(|r| matches!(r, RecoveryOutcome::PendingExplicitResume{..})));
        let view = service.observe("E1".into(),false).await.unwrap();
        assert_eq!(view.status,"unknown");
        assert!(!view.available_actions.can_resume_pending);
        assert!(service.store.guard_pending_dispatch("E1".into()).await.is_err());
        let db=Connection::open(dir.path().join("agent-state.db")).unwrap();
        assert_eq!(count(&db,"runtime_instances"),i64::from(persisted_runtime));
        assert_eq!(count(&db,"execution_runtime_attempts"),1);
        assert_eq!(count(&db,"executions"),1);
        assert!(db.execute("UPDATE execution_runtime_attempts SET runtime_instance_id='R2'",[]).is_err());
        assert!(db.execute("DELETE FROM execution_runtime_attempts",[]).is_err());
    }
}

#[tokio::test]
async fn old_unresolved_recovery_attempt_quarantines_before_any_new_recovery_connect() {
    let dir=tempfile::tempdir().unwrap(); let store=StateStore::open(dir.path().into()).await.unwrap();
    pending(&store,dir.path(),"E1").await;
    let db=Connection::open(dir.path().join("agent-state.db")).unwrap();
    db.execute("INSERT INTO runtime_instances(id,owner_host_instance_id,state,termination_evidence_state,termination_evidence_type,termination_evidence_at,created_at,updated_at) VALUES ('R1','old','terminated','complete','managed_job_destroyed',5,1,5)",[]).unwrap();
    db.execute("UPDATE executions SET runtime_instance_id='R1',thread_id='T1',turn_id='U1',status='reconciling',dispatch_state='uncertain' WHERE id='E1'",[]).unwrap();
    store.reserve_recovery_attempt("E1".into(),"recovery-R2".into(),6).await.unwrap();
    db.execute("INSERT INTO runtime_instances(id,owner_host_instance_id,state,created_at,updated_at) VALUES ('recovery-R2','old','running',6,6)",[]).unwrap();
    drop(db); drop(store);
    let store=StateStore::open(dir.path().into()).await.unwrap();
    let manager=crate::agent::task_manager::AgentTaskManager::new(store.clone(),dir.path().join("must-not-launch.exe"));
    let report=manager.recover_startup().await.unwrap();
    let service=AgentProductService{store,manager};
    assert_eq!(count(&Connection::open(dir.path().join("agent-state.db")).unwrap(),"execution_runtime_attempts"),1);
    assert!(report.iter().any(|r|matches!(r,RecoveryOutcome::Unknown{execution_id,..} if execution_id=="E1")));
    assert_eq!(service.manager.runtime_pool.check_workspace(dir.path().to_str().unwrap()).unwrap_err(),"AGENT_RUNTIME_QUARANTINED");
    assert_eq!(count(&Connection::open(dir.path().join("agent-state.db")).unwrap(),"runtime_instances"),2);
}
