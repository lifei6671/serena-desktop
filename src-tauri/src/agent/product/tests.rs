use super::*;
#[cfg(windows)]
use crate::agent::{codex::app_server::Client, coordinator::now};
use std::sync::Arc;
use std::time::Duration;
#[cfg(windows)]
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
// 只有具体 Runtime/Fake wire 测试保留 Windows gate，纯 Store/协议覆盖跨平台运行。
#[path = "control_tests.rs"]
mod control_tests;
#[path = "observe_tests.rs"]
mod observe_tests;
#[path = "orchestration_tests.rs"]
mod orchestration_tests;
#[path = "persistence_tests.rs"]
#[cfg(windows)]
mod persistence_tests;
#[path = "restart_tests.rs"]
#[cfg(windows)]
mod restart_tests;
#[path = "usage_projection_tests.rs"]
mod usage_projection_tests;
#[path = "work_adapter_tests.rs"]
mod work_adapter_tests;
#[path = "workspace_write_tests.rs"]
mod workspace_write_tests;
fn run(f: impl std::future::Future<Output = ()>) {
    tokio::runtime::Runtime::new().unwrap().block_on(f)
}
fn start(agent: &str, key: &str) -> Value {
    json!({"action":"start","agentId":agent,"requestKey":key,"prompt":"hello"})
}
fn w(root: &std::path::Path, id: &str) -> Option<WorkspaceSnapshot> {
    Some(WorkspaceSnapshot {
        id: id.into(),
        root: root.to_string_lossy().into(),
        generation: 1,
    })
}

/// 仅为 raw SQL fixture 写入与冻结映射一致的 v8 Activity current snapshot。
fn set_v8_execution_state(
    db: &rusqlite::Connection,
    id: &str,
    status: &str,
    dispatch_state: &str,
    activity_phase: Option<ActivityPhase>,
    tool_category: Option<ToolCategory>,
) {
    // 测试不得手写 summary 规则；与 Product 相同的冻结 domain 映射是唯一来源。
    let progress = match status {
        "dispatch_pending" => match dispatch_state {
            "not_dispatched" => ProgressPhase::Pending,
            "dispatching" => ProgressPhase::Dispatching,
            "dispatched" => ProgressPhase::Running,
            "uncertain" => ProgressPhase::Reconciling,
            _ => panic!("invalid fixture dispatch state: {dispatch_state}"),
        },
        "running" | "cancel_requested" | "cancelling" => ProgressPhase::Running,
        "finalizing" => ProgressPhase::Finalizing,
        "reconciling" | "unknown" => ProgressPhase::Reconciling,
        "completed" | "failed" | "cancelled" | "interrupted" => ProgressPhase::Terminal,
        _ => panic!("invalid fixture status: {status}"),
    };
    let summary_code = derive_summary_code(progress, activity_phase, tool_category).unwrap();
    db.execute(
        "UPDATE executions SET status=?1,dispatch_state=?2,activity_phase=?3,tool_category=?4,activity_summary_code=?5 WHERE id=?6",
        rusqlite::params![
            status,
            dispatch_state,
            activity_phase.map(ActivityPhase::as_str),
            tool_category.map(ToolCategory::as_str),
            summary_code,
            id,
        ],
    )
    .unwrap();
}

#[tokio::test]
async fn workspace_claim_exists_forwards_the_authoritative_store_lookup() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("workspace");
    std::fs::create_dir(&root).unwrap();
    let root = root.to_string_lossy().into_owned();
    let store = StateStore::open(directory.path().into()).await.unwrap();
    let service = AgentProductService::new(store.clone());

    assert!(!service.workspace_claim_exists(root.clone()).await.unwrap());
    store
        .product_create_fresh(
            "execution".into(),
            "agent".into(),
            "key".into(),
            "prompt".into(),
            "workspace".into(),
            Some(WorkspaceSnapshot {
                id: "workspace".into(),
                root: root.clone(),
                generation: 1,
            }),
            1,
        )
        .await
        .unwrap();

    assert!(service.workspace_claim_exists(root).await.unwrap());
}

#[tokio::test]
async fn nonterminal_execution_count_reads_product_store_without_runtime_projection() {
    let directory = tempfile::tempdir().unwrap();
    let store = StateStore::open(directory.path().into()).await.unwrap();
    let service = AgentProductService::new(store.clone());
    assert_eq!(service.nonterminal_execution_count().await.unwrap(), 0);

    store
        .product_create_fresh(
            "execution".into(),
            "agent".into(),
            "key".into(),
            "prompt".into(),
            "workspace".into(),
            Some(WorkspaceSnapshot {
                id: "workspace".into(),
                root: directory.path().to_string_lossy().into_owned(),
                generation: 1,
            }),
            1,
        )
        .await
        .unwrap();
    assert_eq!(service.nonterminal_execution_count().await.unwrap(), 1);

    let database = rusqlite::Connection::open(directory.path().join("agent-state.db")).unwrap();
    database
        .execute(
            "UPDATE executions SET status='completed' WHERE id='execution'",
            [],
        )
        .unwrap();
    assert_eq!(service.nonterminal_execution_count().await.unwrap(), 0);
}

#[tokio::test]
async fn initialize_starts_exactly_one_auto_recovery_worker() {
    let directory = tempfile::tempdir().unwrap();
    let store = StateStore::open(directory.path().into()).await.unwrap();
    let (service, _) = TEST_DISCOVERY
        .scope(
            Err("BACKEND_UNAVAILABLE: fixture".into()),
            AgentProductService::initialize(store),
        )
        .await
        .unwrap();
    // 后端不可用时仍须暴露稳定诊断，供跨平台本地读取路径使用。
    assert!(
        service
            .backend_diagnostic()
            .is_some_and(|diagnostic| diagnostic.starts_with("BACKEND_UNAVAILABLE:"))
    );
    assert!(!service.manager.start_auto_recovery_worker());
    service.shutdown().await.unwrap();
}

/// macOS Desktop 发布与 startup recovery 不执行 CLI discovery，仍不冻结 backend 为 unavailable。
#[cfg(target_os = "macos")]
#[tokio::test]
async fn desktop_deferred_initialization_keeps_backend_resolvable() {
    let directory = tempfile::tempdir().unwrap();
    let store = StateStore::open(directory.path().join("store"))
        .await
        .unwrap();
    let (service, report) = crate::agent::codex::TEST_BACKEND_DISCOVERY
        .scope(
            Err("discovery must be deferred".into()),
            AgentProductService::initialize_desktop_deferred(
                store,
                super::super::notification::noop_agent_terminal_notifier(),
            ),
        )
        .await
        .unwrap();
    assert!(report.is_empty());
    assert_eq!(service.backend_diagnostic(), None);
    service.shutdown().await.unwrap();
}

/// macOS discovery 的稳定架构/兼容性诊断不能在产品初始化边界降级成一般不可用。
#[tokio::test]
async fn initialize_preserves_macos_discovery_diagnostic_codes() {
    for code in [
        "CODEX_HOST_ARCH_UNSUPPORTED",
        "CODEX_ARCH_UNSUPPORTED",
        "CODEX_EXECUTABLE_FORMAT_UNSUPPORTED",
        "CODEX_EXECUTABLE_NOT_RUNNABLE",
        "CODEX_COMPATIBILITY_BLOCKED",
    ] {
        let directory = tempfile::tempdir().unwrap();
        let store = StateStore::open(directory.path().into()).await.unwrap();
        let diagnostic = format!("{code}: fixture");
        let (service, _) = TEST_DISCOVERY
            .scope(
                Err(diagnostic.clone()),
                AgentProductService::initialize(store),
            )
            .await
            .unwrap();
        assert_eq!(service.backend_diagnostic(), Some(diagnostic.as_str()));
        assert_eq!(
            ProductError::new(diagnostic, None).code,
            code,
            "{code} 必须保持为公共稳定错误码"
        );
        service.shutdown().await.unwrap();
    }
}

/// 未支持的平台必须暴露真实的四类 Runtime 失败，且不得伪造终止证据释放 Claim。
#[cfg(not(any(windows, target_os = "macos")))]
#[tokio::test]
async fn unsupported_runtime_actions_preserve_unavailable_contract_and_claim() {
    let directory = tempfile::tempdir().unwrap();
    let pending_root = directory.path().join("pending-workspace");
    let source_root = directory.path().join("source-workspace");
    std::fs::create_dir_all(&pending_root).unwrap();
    std::fs::create_dir_all(&source_root).unwrap();
    let store = StateStore::open(directory.path().join("store"))
        .await
        .unwrap();
    let (service, _) = TEST_DISCOVERY
        .scope(
            Err("BACKEND_UNAVAILABLE: fixture".into()),
            AgentProductService::initialize(store.clone()),
        )
        .await
        .unwrap();

    let started = service
        .checked_operation(
            start("pending-agent", "start-key"),
            w(&pending_root, "pending-workspace"),
        )
        .await;
    assert_eq!(started["error"]["code"], "BACKEND_UNAVAILABLE");
    let pending_id = started["error"]["executionId"].as_str().unwrap().to_owned();
    let resumed = service
        .checked_operation(
            json!({"action":"resume_pending","executionId":pending_id}),
            None,
        )
        .await;
    assert_eq!(resumed["error"]["code"], "BACKEND_UNAVAILABLE");

    // Continue fixture 只补齐 Store 自有的终态与 Claim 释放事实，不创建 Runtime 或终止证据。
    store
        .product_create_fresh(
            "source".into(),
            "source-agent".into(),
            "source-key".into(),
            "source prompt".into(),
            "source-workspace".into(),
            w(&source_root, "source-workspace"),
            1,
        )
        .await
        .unwrap();
    let database =
        rusqlite::Connection::open(directory.path().join("store").join("agent-state.db")).unwrap();
    database
        .execute(
            "UPDATE executions SET status='completed',dispatch_state='dispatched',\
             release_evidence_state='complete',release_evidence_kind='same_runtime_cleanup',\
             release_evidence_json='{}',completed_at=2 WHERE id='source'",
            [],
        )
        .unwrap();
    database
        .execute(
            "DELETE FROM workspace_claims WHERE execution_id='source'",
            [],
        )
        .unwrap();
    let source_claim_root = store
        .execution("source".into())
        .await
        .unwrap()
        .unwrap()
        .canonical_workspace_root;
    let execution_count_before_continue = database
        .query_row("SELECT count(*) FROM executions", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap();
    assert_eq!(execution_count_before_continue, 2);
    assert!(
        store
            .workspace_claim(source_claim_root.clone())
            .await
            .unwrap()
            .is_none()
    );
    let continued = service
        .checked_operation(
            json!({"action":"continue","executionId":"source","requestKey":"continue-key","prompt":"continue"}),
            None,
        )
        .await;
    assert_eq!(continued["error"]["code"], "AGENT_CONTINUE_NOT_ALLOWED");
    assert_eq!(
        database
            .query_row("SELECT count(*) FROM executions", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        execution_count_before_continue
    );
    assert!(
        store
            .workspace_claim(source_claim_root)
            .await
            .unwrap()
            .is_none()
    );

    let cancelled = service
        .checked_operation(json!({"action":"cancel","executionId":pending_id}), None)
        .await;
    assert_eq!(cancelled["error"]["code"], "AGENT_PROVIDER_UNAVAILABLE");

    let pending = store.execution(pending_id).await.unwrap().unwrap();
    assert!(pending.runtime_instance_id.is_none());
    assert_ne!(pending.release_evidence_state, "complete");
    assert!(
        store
            .workspace_claim(pending.canonical_workspace_root)
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(
        database
            .query_row("SELECT count(*) FROM runtime_instances", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    service.shutdown().await.unwrap();
}

#[tokio::test]
async fn thread_names_are_shared_persistent_without_control_revision_change() {
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::open(dir.path().into()).await.unwrap();
    for id in ["a", "b"] {
        store
            .product_create_fresh(
                id.into(),
                id.into(),
                "k".into(),
                "prompt".into(),
                "W".into(),
                w(dir.path(), "W"),
                10,
            )
            .await
            .unwrap();
        store.request_cancel(id.into(), 11).await.unwrap();
    }
    let c = rusqlite::Connection::open(dir.path().join("agent-state.db")).unwrap();
    c.execute("UPDATE executions SET thread_id='T'", [])
        .unwrap();
    let service = AgentProductService::new(store.clone());
    let before = service.observe("a".into(), false).await.unwrap();
    let record = store.execution("a".into()).await.unwrap();
    store
        .save_thread_name("T".into(), Some("官方名称".into()))
        .await
        .unwrap();
    let renamed = service.observe("a".into(), false).await.unwrap();
    assert_eq!(renamed.thread_name.as_deref(), Some("官方名称"));
    assert_eq!(renamed.revision, before.revision);
    assert_eq!(renamed.updated_at, before.updated_at);
    assert_eq!(store.execution("a".into()).await.unwrap(), record);
    let reopened = AgentProductService::new(StateStore::open(dir.path().into()).await.unwrap());
    let page = reopened.history_page(None, None).await.unwrap();
    assert!(
        page.executions
            .iter()
            .all(|r| r.thread_name.as_deref() == Some("官方名称") && r.final_result.is_none())
    );
    store.save_thread_name("T".into(), None).await.unwrap();
    assert!(
        reopened
            .observe("b".into(), false)
            .await
            .unwrap()
            .thread_name
            .is_none()
    );
}

#[tokio::test]
async fn provider_opaque_compatibility_projects_legacy_fields_and_safe_session_label() {
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::open(dir.path().into()).await.unwrap();
    store
        .product_create_fresh(
            "e".into(),
            "a".into(),
            "k".into(),
            "prompt".into(),
            "W".into(),
            w(dir.path(), "W"),
            10,
        )
        .await
        .unwrap();
    store.request_cancel("e".into(), 11).await.unwrap();
    let db = rusqlite::Connection::open(dir.path().join("agent-state.db")).unwrap();
    db.execute(
        "UPDATE executions SET thread_id='THREAD', turn_id='TURN' WHERE id='e'",
        [],
    )
    .unwrap();
    let service = AgentProductService::new(store.clone());

    let without_title =
        serde_json::to_value(service.observe("e".into(), false).await.unwrap()).unwrap();
    let assert_codex_provider = |view: &Value| {
        assert_eq!(view["provider"]["id"], "codex");
        assert_eq!(view["provider"]["displayName"], "Codex");
        assert!(
            view["provider"]["displayName"]
                .as_str()
                .is_some_and(|v| !v.is_empty())
        );
        assert!(view["provider"].get("version").is_none());
    };
    assert_codex_provider(&without_title);
    assert_eq!(without_title["threadId"], "THREAD");
    assert_eq!(without_title["threadName"], Value::Null);
    assert_eq!(without_title["turnId"], "TURN");
    assert_eq!(without_title["providerSessionLabel"], Value::Null);

    store
        .save_thread_name("THREAD".into(), Some("Safe persisted title".into()))
        .await
        .unwrap();
    let with_title =
        serde_json::to_value(service.observe("e".into(), false).await.unwrap()).unwrap();
    assert_eq!(with_title["threadId"], "THREAD");
    assert_eq!(with_title["threadName"], "Safe persisted title");
    assert_eq!(with_title["turnId"], "TURN");
    assert_eq!(with_title["providerSessionLabel"], "Safe persisted title");
    assert_codex_provider(&with_title);

    let observed = service
        .checked_operation(
            json!({"action":"observe","executionId":"e","waitMs":0}),
            None,
        )
        .await;
    assert_codex_provider(&observed["data"]);
    let listed = service
        .checked_operation(json!({"action":"list"}), None)
        .await;
    assert_codex_provider(&listed["data"]["executions"][0]);
    let execution_response = success(ProductData::Execution(Box::new(
        service.observe("e".into(), false).await.unwrap(),
    )));
    assert_codex_provider(&execution_response["data"]);
}

#[tokio::test]
async fn provider_opaque_identity_does_not_change_control_revision() {
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::open(dir.path().into()).await.unwrap();
    store
        .product_create_fresh(
            "e".into(),
            "a".into(),
            "k".into(),
            "prompt".into(),
            "W".into(),
            w(dir.path(), "W"),
            10,
        )
        .await
        .unwrap();
    store.request_cancel("e".into(), 11).await.unwrap();
    let service = AgentProductService::new(store.clone());
    let before = service.observe("e".into(), false).await.unwrap();
    let db = rusqlite::Connection::open(dir.path().join("agent-state.db")).unwrap();
    db.execute(
        "INSERT INTO runtime_instances(id,owner_host_instance_id,state,created_at,updated_at) VALUES ('RUNTIME','fixture','terminated',1,1)",
        [],
    )
    .unwrap();
    db.execute(
        "UPDATE executions SET runtime_instance_id='RUNTIME', thread_id='THREAD', turn_id='TURN' WHERE id='e'",
        [],
    )
    .unwrap();
    let with_provider_identity = service.observe("e".into(), false).await.unwrap();
    assert_eq!(
        with_provider_identity.control_revision,
        before.control_revision
    );
    assert_eq!(with_provider_identity.revision, before.revision);

    store
        .save_thread_name("THREAD".into(), Some("Safe persisted title".into()))
        .await
        .unwrap();
    let with_session_label = service.observe("e".into(), false).await.unwrap();
    assert_eq!(with_session_label.control_revision, before.control_revision);
    assert_eq!(
        with_session_label.provider_session_label.as_deref(),
        Some("Safe persisted title")
    );
}

#[test]
fn provider_opaque_compatibility_is_the_only_product_projection_allowlist() {
    let source = include_str!("../product.rs");
    let control_revision = source
        .split("fn control_revision(&self) -> String {")
        .nth(1)
        .unwrap()
        .split("fn activity_revision(&self) -> String {")
        .next()
        .unwrap();
    for identity in [
        "runtime_instance_id",
        "thread_id",
        "thread_name",
        "turn_id",
        "provider_session_label",
    ] {
        assert!(
            !control_revision.contains(identity),
            "control revision must exclude {identity}"
        );
    }

    let views = source.split("async fn views(").nth(1).unwrap();
    assert!(views.contains("let compatibility = ProviderOpaqueCompatibility::project(&s);"));
    for projection in [
        "thread_id: compatibility.thread_id",
        "thread_name: compatibility.thread_name",
        "turn_id: compatibility.turn_id",
        "provider_session_label: compatibility.provider_session_label",
    ] {
        assert!(views.contains(projection), "missing {projection}");
    }
    for direct_snapshot_identity in ["r.thread_id", "r.thread_name", "r.turn_id", "s.thread_name"] {
        assert!(
            !views.contains(direct_snapshot_identity),
            "views must use the compatibility projector, not {direct_snapshot_identity}"
        );
    }
    let private_thread_type = ["Codex", "Thread", "Info"].concat();
    let private_error_type =
        ["super::codex", "::protocol", "::", "Codex", "Error", "Info"].concat();
    assert!(!source.contains(&private_thread_type));
    assert!(!source.contains(&private_error_type));
}

#[tokio::test]
async fn desktop_history_pages_are_read_only_stable_and_workspace_filtered() {
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::open(dir.path().into()).await.unwrap();
    for n in 0..12 {
        let id = format!("e{n:02}");
        store
            .product_create_fresh(
                id.clone(),
                format!("a{n}"),
                "k".into(),
                "hello".into(),
                "W".into(),
                w(dir.path(), "W"),
                10,
            )
            .await
            .unwrap();
        store.request_cancel(id, 11).await.unwrap();
    }
    let other = dir.path().join("other");
    store
        .product_create_fresh(
            "other".into(),
            "other".into(),
            "k".into(),
            "hello".into(),
            "other".into(),
            w(&other, "other"),
            12,
        )
        .await
        .unwrap();
    let service = AgentProductService::new(store.clone());
    let root = store
        .execution("e00".into())
        .await
        .unwrap()
        .unwrap()
        .canonical_workspace_root;
    let before = store
        .product_read(None, None, None, 100)
        .await
        .unwrap()
        .into_iter()
        .map(|s| s.execution)
        .collect::<Vec<_>>();
    let first = service
        .history_page(None, Some(root.clone()))
        .await
        .unwrap();
    assert_eq!(first.executions.len(), 5);
    assert_eq!(first.executions[0].execution_id, "e11");
    let second = service
        .history_page(first.next_cursor, Some(root.clone()))
        .await
        .unwrap();
    assert_eq!(second.executions[0].execution_id, "e06");
    let third = service
        .history_page(second.next_cursor, Some(root))
        .await
        .unwrap();
    assert_eq!(third.executions.len(), 2);
    assert!(third.next_cursor.is_none());
    assert!(third.executions.iter().all(|r| r.final_result.is_none()));
    assert_eq!(
        service.history_page(None, None).await.unwrap().executions[0].execution_id,
        "other"
    );
    assert!(
        service
            .history_page(Some("missing".into()), None)
            .await
            .is_err()
    );
    let after = store
        .product_read(None, None, None, 100)
        .await
        .unwrap()
        .into_iter()
        .map(|s| s.execution)
        .collect::<Vec<_>>();
    assert_eq!(before, after);
}
async fn final_row(s: &AgentProductService, id: &str) -> ExecutionView {
    let end = tokio::time::Instant::now() + Duration::from_secs(90);
    loop {
        let row = s.observe(id.into(), true).await.unwrap();
        if matches!(
            row.status.as_str(),
            "completed" | "failed" | "cancelled" | "interrupted"
        ) {
            return row;
        }
        assert!(
            !matches!(row.status.as_str(), "unknown" | "reconciling"),
            "unexpected failure: {row:?}"
        );
        assert!(tokio::time::Instant::now() < end, "deadline {row:?}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}
#[test]
fn lineage_atomic_guards_and_read_projection() {
    run(async {
        let temp = tempfile::tempdir().unwrap();
        let store = StateStore::open(temp.path().into()).await.unwrap();
        let (a, b) = tokio::join!(
            store.product_create_fresh(
                "e1".into(),
                "a".into(),
                "k1".into(),
                "hello".into(),
                "W1".into(),
                w(temp.path(), "W1"),
                1
            ),
            store.product_create_fresh(
                "e2".into(),
                "a".into(),
                "k2".into(),
                "hello".into(),
                "W1".into(),
                w(temp.path(), "W1"),
                1
            )
        );
        assert_ne!(a.is_ok(), b.is_ok());
        let (ok, err) = match a {
            Ok(ok) => (ok, b.unwrap_err()),
            Err(err) => (b.unwrap(), err),
        };
        assert_eq!(err, "AGENT_LINEAGE_CONFLICT");
        assert_eq!(ok.execution.mode, "workspace_write");
        assert_eq!(ok.execution.provider, "codex");
        assert_eq!(ok.execution.execution_profile_json, "{}");
        let canonical = crate::agent::execution::canonicalize_request(
            serde_json::from_value(json!({
                "agent_id":"a", "request_key":ok.execution.request_key,
                "prompt":"hello", "execution_profile":{}, "workspace_id":"W1",
                "canonical_workspace_root":temp.path(), "provider":"codex",
                "mode":"workspace_write", "thread_id":null
            }))
            .unwrap(),
        )
        .unwrap();
        assert_eq!(ok.execution.request_hash, canonical.request_hash());
        let key = ok.execution.request_key.clone();
        let retry = store
            .product_create_fresh(
                "x".into(),
                "a".into(),
                key.clone(),
                "hello".into(),
                "W1".into(),
                None,
                2,
            )
            .await
            .unwrap();
        assert!(!retry.created);
        assert_eq!(retry.execution_id, ok.execution_id);
        assert_eq!(
            store
                .product_create_fresh(
                    "x".into(),
                    "a".into(),
                    key,
                    "other".into(),
                    "W1".into(),
                    None,
                    2
                )
                .await
                .unwrap_err(),
            "EXECUTION_REQUEST_KEY_CONFLICT"
        );
        store
            .request_cancel(ok.execution_id.clone(), 3)
            .await
            .unwrap();
        assert_eq!(
            store
                .product_create_fresh(
                    "x".into(),
                    "a".into(),
                    "new".into(),
                    "hello".into(),
                    "W2".into(),
                    w(temp.path(), "W2"),
                    4
                )
                .await
                .unwrap_err(),
            "AGENT_LINEAGE_CONFLICT"
        );
        let second = store
            .product_create_fresh(
                "e3".into(),
                "b".into(),
                "new".into(),
                "hello".into(),
                "W2".into(),
                w(temp.path(), "W2"),
                4,
            )
            .await
            .unwrap();
        let s = AgentProductService::new(store);
        let rows = s.views(None, None, None, 20, false).await.unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].execution_id, "e3");
        assert_eq!(rows[0].created_at, 4);
        assert!(rows[0].available_actions.can_resume_pending);
        assert_eq!(rows[0].attention, "pending_explicit_resume");
        assert_eq!(
            s.views(None, Some("a".into()), Some("W2".into()), 20, false)
                .await
                .unwrap()
                .len(),
            0
        );
        assert_eq!(s.checked_operation(json!({"action":"continue","executionId":ok.execution_id,"requestKey":"c","prompt":"hello"}),None).await["error"]["code"],"AGENT_CONTINUE_NOT_ALLOWED");
        assert_eq!(
            s.checked_operation(start("a", "new"), None).await["error"]["code"],
            "AGENT_LINEAGE_CONFLICT"
        );
        assert_eq!(
            s.store
                .execution(second.execution_id)
                .await
                .unwrap()
                .unwrap()
                .revision,
            0
        );
    });
}
#[test]
fn product_dto_rejects_unknown_fields_and_limits() {
    for v in [
        json!({"action":"start","agentId":"a","requestKey":"k","prompt":"p","threadId":"bad"}),
        json!({"action":"list","limit":0}),
        json!({"action":"list","limit":101}),
        json!({"action":"resume_pending","executionId":"e","prompt":"bad"}),
    ] {
        assert!(parse(v).is_err());
    }
    assert_eq!(
        failure("AGENT_SNAPSHOT_CONFLICT".into(), None)["error"]["code"],
        "AGENT_LINEAGE_CONFLICT"
    );
}

#[test]
fn execution_context_projection_is_frozen_and_read_only() {
    run(async {
        let temp = tempfile::tempdir().unwrap();
        let store = StateStore::open(temp.path().into()).await.unwrap();
        let prompt = "  原始任务\n第二行 <literal>  ";
        store
            .product_create_fresh(
                "context-e".into(),
                "context-a".into(),
                "key".into(),
                prompt.into(),
                "original".into(),
                w(temp.path(), "original"),
                1,
            )
            .await
            .unwrap();
        let before = store.execution("context-e".into()).await.unwrap().unwrap();
        let service = AgentProductService::new(store.clone());
        for request in [
            json!({"action":"observe","waitMs":0,"executionId":"context-e"}),
            json!({"action":"list"}),
        ] {
            let response = service
                .checked_operation(request.clone(), w(&temp.path().join("other"), "other"))
                .await;
            assert_eq!(response["ok"], true);
            let view = if request["action"] == "list" {
                &response["data"]["executions"][0]
            } else {
                &response["data"]
            };
            assert_eq!(view["prompt"], prompt);
            assert_eq!(
                view["canonicalWorkspaceRoot"],
                before.canonical_workspace_root
            );
            assert_eq!(view["workspaceId"], "original");
            assert!(view.get("runtimeInstanceId").is_none());
            assert!(view["revision"].is_string());
            assert!(view.get("executionRevision").is_none());
            assert!(view.get("diagnostics").is_none());
        }
        assert_eq!(
            store.execution("context-e".into()).await.unwrap().unwrap(),
            before
        );
        assert!(
            store
                .workspace_claim(before.canonical_workspace_root)
                .await
                .unwrap()
                .is_some()
        );
    });
}

#[cfg(windows)]
async fn fake_service(
    store: StateStore,
    db: std::path::PathBuf,
    runtime: &str,
    turn: &str,
    resume: bool,
    mode: &str,
) -> (
    AgentProductService,
    tokio::sync::oneshot::Sender<()>,
    tokio::task::JoinHandle<Vec<String>>,
) {
    let (service, release, fake, _turn_started) =
        fake_service_with_turn_started(store, db, runtime, turn, resume, mode).await;
    (service, release, fake)
}

/// 构造可显式观察首个 turn/start 的进程内 Provider 测试服务。
#[cfg(windows)]
async fn fake_service_with_turn_started(
    store: StateStore,
    db: std::path::PathBuf,
    runtime: &str,
    turn: &str,
    resume: bool,
    mode: &str,
) -> (
    AgentProductService,
    tokio::sync::oneshot::Sender<()>,
    tokio::task::JoinHandle<Vec<String>>,
    tokio::sync::oneshot::Receiver<()>,
) {
    let (wire, server) = tokio::io::duplex(128 * 1024);
    let (read, write) = tokio::io::split(wire);
    let client = Arc::new(Client::product_test_transport(
        runtime.into(),
        read,
        write,
        tokio::io::empty(),
    ));
    let mut manager = AgentTaskManager::new(store.clone(), "unused".into());
    let prompt_db = db.clone();
    let prompt_runtime = runtime.to_string();
    manager.test_client = Some((client.clone(), db));
    let (release, wait) = tokio::sync::oneshot::channel();
    let (turn_started_tx, turn_started) = tokio::sync::oneshot::channel();
    let turn = turn.to_string();
    let mode = mode.to_string();
    let fake = tokio::spawn(async move {
        let mut io = BufReader::new(server);
        let mut methods = Vec::new();
        let mut wait = Some(wait);
        let mut turn_started_tx = Some(turn_started_tx);
        loop {
            let mut line = String::new();
            if io.read_line(&mut line).await.unwrap() == 0 {
                break;
            }
            let v: Value = serde_json::from_str(&line).unwrap();
            let m = v["method"].as_str().unwrap();
            methods.push(m.into());
            let terminal = json!({"id":turn,"status":"completed","items":[],"itemsView":"summary"});
            let response = match m {
                "initialize" => {
                    json!({"userAgent":"fake","codexHome":"isolated","platformFamily":"windows","platformOs":"windows"})
                }
                "initialized" => continue,
                "thread/start" | "thread/resume" => {
                    if m == "thread/start" {
                        assert_eq!(v["params"]["sandbox"], "workspace-write");
                        assert_eq!(v["params"]["approvalPolicy"], "never");
                        assert_eq!(v["params"]["ephemeral"], false);
                        assert_eq!(v["params"]["historyMode"], "paginated");
                    }
                    assert_eq!(
                        m,
                        if resume {
                            "thread/resume"
                        } else {
                            "thread/start"
                        }
                    );
                    if mode == "missing" {
                        json!({"thread":{"id":"THREAD","turns":[]}})
                    } else {
                        json!({"thread":{"id":if mode=="wrong"{"WRONG"}else{"THREAD"},"turns":[],"historyMode":if mode=="wrong"{"paginated"}else{&mode}}})
                    }
                }
                "turn/start" => {
                    let db = rusqlite::Connection::open(&prompt_db).unwrap();
                    let prompt: String = db
                        .query_row(
                            "SELECT prompt FROM executions WHERE runtime_instance_id=?1",
                            [&prompt_runtime],
                            |r| r.get(0),
                        )
                        .unwrap();
                    assert_eq!(v["params"]["input"][0]["text"], prompt);
                    assert_eq!(v["params"]["threadId"], "THREAD");
                    assert_eq!(v["params"]["approvalPolicy"], "never");
                    let writable_roots = v["params"]["sandboxPolicy"]["writableRoots"]
                        .as_array()
                        .unwrap();
                    assert_eq!(writable_roots.len(), 1);
                    assert!(
                        writable_roots[0]
                            .as_str()
                            .is_some_and(|root| !root.is_empty())
                    );
                    assert_eq!(
                        v["params"]["sandboxPolicy"],
                        json!({
                            "type":"workspaceWrite",
                            "writableRoots":writable_roots,
                            "networkAccess":true,
                            "excludeTmpdirEnvVar":false,
                            "excludeSlashTmp":false
                        })
                    );
                    // acceptance 不代表 Turn 已开始；仅在 fake 收到请求后通知等待者。
                    if let Some(turn_started_tx) = turn_started_tx.take() {
                        let _ = turn_started_tx.send(());
                    }
                    wait.take().unwrap().await.unwrap();
                    let notification = json!({"method":"turn/completed","params":{"threadId":"THREAD","turn":terminal}});
                    io.write_all(format!("{notification}\n").as_bytes())
                        .await
                        .unwrap();
                    json!({"turn":{"id":turn,"status":"inProgress","items":[]}})
                }
                "thread/read" => {
                    json!({"thread":{"id":"THREAD","turns":[],"historyMode":"paginated"}})
                }
                "thread/turns/list" => json!({"data":[terminal],"nextCursor":null}),
                "thread/items/list" => {
                    assert_eq!(v["params"]["turnId"], turn);
                    json!({"data":[{"turnId":turn,"item":{"type":"agentMessage","id":"item","phase":"final_answer","text":"OK"}}],"nextCursor":null})
                }
                "thread/backgroundTerminals/clean" => json!({}),
                "thread/backgroundTerminals/list" => json!({"data":[],"nextCursor":null}),
                _ => panic!("unexpected method {m}"),
            };
            io.write_all(format!("{}\n", json!({"id":v["id"],"result":response})).as_bytes())
                .await
                .unwrap();
        }
        methods
    });
    (
        AgentProductService { store, manager },
        release,
        fake,
        turn_started,
    )
}
// 以下测试使用 fake Codex Runtime 或 Windows 启动恢复边界。
#[cfg(windows)]
#[test]
fn async_receipt_idempotency_and_exact_continuation() {
    run(async {
        let temp = tempfile::tempdir().unwrap();
        let store = StateStore::open(temp.path().into()).await.unwrap();
        let (s, release, fake) = fake_service(
            store.clone(),
            temp.path().join("agent-state.db"),
            "R1",
            "T1",
            false,
            "paginated",
        )
        .await;
        let receipt = tokio::time::timeout(
            Duration::from_secs(5),
            s.checked_operation(start("a", "k"), w(temp.path(), "W1")),
        )
        .await
        .unwrap();
        assert_eq!(receipt["ok"], true, "{receipt}");
        let id = receipt["data"]["executionId"].as_str().unwrap().to_string();
        for workspace in [None, w(temp.path(), "W2")] {
            let r = s.checked_operation(start("a", "k"), workspace).await;
            assert_eq!(r["data"]["executionId"], id);
        }
        assert_ne!(
            s.observe(id.clone(), false).await.unwrap().status,
            "completed"
        );
        release.send(()).unwrap();
        let first = final_row(&s, &id).await;
        assert!(first.available_actions.can_continue);
        let source = store.execution(id.clone()).await.unwrap().unwrap();
        assert_eq!(source.mode, "workspace_write");
        assert!(continuation_core_eligible(&source));
        let mut legacy_provenance = source.clone();
        legacy_provenance.mode = "read_only".into();
        assert!(!continuation_core_eligible(&legacy_provenance));
        assert_eq!(
            s.checked_operation(start("a", "new"), None).await["error"]["code"],
            "AGENT_LINEAGE_CONFLICT"
        );
        drop(s);
        let methods = fake.await.unwrap();
        assert_eq!(methods.iter().filter(|m| *m == "turn/start").count(), 1);
        let (s, release, fake) = fake_service(
            store.clone(),
            temp.path().join("agent-state.db"),
            "R2",
            "T2",
            true,
            "paginated",
        )
        .await;
        let req = json!({"action":"continue","executionId":id,"requestKey":"next","prompt":"next"});
        let r = s
            .checked_operation(req.clone(), w(temp.path(), "WRONG_ACTIVE"))
            .await;
        assert_eq!(r["ok"], true, "{r}");
        let id2 = r["data"]["executionId"].as_str().unwrap().to_string();
        assert_ne!(id, id2);
        // An exact retry is resolved before Provider provenance validation. This
        // fixture makes a fresh validation ineligible while retaining request hash bytes.
        rusqlite::Connection::open(temp.path().join("agent-state.db"))
            .unwrap()
            .execute(
                "UPDATE executions SET final_result_json=NULL WHERE id=?1",
                [&id],
            )
            .unwrap();
        assert_eq!(
            s.checked_operation(req, None).await["data"]["executionId"],
            id2
        );
        release.send(()).unwrap();
        let second = final_row(&s, &id2).await;
        assert_eq!(second.agent_id, first.agent_id);
        assert_eq!(second.workspace_id, first.workspace_id);
        assert_eq!(second.thread_id, first.thread_id);
        assert_ne!(second.turn_id, first.turn_id);
        let continued = store.execution(id2.clone()).await.unwrap().unwrap();
        assert_eq!(continued.mode, "workspace_write");
        assert_eq!(
            continued.canonical_workspace_root,
            source.canonical_workspace_root
        );
        assert_ne!(continued.runtime_instance_id, source.runtime_instance_id);
        assert_eq!(second.result_completeness, "complete");
        assert!(
            store
                .workspace_claim(temp.path().to_string_lossy().into())
                .await
                .unwrap()
                .is_none()
        );
        drop(s);
        let methods = fake.await.unwrap();
        assert_eq!(methods.iter().filter(|m| *m == "thread/resume").count(), 1);
        assert!(!methods.contains(&"thread/start".into()));
        assert_eq!(methods.iter().filter(|m| *m == "turn/start").count(), 1);
    });
}

#[test]
fn continuation_core_and_product_action_filter_are_provider_opaque() {
    // 仅标准化换行，避免 Windows checkout 改变架构边界断言的切片定位。
    let store_source = include_str!("../store/transactions/product.rs").replace("\r\n", "\n");
    let start = store_source
        .find("pub fn continuation_core_eligible")
        .unwrap();
    let end = store_source[start..]
        .find("/// Provider-opaque handoff")
        .unwrap()
        + start;
    let core = &store_source[start..=end];
    for forbidden in [
        "codex",
        "thread_id",
        "turn_id",
        "runtime_instance_id",
        "final_result_json",
        "historyMode",
    ] {
        assert!(!core.contains(forbidden), "core leaked {forbidden}");
    }

    let product_source = include_str!("../product.rs");
    let start = product_source
        .find("let actions = AvailableActions")
        .unwrap();
    let end = product_source[start..].find("let final_result").unwrap() + start;
    let actions = &product_source[start..end];
    for forbidden in [
        "thread_id",
        "turn_id",
        "runtime_instance_id",
        "final_result_json",
        "historyMode",
    ] {
        assert!(
            !actions.contains(forbidden),
            "Product action filter leaked {forbidden}"
        );
    }
    assert!(actions.contains("continuation_core_eligible"));
    assert!(actions.contains(".can_continue("));
}

/// 使用真实 Codex 验证产品层 start、continue、cancel 与 Runtime 收口。
#[cfg(any(windows, target_os = "macos"))]
#[test]
#[ignore = "Isolated fixed Codex Product start/continue/cancel E2E; run alone"]
fn real_fixed_product_continuation_e2e() {
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
    #[cfg(windows)]
    let auth =
        std::path::PathBuf::from(std::env::var_os("USERPROFILE").unwrap()).join(".codex/auth.json");
    #[cfg(target_os = "macos")]
    let auth = std::path::PathBuf::from(std::env::var_os("HOME").unwrap()).join(".codex/auth.json");
    // 凭据只复制到测试临时目录，不写入日志、断言或持久化 evidence。
    std::fs::copy(auth, home.join("auth.json")).unwrap();
    #[cfg(windows)]
    let evidence = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../docs/tasks/evidence/runtime-persistence")
        .join(format!("real-product-{}-{}", std::process::id(), now()));
    #[cfg(target_os = "macos")]
    let evidence = temp.path().join("evidence");
    std::fs::create_dir_all(&evidence).unwrap();
    struct Env(Vec<(&'static str, Option<std::ffi::OsString>)>);
    impl Drop for Env {
        fn drop(&mut self) {
            for (k, v) in &self.0 {
                unsafe {
                    if let Some(v) = v {
                        std::env::set_var(k, v)
                    } else {
                        std::env::remove_var(k)
                    }
                }
            }
        }
    }
    let _env = Env(vec![
        ("CODEX_HOME", std::env::var_os("CODEX_HOME")),
        (
            "SERENA_CONTRACT_RAW_DIR",
            std::env::var_os("SERENA_CONTRACT_RAW_DIR"),
        ),
    ]);
    unsafe {
        std::env::set_var("CODEX_HOME", &home);
        std::env::set_var("SERENA_CONTRACT_RAW_DIR", &evidence);
    }
    run(async {
        let store = StateStore::open(temp.path().join("store")).await.unwrap();
        let (s, startup) = AgentProductService::initialize(store.clone())
            .await
            .unwrap();
        assert!(startup.is_empty());
        let request = json!({"action":"start","agentId":"real-lineage","requestKey":"first","prompt":"Reply exactly PRODUCT_FIRST_OK. Do not use tools or modify files."});
        let receipt = s
            .checked_operation(request.clone(), w(&workspace, "real-workspace"))
            .await;
        std::fs::write(evidence.join("first-receipt.json"), receipt.to_string()).unwrap();
        assert_eq!(receipt["ok"], true, "{receipt}");
        let id = receipt["data"]["executionId"].as_str().unwrap();
        let e1 = final_row(&s, id).await;
        assert_eq!(e1.status, "completed");
        assert_eq!(
            s.checked_operation(request, None).await["data"]["executionId"],
            id
        );
        let r=s.checked_operation(json!({"action":"continue","executionId":id,"requestKey":"second","prompt":"Reply exactly PRODUCT_SECOND_OK. Do not use tools or modify files."}),None).await;
        std::fs::write(evidence.join("second-receipt.json"), r.to_string()).unwrap();
        assert_eq!(r["ok"], true, "{r}");
        let id2 = r["data"]["executionId"].as_str().unwrap();
        let e2 = final_row(&s, id2).await;
        assert_eq!(
            store
                .execution(id.into())
                .await
                .unwrap()
                .unwrap()
                .runtime_instance_id,
            store
                .execution(id2.into())
                .await
                .unwrap()
                .unwrap()
                .runtime_instance_id
        );
        assert_ne!(e1.execution_id, e2.execution_id);
        assert_eq!(e1.agent_id, e2.agent_id);
        assert_eq!(e1.thread_id, e2.thread_id);
        assert_ne!(e1.turn_id, e2.turn_id);
        assert_eq!(e2.status, "completed");
        assert_eq!(e2.result_completeness, "complete");
        assert!(
            store
                .workspace_claim(workspace.to_string_lossy().into())
                .await
                .unwrap()
                .is_none()
        );
        let list = s
            .checked_operation(json!({"action":"list","agentId":"real-lineage"}), None)
            .await;
        assert_eq!(list["data"]["executions"].as_array().unwrap().len(), 2);
        for (i, e) in [(1, &e1), (2, &e2)] {
            let row = store
                .execution(e.execution_id.clone())
                .await
                .unwrap()
                .unwrap();
            let runtime = row.runtime_instance_id.clone().unwrap();
            assert_eq!(
                store.runtime(runtime.clone()).await.unwrap().unwrap().state,
                "running"
            );
            let raw = std::fs::read_to_string(evidence.join(format!("{runtime}.stdin.raw.jsonl")))
                .unwrap();
            let messages: Vec<Value> = raw
                .lines()
                .map(|l| serde_json::from_str(l).unwrap())
                .collect();
            let count = |method: &str| messages.iter().filter(|m| m["method"] == method).count();
            assert_eq!(count("initialize"), 1);
            assert_eq!(count("turn/start"), 2);
            assert_eq!(count("thread/start"), 1);
            assert_eq!(count("thread/resume"), 0);
            std::fs::write(evidence.join(format!("execution-{i}.json")),serde_json::to_vec_pretty(&json!({"view":e,"runtime":runtime,"job":"running","claim":"absent","initialize":count("initialize"),"turnStart":count("turn/start"),"threadStart":count("thread/start"),"threadResume":count("thread/resume")})).unwrap()).unwrap();
        }
        let r=s.checked_operation(json!({"action":"start","agentId":"cancel-lineage","requestKey":"cancel-first","prompt":"Use the shell to sleep for 30 seconds, then reply DONE. Do not modify files."}),w(&workspace,"real-workspace")).await;
        assert_eq!(r["ok"], true);
        let id3 = r["data"]["executionId"].as_str().unwrap();
        let end = tokio::time::Instant::now() + Duration::from_secs(60);
        loop {
            let v = s.observe(id3.into(), false).await.unwrap();
            // The start ACK can reserve a Turn ID before the server accepts
            // interrupts. This case tests cancellation of an authoritative
            // started Turn; early rejected interrupts remain fail-closed.
            if v.status == "running" && v.turn_id.is_some() {
                break;
            }
            assert!(tokio::time::Instant::now() < end);
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let cancel = s
            .checked_operation(json!({"action":"cancel","executionId":id3}), None)
            .await;
        assert_eq!(cancel["ok"], true);
        let e3 = final_row(&s, id3).await;
        assert_eq!(e3.status, "cancelled");
        assert_eq!(e3.provider_terminal_status.as_deref(), Some("interrupted"));
        assert_eq!(
            e3.final_result.as_ref().unwrap()["terminalTurn"]["status"],
            "interrupted"
        );
        assert!(
            store
                .workspace_claim(workspace.to_string_lossy().into())
                .await
                .unwrap()
                .is_none()
        );
        let row = store.execution(id3.into()).await.unwrap().unwrap();
        let rid = row.runtime_instance_id.unwrap();
        s.shutdown().await.unwrap();
        let end = tokio::time::Instant::now() + Duration::from_secs(20);
        while store
            .runtime(rid.clone())
            .await
            .unwrap()
            .unwrap()
            .termination_evidence_state
            != "complete"
        {
            assert!(tokio::time::Instant::now() < end);
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        let raw = std::fs::read_to_string(evidence.join(format!("{rid}.stdin.raw.jsonl"))).unwrap();
        let messages: Vec<Value> = raw
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        for method in ["turn/start", "turn/interrupt"] {
            assert_eq!(
                messages.iter().filter(|m| m["method"] == method).count(),
                if method == "turn/start" { 3 } else { 1 }
            );
        }
        std::fs::write(
            evidence.join("cancel.json"),
            serde_json::to_vec_pretty(
                &json!({"view":e3,"runtime":rid,"job":"complete","claim":"absent"}),
            )
            .unwrap(),
        )
        .unwrap();
        let status = std::process::Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(&workspace)
            .output()
            .unwrap();
        assert!(status.status.success());
        assert!(status.stdout.is_empty());
        println!("TASK009_REAL_PRODUCT_EVIDENCE={}", evidence.display());
    });
}

#[cfg(windows)]
#[test]
fn caller_drop_between_create_and_handoff_keeps_owned_work() {
    run(async {
        let temp = tempfile::tempdir().unwrap();
        let store = StateStore::open(temp.path().into()).await.unwrap();
        let (mut s, release, fake) = fake_service(
            store.clone(),
            temp.path().join("agent-state.db"),
            "DROP",
            "T",
            false,
            "paginated",
        )
        .await;
        let hook = Arc::new((tokio::sync::Notify::new(), tokio::sync::Notify::new()));
        s.manager.test_handoff = Some(hook.clone());
        let s = Arc::new(s);
        let caller = s.clone();
        let workspace = w(temp.path(), "W");
        let request = tokio::spawn(async move {
            caller
                .checked_operation(start("a", "drop"), workspace)
                .await
        });
        hook.0.notified().await;
        request.abort();
        assert!(request.await.unwrap_err().is_cancelled());
        let rows = store.product_read(None, None, None, 20).await.unwrap();
        assert_eq!(rows.len(), 1);
        let id = rows[0].execution.id.clone();
        assert!(rows[0].owns_claim);
        hook.1.notify_one();
        release.send(()).unwrap();
        assert_eq!(final_row(&s, &id).await.status, "completed");
        drop(s);
        let methods = fake.await.unwrap();
        assert_eq!(methods.iter().filter(|m| *m == "turn/start").count(), 1);
    });
}
#[cfg(windows)]
#[test]
fn continuation_rejects_wrong_identity_and_legacy_without_turn() {
    run(async {
        for mode in ["wrong", "legacy", "missing"] {
            let temp = tempfile::tempdir().unwrap();
            let store = StateStore::open(temp.path().into()).await.unwrap();
            let (s, release, fake) = fake_service(
                store.clone(),
                temp.path().join("agent-state.db"),
                "FIRST",
                "T1",
                false,
                "paginated",
            )
            .await;
            let r = s
                .checked_operation(start("a", "k"), w(temp.path(), "W"))
                .await;
            let id = r["data"]["executionId"].as_str().unwrap().to_string();
            release.send(()).unwrap();
            final_row(&s, &id).await;
            drop(s);
            fake.await.unwrap();
            let (s, _release, fake) = fake_service(
                store.clone(),
                temp.path().join("agent-state.db"),
                "SECOND",
                "T2",
                true,
                mode,
            )
            .await;
            let r = s
                .checked_operation(
                    json!({"action":"continue","executionId":id,"requestKey":"c","prompt":"c"}),
                    None,
                )
                .await;
            assert_eq!(r["ok"], false, "{r}");
            assert_eq!(r["error"]["code"], "CODEX_APP_SERVER_INCOMPATIBLE");
            let id2 = r["error"]["executionId"].as_str().unwrap().to_string();
            let end = tokio::time::Instant::now() + Duration::from_secs(5);
            loop {
                let row = store.execution(id2.clone()).await.unwrap().unwrap();
                if row.status == "reconciling" {
                    assert_eq!(row.dispatch_state, "uncertain");
                    assert!(row.turn_id.is_none());
                    break;
                }
                assert!(tokio::time::Instant::now() < end);
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            assert!(
                store
                    .workspace_claim(temp.path().to_string_lossy().into())
                    .await
                    .unwrap()
                    .is_some()
            );
            drop(s);
            let methods = fake.await.unwrap();
            assert!(!methods.contains(&"turn/start".into()));
            assert!(!methods.contains(&"thread/start".into()));
            assert_eq!(methods.iter().filter(|m| *m == "thread/resume").count(), 1);
        }
    });
}
#[cfg(windows)]
#[test]
fn product_resume_receipt_and_duplicate_rejection() {
    run(async {
        let temp = tempfile::tempdir().unwrap();
        let store = StateStore::open(temp.path().into()).await.unwrap();
        store
            .product_create_fresh(
                "pending".into(),
                "a".into(),
                "k".into(),
                "hello".into(),
                "W".into(),
                w(temp.path(), "W"),
                1,
            )
            .await
            .unwrap();
        let (s, release, fake) = fake_service(
            store.clone(),
            temp.path().join("agent-state.db"),
            "RESUME",
            "T",
            false,
            "paginated",
        )
        .await;
        let req = json!({"action":"resume_pending","executionId":"pending"});
        let receipt = tokio::time::timeout(
            Duration::from_secs(5),
            s.checked_operation(req.clone(), None),
        )
        .await
        .unwrap();
        assert_eq!(receipt["ok"], true);
        assert_eq!(
            s.checked_operation(req, None).await["error"]["code"],
            "AGENT_RESUME_NOT_ALLOWED"
        );
        release.send(()).unwrap();
        assert_eq!(final_row(&s, "pending").await.status, "completed");
        drop(s);
        let methods = fake.await.unwrap();
        assert_eq!(methods.iter().filter(|m| *m == "turn/start").count(), 1);
    });
}

#[cfg(windows)]
#[test]
fn concurrent_identical_start_has_one_worker_and_busy_claim_is_independent() {
    run(async {
        let temp = tempfile::tempdir().unwrap();
        let store = StateStore::open(temp.path().into()).await.unwrap();
        let (s, release, fake) = fake_service(
            store.clone(),
            temp.path().join("agent-state.db"),
            "ONE",
            "T",
            false,
            "paginated",
        )
        .await;
        let (a, b) = tokio::join!(
            s.checked_operation(start("a", "k"), w(temp.path(), "W")),
            s.checked_operation(start("a", "k"), w(temp.path(), "W"))
        );
        assert_eq!(a["ok"], true);
        assert_eq!(b["ok"], true);
        assert_eq!(a["data"]["executionId"], b["data"]["executionId"]);
        assert_eq!(
            s.checked_operation(start("another-lineage", "k"), w(temp.path(), "W"))
                .await["error"]["code"],
            "WORKSPACE_CLAIM_CONFLICT"
        );
        release.send(()).unwrap();
        final_row(&s, a["data"]["executionId"].as_str().unwrap()).await;
        drop(s);
        let methods = fake.await.unwrap();
        assert_eq!(methods.iter().filter(|m| *m == "initialize").count(), 1);
        assert_eq!(methods.iter().filter(|m| *m == "turn/start").count(), 1);
    });
}
#[cfg(windows)]
#[test]
fn unknown_reads_and_invalid_continuation_sources_are_fail_closed() {
    run(async {
        for field in [
            "status",
            "release",
            "thread",
            "provenance",
            "wrong-result-owner",
            "claim",
        ] {
            let temp = tempfile::tempdir().unwrap();
            let store = StateStore::open(temp.path().into()).await.unwrap();
            let (s, release, fake) = fake_service(
                store.clone(),
                temp.path().join("agent-state.db"),
                "ORIGINAL",
                "T",
                false,
                "paginated",
            )
            .await;
            let r = s
                .checked_operation(start("a", "k"), w(temp.path(), "W"))
                .await;
            let id = r["data"]["executionId"].as_str().unwrap().to_string();
            release.send(()).unwrap();
            final_row(&s, &id).await;
            drop(s);
            fake.await.unwrap();
            let db = rusqlite::Connection::open(temp.path().join("agent-state.db")).unwrap();
            let sql = match field {
                "status" => "UPDATE executions SET status='unknown'",
                "release" => "UPDATE executions SET release_evidence_state='incomplete'",
                "thread" => "UPDATE executions SET thread_id=NULL",
                "provenance" => "UPDATE executions SET final_result_json=NULL",
                "wrong-result-owner" => {
                    "UPDATE executions SET final_result_json=json_set(final_result_json,'$.executionId','OTHER')"
                }
                _ => {
                    "INSERT INTO workspace_claims(canonical_workspace_root,execution_id,claim_type,acquired_at) SELECT canonical_workspace_root,id,'exclusive_execution',1 FROM executions"
                }
            };
            db.execute_batch(sql).unwrap();
            if field == "status" {
                set_v8_execution_state(&db, &id, "unknown", "dispatched", None, None);
            }
            let service = AgentProductService::new(store.clone());
            let before = store.execution(id.clone()).await.unwrap();
            let view = service
                .checked_operation(
                    json!({"action":"observe","waitMs":0,"executionId":id}),
                    None,
                )
                .await;
            assert_eq!(view["ok"], true);
            assert_eq!(view["data"]["availableActions"]["canContinue"], false);
            if field == "status" {
                assert_eq!(view["data"]["attention"], "manual_resolution_required");
                assert_eq!(
                    service
                        .checked_operation(json!({"action":"cancel","executionId":id}), None)
                        .await["error"]["code"],
                    "AGENT_MANUAL_RESOLUTION_REQUIRED"
                );
            }
            assert_eq!(
                service
                    .checked_operation(
                        json!({"action":"continue","executionId":id,"requestKey":"c","prompt":"c"}),
                        None
                    )
                    .await["error"]["code"],
                "AGENT_CONTINUE_NOT_ALLOWED"
            );
            assert_eq!(store.execution(id).await.unwrap(), before);
        }
    });
}

#[cfg(windows)]
#[test]
fn concurrent_new_continue_keys_have_one_creation_winner() {
    run(async {
        let temp = tempfile::tempdir().unwrap();
        let store = StateStore::open(temp.path().into()).await.unwrap();
        let (source_service, release, source_fake) = fake_service(
            store.clone(),
            temp.path().join("agent-state.db"),
            "SOURCE_RUNTIME",
            "SOURCE_TURN",
            false,
            "paginated",
        )
        .await;
        let source = source_service
            .checked_operation(start("continue-lineage", "source"), w(temp.path(), "W"))
            .await["data"]["executionId"]
            .as_str()
            .unwrap()
            .to_string();
        release.send(()).unwrap();
        final_row(&source_service, &source).await;
        drop(source_service);
        source_fake.await.unwrap();

        let (service, release, fake) = fake_service(
            store,
            temp.path().join("agent-state.db"),
            "CONTINUE_RUNTIME",
            "CONTINUE_TURN",
            true,
            "paginated",
        )
        .await;
        let (first, second) = tokio::join!(
            service.checked_operation(
                json!({"action":"continue","executionId":source,"requestKey":"first","prompt":"first"}),
                None,
            ),
            service.checked_operation(
                json!({"action":"continue","executionId":source,"requestKey":"second","prompt":"second"}),
                None,
            )
        );
        assert_ne!(first["ok"], second["ok"]);
        let (winner, loser) = if first["ok"] == true {
            (first, second)
        } else {
            (second, first)
        };
        assert!(
            ["AGENT_BUSY", "AGENT_CONTINUE_NOT_ALLOWED"]
                .contains(&loser["error"]["code"].as_str().unwrap()),
            "{loser}"
        );
        release.send(()).unwrap();
        final_row(&service, winner["data"]["executionId"].as_str().unwrap()).await;
        drop(service);
        let methods = fake.await.unwrap();
        assert_eq!(
            methods
                .iter()
                .filter(|method| *method == "thread/resume")
                .count(),
            1
        );
        assert_eq!(
            methods
                .iter()
                .filter(|method| *method == "turn/start")
                .count(),
            1
        );
    });
}

#[cfg(windows)]
#[test]
fn concurrent_new_keys_create_only_one_lineage_worker() {
    run(async {
        let temp = tempfile::tempdir().unwrap();
        let store = StateStore::open(temp.path().into()).await.unwrap();
        let (s, release, fake) = fake_service(
            store,
            temp.path().join("agent-state.db"),
            "WINNER",
            "T",
            false,
            "paginated",
        )
        .await;
        let (a, b) = tokio::join!(
            s.checked_operation(start("lineage", "k1"), w(temp.path(), "W")),
            s.checked_operation(start("lineage", "k2"), w(temp.path(), "W"))
        );
        assert_ne!(a["ok"], b["ok"]);
        let (winner, loser) = if a["ok"] == true { (a, b) } else { (b, a) };
        assert_eq!(loser["error"]["code"], "AGENT_LINEAGE_CONFLICT");
        release.send(()).unwrap();
        final_row(&s, winner["data"]["executionId"].as_str().unwrap()).await;
        drop(s);
        let methods = fake.await.unwrap();
        for m in ["initialize", "thread/start", "turn/start"] {
            assert_eq!(methods.iter().filter(|s| *s == m).count(), 1);
        }
    });
}

#[cfg(windows)]
#[test]
fn concurrent_continuations_allow_only_one_new_turn() {
    run(async {
        let temp = tempfile::tempdir().unwrap();
        let store = StateStore::open(temp.path().into()).await.unwrap();
        let (s, release, fake) = fake_service(
            store.clone(),
            temp.path().join("agent-state.db"),
            "SOURCE",
            "T1",
            false,
            "paginated",
        )
        .await;
        let receipt = s
            .checked_operation(start("a", "k"), w(temp.path(), "W"))
            .await;
        let source = receipt["data"]["executionId"].as_str().unwrap().to_string();
        release.send(()).unwrap();
        final_row(&s, &source).await;
        drop(s);
        fake.await.unwrap();
        let (s, release, fake) = fake_service(
            store.clone(),
            temp.path().join("agent-state.db"),
            "CONTINUED",
            "T2",
            true,
            "paginated",
        )
        .await;
        let request = |key: &str| json!({"action":"continue","executionId":source,"requestKey":key,"prompt":"next"});
        let (a, b) = tokio::join!(
            s.checked_operation(request("c1"), None),
            s.checked_operation(request("c2"), None)
        );
        assert_ne!(a["ok"], b["ok"]);
        let (winner, loser) = if a["ok"] == true { (a, b) } else { (b, a) };
        assert_eq!(loser["error"]["code"], "AGENT_BUSY");
        assert_eq!(loser["control"]["requestAccepted"], false);
        assert_eq!(loser["control"]["providerInvoked"], false);
        assert_eq!(
            loser["control"]["nextAction"]["executionId"],
            winner["data"]["executionId"]
        );
        assert_eq!(winner["control"]["requestAccepted"], true);
        assert!(
            store
                .workspace_claim(temp.path().to_string_lossy().into())
                .await
                .unwrap()
                .is_some()
        );
        release.send(()).unwrap();
        final_row(&s, winner["data"]["executionId"].as_str().unwrap()).await;
        drop(s);
        let methods = fake.await.unwrap();
        assert_eq!(methods.iter().filter(|m| *m == "thread/resume").count(), 1);
        assert_eq!(methods.iter().filter(|m| *m == "turn/start").count(), 1);
        assert!(!methods.contains(&"thread/start".into()));
    });
}

#[cfg(windows)]
#[test]
fn production_startup_recovery_classifies_claims_before_service_publication() {
    run(async {
        use crate::agent::provider::port::ProviderReconcileKind;
        for case in ["pending", "terminated", "unknown"] {
            let temp = tempfile::tempdir().unwrap();
            let store = StateStore::open(temp.path().into()).await.unwrap();
            store
                .product_create_fresh(
                    "startup-e".into(),
                    "lineage".into(),
                    "key".into(),
                    "hello".into(),
                    "W1".into(),
                    w(temp.path(), "W1"),
                    1,
                )
                .await
                .unwrap();
            let db = rusqlite::Connection::open(temp.path().join("agent-state.db")).unwrap();
            if case != "pending" {
                db.execute("INSERT INTO runtime_instances(id,owner_host_instance_id,state,created_at,updated_at,termination_evidence_state,termination_evidence_type,termination_evidence_at) VALUES ('R1','old-host',?1,1,1,?2,?3,?4)", rusqlite::params![if case=="terminated" {"terminated"} else {"unknown"},if case=="terminated" {"complete"} else {"unknown"},if case=="terminated" {Some("job_active_processes_zero")} else {None},if case=="terminated" {Some(10)} else {None}]).unwrap();
                set_v8_execution_state(
                    &db,
                    "startup-e",
                    if case == "terminated" {
                        "reconciling"
                    } else {
                        "unknown"
                    },
                    "uncertain",
                    None,
                    None,
                );
                db.execute(
                    "UPDATE executions SET runtime_instance_id='R1' WHERE id='startup-e'",
                    [],
                )
                .unwrap();
            }
            let before = store.execution("startup-e".into()).await.unwrap().unwrap();
            // This is the same publication barrier called by initialize() in Tauri setup.
            // A nonexistent executable proves pending/unknown cannot dispatch or launch R2.
            for _ in 0..3 {
                let manager =
                    AgentTaskManager::new(store.clone(), "C:/no-startup-provider.exe".into());
                let (service, outcomes) =
                    AgentProductService::recover_before_publish(store.clone(), manager)
                        .await
                        .unwrap();
                let row = store.execution("startup-e".into()).await.unwrap().unwrap();
                let claim = store
                    .workspace_claim(row.canonical_workspace_root.clone())
                    .await
                    .unwrap();
                match case {
                    "pending" => {
                        assert!(
                            matches!(&outcomes[0], super::ProviderReconcileItem { subject_id, kind: ProviderReconcileKind::ExecutionPendingExplicitResume } if subject_id=="startup-e")
                        );
                        assert_eq!(row, before);
                        assert!(claim.is_some());
                        assert_eq!(
                            service
                                .observe("startup-e".into(), false)
                                .await
                                .unwrap()
                                .attention,
                            "pending_explicit_resume"
                        );
                    }
                    "unknown" => {
                        assert!(matches!(
                            &outcomes[0],
                            super::ProviderReconcileItem {
                                kind: ProviderReconcileKind::ExecutionUnknown,
                                ..
                            }
                        ));
                        assert_eq!(row, before);
                        assert!(claim.is_some());
                        assert_eq!(
                            service
                                .observe("startup-e".into(), false)
                                .await
                                .unwrap()
                                .attention,
                            "manual_resolution_required"
                        );
                    }
                    _ => {
                        assert_eq!(row.status, "interrupted");
                        assert_eq!(row.runtime_instance_id.as_deref(), Some("R1"));
                        assert_eq!(row.release_evidence_state, "complete");
                        assert!(claim.is_none());
                    }
                }
                assert_eq!(
                    db.query_row("SELECT count(*) FROM runtime_instances", [], |r| r
                        .get::<_, i64>(0))
                        .unwrap(),
                    if case == "pending" { 0 } else { 1 }
                );
            }
        }
    });
}

#[cfg(windows)]
#[test]
fn unavailable_backend_production_initializer_preserves_recovery_and_local_reads() {
    run(async {
        use crate::agent::provider::port::ProviderReconcileKind;
        for code in ["BACKEND_UNAVAILABLE", "CODEX_APP_SERVER_INCOMPATIBLE"] {
            for case in ["pending", "unknown", "terminated", "needs-r2"] {
                let temp = tempfile::tempdir().unwrap();
                let store = StateStore::open(temp.path().into()).await.unwrap();
                store
                    .product_create_fresh(
                        "old".into(),
                        "a".into(),
                        "k".into(),
                        "hello".into(),
                        "W".into(),
                        w(temp.path(), "W"),
                        1,
                    )
                    .await
                    .unwrap();
                let db = rusqlite::Connection::open(temp.path().join("agent-state.db")).unwrap();
                if case != "pending" {
                    set_v8_execution_state(
                        &db,
                        "old",
                        if case == "unknown" {
                            "unknown"
                        } else {
                            "reconciling"
                        },
                        "uncertain",
                        None,
                        None,
                    );
                }
                if matches!(case, "terminated" | "needs-r2") {
                    db.execute("INSERT INTO runtime_instances(id,owner_host_instance_id,state,created_at,updated_at,termination_evidence_state,termination_evidence_type,termination_evidence_at) VALUES ('R1','old','terminated',1,1,'complete','job_active_processes_zero',10)",[]).unwrap();
                    db.execute(
                        "UPDATE executions SET runtime_instance_id='R1' WHERE id='old'",
                        [],
                    )
                    .unwrap();
                }
                if case == "needs-r2" {
                    db.execute("UPDATE executions SET thread_id='T',turn_id='turn',provider_terminal_status='interrupted',provider_terminal_evidence_at=9,provider_terminal_evidence_runtime_instance_id='R1' WHERE id='old'",[]).unwrap();
                }
                let before = store.execution("old".into()).await.unwrap().unwrap();
                let (service, outcomes) = TEST_DISCOVERY
                    .scope(
                        Err(format!("{code}: fixture")),
                        AgentProductService::initialize(store.clone()),
                    )
                    .await
                    .unwrap();
                assert_eq!(
                    service.backend_diagnostic(),
                    Some(format!("{code}: fixture").as_str())
                );
                let after = store.execution("old".into()).await.unwrap().unwrap();
                let claim = store
                    .workspace_claim(after.canonical_workspace_root.clone())
                    .await
                    .unwrap();
                match case {
                    "pending" => {
                        assert!(matches!(
                            &outcomes[0],
                            super::ProviderReconcileItem {
                                kind: ProviderReconcileKind::ExecutionPendingExplicitResume,
                                ..
                            }
                        ));
                        assert_eq!(before, after);
                        assert!(claim.is_some());
                        let r = service
                            .checked_operation(
                                json!({"action":"resume_pending","executionId":"old"}),
                                None,
                            )
                            .await;
                        assert_eq!(r["error"]["code"], code);
                        assert_eq!(
                            before,
                            store.execution("old".into()).await.unwrap().unwrap()
                        );
                    }
                    "unknown" => {
                        assert!(matches!(
                            &outcomes[0],
                            super::ProviderReconcileItem {
                                kind: ProviderReconcileKind::ExecutionUnknown,
                                ..
                            }
                        ));
                        assert_eq!(before, after);
                        assert!(claim.is_some());
                    }
                    "terminated" => {
                        assert!(matches!(
                            &outcomes[0],
                            super::ProviderReconcileItem {
                                kind: ProviderReconcileKind::ExecutionInterrupted,
                                ..
                            }
                        ));
                        assert_eq!(after.status, "interrupted");
                        assert!(claim.is_none());
                    }
                    _ => {
                        if code == "CODEX_APP_SERVER_INCOMPATIBLE" {
                            assert!(outcomes.is_empty());
                        } else {
                            assert!(matches!(
                                &outcomes[0],
                                super::ProviderReconcileItem {
                                    kind: ProviderReconcileKind::ExecutionProviderFailure,
                                    ..
                                }
                            ));
                        }
                        assert!(claim.is_some());
                        assert_ne!(after.result_completeness, "complete");
                    }
                }
                assert_eq!(
                    service
                        .checked_operation(
                            json!({"action":"observe","waitMs":0,"executionId":"old"}),
                            None
                        )
                        .await["ok"],
                    true
                );
                assert_eq!(
                    service
                        .checked_operation(json!({"action":"list"}), None)
                        .await["ok"],
                    true
                );
                assert_eq!(
                    db.query_row("SELECT count(*) FROM runtime_instances", [], |r| r
                        .get::<_, i64>(0))
                        .unwrap(),
                    if matches!(case, "terminated" | "needs-r2") {
                        1
                    } else {
                        0
                    }
                );
            }
            let temp = tempfile::tempdir().unwrap();
            let store = StateStore::open(temp.path().into()).await.unwrap();
            let (service, _) = TEST_DISCOVERY
                .scope(
                    Err(code.into()),
                    AgentProductService::initialize(store.clone()),
                )
                .await
                .unwrap();
            let r = service
                .checked_operation(start("new", "k"), w(temp.path(), "W"))
                .await;
            assert_eq!(r["ok"], false);
            assert_eq!(r["error"]["code"], code);
            let id = r["error"]["executionId"].as_str().unwrap();
            let row = store.execution(id.into()).await.unwrap().unwrap();
            assert_eq!(row.status, "dispatch_pending");
            assert!(row.runtime_instance_id.is_none());
            assert!(
                store
                    .workspace_claim(row.canonical_workspace_root)
                    .await
                    .unwrap()
                    .is_some()
            );
            assert_eq!(
                service.checked_operation(start("new", "k"), None).await["data"]["executionId"],
                id
            );
        }
    });
}

#[cfg(windows)]
#[test]
fn unavailable_continuation_never_accepts_or_dispatches() {
    run(async {
        let temp = tempfile::tempdir().unwrap();
        let store = StateStore::open(temp.path().into()).await.unwrap();
        let (first, release, fake) = fake_service(
            store.clone(),
            temp.path().join("agent-state.db"),
            "R1",
            "T1",
            false,
            "paginated",
        )
        .await;
        let r = first
            .checked_operation(start("a", "k"), w(temp.path(), "W"))
            .await;
        let id = r["data"]["executionId"].as_str().unwrap().to_owned();
        release.send(()).unwrap();
        final_row(&first, &id).await;
        drop(first);
        fake.await.unwrap();
        // This case tests backend discovery after a safely ended old Runtime.
        // An unresolved old Runtime is covered by workspace quarantine tests.
        rusqlite::Connection::open(temp.path().join("agent-state.db")).unwrap().execute(
            "UPDATE runtime_instances SET state='terminated',termination_evidence_state='complete',termination_evidence_type='job_active_processes_zero',termination_evidence_at=10 WHERE id='R1'",[]).unwrap();

        let (service, _) = TEST_DISCOVERY
            .scope(
                Err("BACKEND_UNAVAILABLE: missing".into()),
                AgentProductService::initialize(store.clone()),
            )
            .await
            .unwrap();
        let r = service
            .checked_operation(
                json!({"action":"continue","executionId":id,"requestKey":"c","prompt":"next"}),
                None,
            )
            .await;
        assert_eq!(r["ok"], false);
        assert_eq!(r["error"]["code"], "BACKEND_UNAVAILABLE");
        let row = store
            .execution(r["error"]["executionId"].as_str().unwrap().into())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.status, "dispatch_pending");
        assert!(row.runtime_instance_id.is_none());
        assert!(row.turn_id.is_none());
        assert!(
            store
                .workspace_claim(row.canonical_workspace_root)
                .await
                .unwrap()
                .is_some()
        );
        let db = rusqlite::Connection::open(temp.path().join("agent-state.db")).unwrap();
        assert_eq!(
            db.query_row("SELECT count(*) FROM runtime_instances", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
    });
}

// Validate real Product responses, including failures and nullable control, using
// the repository's existing frontend AJV installation (no runtime dependency).
impl AgentProductService {
    async fn checked_operation(&self, args: Value, workspace: Option<WorkspaceSnapshot>) -> Value {
        let mut args = args;
        // Legacy fixture shorthand: supply the caller's frozen workspace identity.
        // Explicit identity and malformed-input cases are never rewritten.
        if args["action"] == "start"
            && args.get("workspaceId").is_none()
            && args.get("agentId").is_some()
            && args.get("requestKey").is_some()
            && args.get("prompt").is_some()
        {
            let prior = self
                .store
                .product_read(None, args["agentId"].as_str().map(str::to_owned), None, 100)
                .await
                .unwrap_or_default();
            let expected = prior
                .iter()
                .find(|r| r.execution.request_key == args["requestKey"].as_str().unwrap())
                .map(|r| r.execution.workspace_id.clone())
                .or_else(|| workspace.as_ref().map(|w| w.id.clone()))
                .unwrap_or_else(|| "W".into());
            args["workspaceId"] = json!(expected);
        }
        let response = self.operation(args, workspace).await;
        assert_output_contract(&response);
        response
    }
}
fn assert_output_contract(response: &Value) {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let schema = crate::mcp::registry::agent_output_schema();
    let mut child = Command::new("node")
        .current_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap())
        .args(["-e", r#"
const Ajv = require('ajv');
let input = '';
process.stdin.on('data', chunk => input += chunk);
process.stdin.on('end', () => {
  const {schema, response} = JSON.parse(input);
  // P4-006 的公开 Usage revision 使用 uint64；测试 AJV 必须注册同一格式才能编译 execute schema。
  const validate = new Ajv({allErrors:true, formats:{
    uint64: {type:'number', validate:n => Number.isInteger(n) && n >= 0}
  }}).compile(schema);
  if (!validate(response)) throw new Error(JSON.stringify(validate.errors));
  // Prove the validator rejects a broken discriminator and narrowed control types.
  for (const changed of [
    {...response, ok: !response.ok},
    {...response, control:{requestAccepted:true,providerInvoked:'unknown',dispatchCertainty:'uncertain',nextAction:null}}
  ]) if (validate(changed)) throw new Error('invalid response accepted');
});
"#])
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped())
        .spawn().expect("Agent output contract tests require npm install and Node");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(
            json!({"schema":schema,"response":response})
                .to_string()
                .as_bytes(),
        )
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(windows)]
#[test]
#[ignore = "Isolated real fixed Codex long-turn smoke; run alone, temporary Git workspace only"]
fn real_fixed_long_turn_smoke() {
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
    std::fs::write(
        workspace.join("long-turn-fixture.ps1"),
        "Start-Sleep -Seconds 125\nWrite-Output LONG_TURN_SMOKE_COMPLETE\n",
    )
    .unwrap();
    let home = temp.path().join("home");
    std::fs::create_dir(&home).unwrap();
    std::fs::copy(
        std::path::PathBuf::from(std::env::var_os("USERPROFILE").unwrap()).join(".codex/auth.json"),
        home.join("auth.json"),
    )
    .unwrap();
    struct Environment(Option<std::ffi::OsString>);
    impl Drop for Environment {
        fn drop(&mut self) {
            unsafe {
                match &self.0 {
                    Some(v) => std::env::set_var("CODEX_HOME", v),
                    None => std::env::remove_var("CODEX_HOME"),
                }
            }
        }
    }
    let _restore = Environment(std::env::var_os("CODEX_HOME"));
    unsafe {
        std::env::set_var("CODEX_HOME", &home);
    }
    run(async {
        let store = StateStore::open(temp.path().join("store")).await.unwrap();
        let (service, _) = AgentProductService::initialize(store.clone())
            .await
            .unwrap();
        assert!(
            service.backend_diagnostic().is_none(),
            "{:?}",
            service.backend_diagnostic()
        );
        let began = std::time::Instant::now();
        let response=service.checked_operation(json!({"action":"start","agentId":"long-smoke","requestKey":"one","workspaceId":"smoke","prompt":"Execute the provided long-turn-fixture.ps1 engineering regression fixture using powershell.exe -NoProfile -File ./long-turn-fixture.ps1. This authorized fixture intentionally runs for 125 seconds. Set the shell command timeout to at least 180000 milliseconds. If the shell returns a running session, poll that exact session until the process exits. Do not finish early or interrupt the command. After its output LONG_TURN_SMOKE_COMPLETE is observed, report the result. Do not modify files or access the network."}),w(&workspace,"smoke")).await;
        assert_eq!(response["ok"], true, "{response}");
        let id = response["data"]["executionId"].as_str().unwrap();
        loop {
            let response = service
                .checked_operation(
                    json!({"action":"observe","executionId":id,"waitMs":0,"includeResult":true}),
                    None,
                )
                .await;
            assert_eq!(response["ok"], true, "{response}");
            let status = response["data"]["status"].as_str().unwrap();
            println!(
                "long smoke elapsed={}s status={status}",
                began.elapsed().as_secs()
            );
            assert!(
                !matches!(
                    status,
                    "reconciling" | "unknown" | "failed" | "cancelled" | "interrupted"
                ),
                "{response}"
            );
            if status == "completed" {
                std::fs::write(
                    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
                        "../docs/tasks/evidence/TASK-009/review-remediation/real-long-result.json",
                    ),
                    serde_json::to_vec_pretty(&response).unwrap(),
                )
                .unwrap();
                assert!(began.elapsed().as_secs() > 120);
                assert_eq!(response["data"]["resultCompleteness"], "complete");
                assert_eq!(response["data"]["resultAvailable"], true);
                assert!(response["data"].get("finalResult").is_some());
                assert!(
                    store
                        .workspace_claim(
                            response["data"]["canonicalWorkspaceRoot"]
                                .as_str()
                                .unwrap()
                                .into()
                        )
                        .await
                        .unwrap()
                        .is_none()
                );
                break;
            }
            assert!(
                began.elapsed() < Duration::from_secs(360),
                "test watchdog: {response}"
            );
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    });
}
