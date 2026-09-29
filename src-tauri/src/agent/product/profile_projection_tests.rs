//! Execution 请求配置与实际配置到 Product DTO 的投影回归测试。

use super::*;
use rmcp::schemars;
use rusqlite::{Connection, params};
use serde_json::{Value, json};

/// 创建一个历史兼容 Execution，并直接设置其持久化冻结配置。
async fn execution_with_profile(
    store: &StateStore,
    root: &std::path::Path,
    id: &str,
    profile_json: &str,
    effective_profile_json: Option<&str>,
) {
    store
        .product_create_fresh(
            id.into(),
            format!("agent-{id}"),
            format!("key-{id}"),
            "prompt".into(),
            "workspace".into(),
            Some(WorkspaceSnapshot {
                id: "workspace".into(),
                root: root.to_string_lossy().into_owned(),
                generation: 1,
            }),
            1,
        )
        .await
        .unwrap();
    store.request_cancel(id.into(), 2).await.unwrap();
    let database = Connection::open(root.join("agent-state.db")).unwrap();
    database
        .execute(
            "UPDATE executions SET execution_profile_json=?1, effective_execution_profile_json=?2 WHERE id=?3",
            params![profile_json, effective_profile_json, id],
        )
        .unwrap();
}

/// 同时读取 detail、observe 与 list，锁定所有只读 Product 路径的同一投影。
async fn product_path_views(service: &AgentProductService, id: &str) -> Vec<Value> {
    let detail = service
        .agent_query(AgentQueryAction::Get {
            execution_id: id.into(),
            include_result: Some(false),
        })
        .await
        .unwrap();
    let detail = success(detail)["data"].clone();
    let observed = service
        .operation(
            json!({"action":"observe","executionId":id,"waitMs":0}),
            None,
        )
        .await["data"]
        .clone();
    let listed = service
        .operation(json!({"action":"list","limit":100}), None)
        .await["data"]["executions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["executionId"] == id)
        .unwrap()
        .clone();
    vec![detail, observed, listed]
}

/// 验证显式冻结值与历史空配置在所有 Product 读取路径中的投影一致。
#[tokio::test]
async fn frozen_execution_profile_is_identical_across_product_read_paths() {
    let directory = tempfile::tempdir().unwrap();
    let store = StateStore::open(directory.path().into()).await.unwrap();
    execution_with_profile(
        &store,
        directory.path(),
        "explicit-profile",
        r#"{"model":"historical-model","reasoning":"high"}"#,
        Some(r#"{"model":"effective-model","reasoning":"xhigh"}"#),
    )
    .await;
    execution_with_profile(&store, directory.path(), "empty-profile", "{}", None).await;
    execution_with_profile(
        &store,
        directory.path(),
        "effective-model-only",
        "{}",
        Some(r#"{"model":"effective-model"}"#),
    )
    .await;
    execution_with_profile(
        &store,
        directory.path(),
        "effective-reasoning-only",
        "{}",
        Some(r#"{"reasoning":"high"}"#),
    )
    .await;
    let service = AgentProductService::new(store);

    for view in product_path_views(&service, "explicit-profile").await {
        assert_eq!(
            view["executionProfile"],
            json!({"model":"historical-model","reasoning":"high"})
        );
        assert_eq!(
            view["effectiveExecutionProfile"],
            json!({"model":"effective-model","reasoning":"xhigh"})
        );
    }
    for view in product_path_views(&service, "empty-profile").await {
        assert_eq!(
            view["executionProfile"],
            json!({"model":null,"reasoning":null})
        );
        assert_eq!(view["effectiveExecutionProfile"], Value::Null);
    }
    for view in product_path_views(&service, "effective-model-only").await {
        assert_eq!(
            view["effectiveExecutionProfile"],
            json!({"model":"effective-model","reasoning":null})
        );
    }
    for view in product_path_views(&service, "effective-reasoning-only").await {
        assert_eq!(
            view["effectiveExecutionProfile"],
            json!({"model":null,"reasoning":"high"})
        );
    }
}

/// 持久化的 effective `{}` 没有任何 Provider 事实，Product 必须拒绝投影。
#[tokio::test]
async fn empty_effective_profile_is_rejected_by_product_projection() {
    let directory = tempfile::tempdir().unwrap();
    let store = StateStore::open(directory.path().into()).await.unwrap();
    execution_with_profile(
        &store,
        directory.path(),
        "empty-effective",
        "{}",
        Some("{}"),
    )
    .await;
    let service = AgentProductService::new(store);

    let error = service
        .agent_query(AgentQueryAction::Get {
            execution_id: "empty-effective".into(),
            include_result: Some(false),
        })
        .await
        .unwrap_err();
    assert_eq!(
        error.message,
        "Invalid persisted effective execution profile: incomplete evidence"
    );
}

/// 验证公共 schema 强制暴露 required-nullable 的模型与推理字段。
#[test]
fn execution_profile_schema_requires_nullable_model_and_reasoning() {
    let schema = serde_json::to_value(schemars::schema_for!(ExecutionView)).unwrap();
    let view = &schema;
    assert!(
        view["required"]
            .as_array()
            .unwrap()
            .contains(&json!("executionProfile"))
    );
    assert!(
        view["required"]
            .as_array()
            .unwrap()
            .contains(&json!("effectiveExecutionProfile"))
    );
    let profile = &schema["$defs"]["ExecutionProfileProduct"];
    for field in ["model", "reasoning"] {
        assert!(
            profile["required"]
                .as_array()
                .unwrap()
                .contains(&json!(field)),
            "missing required nullable field {field}: {profile}"
        );
        assert_eq!(
            profile["properties"][field]["type"],
            json!(["string", "null"]),
            "field is not nullable: {}",
            profile["properties"][field]
        );
    }
}
