//! CB2-005：真实本地策略、协议处理与持久化 Claim；仅替代 Runtime 创建和 wire 对端。
use super::*;
use crate::agent::{
    codex::app_server::Client,
    execution::CreateExecutionInput,
    provider::{port::ProviderExecutionFailure, port::ProviderReconcileKind},
    store::ExecutionRecord,
};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream};

/// 使用非默认 Role，防止恢复时重新解析路由或丢失冻结身份。
fn request(root: &std::path::Path, key: &str) -> CreateExecutionInput {
    serde_json::from_value(json!({"agent_id":"drain-agent","request_key":key,
        "prompt":"contract only","execution_profile":{},"workspace_id":"drain-workspace",
        "canonical_workspace_root":root.to_str().unwrap(),"provider":"codex",
        "task_role":"review","mode":"read_only"}))
    .unwrap()
}

/// 读取数据库实值并输出矩阵，不用测试预期代替运行证据。
async fn snapshot(
    directory: &tempfile::TempDir,
    store: &StateStore,
    execution: &str,
    phase: &str,
) -> Value {
    let row = store.execution(execution.into()).await.unwrap().unwrap();
    let claim = store
        .workspace_claim(row.canonical_workspace_root.clone())
        .await
        .unwrap();
    let db = rusqlite::Connection::open(directory.path().join("agent-state.db")).unwrap();
    let role: String = db
        .query_row(
            "SELECT task_role FROM executions WHERE id=?1",
            [execution],
            |r| r.get(0),
        )
        .unwrap();
    let counts: Vec<i64> = [
        "executions",
        "runtime_instances",
        "workspace_claims",
        "execution_runtime_attempts",
    ]
    .iter()
    .map(|table| {
        db.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    })
    .collect();
    let runtime = if let Some(runtime) = &row.runtime_instance_id {
        store.runtime(runtime.clone()).await.unwrap()
    } else {
        None
    };
    let value = json!({"execution":row.id,"status":row.status,"dispatchState":row.dispatch_state,
        "provider":row.provider,"taskRole":role,"runtime":row.runtime_instance_id,"thread":row.thread_id,"turn":row.turn_id,
        "claimOwner":claim.as_ref().map(|c| &c.execution_id),"interrupt":row.interrupt_requested_at,
        "attempt":store.has_runtime_attempt(execution.into()).await.unwrap(),"counts":counts,
        "runtimeState":runtime.as_ref().map(|r| &r.state),"termination":runtime.as_ref().map(|r| &r.termination_evidence_state)});
    println!("CB2-005 {phase}: {value}");
    value
}

/// 所有 wire 等待有界，额外 interrupt 或重复 Start 会使顺序断言失败。
async fn receive(wire: &mut BufReader<DuplexStream>, method: &str) -> Value {
    let mut line = String::new();
    assert!(
        tokio::time::timeout(Duration::from_secs(10), wire.read_line(&mut line))
            .await
            .unwrap()
            .unwrap()
            > 0
    );
    let value: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(value["method"], method);
    value
}

/// 回应固定协议请求，保持真实 Client 和 Provider 状态机。
async fn send(wire: &mut BufReader<DuplexStream>, value: Value) {
    wire.write_all(format!("{value}\n").as_bytes())
        .await
        .unwrap();
}

/// 等待 Provider 已经持久化 running，而非只收到 acceptance。
async fn running(store: &StateStore, execution: &str) -> ExecutionRecord {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let row = store.execution(execution.into()).await.unwrap().unwrap();
            if row.status == "running" && row.dispatch_state == "dispatched" {
                return row;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap()
}

/// 停用 pending 后保留 Claim；重新开启仅首次派发一次；running 再停用不取消或释放。
#[tokio::test]
async fn cb2_005_pending_reenable_resume_and_running_drain_matrix() {
    let (directory, broker, mut manager) = fixture().await;
    let store = StateStore::open(directory.path().into()).await.unwrap();
    let created = manager
        .create(request(directory.path(), "pending"))
        .await
        .unwrap();
    let execution = created.execution_id;
    let before = snapshot(&directory, &store, &execution, "before-disable-pending").await;
    assert_eq!(before["status"], "dispatch_pending");
    assert_eq!(before["dispatchState"], "not_dispatched");
    assert_eq!(before["claimOwner"], execution);
    assert_eq!(before["taskRole"], "review");
    assert_eq!(before["counts"], json!([1, 0, 1, 0]));
    assert_eq!(before["attempt"], false);
    agent_provider_set_enabled_impl(&broker, id("codex"), false)
        .await
        .unwrap();
    assert_eq!(
        snapshot(&directory, &store, &execution, "after-disable-pending").await,
        before
    );
    assert_eq!(
        manager
            .resume_pending_execution(&execution)
            .await
            .unwrap_err(),
        ProviderExecutionFailure::State("AGENT_PROVIDER_DISABLED".into())
    );
    assert_eq!(
        manager
            .execute(request(directory.path(), "rejected-new-start"))
            .await
            .unwrap_err(),
        ProviderExecutionFailure::State("AGENT_PROVIDER_DISABLED".into())
    );
    assert_eq!(
        snapshot(&directory, &store, &execution, "rejected-start-and-resume").await,
        before
    );
    assert_eq!(
        store.execution(execution.clone()).await.unwrap().unwrap(),
        created.execution
    );

    // 原 Role route 改动不能重选 pending Execution 的 Provider 或 Role。
    agent_provider_set_role_route_impl(&broker, AgentTaskRole::Review, Some(id("future-provider")))
        .await
        .unwrap();
    agent_provider_set_enabled_impl(&broker, id("codex"), true)
        .await
        .unwrap();
    let runtime_id = format!("runtime-{execution}");
    let (wire, server) = tokio::io::duplex(128 * 1024);
    let (read, write) = tokio::io::split(wire);
    let client = Arc::new(Client::product_test_transport(
        runtime_id.clone(),
        read,
        write,
        tokio::io::empty(),
    ));
    manager.test_client = Some((client.clone(), directory.path().join("agent-state.db")));
    let (finish, wait_finish) = tokio::sync::oneshot::channel();
    let fake = tokio::spawn(async move {
        let mut wire = BufReader::new(server);
        let req = receive(&mut wire, "initialize").await;
        send(&mut wire, json!({"id":req["id"],"result":{"userAgent":"fake","codexHome":"isolated","platformFamily":"windows","platformOs":"windows"}})).await;
        receive(&mut wire, "initialized").await;
        let req = receive(&mut wire, "thread/start").await;
        send(&mut wire, json!({"id":req["id"],"result":{"thread":{"id":"THREAD","turns":[],"historyMode":"paginated"}}})).await;
        let req = receive(&mut wire, "turn/start").await;
        send(&mut wire, json!({"id":req["id"],"result":{"turn":{"id":"TURN","status":"inProgress","items":[],"itemsView":"summary"}}})).await;
        send(&mut wire, json!({"method":"turn/started","params":{"threadId":"THREAD","turn":{"id":"TURN","status":"inProgress","items":[],"itemsView":"summary"}}})).await;
        tokio::time::timeout(Duration::from_secs(10), wait_finish)
            .await
            .unwrap()
            .unwrap();
        send(&mut wire, json!({"method":"turn/completed","params":{"threadId":"THREAD","turn":{"id":"TURN","status":"completed","items":[],"itemsView":"summary"}}})).await;
        for (method, result) in [
            (
                "thread/read",
                json!({"thread":{"id":"THREAD","turns":[],"historyMode":"paginated"}}),
            ),
            (
                "thread/turns/list",
                json!({"data":[{"id":"TURN","status":"completed","items":[],"itemsView":"summary"}],"nextCursor":null}),
            ),
            ("thread/items/list", json!({"data":[],"nextCursor":null})),
            ("thread/backgroundTerminals/clean", json!({})),
            (
                "thread/backgroundTerminals/list",
                json!({"data":[],"nextCursor":null}),
            ),
        ] {
            let req = receive(&mut wire, method).await;
            send(&mut wire, json!({"id":req["id"],"result":result})).await;
        }
        let mut extra = String::new();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(10), wire.read_line(&mut extra))
                .await
                .unwrap()
                .unwrap(),
            0,
            "unexpected cancel/replay: {extra}"
        );
    });
    let worker_manager = manager.clone();
    let worker_id = execution.clone();
    let worker =
        tokio::spawn(async move { worker_manager.resume_pending_execution(&worker_id).await });
    let row = running(&store, &execution).await;
    let claim = store
        .workspace_claim(row.canonical_workspace_root.clone())
        .await
        .unwrap();
    let runtime = store.runtime(runtime_id.clone()).await.unwrap();
    let active = snapshot(&directory, &store, &execution, "re-enable-resume-running").await;
    assert_eq!(active["runtime"], runtime_id);
    assert_eq!(active["thread"], "THREAD");
    assert_eq!(active["turn"], "TURN");
    assert_eq!(active["provider"], "codex");
    assert_eq!(active["taskRole"], "review");
    assert_eq!(active["claimOwner"], execution);
    assert_eq!(active["counts"], json!([1, 1, 1, 0]));
    assert_eq!(active["attempt"], true);
    assert_eq!(active["runtimeState"], "running");
    assert_eq!(active["interrupt"], Value::Null);
    agent_provider_set_enabled_impl(&broker, id("codex"), false)
        .await
        .unwrap();
    assert_eq!(
        snapshot(&directory, &store, &execution, "after-disable-running").await,
        active
    );
    assert_eq!(
        store.execution(execution.clone()).await.unwrap().unwrap(),
        row
    );
    assert!(!worker.is_finished());
    assert_eq!(
        store
            .workspace_claim(row.canonical_workspace_root.clone())
            .await
            .unwrap(),
        claim
    );
    assert_eq!(store.runtime(runtime_id.clone()).await.unwrap(), runtime);
    assert_eq!(
        manager
            .execute(request(directory.path(), "running-rejected-start"))
            .await
            .unwrap_err(),
        ProviderExecutionFailure::State("AGENT_PROVIDER_DISABLED".into())
    );
    assert_eq!(
        snapshot(&directory, &store, &execution, "rejected-running-start").await,
        active
    );
    finish.send(()).unwrap();
    let completed = tokio::time::timeout(Duration::from_secs(10), worker)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(completed.id, execution);
    assert_eq!(completed.status, "completed");
    let end = snapshot(&directory, &store, &execution, "drained-completed").await;
    assert_eq!(end["claimOwner"], Value::Null);
    assert_eq!(end["counts"], json!([1, 1, 0, 0]));
    assert_eq!(end["interrupt"], Value::Null);
    assert_eq!(end["runtime"], runtime_id);
    drop(manager);
    drop(client);
    fake.await.unwrap();
}

/// Disabled 与 unavailable 均不封闭持久化 Provider 的取消入口。
#[tokio::test]
async fn cb2_005_disabled_unavailable_cancel_preserves_persisted_route() {
    let (directory, broker, mut manager) = fixture().await;
    let store = StateStore::open(directory.path().into()).await.unwrap();
    let execution = manager
        .create(request(directory.path(), "cancel"))
        .await
        .unwrap()
        .execution_id;
    agent_provider_set_enabled_impl(&broker, id("codex"), false)
        .await
        .unwrap();
    agent_provider_set_role_route_impl(&broker, AgentTaskRole::Review, Some(id("future-provider")))
        .await
        .unwrap();
    let mut registry = manager.registry().unwrap().as_ref().clone();
    registry
        .set_health(&id("codex"), ProviderHealth::Unavailable)
        .unwrap();
    manager.use_registry(registry);
    let before = snapshot(&directory, &store, &execution, "cancel-before").await;
    assert_eq!(before["claimOwner"], execution);
    assert_eq!(
        manager.cancel(&execution).await.unwrap().status,
        "cancelled"
    );
    let after = snapshot(&directory, &store, &execution, "cancel-after").await;
    assert_eq!(after["provider"], "codex");
    assert_eq!(after["taskRole"], "review");
    assert_eq!(after["dispatchState"], "not_dispatched");
    assert_eq!(after["claimOwner"], Value::Null);
    assert_eq!(after["counts"], json!([1, 0, 0, 0]));
    assert_eq!(
        manager.registry().unwrap().health(&id("codex")).unwrap(),
        ProviderHealth::Unavailable
    );
}

/// Disabled startup 仍运行真实 Codex recovery：无派发可恢复，存在尝试或缺证据则 fail-closed。
#[tokio::test]
async fn cb2_005_disabled_startup_reconcile_claim_matrix() {
    for case in ["pending", "attempt", "unknown", "safe-terminal"] {
        let (directory, broker, mut manager) = fixture().await;
        let store = StateStore::open(directory.path().into()).await.unwrap();
        let execution = manager
            .create(request(directory.path(), case))
            .await
            .unwrap()
            .execution_id;
        let db = rusqlite::Connection::open(directory.path().join("agent-state.db")).unwrap();
        match case {
            "attempt" => store
                .reserve_runtime_attempt(execution.clone(), "reserved-runtime".into(), 1)
                .await
                .unwrap(),
            "unknown" => {
                db.execute(
                    "UPDATE executions SET status='unknown',dispatch_state='uncertain' WHERE id=?1",
                    [&execution],
                )
                .unwrap();
            }
            "safe-terminal" => {
                // 先由真实取消事务产生释放证据，再恢复旧 Claim 模拟历史遗留状态。
                let row = store.execution(execution.clone()).await.unwrap().unwrap();
                let claim = store
                    .workspace_claim(row.canonical_workspace_root)
                    .await
                    .unwrap()
                    .unwrap();
                manager.cancel(&execution).await.unwrap();
                db.execute("INSERT INTO workspace_claims(canonical_workspace_root,execution_id,claim_type,acquired_at) VALUES (?1,?2,?3,?4)", rusqlite::params![claim.canonical_workspace_root,claim.execution_id,claim.claim_type,claim.acquired_at]).unwrap();
            }
            _ => {}
        }
        agent_provider_set_enabled_impl(&broker, id("codex"), false)
            .await
            .unwrap();
        let before = snapshot(
            &directory,
            &store,
            &execution,
            &format!("reconcile-{case}-before"),
        )
        .await;
        let report = manager.reconcile_startup().await.unwrap();
        assert_eq!(report.len(), 1);
        assert_eq!(report[0].subject_id, execution);
        let expected = match case {
            "pending" => ProviderReconcileKind::ExecutionPendingExplicitResume,
            "safe-terminal" => ProviderReconcileKind::ExecutionReleased,
            _ => ProviderReconcileKind::ExecutionUnknown,
        };
        assert_eq!(report[0].kind, expected);
        let after = snapshot(
            &directory,
            &store,
            &execution,
            &format!("reconcile-{case}-after"),
        )
        .await;
        assert_eq!(after["provider"], "codex");
        assert_eq!(after["taskRole"], "review");
        assert_eq!(after["runtime"], Value::Null);
        assert_eq!(after["counts"][0], 1);
        assert_eq!(after["counts"][1], 0);
        if case == "safe-terminal" {
            assert_eq!(after["claimOwner"], Value::Null);
        } else {
            assert_eq!(after["claimOwner"], execution);
        }
        if case == "pending" {
            assert_eq!(after, before);
        }
        if matches!(case, "attempt" | "unknown") {
            assert_eq!(after["status"], "unknown");
        }
        if case == "attempt" {
            assert_eq!(after["attempt"], true);
        }
    }
}
