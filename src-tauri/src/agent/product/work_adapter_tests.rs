use super::*;
use crate::agent::store::transactions::product::WorkExecutionContext;
#[cfg(windows)]
use crate::workspace_registry::WORKSPACE_IN_USE;
use crate::{
    commands::remove_workspace,
    config::{self, AppPaths, ManagerConfig, Workspace},
    serena::SupervisorState,
};
use std::sync::{Mutex, mpsc};

#[path = "work_context_tests.rs"]
mod work_context_tests;

fn start_work(work: &str, key: &str) -> AgentExecuteAction {
    AgentExecuteAction::Start {
        work_run_id: work.into(),
        workspace_id: "W".into(),
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
            1,
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

#[cfg(windows)]
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
    let child = store
        .execution(next.execution_id.clone())
        .await
        .unwrap()
        .unwrap();
    // 并发重试的返回顺序不决定创建者；两者完成后从持久化行读取 Provider 已绑定的线程。
    assert_eq!(child.thread_id, terminal.thread_id);
    // Continue 只继承父 Execution 的冻结快照，不接受调用端 Workspace。
    assert_eq!(child.workspace_id, parent.workspace_id);
    assert_eq!(
        child.canonical_workspace_root,
        parent.canonical_workspace_root
    );
    assert_eq!(child.workspace_generation, parent.workspace_generation);
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
                known_control_revision: None,
                known_activity_revision: None,
                wait_ms: Some(0),
                include_result: Some(true),
                wake_on: None,
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
    let cancel = s.agent_execute(
        AgentExecuteAction::Cancel {
            work_run_id: "work".into(),
            execution_id: "E".into(),
        },
        None,
    );
    #[cfg(any(windows, target_os = "macos"))]
    {
        let cancelled = cancel.await.unwrap();
        assert_eq!(cancelled.status, "cancelled");
        assert!(
            store
                .workspace_claim(dir.path().to_string_lossy().into())
                .await
                .unwrap()
                .is_none()
        );
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        // Work 归属校验通过后，未支持平台的 Provider 仍保留稳定取消错误与 Claim。
        assert_eq!(cancel.await.unwrap_err().code, "AGENT_PROVIDER_UNAVAILABLE");
        assert_eq!(store.execution("E".into()).await.unwrap(), before);
        assert!(
            store
                .workspace_claim(dir.path().to_string_lossy().into())
                .await
                .unwrap()
                .is_some()
        );
    }
    assert_eq!(
        store
            .work_execution_links("work".into())
            .await
            .unwrap()
            .len(),
        1
    );
}

#[cfg(windows)]
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
            known_control_revision: None,
            known_activity_revision: None,
            wait_ms: Some(80),
            include_result: None,
            wake_on: None,
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
                known_control_revision: None,
                known_activity_revision: None,
                wait_ms: wait,
                include_result: None,
                wake_on: None,
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
        known_control_revision: None,
        known_activity_revision: None,
        wait_ms: Some(20_000),
        include_result: None,
        wake_on: None,
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
        known_control_revision: None,
        known_activity_revision: None,
        wait_ms: None,
        include_result: None,
        wake_on: None,
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
    #[cfg(windows)]
    let (s, _release, fake) = fake_service(
        store.clone(),
        dir.path().join("agent-state.db"),
        "REJECT_START",
        "T",
        false,
        "paginated",
    )
    .await;
    #[cfg(not(windows))]
    // 非 Windows 使用真实 unavailable Product，验证拒绝发生在 Provider 边界之前。
    let s = AgentProductService::new(store.clone());
    assert_eq!(
        s.agent_execute(start_work("missing", "key"), None)
            .await
            .unwrap_err()
            .code,
        "WORK_NOT_FOUND"
    );
    // WorkRun 与请求 Workspace 不一致时，不能创建 Claim 或触发 Provider。
    assert_eq!(
        s.agent_execute(
            AgentExecuteAction::Start {
                work_run_id: "work".into(),
                workspace_id: "wrong".into(),
                request_key: "key".into(),
                prompt: "hello".into(),
                delegation_context_json: None,
            },
            w(dir.path(), "wrong"),
        )
        .await
        .unwrap_err()
        .code,
        "WORKSPACE_CONTEXT_MISMATCH"
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
            workspace_id: "W".into(),
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
    ] {
        assert_eq!(
            s.agent_query(action).await.unwrap_err().code,
            "WORK_INVALID_ARGUMENT"
        );
    }
    for action in [
        AgentQueryAction::Observe {
            execution_id: "".into(),
            known_revision: None,
            known_control_revision: None,
            known_activity_revision: None,
            wait_ms: Some(0),
            include_result: None,
            wake_on: None,
        },
        AgentQueryAction::Observe {
            execution_id: "   ".into(),
            known_revision: None,
            known_control_revision: None,
            known_activity_revision: None,
            wait_ms: Some(0),
            include_result: None,
            wake_on: None,
        },
        AgentQueryAction::Observe {
            execution_id: "bad id".into(),
            known_revision: None,
            known_control_revision: None,
            known_activity_revision: None,
            wait_ms: Some(0),
            include_result: None,
            wake_on: None,
        },
        AgentQueryAction::Observe {
            execution_id: "bad\tid".into(),
            known_revision: None,
            known_control_revision: None,
            known_activity_revision: None,
            wait_ms: Some(0),
            include_result: None,
            wake_on: None,
        },
        AgentQueryAction::Observe {
            execution_id: "bad\nid".into(),
            known_revision: None,
            known_control_revision: None,
            known_activity_revision: None,
            wait_ms: Some(0),
            include_result: None,
            wake_on: None,
        },
        AgentQueryAction::Observe {
            execution_id: "E".into(),
            known_revision: None,
            known_control_revision: None,
            known_activity_revision: None,
            wait_ms: Some(20_001),
            include_result: None,
            wake_on: None,
        },
        AgentQueryAction::Observe {
            execution_id: "E".into(),
            known_revision: Some(" ".into()),
            known_control_revision: None,
            known_activity_revision: None,
            wait_ms: None,
            include_result: None,
            wake_on: None,
        },
        AgentQueryAction::Observe {
            execution_id: "E".into(),
            known_revision: None,
            known_control_revision: Some(" ".into()),
            known_activity_revision: None,
            wait_ms: None,
            include_result: None,
            wake_on: None,
        },
        AgentQueryAction::Observe {
            execution_id: "E".into(),
            known_revision: None,
            known_control_revision: None,
            known_activity_revision: Some(" ".into()),
            wait_ms: None,
            include_result: None,
            wake_on: None,
        },
    ] {
        assert_eq!(
            s.agent_query(action).await.unwrap_err().code,
            "AGENT_OBSERVE_INVALID_ARGUMENT"
        );
    }
    assert_eq!(
        s.agent_query(AgentQueryAction::Observe {
            execution_id: "E-1_abc".into(),
            known_revision: None,
            known_control_revision: None,
            known_activity_revision: None,
            wait_ms: Some(0),
            include_result: None,
            wake_on: None,
        })
        .await
        .unwrap_err()
        .code,
        "AGENT_EXECUTION_NOT_FOUND"
    );
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
    #[cfg(windows)]
    assert!(fake.await.unwrap().is_empty());
}

#[cfg(windows)]
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
        known_control_revision: None,
        known_activity_revision: None,
        wait_ms: Some(20_000),
        include_result: None,
        wake_on: None,
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

#[cfg(windows)]
#[tokio::test]
async fn invalid_persisted_activity_rejects_cancel_without_claim_side_effect() {
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::open(dir.path().into()).await.unwrap();
    create_work(&store, dir.path(), "work").await;
    pending_work(&store, dir.path(), "E", "work", "work", 1).await;
    let db = rusqlite::Connection::open(dir.path().join("agent-state.db")).unwrap();
    // Valid SQL field values, but an invalid Activity combination for the view.
    db.execute("UPDATE executions SET last_activity_at=1, activity_phase='tool', tool_category=NULL WHERE id='E'", []).unwrap();
    let before = store.execution("E".into()).await.unwrap();
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
    // 非法 Activity 不允许借由取消路径被悄然修复或推进生命周期。
    assert!(error.accepted_execution_id.is_none());
    assert_eq!(store.execution("E".into()).await.unwrap(), before);
    assert!(
        store
            .workspace_claim(dir.path().to_string_lossy().into())
            .await
            .unwrap()
            .is_some()
    );
    let response = s.adapter_error_response(error).await;
    assert_eq!(response["ok"], false);
    assert_eq!(response["error"]["code"], "AGENT_OPERATION_FAILED");
    assert_eq!(
        response["control"],
        json!({
            "requestAccepted": false, "providerInvoked": false, "dispatchCertainty": "not_dispatched",
            "nextAction": null
        })
    );
    // 拒绝路径不能启动运行时、创建行或释放 Claim。
    assert_eq!(store.execution("E".into()).await.unwrap(), before);
    assert!(
        store
            .workspace_claim(dir.path().to_string_lossy().into())
            .await
            .unwrap()
            .is_some()
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

#[cfg(windows)]
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

#[cfg(windows)]
#[tokio::test]
async fn resolver_start_and_remove_share_supervisor_operation_exclusion() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("workspace");
    std::fs::create_dir_all(&root).unwrap();
    let root = std::fs::canonicalize(root).unwrap();
    let paths = AppPaths {
        runtime_directory: directory.path().join("runtime"),
        config_file: directory.path().join("config.json"),
        log_directory: directory.path().join("logs"),
        app_log: directory.path().join("logs/app.log"),
        serena_log: directory.path().join("logs/serena.log"),
    };
    config::save(
        &paths.config_file,
        &ManagerConfig {
            workspace_registry_revision: 1,
            workspaces: vec![Workspace {
                id: "W".into(),
                name: "Workspace".into(),
                root: root.clone(),
                generation: 1,
            }],
            ..ManagerConfig::default()
        },
    )
    .unwrap();
    let supervisor = Arc::new(SupervisorState::new(paths).unwrap());
    let store = StateStore::open(directory.path().join("agent-state"))
        .await
        .unwrap();
    create_work(&store, &root, "work").await;
    let (service, release, fake, turn_started) = fake_service_with_turn_started(
        store.clone(),
        directory.path().join("agent-state").join("agent-state.db"),
        "REMOVE_RACE",
        "T",
        false,
        "paginated",
    )
    .await;
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let (start_result_tx, start_result_rx) = std::sync::mpsc::channel();
    let (runtime_release_tx, runtime_release_rx) = std::sync::mpsc::channel();
    let release_rx = Arc::new(std::sync::Mutex::new(release_rx));
    let hook_release = release_rx.clone();
    *supervisor.workspace_start_hook.lock().unwrap() = Some(Arc::new(move || {
        entered_tx.send(()).unwrap();
        hook_release.lock().unwrap().recv().unwrap();
    }));
    let service = Arc::new(service);
    let start_service = service.clone();
    let start_supervisor = supervisor.clone();
    // 同步测试钩子必须在独立 runtime 中阻塞，避免占住当前测试 runtime 而无法释放同步点。
    let start = std::thread::spawn(move || {
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async move {
                let result = start_service
                    .agent_execute(start_work("work", "key"), start_supervisor.as_ref())
                    .await;
                start_result_tx.send(result).unwrap();
                // 保持发起方 runtime 存活，直到 fake 已证明后台 worker 收到 turn/start。
                tokio::task::spawn_blocking(move || runtime_release_rx.recv().unwrap())
                    .await
                    .unwrap();
            })
    });
    tokio::task::spawn_blocking(move || entered_rx.recv().unwrap())
        .await
        .unwrap();
    let remove_supervisor = supervisor.clone();
    let remove_service = service.clone();
    let mut remove = tokio::task::spawn_blocking(move || {
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(remove_workspace(&remove_supervisor, &remove_service, "W"))
    });
    assert!(
        tokio::time::timeout(Duration::from_millis(50), &mut remove)
            .await
            .is_err()
    );
    release_tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while store
            .workspace_claim(root.to_string_lossy().into_owned())
            .await
            .unwrap()
            .is_none()
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(remove.await.unwrap(), Err(WORKSPACE_IN_USE.into()));
    let started = tokio::task::spawn_blocking(move || start_result_rx.recv().unwrap())
        .await
        .unwrap()
        .unwrap();
    // accepted receipt 之后，显式等待 fake 收到 turn/start，而非假设其已经开始。
    turn_started.await.unwrap();
    release.send(()).unwrap();
    final_row(service.as_ref(), &started.execution_id).await;
    runtime_release_tx.send(()).unwrap();
    tokio::task::spawn_blocking(move || start.join().unwrap())
        .await
        .unwrap();
    drop(service);
    assert!(
        fake.await
            .unwrap()
            .iter()
            .any(|method| method == "turn/start")
    );
}

#[tokio::test]
async fn resolver_start_after_remove_linearizes_to_workspace_not_found() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("workspace");
    std::fs::create_dir_all(&root).unwrap();
    let root = std::fs::canonicalize(root).unwrap();
    let paths = AppPaths {
        runtime_directory: directory.path().join("runtime"),
        config_file: directory.path().join("config.json"),
        log_directory: directory.path().join("logs"),
        app_log: directory.path().join("logs/app.log"),
        serena_log: directory.path().join("logs/serena.log"),
    };
    config::save(
        &paths.config_file,
        &ManagerConfig {
            workspace_registry_revision: 1,
            workspaces: vec![Workspace {
                id: "W".into(),
                name: "Workspace".into(),
                root: root.clone(),
                generation: 1,
            }],
            ..ManagerConfig::default()
        },
    )
    .unwrap();
    let supervisor = Arc::new(SupervisorState::new(paths).unwrap());
    let store = StateStore::open(directory.path().join("agent-state"))
        .await
        .unwrap();
    create_work(&store, &root, "work").await;
    let service = Arc::new(AgentProductService::new(store.clone()));
    let (remove_entered_tx, remove_entered_rx) = mpsc::channel();
    let (remove_release_tx, remove_release_rx) = mpsc::channel();
    let remove_release_rx = Arc::new(Mutex::new(remove_release_rx));
    let hook_release = remove_release_rx.clone();
    *supervisor.workspace_remove_hook.lock().unwrap() = Some(Arc::new(move || {
        remove_entered_tx.send(()).unwrap();
        hook_release.lock().unwrap().recv().unwrap();
    }));
    let (start_entered_tx, start_entered_rx) = mpsc::channel();
    *supervisor.workspace_start_hook.lock().unwrap() = Some(Arc::new(move || {
        start_entered_tx.send(()).unwrap();
    }));
    let remove_supervisor = supervisor.clone();
    let remove_service = service.clone();
    let remove = std::thread::spawn(move || {
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(remove_workspace(&remove_supervisor, &remove_service, "W"))
    });
    tokio::task::spawn_blocking(move || remove_entered_rx.recv().unwrap())
        .await
        .unwrap();
    let start_service = service.clone();
    let start_supervisor = supervisor.clone();
    let start = std::thread::spawn(move || {
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async move {
                start_service
                    .agent_execute(start_work("work", "key"), start_supervisor.as_ref())
                    .await
            })
    });
    // Remove 持锁时 Resolver Start 无法越过，因而不会到达其 Start 同步点。
    assert!(
        start_entered_rx
            .recv_timeout(Duration::from_millis(50))
            .is_err()
    );
    remove_release_tx.send(()).unwrap();
    assert_eq!(
        tokio::task::spawn_blocking(move || remove.join().unwrap())
            .await
            .unwrap()
            .unwrap()
            .id,
        "W"
    );
    assert_eq!(
        tokio::task::spawn_blocking(move || start.join().unwrap())
            .await
            .unwrap()
            .unwrap_err()
            .code,
        "WORKSPACE_NOT_FOUND"
    );
    assert!(
        store
            .workspace_claim(root.to_string_lossy().into_owned())
            .await
            .unwrap()
            .is_none()
    );
    let connection =
        rusqlite::Connection::open(directory.path().join("agent-state").join("agent-state.db"))
            .unwrap();
    assert_eq!(
        connection
            .query_row("SELECT count(*) FROM executions", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}
