//! P4-006 公共 Usage 到 Product DTO 的投影回归测试。

use super::*;
use rmcp::schemars;
use rusqlite::{Connection, params};
use serde_json::{Value, json};

/// 创建一个可由 Product 读取的独立 Execution，避免 Agent lineage fixture 互相冲突。
async fn execution(store: &StateStore, root: &std::path::Path, id: &str, created_at: i64) {
    store
        .product_create_fresh(
            id.into(),
            format!("agent-{id}"),
            format!("key-{id}"),
            "prompt".into(),
            "workspace".into(),
            w(root, "workspace"),
            created_at,
        )
        .await
        .unwrap();
    store
        .request_cancel(id.into(), created_at + 1)
        .await
        .unwrap();
}

/// 仅为 Product projection fixture 写入公共表；不会创建或读取 private Usage state。
#[allow(clippy::too_many_arguments)]
fn usage(
    database: &Connection,
    execution_id: &str,
    provider_id: &str,
    input_tokens: Option<i64>,
    cached_input_tokens: Option<i64>,
    cache_write_input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    reasoning_tokens: Option<i64>,
    total_tokens: Option<i64>,
    model_context_window: Option<i64>,
    completeness: &str,
    revision: i64,
    updated_at: i64,
) {
    database
        .execute(
            "INSERT INTO execution_usage(
                execution_id,provider_id,input_tokens,cached_input_tokens,cache_write_input_tokens,
                output_tokens,reasoning_tokens,total_tokens,model_context_window,completeness,
                usage_revision,updated_at
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            params![
                execution_id,
                provider_id,
                input_tokens,
                cached_input_tokens,
                cache_write_input_tokens,
                output_tokens,
                reasoning_tokens,
                total_tokens,
                model_context_window,
                completeness,
                revision,
                updated_at,
            ],
        )
        .unwrap();
}

/// 读取序列化 Product Usage，直接断言 JSON public contract。
async fn product_usage(service: &AgentProductService, id: &str) -> Value {
    serde_json::to_value(service.observe(id.into(), false).await.unwrap()).unwrap()["usage"].clone()
}

/// 非 Codex Execution 只读取 public Usage；无行返回 unknown/null，错配仍拒绝整条快照。
#[tokio::test]
async fn fake_provider_public_usage_is_optional_and_never_reads_codex_private_state() {
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::open(dir.path().into()).await.unwrap();
    for (at, id) in [(1, "fake-none"), (2, "fake-public"), (3, "fake-mismatch")] {
        execution(&store, dir.path(), id, at).await;
    }
    let database = Connection::open(dir.path().join("agent-state.db")).unwrap();
    database
        .execute("UPDATE executions SET provider='fake-acp'", [])
        .unwrap();
    usage(
        &database,
        "fake-public",
        "fake-acp",
        Some(11),
        None,
        None,
        Some(4),
        None,
        Some(42),
        Some(128_000),
        "partial",
        7,
        20,
    );
    let service = AgentProductService::new(store);
    let unknown = json!({
        "inputTokens":null,"cachedInputTokens":null,"cacheWriteInputTokens":null,
        "outputTokens":null,"reasoningTokens":null,"totalTokens":null,
        "modelContextWindow":null,"completeness":"unknown","usageRevision":0,"updatedAt":null
    });
    let public = json!({
        "inputTokens":11,"cachedInputTokens":null,"cacheWriteInputTokens":null,
        "outputTokens":4,"reasoningTokens":null,"totalTokens":42,
        "modelContextWindow":128000,"completeness":"partial","usageRevision":7,"updatedAt":20
    });
    for (id, expected) in [("fake-none", unknown), ("fake-public", public)] {
        let detail = service
            .agent_query(AgentQueryAction::Get {
                execution_id: id.into(),
                include_result: Some(false),
            })
            .await
            .unwrap();
        let detail = success(detail);
        let observed = service
            .operation(
                json!({"action":"observe","executionId":id,"waitMs":0}),
                None,
            )
            .await;
        let listed = service.operation(json!({"action":"list"}), None).await;
        let listed = listed["data"]["executions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["executionId"] == id)
            .unwrap();
        for view in [&detail["data"], &observed["data"], listed] {
            assert_eq!(view["provider"]["id"], "fake-acp");
            assert_eq!(view["taskRole"], "general");
            assert_eq!(view["usage"], expected);
        }
    }
    for (table, expected) in [
        ("execution_usage", 1),
        ("codex_execution_usage_state", 0),
        ("codex_thread_usage_epochs", 0),
    ] {
        let count: i64 = database
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, expected, "{table}");
    }

    // public provider_id 与持久化 Execution.provider 错配必须保留原有 fail-closed 语义。
    usage(
        &database,
        "fake-mismatch",
        "codex",
        None,
        None,
        None,
        None,
        None,
        Some(1),
        None,
        "partial",
        1,
        21,
    );
    assert!(
        service
            .observe("fake-mismatch".into(), false)
            .await
            .is_err()
    );
    assert_eq!(
        service.operation(json!({"action":"list"}), None).await["ok"],
        false
    );
}

#[tokio::test]
async fn usage_product_defaults_and_persisted_values_preserve_null_zero_and_completeness() {
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::open(dir.path().into()).await.unwrap();
    for (index, id) in ["absent", "unknown", "partial", "complete", "zero"]
        .iter()
        .enumerate()
    {
        execution(&store, dir.path(), id, index as i64 + 1).await;
    }
    let database = Connection::open(dir.path().join("agent-state.db")).unwrap();
    usage(
        &database, "unknown", "codex", None, None, None, None, None, None, None, "unknown", 3, 13,
    );
    usage(
        &database,
        "partial",
        "codex",
        Some(11),
        None,
        None,
        None,
        None,
        Some(123),
        None,
        "partial",
        4,
        14,
    );
    // complete 是 generic persisted fixture；Codex 写路径不在本任务中改变。
    usage(
        &database,
        "complete",
        "codex",
        None,
        None,
        None,
        None,
        None,
        Some(456),
        None,
        "complete",
        5,
        15,
    );
    usage(
        &database,
        "zero",
        "codex",
        Some(0),
        Some(0),
        Some(0),
        Some(0),
        Some(0),
        Some(0),
        Some(0),
        "partial",
        6,
        16,
    );
    drop(database);
    let service = AgentProductService::new(store.clone());

    assert_eq!(
        product_usage(&service, "absent").await,
        json!({
            "inputTokens":null,"cachedInputTokens":null,"cacheWriteInputTokens":null,
            "outputTokens":null,"reasoningTokens":null,"totalTokens":null,
            "modelContextWindow":null,"completeness":"unknown","usageRevision":0,"updatedAt":null
        })
    );
    let unknown = product_usage(&service, "unknown").await;
    assert_eq!(unknown["completeness"], "unknown");
    assert!(unknown["totalTokens"].is_null());
    assert_eq!(unknown["usageRevision"], 3);
    assert_eq!(unknown["updatedAt"], 13);
    let partial = product_usage(&service, "partial").await;
    assert_eq!(partial["completeness"], "partial");
    assert_eq!(partial["totalTokens"], 123);
    assert_eq!(
        product_usage(&service, "complete").await["completeness"],
        "complete"
    );
    let zero = product_usage(&service, "zero").await;
    for field in [
        "inputTokens",
        "cachedInputTokens",
        "cacheWriteInputTokens",
        "outputTokens",
        "reasoningTokens",
        "totalTokens",
        "modelContextWindow",
    ] {
        assert_eq!(zero[field], 0, "zero must not become null for {field}");
    }

    let listed = service
        .checked_operation(json!({"action":"list","limit":5}), None)
        .await;
    let executions = listed["data"]["executions"].as_array().unwrap();
    assert_eq!(executions.len(), 5);
    for row in executions {
        let id = row["executionId"].as_str().unwrap();
        assert_eq!(row["provider"]["id"], "codex");
        assert_eq!(row["usage"], product_usage(&service, id).await);
    }
}

#[tokio::test]
async fn usage_product_projects_full_row_without_deriving_total_and_history_uses_it() {
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::open(dir.path().into()).await.unwrap();
    execution(&store, dir.path(), "full", 1).await;
    execution(&store, dir.path(), "known-breakdown-null-total", 2).await;
    let database = Connection::open(dir.path().join("agent-state.db")).unwrap();
    usage(
        &database,
        "full",
        "codex",
        Some(1),
        Some(2),
        Some(3),
        Some(4),
        Some(5),
        Some(15),
        Some(128_000),
        "partial",
        7,
        99,
    );
    usage(
        &database,
        "known-breakdown-null-total",
        "codex",
        Some(4),
        Some(5),
        Some(6),
        Some(7),
        Some(8),
        None,
        Some(256_000),
        "partial",
        8,
        100,
    );
    drop(database);
    let service = AgentProductService::new(store);
    assert_eq!(
        product_usage(&service, "full").await,
        json!({
            "inputTokens":1,"cachedInputTokens":2,"cacheWriteInputTokens":3,"outputTokens":4,
            "reasoningTokens":5,"totalTokens":15,"modelContextWindow":128000,
            "completeness":"partial","usageRevision":7,"updatedAt":99
        })
    );
    let known_breakdown = product_usage(&service, "known-breakdown-null-total").await;
    assert_eq!(known_breakdown["inputTokens"], 4);
    assert!(known_breakdown["totalTokens"].is_null());
    let history = service.history_page(None, None).await.unwrap();
    assert_eq!(
        history.executions[0].execution_id,
        "known-breakdown-null-total"
    );
    assert_eq!(
        serde_json::to_value(&history.executions[0]).unwrap()["usage"]["totalTokens"],
        Value::Null
    );
}

#[tokio::test]
async fn usage_product_fails_closed_for_provider_mismatch_and_invalid_persisted_values() {
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::open(dir.path().into()).await.unwrap();
    execution(&store, dir.path(), "mismatch", 1).await;
    execution(&store, dir.path(), "invalid-provider", 2).await;
    execution(&store, dir.path(), "mismatch-2", 3).await;
    let database = Connection::open(dir.path().join("agent-state.db")).unwrap();
    usage(
        &database,
        "mismatch",
        "other",
        None,
        None,
        None,
        None,
        None,
        Some(1),
        None,
        "partial",
        1,
        1,
    );
    usage(
        &database,
        "invalid-provider",
        "",
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        "unknown",
        0,
        0,
    );
    // DB constraints reject invalid generic completeness and negative public counters before Product can lie.
    assert!(database
        .execute(
            "INSERT INTO execution_usage(execution_id,provider_id,total_tokens,completeness,updated_at) VALUES ('mismatch-2','codex',-1,'invalid',0)",
            [],
        )
        .is_err());
    drop(database);
    let service = AgentProductService::new(store);
    assert!(service.observe("mismatch".into(), false).await.is_err());
    assert!(
        service
            .observe("invalid-provider".into(), false)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn usage_product_revision_is_independent_from_control_and_activity_revisions() {
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::open(dir.path().into()).await.unwrap();
    execution(&store, dir.path(), "revision", 1).await;
    let database = Connection::open(dir.path().join("agent-state.db")).unwrap();
    usage(
        &database,
        "revision",
        "codex",
        None,
        None,
        None,
        None,
        None,
        Some(123),
        None,
        "partial",
        1,
        10,
    );
    drop(database);
    let service = AgentProductService::new(store);
    let before = service.observe("revision".into(), false).await.unwrap();
    let database = Connection::open(dir.path().join("agent-state.db")).unwrap();
    database
        .execute(
            "UPDATE execution_usage SET total_tokens=124,usage_revision=2,updated_at=11 WHERE execution_id='revision'",
            [],
        )
        .unwrap();
    drop(database);
    let after = service.observe("revision".into(), false).await.unwrap();
    assert_eq!(before.control_revision, after.control_revision);
    assert_eq!(before.revision, after.revision);
    assert_eq!(before.activity_revision, after.activity_revision);
    assert_eq!(after.usage.total_tokens, Some(124));
    assert_eq!(after.usage.usage_revision, 2);
    assert_eq!(after.usage.updated_at, Some(11));
}

#[test]
fn usage_product_schema_is_typed_camel_case_and_product_read_carries_joined_usage() {
    let schema = schemars::schema_for!(ExecutionView);
    let schema = serde_json::to_value(schema).unwrap();
    let usage = &schema["$defs"]["UsageProduct"];
    for field in ["totalTokens", "completeness", "usageRevision", "updatedAt"] {
        assert!(
            usage["required"]
                .as_array()
                .unwrap()
                .contains(&json!(field)),
            "missing {field}: {usage}"
        );
        assert!(usage["properties"].get(field).is_some());
    }
    assert_eq!(
        schema["$defs"]["UsageCompletenessProduct"]["enum"],
        json!(["unknown", "partial", "complete"])
    );
    let public_usage = usage.to_string();
    for private_identity in [
        "runtimeInstanceId",
        "threadId",
        "turnId",
        "baseline",
        "telemetry",
    ] {
        assert!(!public_usage.contains(private_identity));
    }

    let store_source = include_str!("../store/transactions/product.rs");
    let read = store_source
        .split("pub async fn product_read")
        .nth(1)
        .unwrap();
    assert!(read.contains("LEFT JOIN execution_usage"));
    assert!(read.contains("execution_usage_snapshot_from_left_join"));
    let product_source = include_str!("../product.rs");
    let views = product_source.split("async fn views(").nth(1).unwrap();
    assert!(views.contains("UsageProduct::project(s.usage.as_ref())"));
    assert!(!views.contains("execution_usage("));
    assert!(!views.contains("codex_execution_usage_state"));
    assert!(!views.contains("codex_thread_usage_epochs"));

    let usage_product = UsageProduct::project(None);
    assert_eq!(
        usage_product.completeness,
        UsageCompletenessProduct::Unknown
    );
}
