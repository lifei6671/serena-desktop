use super::*;
use crate::agent::codex::{app_server::managed::ManagedClient, pool::CodexRuntimePool};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

#[derive(Default)]
struct Evidence {
    launches: Mutex<Vec<String>>,
    methods: Mutex<Vec<(String, String)>>,
    shutdowns: Mutex<HashMap<String, usize>>,
    next_turn: AtomicUsize,
    fail_cleanup: AtomicBool,
    fail_shutdown: AtomicBool,
    hold_turn: AtomicBool,
    delayed_title: AtomicBool,
}

#[test]
fn idle_host_crash_child() {
    let Some(root) = std::env::var_os("SERENA_PERSISTENCE_CRASH_ROOT") else {
        return;
    };
    run(async {
        let root = PathBuf::from(root);
        let store = StateStore::open(root.clone()).await.unwrap();
        let s = service(
            store,
            root.join("agent-state.db"),
            Arc::new(Evidence::default()),
        );
        let first = submit(&s, start("crash-lineage", "first"), &root).await;
        std::fs::write(
            root.join("completed.json"),
            serde_json::to_vec(&first).unwrap(),
        )
        .unwrap();
        // Abrupt Host exit: no Agent shutdown and no Rust Drop/async monitor drain.
        std::process::exit(0);
    });
}

#[test]
fn actual_host_restart_converges_idle_orphan_and_cold_continuation() {
    use std::os::windows::process::CommandExt;
    let dir = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "agent::product::tests::persistence_tests::idle_host_crash_child",
            "--nocapture",
        ])
        .env("SERENA_PERSISTENCE_CRASH_ROOT", dir.path())
        .creation_flags(0x08000000)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    run(async {
        let completed: Value =
            serde_json::from_slice(&std::fs::read(dir.path().join("completed.json")).unwrap())
                .unwrap();
        let id = completed["executionId"].as_str().unwrap();
        let store = StateStore::open(dir.path().into()).await.unwrap();
        let original = store.execution(id.into()).await.unwrap().unwrap();
        let runtime = original.runtime_instance_id.as_ref().unwrap();
        assert_eq!(
            store.runtime(runtime.clone()).await.unwrap().unwrap().state,
            "running"
        );
        let evidence = Arc::new(Evidence::default());
        let s = service(
            store.clone(),
            dir.path().join("agent-state.db"),
            evidence.clone(),
        );
        let outcomes = s.manager.recover_startup().await.unwrap();
        assert!(outcomes.iter().any(|o|matches!(o,crate::agent::task_manager::recovery::RecoveryOutcome::OrphanRuntime{runtime_id,..} if runtime_id==runtime)));
        // Fake transport has no Windows Job policy: unknown is mandatory. The
        // real named-Job destruction/complete evidence path has Runtime tests.
        let old = store.runtime(runtime.clone()).await.unwrap().unwrap();
        assert_eq!(old.state, "unknown");
        assert_eq!(old.termination_evidence_state, "unknown");
        assert_eq!(store.execution(id.into()).await.unwrap().unwrap(), original);
        assert!(s.manager.runtime_pool.lease(&store, &original.canonical_workspace_root).await.is_err());
        assert!(evidence.launches.lock().unwrap().is_empty());
        // Transport fixture only: supply the synthetic monitor's evidence; real
        // named Job recovery is covered in runtime/tests.
        let db = rusqlite::Connection::open(dir.path().join("agent-state.db")).unwrap();
        db.execute("UPDATE runtime_instances SET state='terminated',termination_evidence_state='complete',termination_evidence_type='job_active_processes_zero',termination_evidence_at=99 WHERE id=?1",[runtime]).unwrap();
        s.manager.runtime_pool.retry_workspace(&store, &original.canonical_workspace_root).await.unwrap();
        let next = submit(&s, continuation(id, "after-restart"), dir.path()).await;
        assert_eq!(next.thread_id, original.thread_id);
        assert_ne!(next.runtime_instance_id, original.runtime_instance_id);
        assert_eq!(
            counts(&evidence, next.runtime_instance_id.as_ref().unwrap()),
            [1, 0, 1, 1]
        );
        s.shutdown().await.unwrap();
    });
}

fn service(store: StateStore, database: PathBuf, evidence: Arc<Evidence>) -> AgentProductService {
    let manager = AgentTaskManager::new(store.clone(), "fixture.exe".into());
    let pool = manager.runtime_pool.clone();
    let fake_store = store.clone();
    *pool.test_connect.lock().unwrap() = Some(Arc::new(move |runtime, _workspace| {
        let evidence = evidence.clone();
        let database = database.clone();
        let store = fake_store.clone();
        Box::pin(async move {
            evidence.launches.lock().unwrap().push(runtime.clone());
            rusqlite::Connection::open(&database).unwrap().execute(
                "INSERT INTO runtime_instances(id,owner_host_instance_id,state,created_at,updated_at) VALUES (?1,'fixture','running',1,1)", [&runtime]).unwrap();
            let (wire, server) = tokio::io::duplex(128 * 1024);
            let (read, write) = tokio::io::split(wire);
            let client =
                Client::product_test_transport(runtime.clone(), read, write, tokio::io::empty());
            let mut failure = client.failure();
            let fake_runtime = runtime.clone();
            let fake_evidence = evidence.clone();
            let fake_database = database.clone();
            let fake = tokio::spawn(async move {
                let mut io = BufReader::new(server);
                let mut turns: HashMap<String, Vec<Value>> = HashMap::new();
                let mut active = String::new();
                let mut current = String::new();
                let mut roots = 0;
                loop {
                    let mut line = String::new();
                    if io.read_line(&mut line).await.unwrap() == 0 {
                        break;
                    }
                    let v: Value = serde_json::from_str(&line).unwrap();
                    let method = v["method"].as_str().expect("unexpected server reply");
                    fake_evidence
                        .methods
                        .lock()
                        .unwrap()
                        .push((fake_runtime.clone(), method.into()));
                    let result = match method {
                        "initialize" => {
                            json!({"userAgent":"fake","codexHome":"fixture","platformFamily":"windows","platformOs":"windows"})
                        }
                        "initialized" => continue,
                        "thread/start" | "thread/resume" => {
                            if method == "thread/start" {
                                roots += 1;
                                active = format!("{fake_runtime}-root-{roots}");
                            } else {
                                active = v["params"]["threadId"].as_str().unwrap().into();
                            }
                            json!({"thread":{"id":active,"turns":[],"historyMode":"paginated"}})
                        }
                        "turn/start" => {
                            active = v["params"]["threadId"].as_str().unwrap().into();
                            current = format!(
                                "turn-{}",
                                fake_evidence.next_turn.fetch_add(1, Ordering::SeqCst)
                            );
                            send(&mut io, json!({"id":v["id"],"result":{"turn":{"id":current,"status":"inProgress","items":[]}}})).await;
                            send(&mut io, json!({"method":"turn/started","params":{"threadId":active,"turn":{"id":current,"status":"inProgress","items":[]}}})).await;
                            let execution = tokio::time::timeout(Duration::from_secs(5), async {
                                loop {
                                    let id: Option<String> = rusqlite::Connection::open(&fake_database).unwrap().query_row(
                                        "SELECT id FROM executions WHERE runtime_instance_id=?1 AND turn_id=?2", [&fake_runtime,&current], |r| r.get(0)).optional().unwrap();
                                    if let Some(id) = id { break id; }
                                    tokio::task::yield_now().await;
                                }
                            }).await.unwrap();
                            // Real dynamic title path must resolve to this new Execution.
                            send(&mut io, json!({"id":"title-call","method":"item/tool/call","params":{
                                "threadId":active,"turnId":current,"callId":"call","namespace":"codex_app","tool":"set_thread_title","arguments":{"title":current}
                            }})).await;
                            let rename = receive(&mut io).await;
                            assert_eq!(rename["method"], "thread/name/set");
                            assert_eq!(rename["params"]["threadId"], active);
                            assert_eq!(rename["params"]["name"], current);
                            send(&mut io, json!({"id":rename["id"],"result":{}})).await;
                            let title_reply = receive(&mut io).await;
                            assert_eq!(title_reply["result"]["success"], true);
                            send(&mut io, json!({"method":"thread/name/updated","params":{"threadId":active,"threadName":current}})).await;
                            send(&mut io, json!({"method":"item/started","params":{"threadId":active,"turnId":current,
                                "item":{"type":"commandExecution","id":"activity","command":"cargo test","commandActions":[]}}})).await;
                            tokio::time::timeout(Duration::from_secs(5), async {
                                while store
                                    .execution(execution.clone())
                                    .await
                                    .unwrap()
                                    .unwrap()
                                    .tool_category
                                    .as_deref()
                                    != Some("test")
                                {
                                    tokio::task::yield_now().await;
                                }
                            })
                            .await
                            .unwrap();
                            // Delayed previous-Turn events must not bind the next execution.
                            for old in turns.get(&active).into_iter().flatten() {
                                send(&mut io, json!({"method":"turn/completed","params":{"threadId":active,"turn":old}})).await;
                                send(&mut io, json!({"method":"item/started","params":{"threadId":active,"turnId":old["id"],
                                    "item":{"type":"commandExecution","id":"stale","command":"cargo build","commandActions":[]}}})).await;
                            }
                            if fake_evidence.hold_turn.load(Ordering::SeqCst) {
                                continue;
                            }
                            let terminal = json!({"id":current,"status":"completed","items":[],"itemsView":"summary"});
                            turns
                                .entry(active.clone())
                                .or_default()
                                .push(terminal.clone());
                            send(&mut io, json!({"method":"turn/completed","params":{"threadId":active,"turn":terminal}})).await;
                            continue;
                        }
                        "turn/interrupt" => {
                            let terminal = json!({"id":current,"status":"interrupted","items":[],"itemsView":"summary"});
                            turns
                                .entry(active.clone())
                                .or_default()
                                .push(terminal.clone());
                            send(&mut io, json!({"method":"turn/completed","params":{"threadId":active,"turn":terminal}})).await;
                            json!({})
                        }
                        "thread/read" => {
                            json!({"thread":{"id":active,"name":current,"turns":[],"historyMode":"paginated"}})
                        }
                        "thread/turns/list" => json!({"data":turns[&active],"nextCursor":null}),
                        "thread/items/list" => {
                            json!({"data":[{"turnId":current,"item":{"id":"answer","type":"agentMessage","phase":"final_answer","text":"OK"}}],"nextCursor":null})
                        }
                        "thread/backgroundTerminals/clean" => {
                            if fake_evidence.fail_cleanup.swap(false, Ordering::SeqCst) {
                                break;
                            }
                            if fake_evidence.delayed_title.load(Ordering::SeqCst) {
                                send(&mut io, json!({"method":"thread/name/updated","params":{"threadId":active,"threadName":format!("{current} cleanup")}})).await;
                            }
                            json!({})
                        }
                        "thread/backgroundTerminals/list" => json!({"data":[],"nextCursor":null}),
                        other => panic!("Unexpected method {other}"),
                    };
                    send(&mut io, json!({"id":v["id"],"result":result})).await;
                }
            });
            let monitor = tokio::spawn(async move {
                while failure.borrow().is_none() {
                    if failure.changed().await.is_err() {
                        break;
                    }
                }
                fake.await.unwrap();
                *evidence
                    .shutdowns
                    .lock()
                    .unwrap()
                    .entry(runtime.clone())
                    .or_default() += 1;
                if evidence.fail_shutdown.swap(false, Ordering::SeqCst) {
                    return Err(crate::agent::codex::runtime::RuntimeFailure {
                        code: "CODEX_RUNTIME_TERMINATION_TIMEOUT", message: "fixture shutdown has no termination evidence".into(), runtime: None,
                    });
                }
                // Fake Job boundary only; real Windows Job evidence has Runtime tests.
                rusqlite::Connection::open(database).unwrap().execute(
                    "UPDATE runtime_instances SET state='terminated',termination_evidence_state='complete',termination_evidence_type='job_active_processes_zero',termination_evidence_at=10 WHERE id=?1", [&runtime]).unwrap();
                Ok(())
            });
            client.initialize().await.unwrap();
            Ok(ManagedClient::test_owned(client, monitor))
        })
    }));
    AgentProductService { store, manager }
}
use rusqlite::OptionalExtension;
async fn send(io: &mut BufReader<tokio::io::DuplexStream>, value: Value) {
    io.write_all(format!("{value}\n").as_bytes()).await.unwrap();
}
async fn receive(io: &mut BufReader<tokio::io::DuplexStream>) -> Value {
    let mut line = String::new();
    io.read_line(&mut line).await.unwrap();
    serde_json::from_str(&line).unwrap()
}
async fn submit(s: &AgentProductService, request: Value, root: &Path) -> ExecutionView {
    let receipt = s.checked_operation(request, w(root, "W")).await;
    assert_eq!(receipt["ok"], true, "{receipt}");
    let id = receipt["data"]["executionId"].as_str().unwrap();
    let view = final_row(s, id).await;
    // Terminal is durable before the provider returns its lease.
    drop(
        s.manager
            .runtime_pool
            .lease(&s.store, &view.canonical_workspace_root)
            .await,
    );
    view
}
fn continuation(id: &str, key: &str) -> Value {
    json!({"action":"continue","executionId":id,"requestKey":key,"prompt":"next"})
}
fn counts(e: &Evidence, runtime: &str) -> [usize; 4] {
    ["initialize", "thread/start", "thread/resume", "turn/start"].map(|m| {
        e.methods
            .lock()
            .unwrap()
            .iter()
            .filter(|(r, method)| r == runtime && method == m)
            .count()
    })
}

#[test]
fn ownerless_created_attempt_blocks_shutdown_but_precreate_failure_does_not() {
    run(async {
        use crate::agent::codex::runtime::RuntimeFailure;
        let dir = tempfile::tempdir().unwrap();
        let store = StateStore::open(dir.path().into()).await.unwrap();
        let pool = CodexRuntimePool::default();
        let error = || RuntimeFailure {
            code: "CODEX_APP_SERVER_INCOMPATIBLE",
            message: "monitor JoinError fixture".into(),
            runtime: None,
        };
        pool.retain_attempt_failure(&store, "W", "absent", error()).await;
        pool.shutdown().await.unwrap();
        let pool = CodexRuntimePool::default();
        rusqlite::Connection::open(dir.path().join("agent-state.db")).unwrap().execute(
        "INSERT INTO runtime_instances(id,owner_host_instance_id,state,created_at,updated_at) VALUES ('created','fixture','running',1,1)",[]).unwrap();
        pool.retain_attempt_failure(&store, "W", "created", error())
            .await;
        assert!(
            pool.shutdown()
                .await
                .unwrap_err()
                .contains("CODEX_RUNTIME_EVIDENCE_INCOMPLETE")
        );
        assert_eq!(
            store
                .runtime("created".into())
                .await
                .unwrap()
                .unwrap()
                .termination_evidence_state,
            "unknown"
        );
    });
}

#[test]
fn title_received_during_cleanup_is_durable_before_next_warm_turn() {
    run(async {
        let dir = tempfile::tempdir().unwrap();
        let store = StateStore::open(dir.path().into()).await.unwrap();
        let evidence = Arc::new(Evidence::default());
        let s = service(
            store.clone(),
            dir.path().join("agent-state.db"),
            evidence.clone(),
        );
        evidence.delayed_title.store(true, Ordering::SeqCst);
        let first = submit(&s, start("A", "a"), dir.path()).await;
        let refreshed = s.observe(first.execution_id.clone(), false).await.unwrap();
        assert_eq!(refreshed.thread_name.as_deref(), Some("turn-0 cleanup"));
        evidence.delayed_title.store(false, Ordering::SeqCst);
        let next = submit(&s, continuation(&first.execution_id, "next"), dir.path()).await;
        assert_eq!(next.runtime_instance_id, first.runtime_instance_id);
        assert_eq!(next.thread_name.as_deref(), Some("turn-1"));
        s.shutdown().await.unwrap();
    });
}

#[test]
fn cold_continuation_uses_new_runtime_initialize_resume_and_no_start() {
    run(async {
        let dir = tempfile::tempdir().unwrap();
        let store = StateStore::open(dir.path().into()).await.unwrap();
        let evidence = Arc::new(Evidence::default());
        let s = service(
            store.clone(),
            dir.path().join("agent-state.db"),
            evidence.clone(),
        );
        let first = submit(&s, start("A", "a"), dir.path()).await;
        s.shutdown().await.unwrap();
        let s = service(
            store.clone(),
            dir.path().join("agent-state.db"),
            evidence.clone(),
        );
        let next = submit(&s, continuation(&first.execution_id, "next"), dir.path()).await;
        assert_ne!(next.runtime_instance_id, first.runtime_instance_id);
        assert_eq!(next.thread_id, first.thread_id);
        assert_eq!(
            counts(&evidence, next.runtime_instance_id.as_ref().unwrap()),
            [1, 0, 1, 1]
        );
        s.shutdown().await.unwrap();
    });
}

#[test]
fn cancelled_turn_is_reusable_but_cleanup_uncertainty_is_not() {
    run(async {
        let dir = tempfile::tempdir().unwrap();
        let store = StateStore::open(dir.path().into()).await.unwrap();
        let evidence = Arc::new(Evidence::default());
        let s = service(
            store.clone(),
            dir.path().join("agent-state.db"),
            evidence.clone(),
        );
        evidence.hold_turn.store(true, Ordering::SeqCst);
        let receipt = s
            .checked_operation(start("A", "a"), w(dir.path(), "W"))
            .await;
        let id = receipt["data"]["executionId"].as_str().unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            while store
                .execution(id.into())
                .await
                .unwrap()
                .unwrap()
                .tool_category
                .is_none()
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        s.checked_operation(json!({"action":"cancel","executionId":id}), None)
            .await;
        let first = final_row(&s, id).await;
        drop(
            s.manager
                .runtime_pool
                .lease(&s.store, &first.canonical_workspace_root)
                .await,
        );
        assert_eq!(first.status, "cancelled");
        evidence.hold_turn.store(false, Ordering::SeqCst);
        let next = submit(&s, continuation(id, "next"), dir.path()).await;
        assert_eq!(next.runtime_instance_id, first.runtime_instance_id);
        evidence.fail_cleanup.store(true, Ordering::SeqCst);
        let receipt = s
            .checked_operation(continuation(&next.execution_id, "uncertain"), None)
            .await;
        let uncertain = receipt["data"]["executionId"].as_str().unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let row = store.execution(uncertain.into()).await.unwrap().unwrap();
                if row.status == "unknown" {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let lease = s
            .manager
            .runtime_pool
            .lease(&s.store, &first.canonical_workspace_root)
            .await.unwrap();
        assert!(lease.is_none());
        drop(lease);
        assert!(
            store
                .workspace_claim(first.canonical_workspace_root)
                .await
                .unwrap()
                .is_some()
        );
        assert_eq!(
            counts(&evidence, first.runtime_instance_id.as_ref().unwrap()),
            [1, 1, 0, 3]
        );
        s.shutdown().await.unwrap();
    });
}

#[test]
fn shutdown_cancels_active_provider_and_awaits_monitor() {
    run(async {
        let dir = tempfile::tempdir().unwrap();
        let store = StateStore::open(dir.path().into()).await.unwrap();
        let evidence = Arc::new(Evidence::default());
        let s = service(
            store.clone(),
            dir.path().join("agent-state.db"),
            evidence.clone(),
        );
        evidence.hold_turn.store(true, Ordering::SeqCst);
        let receipt = s
            .checked_operation(start("A", "a"), w(dir.path(), "W"))
            .await;
        let id = receipt["data"]["executionId"].as_str().unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            while store
                .execution(id.into())
                .await
                .unwrap()
                .unwrap()
                .tool_category
                .is_none()
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        tokio::time::timeout(Duration::from_secs(5), s.shutdown())
            .await
            .unwrap()
            .unwrap();
        assert!(s.manager.runtime_pool.is_empty());
        let row = store.execution(id.into()).await.unwrap().unwrap();
        assert_eq!(row.status, "unknown"); // Fake recovery executable deliberately unavailable.
        let runtime = row.runtime_instance_id.unwrap();
        assert_eq!(evidence.shutdowns.lock().unwrap()[&runtime], 1);
        assert_eq!(
            store
                .runtime(runtime)
                .await
                .unwrap()
                .unwrap()
                .termination_evidence_state,
            "complete"
        );
    });
}

#[test]
fn warm_two_and_four_turns_release_claim_and_isolate_execution_state() {
    run(async {
        for count in [2, 4] {
            let dir = tempfile::tempdir().unwrap();
            let store = StateStore::open(dir.path().into()).await.unwrap();
            let evidence = Arc::new(Evidence::default());
            let s = service(
                store.clone(),
                dir.path().join("agent-state.db"),
                evidence.clone(),
            );
            let mut views = Vec::new();
            let mut first_snapshot = None;
            for index in 0..count {
                let req = if index == 0 {
                    start("lineage", "start")
                } else {
                    continuation(
                        &views
                            .last()
                            .map(|v: &ExecutionView| v.execution_id.clone())
                            .unwrap(),
                        &format!("key-{index}"),
                    )
                };
                let view = submit(&s, req, dir.path()).await;
                assert_eq!(view.status, "completed");
                assert!(view.available_actions.can_continue);
                assert_eq!(view.thread_name, view.turn_id);
                let row = store
                    .execution(view.execution_id.clone())
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(row.tool_category.as_deref(), Some("test"));
                assert_eq!(
                    row.release_evidence_kind.as_deref(),
                    Some("same_runtime_cleanup")
                );
                assert!(
                    store
                        .workspace_claim(row.canonical_workspace_root.clone())
                        .await
                        .unwrap()
                        .is_none()
                );
                assert_eq!(
                    store
                        .runtime(row.runtime_instance_id.clone().unwrap())
                        .await
                        .unwrap()
                        .unwrap()
                        .state,
                    "running"
                );
                if index == 0 {
                    first_snapshot = Some(row);
                } else {
                    assert_eq!(
                        store
                            .execution(views[0].execution_id.clone())
                            .await
                            .unwrap()
                            .as_ref(),
                        first_snapshot.as_ref()
                    );
                }
                views.push(view);
            }
            assert_eq!(
                views
                    .iter()
                    .map(|v| &v.execution_id)
                    .collect::<HashSet<_>>()
                    .len(),
                count
            );
            assert_eq!(
                views
                    .iter()
                    .map(|v| &v.turn_id)
                    .collect::<HashSet<_>>()
                    .len(),
                count
            );
            assert_eq!(
                views
                    .iter()
                    .map(|v| &v.thread_id)
                    .collect::<HashSet<_>>()
                    .len(),
                1
            );
            assert_eq!(
                views
                    .iter()
                    .map(|v| &v.runtime_instance_id)
                    .collect::<HashSet<_>>()
                    .len(),
                1
            );
            let runtime = evidence.launches.lock().unwrap()[0].clone();
            assert_eq!(evidence.launches.lock().unwrap().len(), 1);
            assert_eq!(counts(&evidence, &runtime), [1, 1, 0, count]);
            s.shutdown().await.unwrap();
            s.shutdown().await.unwrap();
            assert!(s.manager.runtime_pool.is_empty());
            assert_eq!(evidence.shutdowns.lock().unwrap()[&runtime], 1);
            assert_eq!(
                store
                    .runtime(runtime)
                    .await
                    .unwrap()
                    .unwrap()
                    .termination_evidence_state,
                "complete"
            );
        }
    });
}

#[test]
fn cold_resume_and_unloaded_thread_in_existing_workspace_runtime() {
    run(async {
        let dir = tempfile::tempdir().unwrap();
        let store = StateStore::open(dir.path().into()).await.unwrap();
        let evidence = Arc::new(Evidence::default());
        let s = service(
            store.clone(),
            dir.path().join("agent-state.db"),
            evidence.clone(),
        );
        let first = submit(&s, start("A", "a"), dir.path()).await;
        s.shutdown().await.unwrap();
        let s = service(
            store.clone(),
            dir.path().join("agent-state.db"),
            evidence.clone(),
        );
        let fresh = submit(&s, start("B", "b"), dir.path()).await;
        assert_ne!(fresh.thread_id, first.thread_id);
        let next = submit(&s, continuation(&first.execution_id, "next"), dir.path()).await;
        assert_eq!(next.thread_id, first.thread_id);
        assert_eq!(next.runtime_instance_id, fresh.runtime_instance_id);
        assert_ne!(next.runtime_instance_id, first.runtime_instance_id);
        assert_eq!(
            counts(&evidence, next.runtime_instance_id.as_ref().unwrap()),
            [1, 1, 1, 2]
        );
        let third = submit(&s, continuation(&next.execution_id, "third"), dir.path()).await;
        assert_eq!(
            counts(&evidence, third.runtime_instance_id.as_ref().unwrap()),
            [1, 1, 1, 3]
        );
        s.shutdown().await.unwrap();
    });
}

#[test]
fn idle_transport_failure_invalidates_without_replay_and_next_legal_continue_resumes() {
    run(async {
        let dir = tempfile::tempdir().unwrap();
        let store = StateStore::open(dir.path().into()).await.unwrap();
        let evidence = Arc::new(Evidence::default());
        let s = service(
            store.clone(),
            dir.path().join("agent-state.db"),
            evidence.clone(),
        );
        let first = submit(&s, start("A", "a"), dir.path()).await;
        {
            let lease = s
                .manager
                .runtime_pool
                .lease(&s.store, &first.canonical_workspace_root)
                .await.unwrap();
            lease.as_ref().unwrap().client.cancel();
        }
        assert_eq!(
            counts(&evidence, first.runtime_instance_id.as_ref().unwrap()),
            [1, 1, 0, 1]
        );
        let next = submit(&s, continuation(&first.execution_id, "new"), dir.path()).await;
        assert_ne!(next.runtime_instance_id, first.runtime_instance_id);
        assert_eq!(next.thread_id, first.thread_id);
        assert_eq!(
            counts(&evidence, next.runtime_instance_id.as_ref().unwrap()),
            [1, 0, 1, 1]
        );
        s.shutdown().await.unwrap();
    });
}

#[test]
fn shutdown_drains_two_workspaces_and_rejects_new_admission() {
    run(async {
        let dir = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let store = StateStore::open(dir.path().into()).await.unwrap();
        let evidence = Arc::new(Evidence::default());
        let s = service(
            store.clone(),
            dir.path().join("agent-state.db"),
            evidence.clone(),
        );
        submit(&s, start("A", "a"), dir.path()).await;
        submit(&s, start("B", "b"), second.path()).await;
        s.shutdown().await.unwrap();
        s.shutdown().await.unwrap();
        assert!(s.manager.runtime_pool.is_empty());
        assert_eq!(evidence.shutdowns.lock().unwrap().len(), 2);
        assert!(
            evidence
                .shutdowns
                .lock()
                .unwrap()
                .values()
                .all(|count| *count == 1)
        );
        let result = s
            .checked_operation(start("C", "c"), w(dir.path(), "W"))
            .await;
        assert_eq!(result["ok"], false);
        assert_eq!(evidence.launches.lock().unwrap().len(), 2);
    });
}

#[test]
fn pool_serializes_same_workspace_and_shutdown_waits_for_owned_worker() {
    run(async {
        let dir = tempfile::tempdir().unwrap();
        let store = StateStore::open(dir.path().into()).await.unwrap();
        let pool = Arc::new(CodexRuntimePool::default());
        let worker = pool.enter().await.unwrap();
        let lease = pool.lease(&store, "W").await;
        assert!(
            tokio::time::timeout(Duration::from_millis(20), pool.lease(&store, "W"))
                .await
                .is_err()
        );
        let shutdown = tokio::spawn({
            let pool = pool.clone();
            async move { pool.shutdown().await }
        });
        tokio::task::yield_now().await;
        assert!(!shutdown.is_finished());
        drop(lease);
        drop(worker);
        shutdown.await.unwrap().unwrap();
        assert!(pool.enter().await.is_err());
    });
}

#[test]
fn termination_failure_quarantines_only_its_workspace_and_recovery_allows_cold_resume() {
    run(async {
        let dir=tempfile::tempdir().unwrap(); let a=dir.path().join("A"); let b=dir.path().join("B");
        std::fs::create_dir(&a).unwrap(); std::fs::create_dir(&b).unwrap();
        let store=StateStore::open(dir.path().into()).await.unwrap(); let evidence=Arc::new(Evidence::default());
        let s=service(store.clone(),dir.path().join("agent-state.db"),evidence.clone());
        let first=submit(&s,start("A","a1"),&a).await;
        let runtime=first.runtime_instance_id.as_ref().unwrap();
        evidence.fail_shutdown.store(true,Ordering::SeqCst);
        { let lease=s.manager.runtime_pool.lease(&store,&first.canonical_workspace_root).await.unwrap(); lease.as_ref().unwrap().client.cancel(); }
        let blocked=s.checked_operation(continuation(&first.execution_id,"a2"),None).await;
        assert_eq!(blocked["ok"],false,"{blocked}");
        assert_eq!(evidence.launches.lock().unwrap().len(),1);
        assert!(s.manager.runtime_pool.retains_runtime(&first.canonical_workspace_root,runtime));
        let pending=store.product_read(None,Some("A".into()),None,10).await.unwrap().into_iter().find(|r|r.execution.id!=first.execution_id).unwrap().execution;
        assert!(pending.runtime_instance_id.is_none());
        assert_eq!(blocked["error"]["executionId"], pending.id);
        assert_eq!(blocked["control"]["requestAccepted"], true);
        assert_eq!(blocked["control"]["nextAction"]["action"], "manual_resolution");
        let retried=s.checked_operation(continuation(&first.execution_id,"a2"),None).await;
        assert_eq!(retried["data"]["executionId"], pending.id);
        assert_eq!(retried["control"]["nextAction"]["action"], "manual_resolution");
        let view=s.observe(pending.id.clone(),false).await.unwrap();
        assert!(!view.available_actions.can_resume_pending);
        assert!(view.available_actions.can_cancel);
        assert_eq!(view.attention, "manual_resolution_required");
        let resumed=s.checked_operation(json!({"action":"resume_pending","executionId":pending.id}),None).await;
        assert_eq!(resumed["ok"],false);
        assert_eq!(resumed["error"]["code"], "AGENT_RUNTIME_QUARANTINED");
        assert_eq!(resumed["error"]["executionId"], pending.id);
        assert_eq!(resumed["control"]["requestAccepted"], true);
        assert_eq!(resumed["control"]["providerInvoked"], false);
        assert_eq!(resumed["control"]["dispatchCertainty"], "not_dispatched");
        assert_eq!(resumed["control"]["nextAction"], json!({"action":"manual_resolution","executionId":pending.id}));
        assert_eq!(store.execution(pending.id.clone()).await.unwrap().unwrap(), pending);
        assert_eq!(store.product_read(None,Some("A".into()),None,10).await.unwrap().len(), 2);
        let new_start=s.checked_operation(start("A-fresh","blocked"),w(&a,"W")).await;
        assert_eq!(new_start["ok"],false);
        assert_eq!(evidence.launches.lock().unwrap().len(),1);
        let other=submit(&s,start("B","b1"),&b).await;
        let other2=submit(&s,continuation(&other.execution_id,"b2"),&b).await;
        assert_eq!(other.runtime_instance_id,other2.runtime_instance_id);
        assert_eq!(evidence.launches.lock().unwrap().len(),2);
        // The fake monitor's first termination lacked evidence. Even EOF/dropped
        // Client did not admit a replacement. Supply its successful retry evidence.
        let db=rusqlite::Connection::open(dir.path().join("agent-state.db")).unwrap();
        db.execute("UPDATE runtime_instances SET state='terminated',termination_evidence_state='complete',termination_evidence_type='job_active_processes_zero',termination_evidence_at=20 WHERE id=?1",[runtime]).unwrap();
        s.manager.runtime_pool.retry_workspace(&store,&first.canonical_workspace_root).await.unwrap();
        s.manager.runtime_pool.check_workspace(&first.canonical_workspace_root).unwrap();
        let receipt=s.checked_operation(json!({"action":"resume_pending","executionId":pending.id}),None).await;
        assert_eq!(receipt["ok"],true,"{receipt}");
        let final_view=final_row(&s,&pending.id).await;
        assert_eq!(final_view.thread_id,first.thread_id);
        assert_ne!(final_view.runtime_instance_id,first.runtime_instance_id);
        assert_eq!(counts(&evidence,final_view.runtime_instance_id.as_ref().unwrap()),[1,0,1,1]);
        assert_eq!(evidence.launches.lock().unwrap().len(),3);
        s.shutdown().await.unwrap();
    });
}
