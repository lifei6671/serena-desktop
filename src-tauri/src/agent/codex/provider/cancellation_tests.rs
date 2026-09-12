use super::*;
use crate::agent::{execution::CreateExecutionInput, task_manager::AgentTaskManager};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream};

fn run(f: impl std::future::Future<Output = ()>) {
    tokio::runtime::Runtime::new().unwrap().block_on(f)
}
fn input(root: &std::path::Path) -> CreateExecutionInput {
    serde_json::from_value(json!({"agent_id":"cancel-agent","request_key":"cancel-key","prompt":"Count slowly to 10000, one number per line. Do not use tools or modify files.","execution_profile":{},"workspace_id":"isolated","canonical_workspace_root":root.to_str().unwrap(),"mode":"read_only"})).unwrap()
}
async fn recv(s: &mut BufReader<DuplexStream>) -> Value {
    let mut line = String::new();
    assert!(s.read_line(&mut line).await.unwrap() > 0);
    serde_json::from_str(&line).unwrap()
}
async fn send(s: &mut BufReader<DuplexStream>, v: Value) {
    s.write_all(&super::super::protocol::encode(&v).unwrap())
        .await
        .unwrap();
}
async fn reply(s: &mut BufReader<DuplexStream>, r: &Value, v: Value) {
    send(s, json!({"id":r["id"],"result":v})).await;
}
fn turn(status: &str) -> Value {
    json!({"id":"TURN","status":status,"items":[],"itemsView":"summary"})
}
async fn wait_row(
    store: &StateStore,
    id: &str,
    test: impl Fn(&ExecutionRecord) -> bool,
) -> ExecutionRecord {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let row = store.execution(id.into()).await.unwrap().unwrap();
        if test(&row) {
            return row;
        }
        assert!(tokio::time::Instant::now() < deadline, "row wait: {row:?}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}
async fn claimed(store: &StateStore, row: &ExecutionRecord) {
    assert_eq!(
        store
            .workspace_claim(row.canonical_workspace_root.clone())
            .await
            .unwrap()
            .unwrap()
            .execution_id,
        row.id
    );
}

async fn race(case: &'static str) {
    let no_cancel = matches!(case, "no-cancel" | "late-cancel");
    let temp = tempfile::tempdir().unwrap();
    let store = StateStore::open(temp.path().into()).await.unwrap();
    let manager = AgentTaskManager::new(store.clone(), "unused.exe".into());
    let created = manager.create(input(temp.path())).await.unwrap();
    let id = created.execution_id;
    let db = rusqlite::Connection::open(temp.path().join("agent-state.db")).unwrap();
    db.execute("INSERT INTO runtime_instances(id,owner_host_instance_id,state,created_at,updated_at) VALUES ('R1','test','running',1,1)",[]).unwrap();
    if case == "cancel-rollback" {
        db.execute_batch("CREATE TRIGGER cancel_rollback BEFORE DELETE ON workspace_claims BEGIN SELECT RAISE(ABORT,'injected cancelled rollback'); END;").unwrap();
    }
    if case == "same-time" {
        db.execute_batch("CREATE TRIGGER same_millisecond AFTER UPDATE OF provider_terminal_status ON executions WHEN new.provider_terminal_status IS NOT NULL AND new.interrupt_requested_at IS NOT NULL BEGIN UPDATE executions SET interrupt_requested_at=new.provider_terminal_evidence_at WHERE id=new.id; END;").unwrap();
    }
    let (wire, server) = tokio::io::duplex(128 * 1024);
    let (read, write) = tokio::io::split(wire);
    let client = Client::transport("R1".into(), read, write, tokio::io::empty());
    let fs = store.clone();
    let fi = id.clone();
    let fake = tokio::spawn(async move {
        let mut s = BufReader::new(server);
        let r = recv(&mut s).await;
        assert_eq!(r["method"], "initialize");
        reply(&mut s, &r, json!({"userAgent":"fake","codexHome":"isolated","platformFamily":"windows","platformOs":"windows"})).await;
        assert_eq!(recv(&mut s).await["method"], "initialized");
        let r = recv(&mut s).await;
        assert_eq!(r["method"], "thread/start");
        let manager = AgentTaskManager::new(fs.clone(), "must-not-start.exe".into());
        // Cancel wins no longer: Provider already atomically bound running R1.
        let before = fs.execution(fi.clone()).await.unwrap().unwrap();
        assert_eq!(before.dispatch_state, "dispatching");
        assert!(
            fs.cancel_before_dispatch_and_release(fi.clone(), before.revision, now())
                .await
                .is_err()
        );
        if !no_cancel {
            let row = manager.cancel(&fi).await.unwrap();
            assert_eq!(row.status, "dispatch_pending");
            assert_eq!(row.runtime_instance_id.as_deref(), Some("R1"));
            assert!(row.turn_id.is_none());
            assert!(row.interrupt_requested_at.is_some());
            claimed(&fs, &row).await;
            assert_eq!(manager.cancel(&fi).await.unwrap(), row);
        }
        reply(
            &mut s,
            &r,
            json!({"thread":{"id":"THREAD","turns":[],"historyMode":"paginated"}}),
        )
        .await;
        let start = recv(&mut s).await;
        assert_eq!(start["method"], "turn/start");
        // Cover both ACK and notification as the first reliable Turn identity.
        if case == "ack-identity" {
            reply(&mut s, &start, json!({"turn":turn("inProgress")})).await;
        } else {
            send(&mut s,json!({"method":"turn/started","params":{"threadId":"THREAD","turn":turn("inProgress")}})).await;
        }
        let interrupt = if no_cancel {
            Value::Null
        } else {
            recv(&mut s).await
        };
        if !no_cancel {
            assert_eq!(interrupt["method"], "turn/interrupt");
            assert_eq!(
                interrupt["params"],
                json!({"threadId":"THREAD","turnId":"TURN"})
            );
            // Flush persistence may still be racing the observed interrupt bytes.
            // Quiesce that independent event before testing cancel's whole-row no-op.
            let row = wait_row(&fs, &fi, |r| r.dispatch_state == "dispatched").await;
            assert_eq!(row.status, "cancel_requested");
            for _ in 0..4 {
                assert_eq!(manager.cancel(&fi).await.unwrap(), row);
            }
            claimed(&fs, &row).await;
        }
        let terminal_status = if case == "failed-cancel" {
            "failed"
        } else if matches!(
            case,
            "approved-cancel" | "same-time" | "cancel-rollback" | "no-cancel" | "late-cancel"
        ) {
            "interrupted"
        } else {
            "completed"
        };
        let timeout = matches!(
            case,
            "terminal-timeout" | "finalizing-timeout" | "timeout-first"
        );
        if matches!(
            case,
            "ack-first" | "ack-identity" | "approved-cancel" | "same-time" | "cancel-rollback"
        ) {
            reply(&mut s, &interrupt, json!({})).await;
            let row = wait_row(&fs, &fi, |r| r.status == "cancelling").await;
            assert!(row.interrupt_ack_at.is_some());
            assert!(row.provider_terminal_status.is_none());
            claimed(&fs, &row).await;
        }
        if case != "timeout-first" {
            send(&mut s,json!({"method":"turn/completed","params":{"threadId":"THREAD","turn":turn(terminal_status)}})).await;
            let row = wait_row(&fs, &fi, |r| r.status == "finalizing").await;
            assert_eq!(
                row.provider_terminal_status.as_deref(),
                Some(terminal_status)
            );
            if case != "no-cancel" {
                assert_eq!(manager.cancel(&fi).await.unwrap(), row);
            }
            if no_cancel {
                assert!(row.interrupt_requested_at.is_none());
            }
            if matches!(case, "terminal-first" | "finalizing-ack" | "failed-cancel") {
                reply(&mut s, &interrupt, json!({})).await;
            }
        }
        if case != "ack-identity" {
            reply(&mut s, &start, json!({"turn":turn("inProgress")})).await;
        }
        if timeout {
            // Real 15s RPC budget; no tiny scheduling boundary or fake deadline.
            tokio::time::sleep(Duration::from_secs(17)).await;
        } else {
            let r = recv(&mut s).await;
            assert_eq!(r["method"], "thread/read");
            reply(
                &mut s,
                &r,
                json!({"thread":{"id":"THREAD","turns":[],"historyMode":"paginated"}}),
            )
            .await;
            let r = recv(&mut s).await;
            assert_eq!(r["method"], "thread/turns/list");
            reply(
                &mut s,
                &r,
                json!({"data":[turn(terminal_status)],"nextCursor":null}),
            )
            .await;
            let r = recv(&mut s).await;
            assert_eq!(r["method"], "thread/items/list");
            assert_eq!(r["params"]["turnId"], "TURN");
            reply(&mut s,&r,json!({"data":[{"turnId":"TURN","item":{"id":"item","type":"agentMessage","phase":"final_answer","text":"result"}}],"nextCursor":null})).await;
            let r = recv(&mut s).await;
            assert_eq!(r["method"], "thread/backgroundTerminals/clean");
            reply(&mut s, &r, json!({})).await;
            for empty in [false, true] {
                let r = recv(&mut s).await;
                assert_eq!(r["method"], "thread/backgroundTerminals/list");
                let row = fs.execution(fi.clone()).await.unwrap().unwrap();
                assert_eq!(row.status, "finalizing");
                claimed(&fs, &row).await;
                reply(&mut s,&r,json!({"data":if empty {json!([])} else {json!([{"id":"active"}])},"nextCursor":null})).await;
            }
        }
        let mut extra = String::new();
        assert_eq!(
            s.read_line(&mut extra).await.unwrap(),
            0,
            "extra request: {extra}"
        );
    });
    client.initialize().await.unwrap();
    let provider = CodexProvider { runtime_pool: Default::default(),
        store: store.clone(),
        executable: "unused".into(),
        owner: "test".into(),
    };
    let outcome = tokio::time::timeout(Duration::from_secs(25), provider.run_client(&id, &client))
        .await
        .unwrap();
    let row = store.execution(id.clone()).await.unwrap().unwrap();
    assert_eq!(row.runtime_instance_id.as_deref(), Some("R1"));
    assert_eq!(row.turn_id.as_deref(), Some("TURN"));
    assert_eq!(row.dispatch_state, "dispatched");
    if case.contains("timeout") {
        assert!(outcome.is_err());
        assert!(row.interrupt_timeout_at.is_some());
        assert_eq!(
            row.status,
            if case == "timeout-first" {
                "reconciling"
            } else {
                "finalizing"
            }
        );
        claimed(&store, &row).await;
    } else if case == "cancel-rollback" {
        assert!(outcome.unwrap_err().contains("injected cancelled rollback"));
        assert_eq!(row.status, "finalizing");
        assert_eq!(row.background_cleanup_state, "empty");
        assert!(row.final_result_json.is_none());
        assert_eq!(row.result_completeness, "unknown");
        assert!(row.release_evidence_json.is_none());
        assert_ne!(row.release_evidence_state, "complete");
        claimed(&store, &row).await;
    } else if matches!(
        case,
        "approved-cancel" | "same-time" | "no-cancel" | "late-cancel"
    ) {
        assert!(outcome.is_ok(), "{outcome:?}");
        assert_eq!(
            row.status,
            if no_cancel {
                "interrupted"
            } else {
                "cancelled"
            }
        );
        assert_eq!(row.interrupt_requested_at.is_none(), no_cancel);
        assert_eq!(row.provider_terminal_status.as_deref(), Some("interrupted"));
        let result: Value = serde_json::from_str(row.final_result_json.as_ref().unwrap()).unwrap();
        assert_eq!(result["terminalTurn"]["status"], "interrupted");
        assert_eq!(result["threadId"], "THREAD");
        assert_eq!(result["turnId"], "TURN");
        assert_eq!(result["sourceRuntimeId"], "R1");
        assert_eq!(row.result_completeness, "complete");
        assert_eq!(row.release_evidence_state, "complete");
        assert!(
            store
                .workspace_claim(row.canonical_workspace_root.clone())
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(manager.cancel(&id).await.unwrap(), row);
        if case == "same-time" {
            let terminal_at: i64 = db
                .query_row(
                    "SELECT provider_terminal_evidence_at FROM executions WHERE id=?1",
                    [&id],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(row.interrupt_requested_at, Some(terminal_at));
        }
    } else if case == "failed-cancel" {
        assert_eq!(outcome.unwrap_err(), "PROVIDER_TERMINAL_failed");
        assert_eq!(row.status, "failed");
        assert!(row.interrupt_ack_at.is_some());
        assert_eq!(row.result_completeness, "complete");
        assert!(
            store
                .workspace_claim(row.canonical_workspace_root.clone())
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(manager.cancel(&id).await.unwrap(), row);
    } else {
        assert!(outcome.is_ok(), "{outcome:?}");
        assert_eq!(row.status, "completed");
        assert_eq!(row.result_completeness, "complete");
        assert!(
            store
                .workspace_claim(row.canonical_workspace_root.clone())
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(manager.cancel(&id).await.unwrap(), row);
    }
    drop(client);
    fake.await.unwrap();
}

#[test]
fn ack_first() {
    run(race("ack-first"));
}
#[test]
fn failed_terminal_before_ack_still_cleans_and_returns_failure() {
    run(race("failed-cancel"));
}
#[test]
fn pending_cancel_then_start_ack_identity() {
    run(race("ack-identity"));
}
#[test]
fn terminal_before_interrupt_ack() {
    run(race("terminal-first"));
}
#[test]
fn finalizing_then_ack() {
    run(race("finalizing-ack"));
}
#[test]
fn terminal_before_interrupt_timeout() {
    run(race("terminal-timeout"));
}
#[test]
fn finalizing_then_timeout() {
    run(race("finalizing-timeout"));
}
#[test]
fn interrupt_timeout_without_terminal_retains_claim() {
    run(race("timeout-first"));
}
#[test]
fn approved_interrupted_cancellation_finalizes_cancelled() {
    run(race("approved-cancel"));
}
#[test]
fn interrupted_without_cancel_finalizes_interrupted() {
    run(race("no-cancel"));
}
#[test]
fn late_cancel_does_not_attribute_interrupted_to_user() {
    run(race("late-cancel"));
}
#[test]
fn cancelled_finalization_rollback_keeps_claim_and_no_partial_result() {
    run(race("cancel-rollback"));
}
#[test]
fn same_millisecond_intent_and_terminal_use_persisted_attribution() {
    run(race("same-time"));
}

#[test]
fn cancel_wins_and_late_cancel_is_absorbing() {
    run(async {
        let temp = tempfile::tempdir().unwrap();
        let store = StateStore::open(temp.path().into()).await.unwrap();
        let manager = AgentTaskManager::new(store.clone(), "must-not-launch.exe".into());
        let request = input(temp.path());
        let created = manager.create(request.clone()).await.unwrap();
        let row = manager.cancel(&created.execution_id).await.unwrap();
        assert_eq!(row.status, "cancelled");
        assert!(row.runtime_instance_id.is_none());
        assert!(
            store
                .workspace_claim(row.canonical_workspace_root.clone())
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(manager.cancel(&row.id).await.unwrap(), row);
        let result = manager.execute(request).await.unwrap();
        assert!(!result.created);
        let (wire, mut server) = tokio::io::duplex(1024);
        let (read, write) = tokio::io::split(wire);
        let client = Client::transport("unused".into(), read, write, tokio::io::empty());
        let provider = CodexProvider { runtime_pool: Default::default(),
            store: store.clone(),
            owner: "test".into(),
            executable: "unused".into(),
        };
        assert_eq!(provider.run_client(&row.id, &client).await.unwrap(), row);
        drop(client);
        let mut bytes = Vec::new();
        tokio::io::AsyncReadExt::read_to_end(&mut server, &mut bytes)
            .await
            .unwrap();
        assert!(bytes.is_empty());
        // All four terminal rows are absorbing for the public cancel entry.
        let db = rusqlite::Connection::open(temp.path().join("agent-state.db")).unwrap();
        for status in ["completed", "failed", "cancelled", "interrupted"] {
            db.execute(
                "UPDATE executions SET status=?1 WHERE id=?2",
                [status, &row.id],
            )
            .unwrap();
            let before = store.execution(row.id.clone()).await.unwrap().unwrap();
            assert_eq!(manager.cancel(&row.id).await.unwrap(), before);
        }
    });
}

struct BrokenWrite {
    inner: tokio::io::WriteHalf<DuplexStream>,
    armed: std::sync::Arc<std::sync::atomic::AtomicBool>,
    partial: bool,
    wrote_partial: bool,
}
impl tokio::io::AsyncWrite for BrokenWrite {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        if self.armed.load(std::sync::atomic::Ordering::SeqCst) && self.partial {
            if self.wrote_partial {
                return std::task::Poll::Ready(Err(std::io::Error::other(
                    "injected partial write",
                )));
            }
            let result =
                std::pin::Pin::new(&mut self.inner).poll_write(cx, &buf[..buf.len().min(12)]);
            if result.is_ready() {
                self.wrote_partial = true;
            }
            result
        } else {
            std::pin::Pin::new(&mut self.inner).poll_write(cx, buf)
        }
    }
    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        if self.armed.load(std::sync::atomic::Ordering::SeqCst) {
            return std::task::Poll::Ready(Err(std::io::Error::other(
                "injected flush uncertainty",
            )));
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

#[test]
fn partial_write_and_unconfirmed_flush_are_uncertain_without_replay() {
    run(async {
        for case in ["partial", "flush", "rpc-after-flush"] {
            let partial = case == "partial";
            let expected_dispatch = if case == "rpc-after-flush" {
                "dispatched"
            } else {
                "uncertain"
            };
            let temp = tempfile::tempdir().unwrap();
            let store = StateStore::open(temp.path().into()).await.unwrap();
            let manager = AgentTaskManager::new(store.clone(), "unused".into());
            let created = manager.create(input(temp.path())).await.unwrap();
            let id = created.execution_id;
            rusqlite::Connection::open(temp.path().join("agent-state.db")).unwrap().execute("INSERT INTO runtime_instances(id,owner_host_instance_id,state,created_at,updated_at) VALUES ('R1','test','running',1,1)",[]).unwrap();
            let (wire, server) = tokio::io::duplex(128 * 1024);
            let (read, write) = tokio::io::split(wire);
            let armed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let client = Client::transport(
                "R1".into(),
                read,
                BrokenWrite {
                    inner: write,
                    armed: armed.clone(),
                    partial,
                    wrote_partial: false,
                },
                tokio::io::empty(),
            );
            let fs = store.clone();
            let fi = id.clone();
            let fake = tokio::spawn(async move {
                let mut s = BufReader::new(server);
                let r = recv(&mut s).await;
                reply(&mut s,&r,json!({"userAgent":"fake","codexHome":"isolated","platformFamily":"windows","platformOs":"windows"})).await;
                assert_eq!(recv(&mut s).await["method"], "initialized");
                let r = recv(&mut s).await;
                assert_eq!(r["method"], "thread/start");
                armed.store(
                    case != "rpc-after-flush",
                    std::sync::atomic::Ordering::SeqCst,
                );
                reply(
                    &mut s,
                    &r,
                    json!({"thread":{"id":"THREAD","turns":[],"historyMode":"paginated"}}),
                )
                .await;
                if case == "rpc-after-flush" {
                    let r = recv(&mut s).await;
                    assert_eq!(r["method"], "turn/start");
                    wait_row(&fs, &fi, |row| row.dispatch_state == "dispatched").await;
                    send(&mut s,json!({"id":r["id"],"error":{"code":-32000,"message":"injected after persisted flush"}})).await;
                }
                let mut bytes = Vec::new();
                tokio::io::AsyncReadExt::read_to_end(&mut s, &mut bytes)
                    .await
                    .unwrap();
                if partial {
                    assert_eq!(bytes.len(), 12);
                } else if case == "flush" {
                    let request: Value = serde_json::from_slice(&bytes).unwrap();
                    assert_eq!(request["method"], "turn/start");
                } else {
                    assert!(bytes.is_empty(), "automatic replay");
                }
            });
            client.initialize().await.unwrap();
            let provider = CodexProvider { runtime_pool: Default::default(),
                store: store.clone(),
                executable: "unused".into(),
                owner: "test".into(),
            };
            assert!(provider.run_client(&id, &client).await.is_err());
            let row = store.execution(id.clone()).await.unwrap().unwrap();
            assert_eq!(row.dispatch_state, expected_dispatch);
            assert_eq!(row.status, "reconciling");
            claimed(&store, &row).await;
            assert!(provider.run_client(&id, &client).await.is_err());
            // Late same-Runtime identity evidence can bind, never repair uncertainty.
            provider
                .bind(&id, &client, "THREAD", Some("TURN".into()))
                .await
                .unwrap();
            assert!(
                provider
                    .event(
                        &id,
                        Transition::Dispatch {
                            to: DispatchState::Dispatched,
                            runtime_id: None
                        }
                    )
                    .await
                    .is_err()
            );
            let row = store.execution(id.clone()).await.unwrap().unwrap();
            assert_eq!(row.dispatch_state, expected_dispatch);
            assert_eq!(row.turn_id.as_deref(), Some("TURN"));
            assert_eq!(row.runtime_instance_id.as_deref(), Some("R1"));
            claimed(&store, &row).await;
            drop(client);
            fake.await.unwrap();
        }
    });
}

#[cfg(windows)]
#[test]
#[ignore = "Explicit isolated codex-cli 0.153.4 interrupt smoke; run alone"]
fn real_fixed_binary_interrupt_smoke() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = temp.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    assert!(
        std::process::Command::new("git")
            .arg("init")
            .arg(&workspace)
            .output()
            .unwrap()
            .status
            .success()
    );
    let home = temp.path().join("home");
    std::fs::create_dir(&home).unwrap();
    std::fs::copy(
        PathBuf::from(std::env::var_os("USERPROFILE").unwrap()).join(".codex/auth.json"),
        home.join("auth.json"),
    )
    .unwrap();
    let evidence = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../docs/tasks/evidence/TASK-007/approved-mapping-2026-09-09")
        .join(format!("run-{}-{}", std::process::id(), now()));
    std::fs::create_dir_all(&evidence).unwrap();
    struct Environment(Vec<(&'static str, Option<std::ffi::OsString>)>);
    impl Drop for Environment {
        fn drop(&mut self) {
            for (key, value) in &self.0 {
                unsafe {
                    match value {
                        Some(v) => std::env::set_var(key, v),
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
    // Explicit ignored test runs alone; credentials never enter Evidence.
    unsafe {
        std::env::set_var("CODEX_HOME", home);
        std::env::set_var("SERENA_CONTRACT_RAW_DIR", &evidence);
    }
    run(async {
        let store = StateStore::open(temp.path().join("store")).await.unwrap();
        let db = rusqlite::Connection::open(temp.path().join("store/agent-state.db")).unwrap();
        db.execute_batch("CREATE TABLE cancel_trace(seq INTEGER PRIMARY KEY,status TEXT,dispatch TEXT,runtime TEXT,thread TEXT,turn TEXT,requested INTEGER,ack INTEGER,timeout INTEGER,terminal TEXT,terminal_at INTEGER,owns_claim INTEGER);
          CREATE TRIGGER cancel_trace_row AFTER UPDATE ON executions BEGIN
          INSERT INTO cancel_trace(status,dispatch,runtime,thread,turn,requested,ack,timeout,terminal,terminal_at,owns_claim) VALUES (new.status,new.dispatch_state,new.runtime_instance_id,new.thread_id,new.turn_id,new.interrupt_requested_at,new.interrupt_ack_at,new.interrupt_timeout_at,new.provider_terminal_status,new.provider_terminal_evidence_at,EXISTS(SELECT 1 FROM workspace_claims WHERE execution_id=new.id)); END;").unwrap();
        let exe = PathBuf::from(
            r"C:\Users\lifei\AppData\Roaming\npm\node_modules\@openai\codex\node_modules\@openai\codex-win32-x64\vendor\x86_64-pc-windows-msvc\bin\codex.exe",
        );
        let manager = AgentTaskManager::new(store.clone(), exe.clone());
        let created = manager.create(input(&workspace)).await.unwrap();
        let id = created.execution_id;
        let provider = CodexProvider { runtime_pool: Default::default(),
            store: store.clone(),
            executable: exe,
            owner: "task007-smoke".into(),
        };
        let cancel = async {
            let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
            loop {
                let row = store.execution(id.clone()).await.unwrap().unwrap();
                if row.status == "running" && row.turn_id.is_some() {
                    break;
                }
                assert!(
                    tokio::time::Instant::now() < deadline,
                    "no running Turn: {row:?}"
                );
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            let requested = manager.cancel(&id).await.unwrap();
            assert!(requested.interrupt_requested_at.is_some());
            assert_eq!(
                manager.cancel(&id).await.unwrap().interrupt_requested_at,
                requested.interrupt_requested_at
            );
        };
        let (outcome, ()) = tokio::join!(provider.execute(&id), cancel);
        let row = store.execution(id.clone()).await.unwrap().unwrap();
        let runtime = store
            .runtime(row.runtime_instance_id.clone().unwrap())
            .await
            .unwrap()
            .unwrap();
        std::fs::write(
            evidence.join("outcome.txt"),
            format!("{outcome:#?}\nExecution={row:#?}\nRuntime={runtime:#?}"),
        )
        .unwrap();
        let trace:Vec<Value>=db.prepare("SELECT seq,status,dispatch,runtime,thread,turn,requested,ack,timeout,terminal,terminal_at,owns_claim FROM cancel_trace ORDER BY seq").unwrap().query_map([],|r|Ok(json!({"seq":r.get::<_,i64>(0)?,"status":r.get::<_,String>(1)?,"dispatch":r.get::<_,String>(2)?,"runtime":r.get::<_,Option<String>>(3)?,"thread":r.get::<_,Option<String>>(4)?,"turn":r.get::<_,Option<String>>(5)?,"requested":r.get::<_,Option<i64>>(6)?,"ack":r.get::<_,Option<i64>>(7)?,"timeout":r.get::<_,Option<i64>>(8)?,"terminal":r.get::<_,Option<String>>(9)?,"terminal_at":r.get::<_,Option<i64>>(10)?,"owns_claim":r.get::<_,i64>(11)?}))).unwrap().collect::<Result<_,_>>().unwrap();
        std::fs::write(
            evidence.join("db-trace.json"),
            serde_json::to_vec_pretty(&trace).unwrap(),
        )
        .unwrap();
        let runtime_id = row.runtime_instance_id.as_ref().unwrap();
        let requests: Vec<Value> =
            std::fs::read_to_string(evidence.join(format!("{runtime_id}.stdin.raw.jsonl")))
                .unwrap()
                .lines()
                .map(|l| serde_json::from_str(l).unwrap())
                .collect();
        let count = |method: &str| requests.iter().filter(|r| r["method"] == method).count();
        assert_eq!(count("thread/start"), 1);
        assert_eq!(count("turn/start"), 1);
        assert_eq!(count("turn/interrupt"), 1);
        let interrupt = requests
            .iter()
            .find(|r| r["method"] == "turn/interrupt")
            .unwrap();
        assert_eq!(
            interrupt["params"]["threadId"].as_str(),
            row.thread_id.as_deref()
        );
        assert_eq!(
            interrupt["params"]["turnId"].as_str(),
            row.turn_id.as_deref()
        );
        assert!(row.interrupt_ack_at.is_some());
        assert!(row.provider_terminal_status.is_some());
        assert_eq!(runtime.state, "terminated");
        assert_eq!(runtime.termination_evidence_state, "complete");
        assert_eq!(
            runtime.termination_evidence_type.as_deref(),
            Some("job_active_processes_zero")
        );
        assert!(outcome.is_ok(), "{outcome:?}");
        assert_eq!(row.provider_terminal_status.as_deref(), Some("interrupted"));
        assert_eq!(row.status, "cancelled");
        let result: Value = serde_json::from_str(row.final_result_json.as_ref().unwrap()).unwrap();
        assert_eq!(result["terminalTurn"]["status"], "interrupted");
        assert_eq!(result["threadId"].as_str(), row.thread_id.as_deref());
        assert_eq!(result["turnId"].as_str(), row.turn_id.as_deref());
        assert_eq!(result["sourceRuntimeId"], runtime_id.as_str());
        assert_eq!(row.result_completeness, "complete");
        assert_eq!(row.release_evidence_state, "complete");
        assert_eq!(row.background_cleanup_state, "empty");
        assert!(
            store
                .workspace_claim(row.canonical_workspace_root.clone())
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM runtime_instances", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
        std::fs::write(
            evidence.join("final-result.json"),
            serde_json::to_vec_pretty(&result).unwrap(),
        )
        .unwrap();
        std::fs::write(evidence.join("identity.json"),serde_json::to_vec_pretty(&json!({"execution":id,"runtime":runtime_id,"thread":row.thread_id,"turn":row.turn_id,"interrupt_requested_at":row.interrupt_requested_at,"interrupt_ack_at":row.interrupt_ack_at,"interrupt_timeout_at":row.interrupt_timeout_at,"provider_terminal":row.provider_terminal_status,"status":row.status,"dispatch_state":row.dispatch_state,"claim_retained":store.workspace_claim(row.canonical_workspace_root.clone()).await.unwrap().is_some(),"thread_start_count":count("thread/start"),"turn_start_count":count("turn/start"),"interrupt_count":count("turn/interrupt"),"job_convergence":runtime.termination_evidence_type})).unwrap()).unwrap();
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
