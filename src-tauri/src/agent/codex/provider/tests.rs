use super::*;
use crate::agent::{execution::CreateExecutionInput, task_manager::AgentTaskManager};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream};

fn run(future: impl std::future::Future<Output = ()>) {
    tokio::runtime::Runtime::new().unwrap().block_on(future)
}
fn input(root: &std::path::Path) -> CreateExecutionInput {
    serde_json::from_value(json!({"agent_id":"slice-agent","request_key":"slice-key","prompt":"Reply exactly VERTICAL_SLICE_OK. Do not use tools or modify files.","execution_profile":{},"workspace_id":"isolated","canonical_workspace_root":root.to_str().unwrap(),"mode":"read_only"})).unwrap()
}
async fn recv(s: &mut BufReader<DuplexStream>) -> Value {
    let mut line = String::new();
    s.read_line(&mut line).await.unwrap();
    serde_json::from_str(&line).unwrap()
}
async fn send(s: &mut BufReader<DuplexStream>, value: Value) {
    s.write_all(&super::super::protocol::encode(&value).unwrap())
        .await
        .unwrap();
}
async fn reply(s: &mut BufReader<DuplexStream>, req: &Value, value: Value) {
    send(s, json!({"id":req["id"],"result":value})).await;
}
fn turn(status: &str) -> Value {
    json!({"id":"TURN","status":status,"items":[],"itemsView":"summary"})
}

// Hold only turn/start flush until the fake server has persisted terminal evidence.
struct DelayedTurnFlush {
    inner: tokio::io::WriteHalf<DuplexStream>,
    release: Option<tokio::sync::oneshot::Receiver<()>>,
    turn_written: bool,
}
impl tokio::io::AsyncWrite for DelayedTurnFlush {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        if buf.windows(b"turn/start".len()).any(|v| v == b"turn/start") {
            self.turn_written = true;
        }
        std::pin::Pin::new(&mut self.inner).poll_write(cx, buf)
    }
    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        if self.turn_written && let Some(release) = &mut self.release {
            match std::future::Future::poll(std::pin::Pin::new(release), cx) {
                std::task::Poll::Pending => return std::task::Poll::Pending,
                std::task::Poll::Ready(result) => result.expect("fake flush release"),
            }
            self.release = None;
        }
        std::pin::Pin::new(&mut self.inner).poll_flush(cx)
    }
    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

async fn slice_case(case: &'static str) {
    let failed = case.starts_with("failed");
    let late_deadline = case.ends_with("late-deadline");
    let invalid_ack = matches!(case, "wrong-ack-late-deadline" | "error-ack-late-deadline" | "invalid-ack-late-deadline");
    let terminal_status = if failed { "failed" } else { "completed" };
    let temp = tempfile::tempdir().unwrap();
    let store = StateStore::open(temp.path().into()).await.unwrap();
    let manager = AgentTaskManager::new(store.clone(), "does-not-exist.exe".into());
    let request = input(temp.path());
    let created = manager.create(request.clone()).await.unwrap();
    assert!(created.created);
    assert_eq!(created.execution.status, "dispatch_pending");
    assert_eq!(created.execution.dispatch_state, "not_dispatched");
    assert!(created.execution.runtime_instance_id.is_none());
    assert_eq!(
        store
            .workspace_claim(request.canonical_workspace_root.clone())
            .await
            .unwrap()
            .unwrap()
            .execution_id,
        created.execution_id
    );
    let connection = rusqlite::Connection::open(temp.path().join("agent-state.db")).unwrap();
    if case != "resume" {
        connection.execute("INSERT INTO runtime_instances(id,owner_host_instance_id,state,created_at,updated_at) VALUES ('R1','fixture','running',1,1)",[]).unwrap();
    }
    if matches!(case, "rollback" | "failed-rollback") {
        connection.execute_batch("CREATE TRIGGER test_rollback BEFORE DELETE ON workspace_claims BEGIN SELECT RAISE(ABORT,'injected final rollback'); END;").unwrap();
    }
    let (wire, server) = tokio::io::duplex(128 * 1024);
    let (read, write) = tokio::io::split(wire);
    let (flush_release, flush_wait) = tokio::sync::oneshot::channel();
    let client = std::sync::Arc::new(Client::transport(
        "R1".into(),
        read,
        DelayedTurnFlush {
            inner: write,
            release: (case == "failed-delayed-flush").then_some(flush_wait),
            turn_written: false,
        },
        tokio::io::empty(),
    ));
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let (continue_tx, continue_rx) = tokio::sync::oneshot::channel();
    let fake_store = store.clone();
    let id = created.execution_id.clone();
    let request2 = request.clone();
    let fake = tokio::spawn(async move {
        let mut s = BufReader::new(server);
        let req = recv(&mut s).await;
        assert_eq!(req["method"], "initialize");
        if case == "resume" {
            entered_tx.send(()).unwrap();
            continue_rx.await.unwrap();
        }
        reply(&mut s,&req,json!({"userAgent":"fake","codexHome":"isolated","platformFamily":"windows","platformOs":"windows"})).await;
        assert_eq!(recv(&mut s).await["method"], "initialized");
        let req = recv(&mut s).await;
        assert_eq!(req["method"], "thread/start");
        assert_eq!(req["params"]["historyMode"], "paginated");
        assert_eq!(req["params"]["ephemeral"], false);
        assert_eq!(req["params"]["sandbox"], "read-only");
        assert_eq!(req["params"]["cwd"], request2.canonical_workspace_root);
        assert_eq!(req["params"]["approvalPolicy"], "never");
        let row = fake_store.execution(id.clone()).await.unwrap().unwrap();
        assert_eq!(row.runtime_instance_id.as_deref(), Some("R1"));
        assert_eq!(row.dispatch_state, "dispatching");
        assert_eq!(row.status, "dispatch_pending");
        if case == "resume" {
            assert!(
                AgentTaskManager::new(fake_store.clone(), "unused".into())
                    .resume_pending_execution(&id)
                    .await
                    .is_err()
            );
        }
        reply(
            &mut s,
            &req,
            json!({"thread":{"id":"THREAD","turns":[],"historyMode":"paginated"}}),
        )
        .await;
        let req = recv(&mut s).await;
        assert_eq!(req["method"], "turn/start");
        assert_eq!(req["params"]["threadId"], "THREAD");
        if matches!(case, "long" | "failed") {
            reply(&mut s, &req, json!({"turn":turn("inProgress")})).await;
            loop {
                let row = fake_store.execution(id.clone()).await.unwrap().unwrap();
                if row.turn_id.is_some() && row.dispatch_state == "dispatched" {
                    break;
                }
                tokio::task::yield_now().await;
            }
            if case == "long" {
                tokio::time::pause();
                tokio::time::advance(Duration::from_secs(121)).await;
                tokio::time::resume();
                tokio::time::sleep(Duration::from_millis(10)).await;
                let row = fake_store.execution(id.clone()).await.unwrap().unwrap();
                assert_ne!(row.status, "reconciling", "old turn timeout fired");
                assert!(row.provider_terminal_status.is_none());
            }
        }
        if late_deadline {
            while fake_store.execution(id.clone()).await.unwrap().unwrap().dispatch_state != "dispatched" {
                tokio::task::yield_now().await;
            }
        }
        let turn_request = req.clone();
        // Except the ACK-first cases, persist terminal before sending a late ACK.
        send(&mut s,json!({"method":"turn/completed","params":{"threadId":"THREAD","turn":turn(terminal_status)}})).await;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            let row = fake_store.execution(id.clone()).await.unwrap().unwrap();
            if row.status == "finalizing" {
                assert_eq!(row.turn_id.as_deref(), Some("TURN"));
                if case == "failed-delayed-flush" {
                    assert_eq!(row.dispatch_state, "dispatching");
                    assert!(row.final_result_json.is_none());
                    assert!(fake_store.workspace_claim(request2.canonical_workspace_root.clone()).await.unwrap().is_some());
                }
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "terminal was not persisted before ACK"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        if case == "failed-delayed-flush" {
            flush_release.send(()).unwrap();
        }
        {
            if !late_deadline && !matches!(case, "long" | "failed" | "failed-no-ack" | "failed-delayed-flush") {
                reply(&mut s, &req, json!({"turn":turn("inProgress")})).await;
            }
            let req = recv(&mut s).await;
            assert_eq!(req["method"], "thread/read");
            assert_eq!(req["params"]["includeTurns"], false);
            assert_eq!(
                fake_store
                    .execution(id.clone())
                    .await
                    .unwrap()
                    .unwrap()
                    .status,
                "finalizing"
            );
            if late_deadline {
                tokio::time::pause();
                tokio::time::advance(Duration::from_secs(8)).await;
                tokio::time::resume();
            }
            reply(&mut s,&req,json!({"thread":{"id":if case=="wrong-result" {"OTHER"}else{"THREAD"},"turns":[],"historyMode":"paginated"}})).await;
            if case != "wrong-result" {
                let req = recv(&mut s).await;
                assert_eq!(req["method"], "thread/turns/list");
                if late_deadline {
                    tokio::time::pause();
                    tokio::time::advance(Duration::from_secs(8)).await;
                    tokio::time::resume();
                    // Each recovery RPC is within its deadline; only the obsolete
                    // turn/start ACK is late, after exact terminal was persisted.
                    if invalid_ack {
                        let response = if case.starts_with("wrong-ack") {
                            json!({"id":turn_request["id"],"result":{"turn":{"id":"OTHER","status":"inProgress","items":[],"itemsView":"summary"}}})
                        } else if case.starts_with("invalid-ack") {
                            json!({"id":turn_request["id"],"result":{"turn":{"id":"TURN","status":"unknown"}}})
                        } else {
                            json!({"id":turn_request["id"],"error":{"code":-1,"message":"conflict"}})
                        };
                        send(&mut s, response).await;
                        let mut extra = String::new();
                        assert_eq!(s.read_line(&mut extra).await.unwrap(), 0, "unexpected replay: {extra}");
                        return;
                    }
                    reply(&mut s, &turn_request, json!({"turn":turn("inProgress")})).await;
                }
                reply(
                    &mut s,
                    &req,
                    json!({"data":[turn(terminal_status)],"nextCursor":null}),
                )
                .await;
                let req = recv(&mut s).await;
                assert_eq!(req["method"], "thread/items/list");
                assert_eq!(req["params"]["turnId"], "TURN");
                reply(&mut s,&req,json!({"data":[{"turnId":"TURN","item":{"type":"agentMessage","id":"persisted","phase":"final_answer","text":"VERTICAL_SLICE_OK"}}],"nextCursor":null})).await;
                let req = recv(&mut s).await;
                assert_eq!(req["method"], "thread/backgroundTerminals/clean");
                reply(&mut s, &req, json!({})).await;
                for round in 0..2 {
                    let req = recv(&mut s).await;
                    assert_eq!(req["method"], "thread/backgroundTerminals/list");
                    assert_eq!(req["params"]["threadId"], "THREAD");
                    let row = fake_store.execution(id.clone()).await.unwrap().unwrap();
                    assert_eq!(row.status, "finalizing");
                    assert_ne!(row.release_evidence_state, "complete");
                    assert!(
                        fake_store
                            .workspace_claim(request2.canonical_workspace_root.clone())
                            .await
                            .unwrap()
                            .is_some()
                    );
                    let duplicate =
                        AgentTaskManager::new(fake_store.clone(), "not-a-binary.exe".into())
                            .execute(request2.clone())
                            .await
                            .unwrap();
                    assert!(!duplicate.created);
                    assert_eq!(duplicate.execution_id, id);
                    reply(&mut s,&req,json!({"data":if round==0 {json!([{"id":"still-active"}])}else{json!([])},"nextCursor":null})).await;
                }
            }
        }
        let mut extra = String::new();
        assert_eq!(
            s.read_line(&mut extra).await.unwrap(),
            0,
            "extra dispatch/fallback: {extra}"
        );
    });
    let provider = CodexProvider {
        store: store.clone(),
        executable: "unused".into(),
        owner: "fixture".into(),
    };
    let outcome = if case == "resume" {
        for _ in 0..3 {
            assert!(matches!(
                &manager.recover_startup().await.unwrap()[0],
                crate::agent::task_manager::recovery::RecoveryOutcome::PendingExplicitResume { .. }
            ));
        }
        let duplicate = manager.execute(request.clone()).await.unwrap();
        assert!(!duplicate.created);
        assert_eq!(duplicate.execution, created.execution);
        assert_eq!(
            connection
                .query_row("SELECT count(*) FROM runtime_instances", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
        let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(3));
        let mut handles = Vec::new();
        for connection_store in [
            store.clone(),
            StateStore::open(temp.path().into()).await.unwrap(),
        ] {
            let mut resume_manager = AgentTaskManager::new(connection_store, "unused".into());
            resume_manager.test_client = Some((client.clone(), temp.path().join("agent-state.db")));
            let barrier = barrier.clone();
            let id = created.execution_id.clone();
            handles.push(tokio::spawn(async move {
                barrier.wait().await;
                resume_manager.resume_pending_execution(&id).await
            }));
        }
        let mut one = handles.remove(0);
        let mut two = handles.remove(0);
        barrier.wait().await;
        tokio::time::timeout(Duration::from_secs(5), entered_rx)
            .await
            .unwrap()
            .unwrap();
        // Winner is held before initialize reply, still not_dispatched/runtime NULL.
        // The other call must reject NOW, not only after the winner has dispatched.
        let (rejected, first) = tokio::time::timeout(Duration::from_secs(5), async {
            tokio::select! {r=&mut one=>(r.unwrap(),true),r=&mut two=>(r.unwrap(),false)}
        })
        .await
        .unwrap();
        assert!(
            matches!(rejected,Err(ExecutionFailure::State(ref e)) if e=="PENDING_RESUME_REJECTED")
        );
        assert_eq!(
            store
                .execution(created.execution_id.clone())
                .await
                .unwrap()
                .unwrap(),
            created.execution
        );
        assert_eq!(
            connection
                .query_row("SELECT count(*) FROM runtime_instances", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        continue_tx.send(()).unwrap();
        let completed = if first {
            two.await.unwrap()
        } else {
            one.await.unwrap()
        };
        completed.map_err(|e| format!("{e:?}"))
    } else {
        client.initialize().await.unwrap();
        tokio::time::timeout(
            Duration::from_secs(if case == "long" { 150 } else if late_deadline { 60 } else { 15 }),
            provider.run_client(&created.execution_id, &client),
        )
        .await
        .unwrap()
    };
    let row = store
        .execution(created.execution_id.clone())
        .await
        .unwrap()
        .unwrap();
    if (late_deadline && !invalid_ack) || matches!(case, "success" | "resume" | "long" | "failed" | "failed-late-ack" | "failed-no-ack" | "failed-delayed-flush") {
        if failed {
            assert_eq!(outcome.as_ref().unwrap_err(), "PROVIDER_TERMINAL_failed");
        } else {
            assert!(outcome.is_ok(), "{outcome:?}");
        }
        assert_eq!(row.status, terminal_status);
        assert_eq!(row.result_completeness, "complete");
        assert_eq!(row.release_evidence_state, "complete");
        assert!(
            store
                .workspace_claim(request.canonical_workspace_root.clone())
                .await
                .unwrap()
                .is_none()
        );
    } else {
        assert!(outcome.is_err());
        assert_eq!(row.status, "finalizing");
        assert!(row.final_result_json.is_none());
        assert_ne!(row.release_evidence_state, "complete");
        assert!(
            store
                .workspace_claim(request.canonical_workspace_root.clone())
                .await
                .unwrap()
                .is_some()
        );
    }
    if matches!(case, "rollback" | "failed-rollback") {
        assert!(outcome.unwrap_err().contains("injected final rollback"));
    }
    assert_eq!(row.provider_terminal_status.as_deref(), Some(terminal_status));
    assert_eq!(row.thread_id.as_deref(), Some("THREAD"));
    assert_eq!(row.turn_id.as_deref(), Some("TURN"));
    assert_eq!(row.runtime_instance_id.as_deref(), Some("R1"));
    let duplicate = manager.execute(request).await.unwrap();
    assert!(!duplicate.created);
    assert_eq!(duplicate.execution_id, created.execution_id);
    if case == "resume" {
        assert!(
            manager
                .resume_pending_execution(&created.execution_id)
                .await
                .is_err()
        );
        assert_eq!(
            connection
                .query_row("SELECT count(*) FROM runtime_instances", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }
    drop(client);
    fake.await.unwrap();
}

#[test]
fn terminal_before_ack_and_idempotent_success() {
    run(slice_case("success"));
}
#[test]
fn explicit_resume_concurrent_callers_share_one_first_dispatch_and_complete() {
    run(slice_case("resume"));
}
#[test]
fn final_transaction_rollback_retains_claim_and_result_unknown() {
    run(slice_case("rollback"));
}
#[test]
fn result_identity_mismatch_cannot_complete() {
    run(slice_case("wrong-result"));
}
#[test]
fn provider_failure_finalizes_before_returning_error() {
    run(slice_case("failed"));
}

#[test]
fn failed_terminal_before_late_ack_finalizes() {
    run(slice_case("failed-late-ack"));
}
#[test]
fn failed_terminal_without_ack_finalizes() {
    run(slice_case("failed-no-ack"));
}
#[test]
fn failed_terminal_before_flush_without_ack_finalizes() {
    run(slice_case("failed-delayed-flush"));
}
#[test]
fn failed_final_transaction_rollback_retains_claim_and_result() {
    run(slice_case("failed-rollback"));
}

#[test]
#[ignore = "Explicit isolated codex-cli 0.153.4 vertical slice; run alone"]
fn real_fixed_binary_vertical_slice() {
    real_slice(false);
}

#[test]
#[ignore = "Explicit isolated codex-cli 0.153.4 pending Resume; run alone"]
fn real_fixed_binary_explicit_resume() {
    real_slice(true);
}

fn real_slice(explicit_resume: bool) {
    let temp = tempfile::tempdir().unwrap();
    let workspace = temp.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let git = std::process::Command::new("git")
        .arg("init")
        .arg(&workspace)
        .output()
        .unwrap();
    assert!(git.status.success());
    let home = temp.path().join("codex-home");
    std::fs::create_dir(&home).unwrap();
    std::fs::copy(
        PathBuf::from(std::env::var_os("USERPROFILE").unwrap()).join(".codex/auth.json"),
        home.join("auth.json"),
    )
    .unwrap();
    let evidence = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(if explicit_resume {
            "../docs/tasks/evidence/TASK-008/explicit-resume-2026-09-09"
        } else {
            "../docs/tasks/evidence/TASK-006/implementation-2026-09-09"
        })
        .join(format!("run-{}-{}", std::process::id(), now()));
    std::fs::create_dir_all(&evidence).unwrap();
    struct Environment(Vec<(&'static str, Option<std::ffi::OsString>)>);
    impl Drop for Environment {
        fn drop(&mut self) {
            for (key, value) in &self.0 {
                unsafe {
                    match value {
                        Some(value) => std::env::set_var(key, value),
                        None => std::env::remove_var(key),
                    }
                }
            }
        }
    }
    let _env = Environment(vec![
        ("CODEX_HOME", std::env::var_os("CODEX_HOME")),
        (
            "SERENA_CONTRACT_RAW_DIR",
            std::env::var_os("SERENA_CONTRACT_RAW_DIR"),
        ),
    ]);
    // This ignored test is invoked alone in its own process, never in the ordinary suite.
    unsafe {
        std::env::set_var("CODEX_HOME", home);
        std::env::set_var("SERENA_CONTRACT_RAW_DIR", &evidence);
    }
    run(async {
        let store = StateStore::open(temp.path().join("store")).await.unwrap();
        let audit = rusqlite::Connection::open(temp.path().join("store/agent-state.db")).unwrap();
        // Test-only observation of committed transitions; no production schema change.
        audit.execute_batch("CREATE TABLE slice_trace(seq INTEGER PRIMARY KEY,kind TEXT,status TEXT,dispatch TEXT,runtime TEXT,thread TEXT,turn TEXT,owns_claim INTEGER,runtime_running INTEGER);
            CREATE TRIGGER slice_created AFTER INSERT ON executions BEGIN INSERT INTO slice_trace(kind,status,dispatch,runtime,thread,turn) VALUES ('created',new.status,new.dispatch_state,new.runtime_instance_id,new.thread_id,new.turn_id); END;
            CREATE TRIGGER slice_claim AFTER INSERT ON workspace_claims BEGIN INSERT INTO slice_trace(kind,owns_claim) VALUES ('claim_acquired',1); END;
            CREATE TRIGGER slice_transition AFTER UPDATE OF status,dispatch_state,runtime_instance_id,thread_id,turn_id ON executions BEGIN
            INSERT INTO slice_trace(kind,status,dispatch,runtime,thread,turn,owns_claim,runtime_running) VALUES ('transition',new.status,new.dispatch_state,new.runtime_instance_id,new.thread_id,new.turn_id,EXISTS(SELECT 1 FROM workspace_claims WHERE execution_id=new.id),EXISTS(SELECT 1 FROM runtime_instances WHERE id=new.runtime_instance_id AND state='running')); END;
            CREATE TRIGGER slice_released AFTER DELETE ON workspace_claims BEGIN INSERT INTO slice_trace(kind,status,owns_claim) SELECT 'claim_released',status,0 FROM executions WHERE id=old.execution_id; END;").unwrap();
        let exe = PathBuf::from(
            r"C:\Users\lifei\AppData\Roaming\npm\node_modules\@openai\codex\node_modules\@openai\codex-win32-x64\vendor\x86_64-pc-windows-msvc\bin\codex.exe",
        );
        let manager = AgentTaskManager::new(store.clone(), exe);
        let request = input(&workspace);
        let result = if explicit_resume {
            let mut created = manager.create(request.clone()).await.unwrap();
            for _ in 0..3 {
                assert!(matches!(
                    &manager.recover_startup().await.unwrap()[0],
                    crate::agent::task_manager::recovery::RecoveryOutcome::PendingExplicitResume { .. }
                ));
            }
            let duplicate = manager.execute(request.clone()).await.unwrap();
            assert!(!duplicate.created);
            assert_eq!(duplicate.execution, created.execution);
            manager
                .resume_pending_execution(&created.execution_id)
                .await
                .map(|execution| {
                    created.execution = execution;
                    created
                })
        } else {
            manager.execute(request.clone()).await
        };
        std::fs::write(evidence.join("outcome.txt"), format!("{result:#?}")).unwrap();
        let result = result.unwrap();
        let row = &result.execution;
        assert!(result.created);
        assert_eq!(row.status, "completed");
        assert_eq!(row.dispatch_state, "dispatched");
        assert_eq!(
            row.runtime_instance_id.as_ref().unwrap(),
            &format!("runtime-{}", row.id)
        );
        assert_eq!(
            row.provider_terminal_evidence_runtime_instance_id,
            row.runtime_instance_id
        );
        assert_eq!(row.provider_terminal_status.as_deref(), Some("completed"));
        assert_eq!(row.background_cleanup_state, "empty");
        assert_eq!(row.result_completeness, "complete");
        assert_eq!(row.release_evidence_state, "complete");
        assert!(
            store
                .workspace_claim(request.canonical_workspace_root.clone())
                .await
                .unwrap()
                .is_none()
        );
        let final_result: Value =
            serde_json::from_str(row.final_result_json.as_ref().unwrap()).unwrap();
        assert_eq!(final_result["executionId"], row.id);
        assert_eq!(final_result["threadId"].as_str(), row.thread_id.as_deref());
        assert_eq!(final_result["turnId"].as_str(), row.turn_id.as_deref());
        assert_eq!(final_result["historyMode"], "paginated");
        assert_eq!(
            final_result["sourceRuntimeId"].as_str(),
            row.runtime_instance_id.as_deref()
        );
        assert_eq!(final_result["terminalTurn"]["status"], "completed");
        assert_eq!(final_result["resultCompleteness"], "complete");
        assert!(
            final_result["finalResult"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v["phase"] == "final_answer"
                    && v["text"].as_str().unwrap().contains("VERTICAL_SLICE_OK"))
        );
        let duplicate = manager.execute(request).await.unwrap();
        assert!(!duplicate.created);
        assert_eq!(duplicate.execution, row.clone());
        let runtime = store
            .runtime(row.runtime_instance_id.clone().unwrap())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(runtime.termination_evidence_state, "complete");
        let db = rusqlite::Connection::open(temp.path().join("store/agent-state.db")).unwrap();
        let counts: (i64, i64) = db
            .query_row(
                "SELECT (SELECT count(*) FROM executions),(SELECT count(*) FROM runtime_instances)",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(counts, (1, 1));
        if explicit_resume {
            assert!(manager.resume_pending_execution(&row.id).await.is_err());
            let raw =
                std::fs::read_to_string(evidence.join(format!("{}.stdin.raw.jsonl", runtime.id)))
                    .unwrap();
            let requests: Vec<Value> = raw
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
            for method in ["thread/start", "turn/start"] {
                assert_eq!(requests.iter().filter(|v| v["method"] == method).count(), 1);
            }
            std::fs::write(evidence.join("resume-identity.json"),serde_json::to_vec_pretty(&json!({"execution":row.id,"runtime":row.runtime_instance_id,"thread":row.thread_id,"turn":row.turn_id,"status":row.status,"resultCompleteness":row.result_completeness,"claim":"absent","runtimeCount":1,"threadStartCount":1,"turnStartCount":1,"jobConvergence":runtime.termination_evidence_state})).unwrap()).unwrap();
        }
        let cleanup_runtime: String = db
            .query_row(
                "SELECT background_cleanup_runtime_instance_id FROM executions",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(Some(&cleanup_runtime), row.runtime_instance_id.as_ref());
        let trace = audit.prepare("SELECT kind,status,dispatch,runtime,thread,turn,owns_claim,runtime_running FROM slice_trace ORDER BY seq").unwrap()
            .query_map([], |r| Ok(json!({"kind":r.get::<_,String>(0)?,"status":r.get::<_,Option<String>>(1)?,"dispatch":r.get::<_,Option<String>>(2)?,"runtime":r.get::<_,Option<String>>(3)?,"thread":r.get::<_,Option<String>>(4)?,"turn":r.get::<_,Option<String>>(5)?,"ownsClaim":r.get::<_,Option<i64>>(6)?,"runtimeRunning":r.get::<_,Option<i64>>(7)?}))).unwrap()
            .collect::<rusqlite::Result<Vec<_>>>().unwrap();
        assert_eq!(trace[0]["kind"], "created");
        assert_eq!(trace[0]["status"], "dispatch_pending");
        assert_eq!(trace[0]["dispatch"], "not_dispatched");
        assert!(trace[0]["runtime"].is_null());
        assert_eq!(trace[1]["kind"], "claim_acquired");
        let binding = trace
            .iter()
            .find(|v| v["dispatch"] == "dispatching")
            .unwrap();
        assert_eq!(binding["ownsClaim"], 1);
        assert_eq!(binding["runtimeRunning"], 1);
        assert!(
            trace
                .iter()
                .filter(|v| !v["runtime"].is_null())
                .all(|v| v["runtime"].as_str() == row.runtime_instance_id.as_deref())
        );
        assert_eq!(trace.last().unwrap()["kind"], "claim_released");
        assert_eq!(trace.last().unwrap()["status"], "completed");
        std::fs::write(
            evidence.join("db-trace.json"),
            serde_json::to_vec_pretty(&trace).unwrap(),
        )
        .unwrap();
        std::fs::write(evidence.join("final-state.txt"),format!("Execution={row:#?}\nRuntime={runtime:#?}\nClaim=None\ncounts={counts:?}\ncleanup_runtime={cleanup_runtime}\nDuplicate dispatch=false")).unwrap();
        assert!(
            std::process::Command::new("git")
                .arg("-C")
                .arg(&workspace)
                .args(["status", "--porcelain"])
                .output()
                .unwrap()
                .stdout
                .is_empty()
        );
    });
}

#[test]
fn coordinator_rejects_cleanup_from_another_execution() {
    run(async {
        let temp = tempfile::tempdir().unwrap();
        let store = StateStore::open(temp.path().into()).await.unwrap();
        let db = rusqlite::Connection::open(temp.path().join("agent-state.db")).unwrap();
        db.execute("INSERT INTO runtime_instances(id,owner_host_instance_id,state,created_at,updated_at) VALUES ('R1','fixture','running',1,1)", []).unwrap();
        for id in ["E1", "E2"] {
            let mut request = input(&temp.path().join(id));
            request.agent_id = id.into();
            store
                .create_execution(
                    id.into(),
                    crate::agent::execution::canonicalize_request(request).unwrap(),
                    now(),
                )
                .await
                .unwrap();
            store
                .transition_execution(
                    id.into(),
                    0,
                    Transition::Dispatch {
                        to: DispatchState::Dispatching,
                        runtime_id: Some("R1".into()),
                    },
                    now(),
                )
                .await
                .unwrap();
            store
                .bind_protocol_identity(
                    id.into(),
                    1,
                    "R1".into(),
                    id.into(),
                    Some("TURN".into()),
                    now(),
                )
                .await
                .unwrap();
            store
                .transition_execution(
                    id.into(),
                    2,
                    Transition::ProviderTerminal {
                        runtime_id: "R1".into(),
                        status: Status::Completed,
                    },
                    now(),
                )
                .await
                .unwrap();
        }
        let (wire, server) = tokio::io::duplex(128 * 1024);
        let (read, write) = tokio::io::split(wire);
        let client = Client::transport("R1".into(), read, write, tokio::io::empty());
        let fake = tokio::spawn(async move {
            let mut s = BufReader::new(server);
            let req = recv(&mut s).await;
            reply(&mut s,&req,json!({"userAgent":"fake","codexHome":"isolated","platformFamily":"windows","platformOs":"windows"})).await;
            assert_eq!(recv(&mut s).await["method"], "initialized");
            for (method, thread, result) in [
                (
                    "thread/read",
                    "E1",
                    json!({"thread":{"id":"E1","historyMode":"paginated","turns":[]}}),
                ),
                (
                    "thread/turns/list",
                    "E1",
                    json!({"data":[turn("completed")],"nextCursor":null}),
                ),
                (
                    "thread/items/list",
                    "E1",
                    json!({"data":[{"turnId":"TURN","item":{"type":"agentMessage","id":"i","phase":"final_answer","text":"ok"}}],"nextCursor":null}),
                ),
                ("thread/backgroundTerminals/clean", "E2", json!({})),
                (
                    "thread/backgroundTerminals/list",
                    "E2",
                    json!({"data":[],"nextCursor":null}),
                ),
            ] {
                let req = recv(&mut s).await;
                assert_eq!(req["method"], method);
                assert_eq!(req["params"]["threadId"], thread);
                reply(&mut s, &req, result).await;
            }
            let mut extra = String::new();
            assert_eq!(s.read_line(&mut extra).await.unwrap(), 0);
        });
        client.initialize().await.unwrap();
        let result = client
            .recover_result(
                RecoveryScope::same_runtime_for_execution(&store, "E1", "R1")
                    .await
                    .unwrap(),
            )
            .await
            .unwrap();
        let empty = client
            .cleanup(CleanupScope::for_execution(&store, "E2").await.unwrap())
            .await
            .unwrap();
        let before = store.execution("E1".into()).await.unwrap();
        assert_eq!(
            WorkspaceExecutionCoordinator {
                store: store.clone()
            }
            .finish("E1", result, empty)
            .await
            .unwrap_err(),
            "EXECUTION_RESULT_IDENTITY_MISMATCH"
        );
        assert_eq!(store.execution("E1".into()).await.unwrap(), before);
        for id in ["E1", "E2"] {
            assert!(
                store
                    .workspace_claim(temp.path().join(id).to_str().unwrap().into())
                    .await
                    .unwrap()
                    .is_some()
            );
        }
        drop(client);
        fake.await.unwrap();
    });
}

#[test]
fn long_turn_survives_old_120_second_boundary() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(slice_case("long"));
}

#[test]
fn completed_and_failed_terminal_ack_after_rpc_deadline() {
    for case in ["late-deadline", "failed-late-deadline"] {
        tokio::runtime::Builder::new_current_thread()
            .enable_all().build().unwrap().block_on(slice_case(case));
    }
}

#[test]
fn late_terminal_ack_conflicting_identity_or_error_retains_claim() {
    for case in ["wrong-ack-late-deadline", "error-ack-late-deadline", "invalid-ack-late-deadline"] {
        tokio::runtime::Builder::new_current_thread()
            .enable_all().build().unwrap().block_on(slice_case(case));
    }
}
