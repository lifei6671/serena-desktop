use super::*;
use crate::agent::{
    activity::ToolCategory,
    execution::{CreateExecutionInput, canonicalize_request},
    provider::{
        ProviderId,
        port::AgentEventSink,
        telemetry::{AgentActivityEvent, AgentTelemetryEvent, UsageEvent},
    },
};
use serde_json::json;

fn input(root: &std::path::Path, key: &str) -> CreateExecutionInput {
    serde_json::from_value(json!({
        "agent_id": format!("telemetry-projector-{key}"),
        "request_key": key,
        "prompt": "telemetry test",
        "execution_profile": {},
        "workspace_id": key,
        "canonical_workspace_root": root.to_str().unwrap(),
        "mode": "read_only"
    }))
    .unwrap()
}

async fn create(store: &StateStore, id: &str, root: &std::path::Path, key: &str) -> String {
    store
        .create_execution(
            id.into(),
            canonicalize_request(input(root, key)).unwrap(),
            1,
        )
        .await
        .unwrap()
        .execution_id
}

/// 构造安全 Usage event，验证 projector 只委托 Store 而不理解私有 identity。
fn usage_event(execution_id: String) -> UsageEvent {
    UsageEvent::cumulative(
        execution_id,
        ProviderId::new("codex".into()).unwrap(),
        16,
        None,
        None,
        None,
        None,
        None,
        None,
        10,
    )
}

#[tokio::test]
async fn projector_binds_activity_and_delegates_usage_only_for_its_exact_execution() {
    let directory = tempfile::tempdir().unwrap();
    let store = StateStore::open(directory.path().into()).await.unwrap();
    let first = create(
        &store,
        "projector-first",
        directory.path(),
        "projector-first",
    )
    .await;
    let other_root = directory.path().join("other");
    std::fs::create_dir(&other_root).unwrap();
    let other = create(&store, "projector-other", &other_root, "projector-other").await;
    let projector = ExecutionTelemetryProjector::new(store.clone(), first.clone());

    projector
        .publish(AgentTelemetryEvent::Activity(AgentActivityEvent::tool(
            first.clone(),
            ToolCategory::Test,
            10,
        )))
        .await;
    projector
        .publish(AgentTelemetryEvent::Activity(AgentActivityEvent::tool(
            other.clone(),
            ToolCategory::Command,
            11,
        )))
        .await;
    // 注入仅 Usage Store 能消费的故障，证明 projector 对匹配 event 做透明委托。
    store.inject_observability_failure(crate::agent::store::ObservabilityFault::UsageProjection);
    projector
        .publish(AgentTelemetryEvent::Usage(usage_event(first.clone())))
        .await;

    let projected = store.execution(first).await.unwrap().unwrap();
    let untouched = store.execution(other).await.unwrap().unwrap();
    assert_eq!(projected.last_activity_at, Some(10));
    assert_eq!(projected.activity_phase.as_deref(), Some("tool"));
    assert_eq!(projected.tool_category.as_deref(), Some("test"));
    assert!(untouched.last_activity_at.is_none());
    assert!(untouched.activity_phase.is_none());
    assert!(untouched.tool_category.is_none());
    assert!(
        !store.observability_failure_pending(
            crate::agent::store::ObservabilityFault::UsageProjection
        )
    );
}

#[tokio::test]
async fn projector_swallows_activity_persistence_failures() {
    let directory = tempfile::tempdir().unwrap();
    let store = StateStore::open(directory.path().into()).await.unwrap();
    let execution_id = create(
        &store,
        "projector-failure",
        directory.path(),
        "projector-failure",
    )
    .await;
    store.inject_observability_failure(crate::agent::store::ObservabilityFault::Activity);
    let projector = ExecutionTelemetryProjector::new(store.clone(), execution_id.clone());

    projector
        .publish(AgentTelemetryEvent::Activity(AgentActivityEvent::provider(
            execution_id.clone(),
            10,
        )))
        .await;

    let row = store
        .execution(execution_id.clone())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.status, "dispatch_pending");
    assert!(row.last_activity_at.is_none());
    assert!(
        !store.observability_failure_pending(crate::agent::store::ObservabilityFault::Activity)
    );
}

#[test]
fn projector_stays_provider_agnostic_and_telemetry_stays_closed() {
    let source = include_str!("../telemetry_projector.rs");
    assert!(!source.contains("thread_id"));
    assert!(!source.contains("turn_id"));
    assert!(!source.contains("codex::"));
    assert!(source.contains("AgentTelemetryEvent::Activity"));
    assert!(source.contains("AgentTelemetryEvent::Usage"));
    assert!(source.contains("project_execution_usage"));
}
