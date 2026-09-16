use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use super::*;
use crate::agent::{
    activity::ToolCategory,
    provider::{
        ProviderId, ProviderOutcome, ProviderResultCompleteness,
        telemetry::{AgentActivityEvent, AgentTelemetryEvent, UsageEvent},
    },
};
use serde_json::json;

#[derive(Debug, PartialEq, Eq)]
enum ProviderCall {
    Descriptor,
    Capabilities,
    Execute(String),
    Cancel(String),
    ValidateContinuation(String),
    StartupReconcile,
}

#[derive(Default)]
struct FakeProvider {
    calls: Mutex<Vec<ProviderCall>>,
}

impl FakeProvider {
    fn record(&self, call: ProviderCall) {
        self.calls.lock().unwrap().push(call);
    }
}

impl AgentProvider for FakeProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        self.record(ProviderCall::Descriptor);
        ProviderDescriptor {
            id: ProviderId::new("fake".into()).unwrap(),
            display_name: "Fake Provider".into(),
            version: Some("1.0.0".into()),
        }
    }

    fn capabilities(&self) -> ProviderCapabilities {
        self.record(ProviderCall::Capabilities);
        ProviderCapabilities {
            can_execute: true,
            can_continue: false,
            can_cancel: true,
            can_recover: true,
            activity: false,
            token_usage: false,
        }
    }

    fn execute<'a>(
        &'a self,
        context: ProviderExecutionContext,
        acceptance: Arc<dyn ProviderAcceptanceSink>,
        _telemetry: Arc<dyn AgentEventSink>,
    ) -> ProviderFuture<'a, Result<ProviderRunResult, ProviderExecutionFailure>> {
        Box::pin(async move {
            self.record(ProviderCall::Execute(context.execution_id.clone()));
            acceptance.accepted();
            Ok(ProviderRunResult {
                execution_id: context.execution_id,
                outcome: ProviderOutcome::Completed,
                result: Some(json!({ "answer": 42 })),
                result_completeness: ProviderResultCompleteness::Complete,
                diagnostic_code: None,
            })
        })
    }

    fn cancel<'a>(
        &'a self,
        context: ProviderCancelContext,
    ) -> ProviderFuture<'a, Result<(), ProviderError>> {
        Box::pin(async move {
            self.record(ProviderCall::Cancel(context.execution_id));
            Ok(())
        })
    }

    fn validate_continuation<'a>(
        &'a self,
        context: ProviderContinuationContext,
    ) -> ProviderFuture<'a, Result<ProviderContinuationDecision, ProviderError>> {
        Box::pin(async move {
            self.record(ProviderCall::ValidateContinuation(
                context.source_execution_id,
            ));
            Ok(ProviderContinuationDecision::Eligible)
        })
    }

    fn startup_reconcile<'a>(
        &'a self,
        _context: ProviderStartupContext,
    ) -> ProviderFuture<'a, Result<ProviderReconcileSummary, ProviderError>> {
        Box::pin(async move {
            self.record(ProviderCall::StartupReconcile);
            Ok(ProviderReconcileSummary {
                items: vec![ProviderReconcileItem {
                    subject_id: "execution-1".into(),
                    kind: ProviderReconcileKind::ExecutionReleased,
                }],
            })
        })
    }
}

struct FakeEventSink;

impl AgentEventSink for FakeEventSink {}

#[derive(Default)]
struct FakeAcceptanceSink(AtomicUsize);

impl ProviderAcceptanceSink for FakeAcceptanceSink {
    fn accepted(&self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

fn assert_object_safe(_provider: Arc<dyn AgentProvider>) {}

fn assert_event_sink_object_safe(_sink: Arc<dyn AgentEventSink>) {}

#[derive(Default)]
struct RecordingEventSink {
    events: Mutex<Vec<AgentTelemetryEvent>>,
}

impl AgentEventSink for RecordingEventSink {
    fn publish<'a>(&'a self, event: AgentTelemetryEvent) -> ProviderFuture<'a, ()> {
        Box::pin(async move {
            self.events.lock().unwrap().push(event);
        })
    }
}

#[tokio::test]
async fn event_sink_is_object_safe_and_accepts_the_closed_telemetry_set() {
    let recording = Arc::new(RecordingEventSink::default());
    let sink: Arc<dyn AgentEventSink> = recording.clone();
    assert_event_sink_object_safe(sink.clone());

    sink.publish(AgentTelemetryEvent::Activity(AgentActivityEvent::tool(
        "execution-1".into(),
        ToolCategory::Test,
        10,
    )))
    .await;
    sink.publish(AgentTelemetryEvent::Usage(UsageEvent::new(
        "execution-1".into(),
    )))
    .await;

    assert_eq!(
        *recording.events.lock().unwrap(),
        [
            AgentTelemetryEvent::Activity(AgentActivityEvent::tool(
                "execution-1".into(),
                ToolCategory::Test,
                10,
            )),
            AgentTelemetryEvent::Usage(UsageEvent::new("execution-1".into())),
        ]
    );
}

#[tokio::test]
async fn event_sink_default_publish_is_a_noop() {
    let sink: Arc<dyn AgentEventSink> = Arc::new(FakeEventSink);

    sink.publish(AgentTelemetryEvent::Activity(AgentActivityEvent::provider(
        "execution-1".into(),
        10,
    )))
    .await;
}

#[test]
fn telemetry_contract_excludes_private_identity_evidence_and_wire_derives() {
    let source = include_str!("../telemetry.rs");

    for forbidden in [
        "runtime_id",
        "thread_id",
        "turn_id",
        "Terminal",
        "terminal",
        "Claim",
        "claim",
        "release",
        "cleanup",
        "job",
        "recovery",
        "reasoning",
        "argv",
        "stdout",
        "stderr",
        "prompt",
        "source",
        "diff",
        "Serialize",
        "Deserialize",
        "JsonSchema",
        "serde",
    ] {
        assert!(
            !source.contains(forbidden),
            "telemetry contract contains forbidden text {forbidden}"
        );
    }

    for private_field in [
        "pub execution_id:",
        "pub phase:",
        "pub tool_category:",
        "pub observed_at:",
    ] {
        assert!(
            !source.contains(private_field),
            "telemetry contract exposes field {private_field}"
        );
    }
}

#[tokio::test]
async fn trait_object_dynamically_dispatches_the_complete_provider_contract() {
    let fake = Arc::new(FakeProvider::default());
    let provider: Arc<dyn AgentProvider> = fake.clone();
    let acceptance = Arc::new(FakeAcceptanceSink::default());
    assert_object_safe(provider.clone());

    let descriptor = provider.descriptor();
    assert_eq!(descriptor.id.as_str(), "fake");
    assert_eq!(descriptor.display_name, "Fake Provider");
    assert_eq!(descriptor.version.as_deref(), Some("1.0.0"));

    let capabilities = provider.capabilities();
    assert!(capabilities.can_execute);
    assert!(!capabilities.can_continue);
    assert!(capabilities.can_cancel);
    assert!(capabilities.can_recover);
    assert!(!capabilities.activity);
    assert!(!capabilities.token_usage);

    let run_result = provider
        .execute(
            ProviderExecutionContext {
                execution_id: "execution-1".into(),
            },
            acceptance.clone(),
            Arc::new(FakeEventSink),
        )
        .await
        .unwrap();
    assert_eq!(
        run_result,
        ProviderRunResult {
            execution_id: "execution-1".into(),
            outcome: ProviderOutcome::Completed,
            result: Some(json!({ "answer": 42 })),
            result_completeness: ProviderResultCompleteness::Complete,
            diagnostic_code: None,
        }
    );
    assert_eq!(acceptance.0.load(Ordering::SeqCst), 1);

    provider
        .cancel(ProviderCancelContext {
            execution_id: "execution-1".into(),
        })
        .await
        .unwrap();
    assert_eq!(
        provider
            .validate_continuation(ProviderContinuationContext {
                source_execution_id: "execution-1".into(),
            })
            .await
            .unwrap(),
        ProviderContinuationDecision::Eligible
    );
    assert_eq!(
        serde_json::to_value(ProviderStartupContext {}).unwrap(),
        json!({})
    );
    assert_eq!(
        provider
            .startup_reconcile(ProviderStartupContext {})
            .await
            .unwrap(),
        ProviderReconcileSummary {
            items: vec![ProviderReconcileItem {
                subject_id: "execution-1".into(),
                kind: ProviderReconcileKind::ExecutionReleased,
            }],
        }
    );

    assert_eq!(
        *fake.calls.lock().unwrap(),
        [
            ProviderCall::Descriptor,
            ProviderCall::Capabilities,
            ProviderCall::Execute("execution-1".into()),
            ProviderCall::Cancel("execution-1".into()),
            ProviderCall::ValidateContinuation("execution-1".into()),
            ProviderCall::StartupReconcile,
        ]
    );
}

#[test]
fn continuation_validation_contract_is_internal_and_provider_agnostic() {
    let source = include_str!("../port.rs");
    let contract = source
        .split_once("pub struct ProviderContinuationContext")
        .unwrap()
        .1;
    let identifiers: Vec<_> = contract
        .split(|character: char| !character.is_alphanumeric() && character != '_')
        .filter(|identifier| !identifier.is_empty())
        .collect();

    for required in [
        "source_execution_id",
        "ProviderContinuationDecision",
        "Eligible",
        "Ineligible",
        "validate_continuation",
    ] {
        assert!(identifiers.contains(&required));
    }
    for forbidden in [
        "Serialize",
        "Deserialize",
        "serde",
        "thread",
        "turn",
        "runtime",
        "job",
        "historyMode",
        "history_mode",
        "Claim",
        "claim",
        "evidence",
        "finalResult",
        "final_result",
    ] {
        assert!(
            !identifiers.contains(&forbidden),
            "continuation validation contains forbidden contract identifier {forbidden}"
        );
    }
}

#[test]
fn reconcile_summary_exposes_exactly_the_frozen_typed_contract() {
    fn kind_name(kind: ProviderReconcileKind) -> &'static str {
        match kind {
            ProviderReconcileKind::OrphanResourceRecovered => "orphan_resource_recovered",
            ProviderReconcileKind::OrphanResourceUnknown => "orphan_resource_unknown",
            ProviderReconcileKind::ExecutionReleased => "execution_released",
            ProviderReconcileKind::ExecutionInconsistent => "execution_inconsistent",
            ProviderReconcileKind::ExecutionPendingExplicitResume => {
                "execution_pending_explicit_resume"
            }
            ProviderReconcileKind::ExecutionUnknown => "execution_unknown",
            ProviderReconcileKind::ExecutionProviderFailure => "execution_provider_failure",
            ProviderReconcileKind::ExecutionInterrupted => "execution_interrupted",
        }
    }

    let items: Vec<_> = [
        ProviderReconcileKind::OrphanResourceRecovered,
        ProviderReconcileKind::OrphanResourceUnknown,
        ProviderReconcileKind::ExecutionReleased,
        ProviderReconcileKind::ExecutionInconsistent,
        ProviderReconcileKind::ExecutionPendingExplicitResume,
        ProviderReconcileKind::ExecutionUnknown,
        ProviderReconcileKind::ExecutionProviderFailure,
        ProviderReconcileKind::ExecutionInterrupted,
    ]
    .into_iter()
    .map(|kind| ProviderReconcileItem {
        subject_id: kind_name(kind).into(),
        kind,
    })
    .collect();

    let summary = ProviderReconcileSummary {
        items: items.clone(),
    };
    assert_eq!(summary.items, items);

    let source = include_str!("../port.rs");
    let summary_contract = source
        .split_once("pub trait ProviderAcceptanceSink")
        .unwrap()
        .1
        .split_once("pub trait AgentProvider")
        .unwrap()
        .0;
    let identifiers: Vec<_> = summary_contract
        .split(|character: char| !character.is_alphanumeric() && character != '_')
        .filter(|identifier| !identifier.is_empty())
        .collect();

    for required in [
        "items",
        "ProviderReconcileItem",
        "subject_id",
        "kind",
        "ProviderReconcileKind",
    ] {
        assert!(identifiers.contains(&required));
    }
    for forbidden in [
        "Serialize",
        "Deserialize",
        "serde",
        "Runtime",
        "runtime",
        "thread",
        "turn",
        "job",
        "historyMode",
        "history_mode",
        "Claim",
        "claim",
        "identity",
        "evidence",
    ] {
        assert!(
            !identifiers.contains(&forbidden),
            "reconcile summary contains forbidden contract identifier {forbidden}"
        );
    }
}

#[test]
fn production_port_uses_only_the_frozen_object_safe_boundary() {
    let source = include_str!("../port.rs");

    assert!(source.contains("pub trait AgentEventSink: Send + Sync {"));
    assert!(source.contains("fn publish<'a>("));
    assert!(source.contains(
        "pub enum ProviderExecutionFailure {\n    State(String),\n    Runtime { code: String, message: String },\n}"
    ));

    for forbidden in [
        "async_trait",
        "async fn",
        "Serialize",
        "Deserialize",
        "serde",
        "RuntimeFailure",
        "RuntimePool",
        "runtime",
        "owner",
        "identity",
        "Claim",
        "claim",
        "thread",
        "turn",
        "job",
        "historyMode",
        "history_mode",
        "TelemetryProjector",
        "terminal evidence",
    ] {
        assert!(
            !source.contains(forbidden),
            "production port contains forbidden contract text {forbidden}"
        );
    }
}
