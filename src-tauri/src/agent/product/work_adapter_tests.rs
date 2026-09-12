use super::*;
use crate::agent::store::transactions::product::WorkExecutionContext;

#[path = "work_context_tests.rs"]
mod work_context_tests;

fn start_work(work: &str, key: &str) -> AgentExecuteAction {
    AgentExecuteAction::Start {
        work_run_id: work.into(),
        request_key: key.into(),
        prompt: "hello".into(),
        delegation_context_json: None,
    }
}
fn continue_work(work: &str, parent: &str, key: &str) -> AgentExecuteAction {
    AgentExecuteAction::Continue {
        work_run_id: work.into(),
        parent_execution_id: parent.into(),
        request_key: key.into(),
        prompt: "next".into(),
        delegation_context_json: None,
    }
}
async fn create_work(store: &StateStore, root: &std::path::Path, id: &str) {
    store
        .create_work_run(
            id.into(),
            "W".into(),
            root.to_string_lossy().into(),
            "title".into(),
            None,
            1,
        )
        .await
        .unwrap();
}
async fn pending_work(
    store: &StateStore,
    root: &std::path::Path,
    id: &str,
    agent: &str,
    work: &str,
    time: i64,
) {
    store
        .product_create_fresh_with_work(
            id.into(),
            agent.into(),
            "key".into(),
            "hello".into(),
            "W".into(),
            w(root, "W"),
            Some(WorkExecutionContext {
                work_run_id: work.into(),
                parent_execution_id: None,
                delegation_context_json: None,
            }),
            time,
        )
        .await
        .unwrap();
}
fn view(data: ProductData) -> ExecutionView {
    match data {
        ProductData::Execution(v) => *v,
        _ => panic!("expected execution"),
    }
}
fn list(data: ProductData) -> Vec<ExecutionView> {
    match data {
        ProductData::List { executions } => executions,
        _ => panic!("expected list"),
    }
}

#[tokio::test]
async fn start_durable_receipt_retry_lineage_and_continuation_reuse_existing_worker() {
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::open(dir.path().into()).await.unwrap();
    create_work(&store, dir.path(), "work").await;
    let (s, release, fake) = fake_service(
        store.clone(),
        dir.path().join("agent-state.db"),
        "ADAPTER1",
        "T1",
        false,
        "paginated",
    )
    .await;
    // Fake turn/start cannot finish until release is sent; receipt must precede it.
    let first = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(
            s.agent_execute(start_work("work", "key"), w(dir.path(), "W")),
            s.agent_execute(start_work("work", "key"), w(dir.path(), "W"))
        )
    })
    .await
    .unwrap();
    let (first, retry) = (first.0.unwrap(), first.1.unwrap());
    assert_eq!(retry.execution_id, first.execution_id);
    let id = first.execution_id.clone();
    assert_eq!(first.agent_id, "work");
    assert_eq!(first.prompt, "hello");
    assert_ne!(first.status, "completed");
    let link = store
        .work_execution_link(id.clone())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(link.work_run_id, "work");
    assert_eq!(link.parent_execution_id, None);
    assert_eq!(link.delegation_context_json.as_deref(), None);
    for current in [None, w(dir.path(), "wrong")] {
        assert_eq!(
            s.agent_execute(start_work("work", "key"), current)
                .await
                .unwrap()
                .execution_id,
            id
        );
    }
    assert_eq!(
        s.agent_execute(start_work("work", "new-key"), w(dir.path(), "W"))
            .await
            .unwrap_err()
            .code,
        "AGENT_LINEAGE_CONFLICT"
    );
    assert_eq!(
        store
            .work_execution_links("work".into())
            .await
            .unwrap()
            .len(),
        1
    );
    release.send(()).unwrap();
    let terminal = final_row(&s, &id).await;
    let parent = store.execution(id.clone()).await.unwrap().unwrap();
    assert_eq!(terminal.status, "completed");
    drop(s);
    let methods = fake.await.unwrap();
    assert_eq!(methods.iter().filter(|m| *m == "turn/start").count(), 1);

    let (s, release, fake) = fake_service(
        store.clone(),
        dir.path().join("agent-state.db"),
        "ADAPTER2",
        "T2",
        true,
        "paginated",
    )
    .await;
    let (next, retry) = tokio::join!(
        s.agent_execute(continue_work("work", &id, "next"), None),
        s.agent_execute(continue_work("work", &id, "next"), None)
    );
    let next = next.unwrap();
    assert_eq!(next.prompt, "next");
    assert_eq!(retry.unwrap().execution_id, next.execution_id);
    assert_ne!(next.execution_id, id);
    assert_eq!(next.thread_id, terminal.thread_id);
    assert_eq!(
        s.agent_execute(continue_work("work", &id, "other-key"), None)
            .await
            .unwrap_err()
            .code,
        "AGENT_BUSY"
    );
    assert_eq!(
        s.agent_execute(continue_work("work", &id, "next"), None)
            .await
            .unwrap()
            .execution_id,
        next.execution_id
    );
    assert_eq!(
        store.execution(id.clone()).await.unwrap(),
        Some(parent.clone())
    );
    assert_eq!(
        store
            .work_execution_link(next.execution_id.clone())
            .await
            .unwrap()
            .unwrap()
            .parent_execution_id,
        Some(id.clone())
    );
    assert_eq!(
        store
            .work_execution_links("work".into())
            .await
            .unwrap()
            .len(),
        2
    );
    release.send(()).unwrap();
    final_row(&s, &next.execution_id).await;
    assert_eq!(store.execution(id).await.unwrap(), Some(parent));
    let record = store.execution(next.execution_id.clone()).await.unwrap();
    let mut results = Vec::new();
    for _ in 0..2 {
        results.push(
            view(
                s.agent_query(AgentQueryAction::Get {
                    execution_id: next.execution_id.clone(),
                    include_result: Some(true),
                })
                .await
                .unwrap(),
            )
            .final_result,
        );
        let observed = view(
            s.agent_query(AgentQueryAction::Observe {
                execution_id: next.execution_id.clone(),
                known_revision: None,
                wait_ms: Some(0),
                include_result: Some(true),
            })
            .await
            .unwrap(),
        );
        assert_eq!(observed.final_result, results[0]);
    }
    assert!(results[0].is_some());
    assert_eq!(results[0], results[1]);
    assert!(
        view(
            s.agent_query(AgentQueryAction::Get {
                execution_id: next.execution_id.clone(),
                include_result: None
            })
            .await
            .unwrap()
        )
        .final_result
        .is_none()
    );
    assert_eq!(store.execution(next.execution_id).await.unwrap(), record);
    drop(s);
    let methods = fake.await.unwrap();
    assert_eq!(methods.iter().filter(|m| *m == "turn/start").count(), 1);
    assert_eq!(methods.iter().filter(|m| *m == "thread/resume").count(), 1);
    assert!(!methods.iter().any(|m| m == "thread/start"));
}

#[tokio::test]
async fn wrong_work_guards_are_side_effect_free_and_cancel_can_converge_inactive_work() {
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::open(dir.path().into()).await.unwrap();
    create_work(&store, dir.path(), "work").await;
    create_work(&store, dir.path(), "wrong").await;
    pending_work(&store, dir.path(), "E", "work", "work", 1).await;
    let s = AgentProductService::new(store.clone());
    let before = store.execution("E".into()).await.unwrap();
    let claim = store
        .workspace_claim(dir.path().to_string_lossy().into())
        .await
        .unwrap()
        .unwrap();
    for action in [
        continue_work("wrong", "E", "next"),
        AgentExecuteAction::Cancel {
            work_run_id: "wrong".into(),
            execution_id: "E".into(),
        },
        AgentExecuteAction::ResumePending {
            work_run_id: "wrong".into(),
            execution_id: "E".into(),
        },
    ] {
        assert_eq!(
            s.agent_execute(action, None).await.unwrap_err().code,
            "EXECUTION_NOT_IN_WORK"
        );
        assert_eq!(store.execution("E".into()).await.unwrap(), before);
        assert_eq!(
            store
                .workspace_claim(dir.path().to_string_lossy().into())
                .await
                .unwrap()
                .unwrap()
                .execution_id,
            claim.execution_id
        );
        assert!(!store.product_worker_owned("E"));
        assert_eq!(
            store
                .work_execution_links("work".into())
                .await
                .unwrap()
                .len(),
            1
        );
    }
    let db = rusqlite::Connection::open(dir.path().join("agent-state.db")).unwrap();
    db.execute("UPDATE work_runs SET status='failed' WHERE id='work'", [])
        .unwrap();
    assert_eq!(
        s.agent_execute(
            AgentExecuteAction::ResumePending {
                work_run_id: "work".into(),
                execution_id: "E".into()
            },
            None
        )
        .await
        .unwrap_err()
        .code,
        "WORK_NOT_ACTIVE"
    );
    assert_eq!(store.execution("E".into()).await.unwrap(), before);
    let cancelled = s
        .agent_execute(
            AgentExecuteAction::Cancel {
                work_run_id: "work".into(),
                execution_id: "E".into(),
            },
            None,
        )
        .await
        .unwrap();
    assert_eq!(cancelled.status, "cancelled");
    assert!(
        store
            .workspace_claim(dir.path().to_string_lossy().into())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        store
            .work_execution_links("work".into())
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn resume_pending_uses_existing_explicit_pipeline_and_rejects_replay() {
    for state in ["dispatching", "dispatched", "uncertain", "attempted"] {
        let dir = tempfile::tempdir().unwrap();
        let store = StateStore::open(dir.path().into()).await.unwrap();
        create_work(&store, dir.path(), "work").await;
        pending_work(&store, dir.path(), "E", "work", "work", 1).await;
        let db = rusqlite::Connection::open(dir.path().join("agent-state.db")).unwrap();
        if state == "attempted" {
            db.execute(
                "INSERT INTO execution_runtime_attempts VALUES ('E','attempt',1)",
                [],
            )
            .unwrap();
        } else {
            db.execute(
                "UPDATE executions SET dispatch_state=?1 WHERE id='E'",
                [state],
            )
            .unwrap();
        }
        let before = store.execution("E".into()).await.unwrap();
        let (s, _release, fake) = fake_service(
            store.clone(),
            dir.path().join("agent-state.db"),
            "REJECT_RESUME",
            "T",
            false,
            "paginated",
        )
        .await;
        assert_eq!(
            s.agent_execute(
                AgentExecuteAction::ResumePending {
                    work_run_id: "work".into(),
                    execution_id: "E".into()
                },
                None
            )
            .await
            .unwrap_err()
            .code,
            "AGENT_RESUME_NOT_ALLOWED"
        );
        assert_eq!(store.execution("E".into()).await.unwrap(), before);
        assert!(!store.product_worker_owned("E"));
        drop(s);
        assert!(
            fake.await.unwrap().is_empty(),
            "Provider invoked for {state}"
        );
    }
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::open(dir.path().into()).await.unwrap();
    create_work(&store, dir.path(), "work").await;
    pending_work(&store, dir.path(), "E", "work", "work", 1).await;
    let (s, release, fake) = fake_service(
        store.clone(),
        dir.path().join("agent-state.db"),
        "RESUME_ADAPTER",
        "T",
        false,
        "paginated",
    )
    .await;
    assert_eq!(
        s.agent_execute(
            AgentExecuteAction::ResumePending {
                work_run_id: "work".into(),
                execution_id: "E".into()
            },
            None
        )
        .await
        .unwrap()
        .execution_id,
        "E"
    );
    release.send(()).unwrap();
    final_row(&s, "E").await;
    drop(s);
    let methods = fake.await.unwrap();
    assert_eq!(methods.iter().filter(|m| *m == "turn/start").count(), 1);
    assert_eq!(
        store
            .work_execution_links("work".into())
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn query_exact_get_observe_timeout_and_change_are_read_only() {
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::open(dir.path().into()).await.unwrap();
    create_work(&store, dir.path(), "work").await;
    pending_work(&store, dir.path(), "E", "work", "work", 1).await;
    let s = AgentProductService::new(store.clone());
    let before = store.execution("E".into()).await.unwrap().unwrap();
    let initial = view(
        s.agent_query(AgentQueryAction::Get {
            execution_id: "E".into(),
            include_result: None,
        })
        .await
        .unwrap(),
    );
    assert_eq!(initial.revision, initial.control_revision);
    assert_ne!(initial.control_revision, before.revision.to_string());
    let started = Instant::now();
    let same = view(
        s.agent_query(AgentQueryAction::Observe {
            execution_id: "E".into(),
            known_revision: Some(initial.control_revision.clone()),
            wait_ms: Some(80),
            include_result: None,
        })
        .await
        .unwrap(),
    );
    assert!(started.elapsed() >= Duration::from_millis(80));
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(same.unchanged, Some(true));
    assert_eq!(store.execution("E".into()).await.unwrap(), Some(before));
    for wait in [Some(20_000), None] {
        let immediate = tokio::time::timeout(
            Duration::from_secs(1),
            s.agent_query(AgentQueryAction::Observe {
                execution_id: "E".into(),
                known_revision: Some("different-opaque-token".into()),
                wait_ms: wait,
                include_result: None,
            }),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(view(immediate).unchanged, Some(false));
    }
    let waiter = s.agent_query(AgentQueryAction::Observe {
        execution_id: "E".into(),
        known_revision: Some(initial.control_revision),
        wait_ms: Some(20_000),
        include_result: None,
    });
    let change = async {
        tokio::time::sleep(Duration::from_millis(60)).await;
        store.request_cancel("E".into(), 2).await.unwrap();
    };
    let started = Instant::now();
    let (result, ()) = tokio::join!(waiter, change);
    assert_eq!(view(result.unwrap()).status, "cancelled");
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(
        store
            .execution("E".into())
            .await
            .unwrap()
            .unwrap()
            .runtime_instance_id,
        None
    );
    assert_eq!(
        s.agent_query(AgentQueryAction::Get {
            execution_id: "missing".into(),
            include_result: None
        })
        .await
        .unwrap_err()
        .code,
        "AGENT_EXECUTION_NOT_FOUND"
    );
}

#[tokio::test(start_paused = true)]
async fn observe_default_wait_is_fifteen_seconds() {
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::open(dir.path().into()).await.unwrap();
    create_work(&store, dir.path(), "work").await;
    pending_work(&store, dir.path(), "E", "work", "work", 1).await;
    let s = AgentProductService::new(store.clone());
    let started = Instant::now();
    s.agent_query(AgentQueryAction::Observe {
        execution_id: "E".into(),
        known_revision: None,
        wait_ms: None,
        include_result: None,
    })
    .await
    .unwrap();
    assert_eq!(started.elapsed(), Duration::from_secs(15));
    assert_eq!(
        store
            .execution("E".into())
            .await
            .unwrap()
            .unwrap()
            .runtime_instance_id,
        None
    );
}

#[tokio::test]
async fn query_list_uses_links_not_agent_identity_and_enforces_limits() {
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::open(dir.path().into()).await.unwrap();
    create_work(&store, dir.path(), "work").await;
    create_work(&store, dir.path(), "other").await;
    pending_work(&store, dir.path(), "foreign", "foreign-agent", "other", 5).await;
    store.request_cancel("foreign".into(), 6).await.unwrap();
    for i in (0..21).rev() {
        let id = format!("e{i:02}");
        pending_work(&store, dir.path(), &id, &id, "work", 10).await;
        store.request_cancel(id, 20).await.unwrap();
    }
    // Historical unlinked execution has the Work's agent id; membership still excludes it.
    store
        .product_create_fresh(
            "legacy".into(),
            "work".into(),
            "key".into(),
            "hello".into(),
            "W".into(),
            w(dir.path(), "W"),
            30,
        )
        .await
        .unwrap();
    let s = AgentProductService::new(store.clone());
    let before = store
        .product_read(None, None, None, 100)
        .await
        .unwrap()
        .into_iter()
        .map(|r| r.execution)
        .collect::<Vec<_>>();
    for (limit, count) in [(None, 20), (Some(1), 1), (Some(100), 21)] {
        let rows = list(
            s.agent_query(AgentQueryAction::List {
                work_run_id: "work".into(),
                limit,
            })
            .await
            .unwrap(),
        );
        assert_eq!(rows.len(), count);
        assert_eq!(
            rows.iter()
                .map(|r| r.execution_id.clone())
                .collect::<Vec<_>>(),
            (0..count).map(|i| format!("e{i:02}")).collect::<Vec<_>>()
        );
        assert!(rows.iter().all(|r| r.agent_id != "work"));
    }
    let other = list(
        s.agent_query(AgentQueryAction::List {
            work_run_id: "other".into(),
            limit: None,
        })
        .await
        .unwrap(),
    );
    assert_eq!(other.len(), 1);
    assert_eq!(other[0].execution_id, "foreign");
    assert_eq!(
        s.agent_query(AgentQueryAction::List {
            work_run_id: "missing".into(),
            limit: None
        })
        .await
        .unwrap_err()
        .code,
        "WORK_NOT_FOUND"
    );
    for limit in [0, 101, u32::MAX] {
        assert_eq!(
            s.agent_query(AgentQueryAction::List {
                work_run_id: "work".into(),
                limit: Some(limit)
            })
            .await
            .unwrap_err()
            .code,
            "WORK_INVALID_ARGUMENT"
        );
    }
    let after = store
        .product_read(None, None, None, 100)
        .await
        .unwrap()
        .into_iter()
        .map(|r| r.execution)
        .collect::<Vec<_>>();
    assert_eq!(before, after);
}

#[tokio::test]
async fn adapter_validation_and_start_work_guards_create_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::open(dir.path().into()).await.unwrap();
    create_work(&store, dir.path(), "work").await;
    let (s, _release, fake) = fake_service(
        store.clone(),
        dir.path().join("agent-state.db"),
        "REJECT_START",
        "T",
        false,
        "paginated",
    )
    .await;
    assert_eq!(
        s.agent_execute(start_work("missing", "key"), None)
            .await
            .unwrap_err()
            .code,
        "WORK_NOT_FOUND"
    );
    for current in [
        None,
        w(dir.path(), "wrong"),
        w(&dir.path().join("other-root"), "W"),
    ] {
        assert_eq!(
            s.agent_execute(start_work("work", "key"), current)
                .await
                .unwrap_err()
                .code,
            "WORKSPACE_CONTEXT_MISMATCH"
        );
    }
    for action in [
        start_work(" ", "key"),
        start_work("work", " "),
        AgentExecuteAction::Start {
            work_run_id: "work".into(),
            request_key: "key".into(),
            prompt: " \n".into(),
            delegation_context_json: None,
        },
        continue_work("work", " ", "key"),
        AgentExecuteAction::Cancel {
            work_run_id: "work".into(),
            execution_id: "".into(),
        },
        AgentExecuteAction::ResumePending {
            work_run_id: "".into(),
            execution_id: "E".into(),
        },
    ] {
        assert_eq!(
            s.agent_execute(action, None).await.unwrap_err().code,
            "WORK_INVALID_ARGUMENT"
        );
    }
    for action in [
        AgentQueryAction::Get {
            execution_id: " ".into(),
            include_result: None,
        },
        AgentQueryAction::List {
            work_run_id: "".into(),
            limit: None,
        },
        AgentQueryAction::Observe {
            execution_id: "E".into(),
            known_revision: None,
            wait_ms: Some(20_001),
            include_result: None,
        },
        AgentQueryAction::Observe {
            execution_id: "E".into(),
            known_revision: Some(" ".into()),
            wait_ms: None,
            include_result: None,
        },
    ] {
        assert_eq!(
            s.agent_query(action).await.unwrap_err().code,
            "WORK_INVALID_ARGUMENT"
        );
    }
    let db = rusqlite::Connection::open(dir.path().join("agent-state.db")).unwrap();
    for status in ["completed", "failed", "cancelled"] {
        db.execute("UPDATE work_runs SET status=?1", [status])
            .unwrap();
        assert_eq!(
            s.agent_execute(start_work("work", "key"), w(dir.path(), "W"))
                .await
                .unwrap_err()
                .code,
            "WORK_NOT_ACTIVE"
        );
    }
    assert!(
        store
            .product_read(None, None, None, 100)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        store
            .work_execution_links("work".into())
            .await
            .unwrap()
            .is_empty()
    );
    drop(s);
    assert!(fake.await.unwrap().is_empty());
}

#[tokio::test]
async fn dropping_adapter_caller_and_observer_keeps_owned_execution_running() {
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::open(dir.path().into()).await.unwrap();
    create_work(&store, dir.path(), "work").await;
    let (mut s, release, fake) = fake_service(
        store.clone(),
        dir.path().join("agent-state.db"),
        "ADAPTER_DROP",
        "T",
        false,
        "paginated",
    )
    .await;
    let hook = Arc::new((tokio::sync::Notify::new(), tokio::sync::Notify::new()));
    s.manager.test_handoff = Some(hook.clone());
    let s = Arc::new(s);
    let caller = s.clone();
    let current = w(dir.path(), "W");
    let request = tokio::spawn(async move {
        caller
            .agent_execute(start_work("work", "key"), current)
            .await
    });
    tokio::time::timeout(Duration::from_secs(5), hook.0.notified())
        .await
        .unwrap();
    request.abort();
    assert!(request.await.unwrap_err().is_cancelled());
    let links = store.work_execution_links("work".into()).await.unwrap();
    assert_eq!(links.len(), 1);
    let id = links[0].execution_id.clone();
    let row = store.execution(id.clone()).await.unwrap().unwrap();
    let observer = s.agent_query(AgentQueryAction::Observe {
        execution_id: id.clone(),
        known_revision: None,
        wait_ms: Some(20_000),
        include_result: None,
    });
    assert!(
        tokio::time::timeout(Duration::from_millis(80), observer)
            .await
            .is_err()
    );
    assert_eq!(store.execution(id.clone()).await.unwrap(), Some(row));
    hook.1.notify_one();
    release.send(()).unwrap();
    assert_eq!(final_row(&s, &id).await.status, "completed");
    drop(s);
    let methods = fake.await.unwrap();
    assert_eq!(methods.iter().filter(|m| *m == "turn/start").count(), 1);
}

#[tokio::test]
async fn cancel_projection_failure_preserves_committed_execution_identity() {
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::open(dir.path().into()).await.unwrap();
    create_work(&store, dir.path(), "work").await;
    pending_work(&store, dir.path(), "E", "work", "work", 1).await;
    let db = rusqlite::Connection::open(dir.path().join("agent-state.db")).unwrap();
    // Valid SQL field values, but an invalid Activity combination for the view.
    db.execute("UPDATE executions SET last_activity_at=1, activity_phase='tool', tool_category=NULL WHERE id='E'", []).unwrap();
    let before = store.execution("E".into()).await.unwrap().unwrap();
    let work = store.work_run("work".into()).await.unwrap();
    let links = store.work_execution_links("work".into()).await.unwrap();
    assert!(
        store
            .workspace_claim(dir.path().to_string_lossy().into())
            .await
            .unwrap()
            .is_some()
    );
    let (s, _release, fake) = fake_service(
        store.clone(),
        dir.path().join("agent-state.db"),
        "CANCEL_PROJECTION",
        "T",
        false,
        "paginated",
    )
    .await;
    assert_eq!(
        s.observe("E".into(), false).await.unwrap_err(),
        "Invalid persisted execution activity"
    );
    let error = s
        .agent_execute(
            AgentExecuteAction::Cancel {
                work_run_id: "work".into(),
                execution_id: "E".into(),
            },
            None,
        )
        .await
        .unwrap_err();
    let committed = store.execution("E".into()).await.unwrap().unwrap();
    assert_eq!(committed.status, "cancelled");
    assert_eq!(committed.revision, before.revision + 1);
    assert_eq!(committed.dispatch_state, before.dispatch_state);
    assert!(
        store
            .workspace_claim(dir.path().to_string_lossy().into())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(error.accepted_execution_id.as_deref(), Some("E"));
    let response = s.adapter_error_response(error).await;
    assert_eq!(response["ok"], false);
    assert_eq!(response["error"]["code"], "AGENT_OPERATION_FAILED");
    assert_eq!(response["error"]["executionId"], "E");
    assert_eq!(
        response["control"],
        json!({
            "requestAccepted": true, "providerInvoked": null, "dispatchCertainty": "uncertain",
            "nextAction": {"action": "observe", "waitMs": 20000, "executionId": "E"}
        })
    );
    // Error projection is a read: it cannot cancel twice, dispatch, or create rows.
    assert_eq!(store.execution("E".into()).await.unwrap(), Some(committed));
    assert!(
        store
            .workspace_claim(dir.path().to_string_lossy().into())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(store.work_run("work".into()).await.unwrap(), work);
    assert_eq!(
        store.work_execution_links("work".into()).await.unwrap(),
        links
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM executions", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    for table in ["runtime_instances", "execution_runtime_attempts"] {
        assert_eq!(
            db.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    drop(s);
    assert!(fake.await.unwrap().is_empty());
}

#[tokio::test]
async fn cancel_transaction_failure_remains_rejected_without_mutation() {
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::open(dir.path().into()).await.unwrap();
    create_work(&store, dir.path(), "work").await;
    pending_work(&store, dir.path(), "E", "work", "work", 1).await;
    let db = rusqlite::Connection::open(dir.path().join("agent-state.db")).unwrap();
    db.execute_batch("CREATE TRIGGER reject_cancel AFTER UPDATE OF status ON executions WHEN NEW.status='cancelled' BEGIN SELECT RAISE(ABORT,'cancel transaction rejected'); END;").unwrap();
    let before = store.execution("E".into()).await.unwrap();
    let claim = store
        .workspace_claim(dir.path().to_string_lossy().into())
        .await
        .unwrap();
    let work = store.work_run("work".into()).await.unwrap();
    let links = store.work_execution_links("work".into()).await.unwrap();
    let (s, _release, fake) = fake_service(
        store.clone(),
        dir.path().join("agent-state.db"),
        "CANCEL_REJECTED",
        "T",
        false,
        "paginated",
    )
    .await;
    let error = s
        .agent_execute(
            AgentExecuteAction::Cancel {
                work_run_id: "work".into(),
                execution_id: "E".into(),
            },
            None,
        )
        .await
        .unwrap_err();
    assert!(error.message.contains("cancel transaction rejected"));
    assert!(error.accepted_execution_id.is_none());
    let response = s.adapter_error_response(error).await;
    assert_eq!(response["ok"], false);
    assert_eq!(response["error"]["code"], "AGENT_OPERATION_FAILED");
    assert_eq!(
        response["control"],
        json!({"requestAccepted": false, "providerInvoked": false, "dispatchCertainty": "not_dispatched", "nextAction": null})
    );
    assert_eq!(store.execution("E".into()).await.unwrap(), before);
    assert_eq!(
        store
            .workspace_claim(dir.path().to_string_lossy().into())
            .await
            .unwrap(),
        claim
    );
    assert_eq!(store.work_run("work".into()).await.unwrap(), work);
    assert_eq!(
        store.work_execution_links("work".into()).await.unwrap(),
        links
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM executions", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    for table in ["runtime_instances", "execution_runtime_attempts"] {
        assert_eq!(
            db.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    drop(s);
    assert!(fake.await.unwrap().is_empty());
}
