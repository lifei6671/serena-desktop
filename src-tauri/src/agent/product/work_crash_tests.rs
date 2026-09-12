//! Work/recovery seams across real abrupt Host-process exits; fake Provider only.
use super::*;
use crate::agent::{
    store::transactions::product::WorkExecutionContext,
    task_manager::recovery::RecoveryOutcome,
    work::{FinishOutcome, HostAcceptance, UpdateAction, WorkProductService},
};

// Complete SQL values for cross-process equality checks, not recovery evidence.
fn work_crash_rows(root: &Path) -> Value {
    let mut db = rusqlite::Connection::open(root.join("agent-state.db")).unwrap();
    let tx = db.transaction().unwrap();
    let mut snapshot = serde_json::Map::new();
    for table in [
        "work_runs",
        "work_execution_links",
        "executions",
        "workspace_claims",
        "runtime_instances",
        "execution_runtime_attempts",
    ] {
        let mut query = tx
            .prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))
            .unwrap();
        let columns = query.column_count();
        let rows: Vec<Vec<String>> = query
            .query_map([], |row| {
                (0..columns)
                    .map(|column| row.get_ref(column).map(|value| format!("{value:?}")))
                    .collect()
            })
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        snapshot.insert(table.into(), json!(rows));
    }
    Value::Object(snapshot)
}

fn complete_work(work: &str, execution: &str) -> UpdateAction {
    UpdateAction::Finish {
        work_run_id: work.into(),
        outcome: FinishOutcome::Completed,
        acceptance: Some(HostAcceptance {
            summary: "Host reviewed the resumed result".into(),
            execution_ids: vec![execution.into()],
        }),
    }
}

#[test]
fn work_pending_host_crash_child() {
    let Some(root) = std::env::var_os("SERENA_WORK_PENDING_CRASH_ROOT") else {
        return;
    };
    run(async {
        let root = PathBuf::from(root).canonicalize().unwrap();
        let store = StateStore::open(root.clone()).await.unwrap();
        let (work, execution, key, prompt) = (
            "work-pending",
            "E-pending",
            "pending-key",
            "Run pending test task",
        );
        store
            .create_work_run(
                work.into(),
                "W".into(),
                root.to_string_lossy().into(),
                "Pending crash".into(),
                None,
                1,
            )
            .await
            .unwrap();
        store
            .product_create_fresh_with_work(
                execution.into(),
                work.into(),
                key.into(),
                prompt.into(),
                "W".into(),
                w(&root, "W"),
                Some(WorkExecutionContext {
                    work_run_id: work.into(),
                    parent_execution_id: None,
                    delegation_context_json: None,
                }),
                2,
            )
            .await
            .unwrap();
        let row = store.execution(execution.into()).await.unwrap().unwrap();
        assert_eq!(row.status, "dispatch_pending");
        assert_eq!(row.dispatch_state, "not_dispatched");
        assert!(row.runtime_instance_id.is_none());
        assert!(row.provider_terminal_status.is_none());
        let rows = work_crash_rows(&root);
        assert_eq!(rows["runtime_instances"], json!([]));
        assert_eq!(rows["execution_runtime_attempts"], json!([]));
        assert_eq!(
            store
                .workspace_claim(root.to_string_lossy().into())
                .await
                .unwrap()
                .unwrap()
                .execution_id,
            execution
        );
        let link = store
            .work_execution_link(execution.into())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(link.work_run_id, work);
        assert!(link.parent_execution_id.is_none());
        std::fs::write(root.join("pending-crash.json"), serde_json::to_vec(&json!({
            "workRunId":work,"executionId":execution,"requestKey":key,"prompt":prompt,"rows":rows,
        })).unwrap()).unwrap();
        // No graceful shutdown, Provider startup, Rust Drop, or async monitor drain.
        std::process::exit(0);
    });
}

#[test]
fn actual_work_pending_host_crash_preserves_identity_until_explicit_resume() {
    use std::os::windows::process::CommandExt;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "agent::product::tests::persistence_tests::work_crash_tests::work_pending_host_crash_child", "--nocapture"])
        .env("SERENA_WORK_PENDING_CRASH_ROOT", &root).creation_flags(0x08000000).output().unwrap();
    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    run(async {
        let metadata: Value =
            serde_json::from_slice(&std::fs::read(root.join("pending-crash.json")).unwrap())
                .unwrap();
        let work = metadata["workRunId"].as_str().unwrap();
        let id = metadata["executionId"].as_str().unwrap();
        let store = StateStore::open(root.clone()).await.unwrap();
        assert_eq!(work_crash_rows(&root), metadata["rows"]);
        let original = store.execution(id.into()).await.unwrap().unwrap();
        assert_eq!(original.agent_id, work);
        assert_eq!(original.request_key, metadata["requestKey"]);
        assert_eq!(original.prompt, metadata["prompt"]);
        assert_eq!(original.status, "dispatch_pending");
        assert_eq!(original.dispatch_state, "not_dispatched");
        assert!(original.runtime_instance_id.is_none());
        assert!(original.provider_terminal_status.is_none());
        let work_row = store.work_run(work.into()).await.unwrap().unwrap();
        assert_eq!(work_row.status, "active");
        let link = store.work_execution_link(id.into()).await.unwrap().unwrap();
        assert_eq!(link.work_run_id, work);
        assert!(link.parent_execution_id.is_none());
        assert_eq!(
            store
                .workspace_claim(root.to_string_lossy().into())
                .await
                .unwrap()
                .unwrap()
                .execution_id,
            id
        );
        let evidence = Arc::new(Evidence::default());
        let recovery = service(store.clone(), root.join("agent-state.db"), evidence.clone());
        for _ in 0..2 {
            let outcomes = recovery.manager.recover_startup().await.unwrap();
            assert!(
                matches!(outcomes.as_slice(), [RecoveryOutcome::PendingExplicitResume { execution_id }] if execution_id==id),
                "{outcomes:?}"
            );
            assert_eq!(work_crash_rows(&root), metadata["rows"]);
            assert!(evidence.launches.lock().unwrap().is_empty());
            assert!(evidence.methods.lock().unwrap().is_empty());
        }
        let work_service = WorkProductService::new(store.clone());
        assert_eq!(
            work_service
                .update(complete_work(work, id), None)
                .await
                .unwrap_err(),
            "WORK_HAS_ACTIVE_EXECUTIONS"
        );
        assert_eq!(work_crash_rows(&root), metadata["rows"]);
        drop(recovery);

        let (s, release, fake) = fake_service(
            store.clone(),
            root.join("agent-state.db"),
            "PENDING_RESUMED",
            "TURN",
            false,
            "paginated",
        )
        .await;
        let retry = s
            .agent_execute(
                AgentExecuteAction::Start {
                    work_run_id: work.into(),
                    request_key: metadata["requestKey"].as_str().unwrap().into(),
                    prompt: metadata["prompt"].as_str().unwrap().into(),
                    delegation_context_json: None,
                },
                w(&root, "W"),
            )
            .await
            .unwrap();
        assert_eq!(retry.execution_id, id);
        assert_eq!(work_crash_rows(&root), metadata["rows"]);
        let resumed = s
            .agent_execute(
                AgentExecuteAction::ResumePending {
                    work_run_id: work.into(),
                    execution_id: id.into(),
                },
                None,
            )
            .await
            .unwrap();
        assert_eq!(resumed.execution_id, id);
        release.send(()).unwrap();
        let terminal = final_row(&s, id).await;
        assert_eq!(terminal.status, "completed");
        assert!(
            store
                .workspace_claim(root.to_string_lossy().into())
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(
            store.work_execution_link(id.into()).await.unwrap(),
            Some(link)
        );
        let rows = work_crash_rows(&root);
        assert_eq!(rows["executions"].as_array().unwrap().len(), 1);
        assert_eq!(rows["work_execution_links"].as_array().unwrap().len(), 1);
        drop(s);
        let methods = fake.await.unwrap();
        for method in ["initialize", "thread/start", "turn/start"] {
            assert_eq!(
                methods.iter().filter(|m| *m == method).count(),
                1,
                "{methods:?}"
            );
        }
        assert!(!methods.iter().any(|m| m == "thread/resume"));
        let finished = work_service
            .update(complete_work(work, id), None)
            .await
            .unwrap();
        assert_eq!(finished.status, "completed");
        let acceptance: Value =
            serde_json::from_str(finished.acceptance_json.as_deref().unwrap()).unwrap();
        assert_eq!(acceptance["decision"], "accepted");
        assert_eq!(acceptance["executionIds"], json!([id]));
        let final_rows = work_crash_rows(&root);
        drop(work_service);
        drop(store);
        let reopened = StateStore::open(root.clone()).await.unwrap();
        assert_eq!(
            reopened.work_run(work.into()).await.unwrap(),
            Some(finished)
        );
        assert_eq!(
            reopened.execution(id.into()).await.unwrap().unwrap().status,
            "completed"
        );
        assert_eq!(
            reopened
                .work_execution_links(work.into())
                .await
                .unwrap()
                .len(),
            1
        );
        assert_eq!(work_crash_rows(&root), final_rows);
    });
}

#[test]
fn work_dispatched_host_crash_child() {
    let Some(root) = std::env::var_os("SERENA_WORK_DISPATCHED_CRASH_ROOT") else {
        return;
    };
    run(async {
        let root = PathBuf::from(root).canonicalize().unwrap();
        let store = StateStore::open(root.clone()).await.unwrap();
        let (work, key, prompt) = ("work-dispatched", "dispatched-key", "Run held test task");
        store
            .create_work_run(
                work.into(),
                "W".into(),
                root.to_string_lossy().into(),
                "Dispatched crash".into(),
                None,
                1,
            )
            .await
            .unwrap();
        let evidence = Arc::new(Evidence::default());
        evidence.hold_turn.store(true, Ordering::SeqCst);
        let s = service(store.clone(), root.join("agent-state.db"), evidence.clone());
        let receipt = s
            .agent_execute(
                AgentExecuteAction::Start {
                    work_run_id: work.into(),
                    request_key: key.into(),
                    prompt: prompt.into(),
                    delegation_context_json: None,
                },
                w(&root, "W"),
            )
            .await
            .unwrap();
        let id = receipt.execution_id;
        let row = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let row = store.execution(id.clone()).await.unwrap().unwrap();
                if row.runtime_instance_id.is_some()
                    && row.thread_id.is_some()
                    && row.turn_id.is_some()
                    && row.dispatch_state == "dispatched"
                    && row.tool_category.as_deref() == Some("test")
                {
                    break row;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("fixture did not cross the persisted Provider boundary");
        assert_eq!(row.status, "running");
        let runtime = row.runtime_instance_id.as_ref().unwrap();
        assert_eq!(counts(&evidence, runtime), [1, 1, 0, 1]);
        assert_eq!(
            store.runtime(runtime.clone()).await.unwrap().unwrap().state,
            "running"
        );
        assert_eq!(
            store
                .workspace_claim(root.to_string_lossy().into())
                .await
                .unwrap()
                .unwrap()
                .execution_id,
            id
        );
        let link = store
            .work_execution_link(id.clone())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(link.work_run_id, work);
        assert!(link.parent_execution_id.is_none());
        std::fs::write(root.join("dispatched-crash.json"), serde_json::to_vec(&json!({
            "workRunId":work,"executionId":id,"runtimeId":runtime,"requestKey":key,"prompt":prompt,
            "rows":work_crash_rows(&root),"providerCounts":counts(&evidence,runtime),
        })).unwrap()).unwrap();
        // Abrupt exit with a held Provider turn; no cancel/shutdown or synthetic termination evidence.
        std::process::exit(0);
    });
}

#[test]
fn actual_work_dispatched_host_crash_retains_unknown_and_blocks_finish() {
    use std::os::windows::process::CommandExt;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "agent::product::tests::persistence_tests::work_crash_tests::work_dispatched_host_crash_child", "--nocapture"])
        .env("SERENA_WORK_DISPATCHED_CRASH_ROOT", &root).creation_flags(0x08000000).output().unwrap();
    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    run(async {
        let metadata: Value =
            serde_json::from_slice(&std::fs::read(root.join("dispatched-crash.json")).unwrap())
                .unwrap();
        let work = metadata["workRunId"].as_str().unwrap();
        let id = metadata["executionId"].as_str().unwrap();
        let runtime = metadata["runtimeId"].as_str().unwrap();
        let store = StateStore::open(root.clone()).await.unwrap();
        assert_eq!(work_crash_rows(&root), metadata["rows"]);
        assert_eq!(metadata["providerCounts"], json!([1, 1, 0, 1]));
        let original = store.execution(id.into()).await.unwrap().unwrap();
        assert_eq!(original.status, "running");
        assert_eq!(original.runtime_instance_id.as_deref(), Some(runtime));
        assert_eq!(
            store.runtime(runtime.into()).await.unwrap().unwrap().state,
            "running"
        );
        let work_row = store.work_run(work.into()).await.unwrap().unwrap();
        assert_eq!(work_row.status, "active");
        let link = store.work_execution_link(id.into()).await.unwrap().unwrap();
        assert_eq!(link.work_run_id, work);
        assert!(link.parent_execution_id.is_none());
        let claim = store
            .workspace_claim(root.to_string_lossy().into())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(claim.execution_id, id);
        let evidence = Arc::new(Evidence::default());
        let s = service(store.clone(), root.join("agent-state.db"), evidence.clone());
        let outcomes = s.manager.recover_startup().await.unwrap();
        // A claimed Execution uses claimed recovery, not the idle OrphanRuntime outcome.
        assert!(
            outcomes
                .iter()
                .any(|o| matches!(o,RecoveryOutcome::Unknown{execution_id,..} if execution_id==id)),
            "{outcomes:?}"
        );
        let old_runtime = store.runtime(runtime.into()).await.unwrap().unwrap();
        assert_eq!(old_runtime.state, "unknown");
        assert_eq!(old_runtime.termination_evidence_state, "unknown");
        let unresolved = store.execution(id.into()).await.unwrap().unwrap();
        assert_eq!(unresolved.status, "unknown");
        assert_eq!(unresolved.dispatch_state, "dispatched");
        assert_eq!(unresolved.runtime_instance_id, original.runtime_instance_id);
        assert_eq!(unresolved.thread_id, original.thread_id);
        assert_eq!(unresolved.turn_id, original.turn_id);
        assert_eq!(unresolved.agent_id, original.agent_id);
        assert_eq!(unresolved.request_key, original.request_key);
        assert_eq!(unresolved.request_hash, original.request_hash);
        assert_eq!(unresolved.prompt, original.prompt);
        assert_eq!(store.work_run(work.into()).await.unwrap(), Some(work_row));
        assert_eq!(
            store.work_execution_link(id.into()).await.unwrap(),
            Some(link)
        );
        assert_eq!(
            store
                .workspace_claim(root.to_string_lossy().into())
                .await
                .unwrap(),
            Some(claim)
        );
        let recovered_rows = work_crash_rows(&root);
        for table in ["work_runs", "work_execution_links", "workspace_claims"] {
            assert_eq!(recovered_rows[table], metadata["rows"][table]);
        }
        assert_eq!(recovered_rows["executions"].as_array().unwrap().len(), 1);
        assert_eq!(
            recovered_rows["runtime_instances"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            recovered_rows["execution_runtime_attempts"],
            metadata["rows"]["execution_runtime_attempts"]
        );
        assert!(evidence.launches.lock().unwrap().is_empty());
        assert!(evidence.methods.lock().unwrap().is_empty());
        let work_service = WorkProductService::new(store.clone());
        for action in [
            complete_work(work, id),
            UpdateAction::Finish {
                work_run_id: work.into(),
                outcome: FinishOutcome::Failed,
                acceptance: None,
            },
        ] {
            assert_eq!(
                work_service.update(action, None).await.unwrap_err(),
                "WORK_HAS_ACTIVE_EXECUTIONS"
            );
            assert_eq!(work_crash_rows(&root), recovered_rows);
        }
        let retry = s
            .agent_execute(
                AgentExecuteAction::Start {
                    work_run_id: work.into(),
                    request_key: metadata["requestKey"].as_str().unwrap().into(),
                    prompt: metadata["prompt"].as_str().unwrap().into(),
                    delegation_context_json: None,
                },
                w(&root, "W"),
            )
            .await
            .unwrap();
        assert_eq!(retry.execution_id, id);
        assert_eq!(retry.status, "unknown");
        assert_eq!(work_crash_rows(&root), recovered_rows);
        assert!(evidence.launches.lock().unwrap().is_empty());
        assert!(evidence.methods.lock().unwrap().is_empty());
        // Stop at fail-closed: never fabricate Job evidence to release the Claim or finish Work.
    });
}
