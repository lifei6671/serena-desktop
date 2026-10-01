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

/// typed Direct 分支写入当前 Execution，失败仍只丢 telemetry，不改变 lifecycle。
#[tokio::test]
async fn projector_routes_direct_usage_without_codex_private_state() {
    use crate::agent::usage::{UsageCompleteness, UsageSnapshot};
    let directory = tempfile::tempdir().unwrap();
    let store = StateStore::open(directory.path().into()).await.unwrap();
    let mut request = input(directory.path(), "direct-projector");
    request.provider = ProviderId::new("codebuddy".into()).unwrap();
    store
        .create_execution(
            "direct-projector".into(),
            canonicalize_request(request).unwrap(),
            1,
        )
        .await
        .unwrap();
    let before = store
        .execution("direct-projector".into())
        .await
        .unwrap()
        .unwrap();
    let snapshot = UsageSnapshot {
        execution_id: "direct-projector".into(),
        provider_id: ProviderId::new("codebuddy".into()).unwrap(),
        input_tokens: Some(2),
        cached_input_tokens: None,
        cache_write_input_tokens: None,
        output_tokens: Some(0),
        reasoning_tokens: None,
        total_tokens: Some(2),
        model_context_window: None,
        completeness: UsageCompleteness::Complete,
        revision: 0,
        updated_at: 2,
    };
    let projector = ExecutionTelemetryProjector::new(store.clone(), "direct-projector".into());
    let mut foreign = snapshot.clone();
    foreign.execution_id = "foreign".into();
    projector
        .publish(AgentTelemetryEvent::Usage(UsageEvent::direct(foreign)))
        .await;
    assert!(
        store
            .execution_usage("direct-projector".into())
            .await
            .unwrap()
            .is_none()
    );
    projector
        .publish(AgentTelemetryEvent::Usage(UsageEvent::direct(
            snapshot.clone(),
        )))
        .await;
    let usage = store
        .execution_usage("direct-projector".into())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(usage.total_tokens, Some(2));
    assert_eq!(usage.input_tokens, Some(2));
    assert_eq!(usage.completeness, UsageCompleteness::Complete);
    // 不同的 late final 被拒绝；AgentEventSink 的成功返回不升级成 Provider failure。
    let mut late = snapshot;
    late.total_tokens = Some(3);
    projector
        .publish(AgentTelemetryEvent::Usage(UsageEvent::direct(late)))
        .await;
    assert_eq!(
        store
            .execution_usage("direct-projector".into())
            .await
            .unwrap()
            .unwrap(),
        usage
    );
    assert_eq!(
        store
            .execution("direct-projector".into())
            .await
            .unwrap()
            .unwrap(),
        before
    );
}
