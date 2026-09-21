use super::*;
use crate::agent::{
    activity::{ActivityPhase, ToolCategory},
    execution::{CreateExecutionInput, canonicalize_request},
    provider::{
        ProviderCancelContext, ProviderErrorCode, ProviderExecutionContext, ProviderStartupContext,
        port::{
            AgentEventSink, AgentProvider, ProviderAcceptanceSink, ProviderContinuationContext,
            ProviderContinuationDecision,
        },
        registry::{ProviderHealth, ProviderRegistry},
        telemetry::AgentTelemetryEvent,
    },
    task_manager::AgentTaskManager,
};
use serde_json::json;
use std::sync::Mutex;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

struct NoopEventSink;

impl AgentEventSink for NoopEventSink {}

/// 记录 adapter 发布值，验证绑定后的 Usage 能按接收顺序进入 sink。
#[derive(Default)]
struct RecordingEventSink {
    events: Mutex<Vec<AgentTelemetryEvent>>,
}

impl AgentEventSink for RecordingEventSink {
    fn publish<'a>(
        &'a self,
        event: AgentTelemetryEvent,
    ) -> crate::agent::provider::port::ProviderFuture<'a, ()> {
        Box::pin(async move {
            self.events.lock().unwrap().push(event);
        })
    }
}

struct NoopAcceptanceSink;

impl ProviderAcceptanceSink for NoopAcceptanceSink {
    fn accepted(&self) {}
}

fn run(future: impl std::future::Future<Output = ()>) {
    tokio::runtime::Runtime::new().unwrap().block_on(future)
}

async fn store() -> (tempfile::TempDir, StateStore) {
    let directory = tempfile::tempdir().unwrap();
    let store = StateStore::open(directory.path().into()).await.unwrap();
    (directory, store)
}

fn codex(store: StateStore) -> CodexProvider {
    CodexProvider {
        store,
        executable: "unused.exe".into(),
        backend_error: None,
        owner: "adapter-test".into(),
        runtime_pool: Default::default(),
    }
}

async fn continuation_source(
    store: &StateStore,
    root: &std::path::Path,
    missing: Option<&str>,
) -> String {
    let manager = AgentTaskManager::new(store.clone(), "must-not-launch.exe".into());
    let mut request = input(root);
    request.request_key = format!("continuation-{}", missing.unwrap_or("valid"));
    let id = manager.create(request).await.unwrap().execution_id;
    let runtime_id = format!("runtime-{}", missing.unwrap_or("valid"));
    let database = rusqlite::Connection::open(root.join("agent-state.db")).unwrap();
    database
        .execute(
            &format!(
                "INSERT INTO runtime_instances(id,owner_host_instance_id,state,created_at,updated_at,termination_evidence_state,termination_evidence_type,termination_evidence_at) VALUES ('{runtime_id}','adapter-test','terminated',1,1,'complete','job_active_processes_zero',1)"
            ),
            [],
        )
        .unwrap();
    let fields = match missing {
        Some("thread") => format!("turn_id='turn-1',runtime_instance_id='{runtime_id}',"),
        Some("turn") => format!("thread_id='thread-1',runtime_instance_id='{runtime_id}',"),
        Some("runtime") => "thread_id='thread-1',turn_id='turn-1',".into(),
        None => {
            format!("thread_id='thread-1',turn_id='turn-1',runtime_instance_id='{runtime_id}',")
        }
        Some(_) => panic!("unsupported continuation fixture"),
    };
    database
        .execute(
            &format!("UPDATE executions SET {fields}final_result_json=?1 WHERE id=?2"),
            [
                json!({
                    "historyMode": "paginated",
                    "executionId": id.as_str(),
                    "threadId": "thread-1",
                    "turnId": "turn-1",
                    "sourceRuntimeId": runtime_id,
                })
                .to_string(),
                id.clone(),
            ],
        )
        .unwrap();
    database
        .execute("DELETE FROM workspace_claims WHERE execution_id=?1", [&id])
        .unwrap();
    id
}

#[test]
fn execution_failures_map_to_the_provider_boundary_without_runtime_identity() {
    assert_eq!(
        provider_execution_failure(ExecutionFailure::State("AGENT_RUNTIME_QUARANTINED".into())),
        ProviderExecutionFailure::State("AGENT_RUNTIME_QUARANTINED".into())
    );
    assert_eq!(
        provider_execution_failure(ExecutionFailure::Runtime(
            super::super::runtime::RuntimeFailure {
                code: "CODEX_RUNTIME_TEST_FAILED",
                message: "safe diagnostic".into(),
                runtime: None,
            }
        )),
        ProviderExecutionFailure::Runtime {
            code: "CODEX_RUNTIME_TEST_FAILED".into(),
            message: "safe diagnostic".into(),
        }
    );
}

fn runtime_failure(code: &'static str) -> super::super::runtime::RuntimeFailure {
    super::super::runtime::RuntimeFailure {
        code,
        message: "private runtime failure".into(),
        runtime: None,
    }
}

#[test]
fn recovery_outcomes_map_all_frozen_kinds_in_input_order() {
    let outcomes = [
        RecoveryOutcome::OrphanRuntime {
            runtime_id: "orphan-recovered".into(),
            failure: None,
        },
        RecoveryOutcome::Released {
            execution_id: "released".into(),
        },
        RecoveryOutcome::OrphanRuntime {
            runtime_id: "orphan-unknown".into(),
            failure: Some(runtime_failure("ORPHAN_FAILURE")),
        },
        RecoveryOutcome::Inconsistent {
            execution_id: "inconsistent".into(),
            code: "PRIVATE_INCONSISTENCY",
        },
        RecoveryOutcome::PendingExplicitResume {
            execution_id: "pending-resume".into(),
        },
        RecoveryOutcome::Unknown {
            execution_id: "unknown".into(),
            failure: Some(runtime_failure("UNKNOWN_FAILURE")),
        },
        RecoveryOutcome::RuntimeFailure {
            execution_id: "provider-failure".into(),
            failure: runtime_failure("PROVIDER_FAILURE"),
        },
        RecoveryOutcome::Interrupted {
            execution: Box::new(row("interrupted", "unknown", None)),
            result_diagnostic: Some("private result diagnostic".into()),
        },
    ];

    assert_eq!(
        outcomes
            .iter()
            .map(provider_reconcile_item)
            .collect::<Vec<_>>(),
        vec![
            ProviderReconcileItem {
                subject_id: "orphan-recovered".into(),
                kind: ProviderReconcileKind::OrphanResourceRecovered,
            },
            ProviderReconcileItem {
                subject_id: "released".into(),
                kind: ProviderReconcileKind::ExecutionReleased,
            },
            ProviderReconcileItem {
                subject_id: "orphan-unknown".into(),
                kind: ProviderReconcileKind::OrphanResourceUnknown,
            },
            ProviderReconcileItem {
                subject_id: "inconsistent".into(),
                kind: ProviderReconcileKind::ExecutionInconsistent,
            },
            ProviderReconcileItem {
                subject_id: "pending-resume".into(),
                kind: ProviderReconcileKind::ExecutionPendingExplicitResume,
            },
            ProviderReconcileItem {
                subject_id: "unknown".into(),
                kind: ProviderReconcileKind::ExecutionUnknown,
            },
            ProviderReconcileItem {
                subject_id: "provider-failure".into(),
                kind: ProviderReconcileKind::ExecutionProviderFailure,
            },
            ProviderReconcileItem {
                subject_id: "execution-1".into(),
                kind: ProviderReconcileKind::ExecutionInterrupted,
            },
        ]
    );
}

fn input(root: &std::path::Path) -> CreateExecutionInput {
    serde_json::from_value(json!({
        "agent_id": "adapter-agent",
        "request_key": "adapter-key",
        "prompt": "adapter test",
        "execution_profile": {},
        "workspace_id": "adapter-workspace",
        "canonical_workspace_root": root.to_str().unwrap(),
        "mode": "read_only"
    }))
    .unwrap()
}

async fn prepare_backend_recovery(store: &StateStore, root: &std::path::Path) -> String {
    let manager = AgentTaskManager::new(store.clone(), "must-not-launch.exe".into());
    let id = manager.create(input(root)).await.unwrap().execution_id;
    let database = rusqlite::Connection::open(root.join("agent-state.db")).unwrap();
    database
        .execute(
            "INSERT INTO runtime_instances(id,owner_host_instance_id,state,created_at,updated_at,termination_evidence_state,termination_evidence_type,termination_evidence_at) VALUES ('R1','old-host','terminated',1,1,'complete','job_active_processes_zero',10)",
            [],
        )
        .unwrap();
    database
        .execute(
            "UPDATE executions SET status='reconciling',dispatch_state='uncertain',runtime_instance_id='R1' WHERE id=?1",
            [&id],
        )
        .unwrap();
    drop(database);
    let row = store.execution(id.clone()).await.unwrap().unwrap();
    store
        .bind_protocol_identity(
            id.clone(),
            row.revision,
            "R1".into(),
            "thread-1".into(),
            Some("turn-1".into()),
            crate::agent::coordinator::now(),
        )
        .await
        .unwrap();
    id
}

fn row(status: &str, completeness: &str, final_result_json: Option<String>) -> ExecutionRecord {
    ExecutionRecord {
        id: "execution-1".into(),
        agent_id: "agent-1".into(),
        request_key: "request-1".into(),
        request_hash: "hash-1".into(),
        prompt: "prompt".into(),
        execution_profile_json: "{}".into(),
        workspace_id: "workspace-1".into(),
        canonical_workspace_root: "C:/workspace".into(),
        workspace_generation: 1,
        provider: "codex".into(),
        mode: "read_only".into(),
        parent_execution_id: None,
        thread_id: Some("private-thread".into()),
        turn_id: Some("private-turn".into()),
        provider_terminal_status: Some(status.into()),
        error_code: Some("STABLE_DIAGNOSTIC".into()),
        error_message: Some("private raw message".into()),
        provider_terminal_evidence_runtime_instance_id: Some("private-runtime".into()),
        provider_terminal_evidence_at: Some(1),
        runtime_instance_id: Some("private-runtime".into()),
        status: status.into(),
        dispatch_state: "dispatched".into(),
        revision: 7,
        background_cleanup_state: "empty".into(),
        release_evidence_state: "complete".into(),
        release_evidence_kind: Some("private-release".into()),
        release_evidence_json: Some("{\"job\":\"private-job\"}".into()),
        result_completeness: completeness.into(),
        final_result_json,
        interrupt_requested_at: None,
        interrupt_ack_at: None,
        interrupt_timeout_at: None,
        interrupt_diagnostic: Some("private interrupt diagnostic".into()),
        last_activity_at: Some(2),
        activity_phase: Some("provider".into()),
        tool_category: Some("command".into()),
        activity_summary_code: None,
        activity_sequence: 0,
    }
}

#[test]
fn descriptor_and_capabilities_match_the_connected_port() {
    run(async {
        let (_directory, store) = store().await;
        let provider = codex(store);

        let descriptor = provider.descriptor();
        assert_eq!(descriptor.id.as_str(), "codex");
        assert_eq!(descriptor.display_name, "Codex");
        assert_eq!(
            descriptor.version.as_deref(),
            Some(super::super::protocol::VERSION)
        );
        assert_eq!(
            provider.capabilities(),
            ProviderCapabilities {
                can_execute: true,
                can_continue: true,
                can_cancel: true,
                can_recover: true,
                activity: true,
                token_usage: false,
            }
        );
    });
}

#[test]
fn continuation_validation_accepts_only_matching_paginated_managed_provenance() {
    run(async {
        let (directory, store) = store().await;
        let id = continuation_source(&store, directory.path(), None).await;
        let provider = codex(store.clone());
        let context = || ProviderContinuationContext {
            source_execution_id: id.clone(),
        };

        assert_eq!(
            provider.validate_continuation(context()).await.unwrap(),
            ProviderContinuationDecision::Eligible
        );

        let database = rusqlite::Connection::open(directory.path().join("agent-state.db")).unwrap();
        for result in [
            json!({
                "historyMode": "legacy",
                "executionId": id.as_str(),
                "threadId": "thread-1",
                "turnId": "turn-1",
                "sourceRuntimeId": "runtime-1",
            }),
            json!({
                "executionId": id.as_str(),
                "threadId": "thread-1",
                "turnId": "turn-1",
                "sourceRuntimeId": "runtime-1",
            }),
            json!({
                "historyMode": "paginated",
                "executionId": "other-execution",
                "threadId": "thread-1",
                "turnId": "turn-1",
                "sourceRuntimeId": "runtime-1",
            }),
            json!({
                "historyMode": "paginated",
                "threadId": "thread-1",
                "turnId": "turn-1",
                "sourceRuntimeId": "runtime-1",
            }),
            json!({
                "historyMode": "paginated",
                "executionId": id.as_str(),
                "threadId": "other-thread",
                "turnId": "turn-1",
                "sourceRuntimeId": "runtime-1",
            }),
            json!({
                "historyMode": "paginated",
                "executionId": id.as_str(),
                "turnId": "turn-1",
                "sourceRuntimeId": "runtime-1",
            }),
            json!({
                "historyMode": "paginated",
                "executionId": id.as_str(),
                "threadId": "thread-1",
                "turnId": "other-turn",
                "sourceRuntimeId": "runtime-1",
            }),
            json!({
                "historyMode": "paginated",
                "executionId": id.as_str(),
                "threadId": "thread-1",
                "sourceRuntimeId": "runtime-1",
            }),
            json!({
                "historyMode": "paginated",
                "executionId": id.as_str(),
                "threadId": "thread-1",
                "turnId": "turn-1",
                "sourceRuntimeId": "other-runtime",
            }),
            json!({
                "historyMode": "paginated",
                "executionId": id.as_str(),
                "threadId": "thread-1",
                "turnId": "turn-1",
            }),
        ] {
            database
                .execute(
                    "UPDATE executions SET final_result_json=?1 WHERE id=?2",
                    [result.to_string(), id.clone()],
                )
                .unwrap();
            assert_eq!(
                provider.validate_continuation(context()).await.unwrap(),
                ProviderContinuationDecision::Ineligible
            );
        }

        for missing in ["thread", "turn", "runtime"] {
            let (missing_directory, missing_store) = self::store().await;
            let missing_id =
                continuation_source(&missing_store, missing_directory.path(), Some(missing)).await;
            assert_eq!(
                codex(missing_store)
                    .validate_continuation(ProviderContinuationContext {
                        source_execution_id: missing_id,
                    })
                    .await
                    .unwrap(),
                ProviderContinuationDecision::Ineligible
            );
        }
    });
}

#[test]
fn continuation_validation_maps_missing_and_unreadable_sources_to_stable_provider_errors() {
    run(async {
        let (directory, store) = store().await;
        let provider = codex(store.clone());
        let missing = provider
            .validate_continuation(ProviderContinuationContext {
                source_execution_id: "missing-source".into(),
            })
            .await
            .unwrap_err();
        assert_eq!(
            missing.code,
            ProviderErrorCode::AgentProviderOperationFailed
        );

        let database = rusqlite::Connection::open(directory.path().join("agent-state.db")).unwrap();
        database.execute("DROP TABLE executions", []).unwrap();
        let unreadable = provider
            .validate_continuation(ProviderContinuationContext {
                source_execution_id: "source".into(),
            })
            .await
            .unwrap_err();
        assert_eq!(
            unreadable.code,
            ProviderErrorCode::AgentProviderOperationFailed
        );
    });
}

#[test]
fn runtime_continuation_target_keeps_parent_source_authoritative_with_bounded_legacy_fallback() {
    run(async {
        let (directory, store) = store().await;
        let source = continuation_source(&store, directory.path(), None).await;
        let database = rusqlite::Connection::open(directory.path().join("agent-state.db")).unwrap();
        database
            .execute(
                "UPDATE executions SET status='completed' WHERE id=?1",
                [&source],
            )
            .unwrap();
        database
            .execute(
                "DELETE FROM workspace_claims WHERE execution_id=?1",
                [&source],
            )
            .unwrap();
        let provider = codex(store.clone());

        let mut request = input(directory.path());
        request.request_key = "current".into();
        request.parent_execution_id = Some(source.clone());
        let child = store
            .create_execution("current".into(), canonicalize_request(request).unwrap(), 2)
            .await
            .unwrap()
            .execution;
        for copied_thread in [None, Some("thread-1"), Some("wrong-thread")] {
            let mut child = child.clone();
            child.thread_id = copied_thread.map(str::to_owned);
            assert_eq!(
                provider.runtime_continuation_target(&child).await.unwrap(),
                Some("thread-1".into())
            );
        }

        let mut legacy = input(directory.path());
        legacy.agent_id = "legacy-agent".into();
        legacy.request_key = "legacy".into();
        legacy.canonical_workspace_root = directory.path().join("legacy").to_string_lossy().into();
        legacy.thread_id = Some("legacy-thread".into());
        let legacy = store
            .create_execution("legacy".into(), canonicalize_request(legacy).unwrap(), 2)
            .await
            .unwrap()
            .execution;
        assert_eq!(
            provider.runtime_continuation_target(&legacy).await.unwrap(),
            Some("legacy-thread".into())
        );

        let mut fresh = input(directory.path());
        fresh.agent_id = "fresh-agent".into();
        fresh.request_key = "fresh".into();
        fresh.canonical_workspace_root = directory.path().join("fresh").to_string_lossy().into();
        let fresh = store
            .create_execution("fresh".into(), canonicalize_request(fresh).unwrap(), 2)
            .await
            .unwrap()
            .execution;
        assert_eq!(
            provider.runtime_continuation_target(&fresh).await.unwrap(),
            None
        );

        let child = store.execution("current".into()).await.unwrap().unwrap();
        database
            .execute(
                "UPDATE executions SET final_result_json='{}' WHERE id=?1",
                [&source],
            )
            .unwrap();
        assert_eq!(
            provider
                .runtime_continuation_target(&child)
                .await
                .unwrap_err(),
            "AGENT_CONTINUE_NOT_ALLOWED"
        );
    });
}

#[test]
fn activity_telemetry_requires_all_four_identity_bindings() {
    let row = row("running", "unknown", None);
    let activity = super::super::protocol::Activity {
        thread_id: "private-thread".into(),
        turn_id: "private-turn".into(),
        phase: ActivityPhase::Tool,
        tool_category: Some(ToolCategory::Test),
        observed_at: 10,
    };

    let mapped = activity_telemetry_event(
        "execution-1",
        "private-runtime",
        "private-runtime",
        "private-thread",
        &row,
        &activity,
    )
    .unwrap();
    assert_eq!(mapped.execution_id(), "execution-1");
    assert_eq!(mapped.phase(), ActivityPhase::Tool);
    assert_eq!(mapped.tool_category(), Some(ToolCategory::Test));

    assert!(
        activity_telemetry_event(
            "execution-1",
            "other-runtime",
            "private-runtime",
            "private-thread",
            &row,
            &activity,
        )
        .is_none()
    );
    let mut other_runtime = row.clone();
    other_runtime.runtime_instance_id = Some("other-runtime".into());
    assert!(
        activity_telemetry_event(
            "execution-1",
            "private-runtime",
            "private-runtime",
            "private-thread",
            &other_runtime,
            &activity,
        )
        .is_none()
    );
    assert!(
        activity_telemetry_event(
            "execution-1",
            "private-runtime",
            "private-runtime",
            "other-thread",
            &row,
            &activity,
        )
        .is_none()
    );
    let mut other_thread = activity.clone();
    other_thread.thread_id = "other-thread".into();
    assert!(
        activity_telemetry_event(
            "execution-1",
            "private-runtime",
            "private-runtime",
            "private-thread",
            &row,
            &other_thread,
        )
        .is_none()
    );
    let mut other_turn = activity.clone();
    other_turn.turn_id = "other-turn".into();
    assert!(
        activity_telemetry_event(
            "execution-1",
            "private-runtime",
            "private-runtime",
            "private-thread",
            &row,
            &other_turn,
        )
        .is_none()
    );
    let mut invalid = activity;
    invalid.phase = ActivityPhase::Provider;
    assert!(
        activity_telemetry_event(
            "execution-1",
            "private-runtime",
            "private-runtime",
            "private-thread",
            &row,
            &invalid,
        )
        .is_none()
    );
}

/// 构造已通过 protocol 数字验证的 Codex private cumulative Usage fixture。
fn usage(total_tokens: i64) -> super::super::protocol::Usage {
    super::super::protocol::Usage {
        thread_id: "private-thread".into(),
        turn_id: "private-turn".into(),
        total: super::super::protocol::UsageSnapshot {
            total_tokens,
            input_tokens: Some(60),
            cached_input_tokens: Some(10),
            cache_write_input_tokens: None,
            output_tokens: Some(30),
            reasoning_output_tokens: Some(5),
        },
        last: super::super::protocol::UsageSnapshot {
            total_tokens: 40,
            input_tokens: Some(25),
            cached_input_tokens: Some(5),
            cache_write_input_tokens: Some(0),
            output_tokens: Some(10),
            reasoning_output_tokens: Some(2),
        },
        model_context_window: Some(258400),
        observed_at: 10,
    }
}

#[test]
fn usage_telemetry_requires_exact_identity_and_exposes_only_cumulative_total_shape() {
    let row = row("running", "unknown", None);
    let usage = usage(100);
    let mapped = usage_telemetry_event(
        "execution-1",
        "private-runtime",
        "private-runtime",
        "private-thread",
        &row,
        &usage,
    )
    .unwrap();

    assert_eq!(mapped.execution_id(), "execution-1");
    assert_eq!(mapped.provider_id().as_str(), "codex");
    assert_eq!(mapped.cumulative_total_tokens(), 100);
    assert_eq!(mapped.cache_write_input_tokens(), None);
    assert_eq!(mapped.model_context_window(), Some(258400));
    assert_eq!(mapped.observed_at(), 10);

    assert!(
        usage_telemetry_event(
            "execution-1",
            "other-runtime",
            "private-runtime",
            "private-thread",
            &row,
            &usage,
        )
        .is_none()
    );
    let mut wrong_row_id = row.clone();
    wrong_row_id.id = "other-execution".into();
    assert!(
        usage_telemetry_event(
            "execution-1",
            "private-runtime",
            "private-runtime",
            "private-thread",
            &wrong_row_id,
            &usage,
        )
        .is_none()
    );
    let mut wrong_runtime = row.clone();
    wrong_runtime.runtime_instance_id = Some("other-runtime".into());
    assert!(
        usage_telemetry_event(
            "execution-1",
            "private-runtime",
            "private-runtime",
            "private-thread",
            &wrong_runtime,
            &usage,
        )
        .is_none()
    );
    assert!(
        usage_telemetry_event(
            "execution-1",
            "private-runtime",
            "private-runtime",
            "other-thread",
            &row,
            &usage,
        )
        .is_none()
    );
    let mut wrong_thread = usage.clone();
    wrong_thread.thread_id = "other-thread".into();
    assert!(
        usage_telemetry_event(
            "execution-1",
            "private-runtime",
            "private-runtime",
            "private-thread",
            &row,
            &wrong_thread,
        )
        .is_none()
    );
    let mut wrong_turn = usage;
    wrong_turn.turn_id = "other-turn".into();
    assert!(
        usage_telemetry_event(
            "execution-1",
            "private-runtime",
            "private-runtime",
            "private-thread",
            &row,
            &wrong_turn,
        )
        .is_none()
    );
}

#[test]
fn usage_telemetry_is_stateless_for_duplicates_and_out_of_order_totals() {
    let row = row("running", "unknown", None);
    let map = |usage| {
        usage_telemetry_event(
            "execution-1",
            "private-runtime",
            "private-runtime",
            "private-thread",
            &row,
            &usage,
        )
        .unwrap()
    };

    let first = map(usage(100));
    let duplicate = map(usage(100));
    let out_of_order = map(usage(50));
    assert_eq!(first, duplicate);
    assert_eq!(first.cumulative_total_tokens(), 100);
    assert_eq!(out_of_order.cumulative_total_tokens(), 50);
    run(async {
        let sink = RecordingEventSink::default();
        sink.publish(AgentTelemetryEvent::Usage(first.clone()))
            .await;
        sink.publish(AgentTelemetryEvent::Usage(duplicate.clone()))
            .await;
        sink.publish(AgentTelemetryEvent::Usage(out_of_order.clone()))
            .await;
        let events = sink.events.lock().unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0], AgentTelemetryEvent::Usage(first));
        assert_eq!(events[1], AgentTelemetryEvent::Usage(duplicate));
        assert_eq!(events[2], AgentTelemetryEvent::Usage(out_of_order));
    });
}

#[test]
fn usage_baseline_intent_never_treats_cold_continue_as_observed() {
    assert_eq!(
        usage_baseline_intent(None, false),
        crate::agent::store::CodexUsageBaselineIntent::FreshZero
    );
    assert_eq!(
        usage_baseline_intent(Some("thread"), true),
        crate::agent::store::CodexUsageBaselineIntent::WarmObservedSameEpoch
    );
    assert_eq!(
        usage_baseline_intent(Some("thread"), false),
        crate::agent::store::CodexUsageBaselineIntent::Unknown
    );
}

#[test]
fn late_turn_usage_is_detected_before_provider_agnostic_publication() {
    let row = row("running", "unknown", None);
    let current = usage(100);
    assert!(!is_late_usage_turn(&row, "private-thread", &current));
    let mut previous = current.clone();
    previous.turn_id = "previous-turn".into();
    assert!(is_late_usage_turn(&row, "private-thread", &previous));
    let mut other_thread = previous;
    other_thread.thread_id = "other-thread".into();
    assert!(!is_late_usage_turn(&row, "private-thread", &other_thread));
    let mut before_turn_bind = row;
    before_turn_bind.turn_id = None;
    assert!(is_late_usage_turn(
        &before_turn_bind,
        "private-thread",
        &current
    ));
}

/// 从真实 transport 读取一条 App Server 请求，避免用 source-string 代替 provider 行为验证。
async fn transport_request(server: &mut BufReader<tokio::io::DuplexStream>) -> serde_json::Value {
    let mut line = String::new();
    server.read_line(&mut line).await.unwrap();
    serde_json::from_str(&line).unwrap()
}

/// 以协议编码回复 transport 请求，保留真实 Client decode/event loop。
async fn transport_reply(
    server: &mut BufReader<tokio::io::DuplexStream>,
    request: &serde_json::Value,
    result: serde_json::Value,
) {
    server
        .write_all(
            &super::super::protocol::encode(&json!({"id": request["id"], "result": result}))
                .unwrap(),
        )
        .await
        .unwrap();
}

/// 发送一条真实 Usage notification；`turn` 用于制造 current 或 previous-turn 情形。
async fn transport_usage(server: &mut BufReader<tokio::io::DuplexStream>, turn: &str, total: i64) {
    server
        .write_all(
            &super::super::protocol::encode(&json!({
                "method": "thread/tokenUsage/updated",
                "params": {"threadId":"THREAD","turnId":turn,"tokenUsage":{
                    "total":{"totalTokens":total,"inputTokens":60,"cachedInputTokens":10,"outputTokens":30,"reasoningOutputTokens":5},
                    "last":{"totalTokens":40,"inputTokens":25,"cachedInputTokens":5,"outputTokens":10,"reasoningOutputTokens":2},
                    "modelContextWindow":258400
                }}
            }))
            .unwrap(),
        )
        .await
        .unwrap();
}

/// 以最小真实 provider/App Server 循环证明 freeze 边界与 late-turn telemetry 的隔离。
async fn run_late_usage_provider(
    inject_invalidation_failure: bool,
    inject_grace_failure: bool,
    inject_freeze_failure: bool,
    inject_projection_failure: bool,
    preterminal_previous_turn: bool,
    grace_previous_turn: bool,
) -> crate::agent::usage::UsageSnapshot {
    let directory = tempfile::tempdir().unwrap();
    let store = StateStore::open(directory.path().into()).await.unwrap();
    let execution_id = AgentTaskManager::new(store.clone(), "unused.exe".into())
        .create(input(directory.path()))
        .await
        .unwrap()
        .execution_id;
    rusqlite::Connection::open(directory.path().join("agent-state.db"))
        .unwrap()
        .execute("INSERT INTO runtime_instances(id,owner_host_instance_id,state,created_at,updated_at) VALUES('P4-RUNTIME','fixture','running',1,1)", [])
        .unwrap();
    let (wire, server) = tokio::io::duplex(128 * 1024);
    let (read, write) = tokio::io::split(wire);
    let client = super::super::app_server::Client::transport(
        "P4-RUNTIME".into(),
        read,
        write,
        tokio::io::empty(),
    );
    let fake_store = store.clone();
    let fake_execution_id = execution_id.clone();
    let database_path = directory.path().join("agent-state.db");
    let fake = tokio::spawn(async move {
        let mut server = BufReader::new(server);
        let initialize = transport_request(&mut server).await;
        assert_eq!(initialize["method"], "initialize");
        transport_reply(&mut server, &initialize, json!({"userAgent":"fake","codexHome":"home","platformFamily":"windows","platformOs":"windows"})).await;
        assert_eq!(
            transport_request(&mut server).await["method"],
            "initialized"
        );
        let thread_start = transport_request(&mut server).await;
        assert_eq!(thread_start["method"], "thread/start");
        transport_reply(
            &mut server,
            &thread_start,
            json!({"thread":{"id":"THREAD","name":"fixture","turns":[],"historyMode":"paginated"}}),
        )
        .await;
        // 在接收 turn/start 前，baseline 必须已由 bind 后的 provider 路径写入。
        for _ in 0..100 {
            let connection = rusqlite::Connection::open(&database_path).unwrap();
            let baseline: Option<String> = connection
                .query_row(
                    "SELECT baseline_kind FROM codex_execution_usage_state WHERE execution_id=?1",
                    [&fake_execution_id],
                    |row| row.get(0),
                )
                .ok();
            if baseline.as_deref() == Some("fresh_zero") {
                break;
            }
            tokio::task::yield_now().await;
        }
        let connection = rusqlite::Connection::open(&database_path).unwrap();
        assert_eq!(
            connection
                .query_row(
                    "SELECT baseline_kind FROM codex_execution_usage_state WHERE execution_id=?1",
                    [&fake_execution_id],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            "fresh_zero"
        );
        let turn_start = transport_request(&mut server).await;
        assert_eq!(turn_start["method"], "turn/start");
        transport_reply(
            &mut server,
            &turn_start,
            json!({"turn":{"id":"TURN","status":"inProgress","items":[],"itemsView":"summary"}}),
        )
        .await;
        server.write_all(&super::super::protocol::encode(&json!({"method":"turn/started","params":{"threadId":"THREAD","turn":{"id":"TURN","status":"inProgress","items":[]}}})).unwrap()).await.unwrap();
        transport_usage(&mut server, "TURN", 100).await;
        for _ in 0..100 {
            if fake_store
                .execution_usage(fake_execution_id.clone())
                .await
                .unwrap()
                .is_some()
            {
                break;
            }
            tokio::task::yield_now().await;
        }
        if inject_invalidation_failure {
            fake_store.inject_observability_failure(
                crate::agent::store::ObservabilityFault::UsageInvalidation,
            );
        }
        if inject_grace_failure {
            fake_store.inject_observability_failure(
                crate::agent::store::ObservabilityFault::UsageTerminalGrace,
            );
        }
        if preterminal_previous_turn {
            transport_usage(&mut server, "PREVIOUS", 50).await;
        }
        server.write_all(&super::super::protocol::encode(&json!({"method":"turn/completed","params":{"threadId":"THREAD","turn":{"id":"TURN","status":"completed","items":[]}}})).unwrap()).await.unwrap();
        for (method, result) in [
            (
                "thread/read",
                json!({"thread":{"id":"THREAD","name":"fixture","turns":[],"historyMode":"paginated"}}),
            ),
            (
                "thread/turns/list",
                json!({"data":[{"id":"TURN","status":"completed","items":[],"itemsView":"summary"}],"nextCursor":null}),
            ),
            (
                "thread/items/list",
                json!({"data":[{"turnId":"TURN","item":{"type":"agentMessage","id":"answer","phase":"final_answer","text":"ok"}}],"nextCursor":null}),
            ),
            ("thread/backgroundTerminals/clean", json!({})),
            (
                "thread/backgroundTerminals/list",
                json!({"data":[],"nextCursor":null}),
            ),
        ] {
            let request = transport_request(&mut server).await;
            assert_eq!(request["method"], method);
            transport_reply(&mut server, &request, result).await;
        }
        // `finish` 已提交 Execution terminal 与 Claim release 后，provider 才可进入 grace drain 并接收此 exact Usage。
        for _ in 0..100 {
            let row = fake_store
                .execution(fake_execution_id.clone())
                .await
                .unwrap()
                .unwrap();
            let claim = fake_store
                .workspace_claim(row.canonical_workspace_root.clone())
                .await
                .unwrap();
            if row.status == "completed" && claim.is_none() {
                if inject_grace_failure {
                    return;
                }
                server.write_all(&super::super::protocol::encode(&json!({
                    "method":"item/started",
                    "params":{"threadId":"THREAD","turnId":"TURN","item":{"type":"fileChange"}}
                })).unwrap()).await.unwrap();
                server
                    .write_all(
                        &super::super::protocol::encode(&json!({
                            "method":"item/commandExecution/requestApproval",
                            "params":{"threadId":"THREAD","turnId":"TURN","itemId":"approval"}
                        }))
                        .unwrap(),
                    )
                    .await
                    .unwrap();
                if grace_previous_turn {
                    transport_usage(&mut server, "PREVIOUS", 50).await;
                }
                if inject_projection_failure {
                    fake_store.inject_observability_failure(
                        crate::agent::store::ObservabilityFault::UsageProjection,
                    );
                }
                transport_usage(&mut server, "TURN", 150).await;
                server.flush().await.unwrap();
                if inject_projection_failure {
                    for _ in 0..100 {
                        tokio::task::yield_now().await;
                    }
                    return;
                }
                for _ in 0..100 {
                    let connection = rusqlite::Connection::open(&database_path).unwrap();
                    let latest: Option<String> = connection
                        .query_row(
                            "SELECT latest_cumulative_json FROM codex_execution_usage_state WHERE execution_id=?1",
                            [&fake_execution_id],
                            |row| row.get(0),
                        )
                        .unwrap();
                    if latest.is_some_and(|latest| latest.contains("\"totalTokens\":150")) {
                        if inject_freeze_failure {
                            fake_store.inject_observability_failure(
                                crate::agent::store::ObservabilityFault::UsageFreeze,
                            );
                        }
                        return;
                    }
                    tokio::task::yield_now().await;
                }
                panic!("exact late Usage was not consumed during terminal grace");
            }
            tokio::task::yield_now().await;
        }
        panic!("Execution/Claim were not terminal before Usage grace drain");
    });
    client.initialize().await.unwrap();
    let provider = codex(store.clone());
    let projector = crate::agent::telemetry_projector::ExecutionTelemetryProjector::new(
        store.clone(),
        execution_id.clone(),
    );
    let outcome = provider
        .run_client_with_acceptance_and_telemetry(
            &execution_id,
            &client,
            &NoopAcceptanceSink,
            &projector,
        )
        .await;
    assert!(outcome.is_ok(), "{outcome:?}");
    fake.await.unwrap();
    let usage = store
        .execution_usage(execution_id.clone())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        usage.total_tokens,
        Some(if inject_grace_failure || inject_projection_failure {
            100
        } else {
            150
        })
    );
    let telemetry_state = {
        let connection =
            rusqlite::Connection::open(directory.path().join("agent-state.db")).unwrap();
        connection
            .query_row(
                "SELECT telemetry_state FROM codex_execution_usage_state WHERE execution_id=?1",
                [&execution_id],
                |row| row.get::<_, String>(0),
            )
            .unwrap()
    };
    assert_eq!(
        telemetry_state,
        if inject_freeze_failure {
            "terminal_grace"
        } else {
            "frozen"
        }
    );
    assert_eq!(
        store.execution(execution_id).await.unwrap().unwrap().status,
        "completed"
    );
    usage
}

#[tokio::test]
async fn provider_freezes_before_turn_start_and_late_turn_degrades_public_usage() {
    let usage = run_late_usage_provider(false, false, false, false, false, false).await;
    assert_eq!(
        (usage.total_tokens, usage.completeness),
        (Some(150), crate::agent::usage::UsageCompleteness::Partial)
    );
}

#[tokio::test]
async fn provider_late_turn_invalidation_failure_does_not_fail_execution() {
    let usage = run_late_usage_provider(true, false, false, false, true, false).await;
    assert_eq!(
        (usage.total_tokens, usage.completeness),
        (Some(150), crate::agent::usage::UsageCompleteness::Partial)
    );
}

#[tokio::test]
async fn provider_grace_wrong_turn_drops_without_baseline_invalidation() {
    let usage = run_late_usage_provider(false, false, false, false, false, true).await;
    assert_eq!(
        (usage.total_tokens, usage.completeness),
        (Some(150), crate::agent::usage::UsageCompleteness::Partial)
    );
}

#[tokio::test]
async fn provider_grace_store_failure_still_finishes_and_releases_claim() {
    let usage = run_late_usage_provider(false, true, false, false, false, false).await;
    assert_eq!(
        (usage.total_tokens, usage.completeness),
        (Some(100), crate::agent::usage::UsageCompleteness::Partial)
    );
}

#[tokio::test]
async fn provider_freeze_store_failure_does_not_change_terminal_result() {
    let usage = run_late_usage_provider(false, false, true, false, false, false).await;
    assert_eq!(
        (usage.total_tokens, usage.completeness),
        (Some(150), crate::agent::usage::UsageCompleteness::Partial)
    );
}

#[tokio::test]
async fn provider_grace_usage_projection_failure_does_not_change_terminal_result() {
    let usage = run_late_usage_provider(false, false, false, true, false, false).await;
    assert_eq!(
        (usage.total_tokens, usage.completeness),
        (Some(100), crate::agent::usage::UsageCompleteness::Partial)
    );
}

/// paused Tokio clock 验证：finish 已耗尽窗口时不会额外 receive，一进入 drain 就冻结。
#[tokio::test(start_paused = true)]
async fn provider_grace_deadline_expired_before_drain_freezes_without_receiving() {
    let (directory, store) = store().await;
    let execution_id = AgentTaskManager::new(store.clone(), "unused.exe".into())
        .create(input(directory.path()))
        .await
        .unwrap()
        .execution_id;
    let database = rusqlite::Connection::open(directory.path().join("agent-state.db")).unwrap();
    database
        .execute(
            "INSERT INTO runtime_instances(id,owner_host_instance_id,state,created_at,updated_at)
             VALUES('deadline-runtime','fixture','running',1,1)",
            [],
        )
        .unwrap();
    database
        .execute(
            "UPDATE executions SET runtime_instance_id='deadline-runtime',thread_id='deadline-thread'
             WHERE id=?1",
            [&execution_id],
        )
        .unwrap();
    store
        .prepare_codex_usage_baseline(
            execution_id.clone(),
            "deadline-runtime".into(),
            "deadline-thread".into(),
            crate::agent::store::CodexUsageBaselineIntent::FreshZero,
            1,
        )
        .await
        .unwrap();
    database
        .execute(
            "UPDATE executions SET turn_id='deadline-turn' WHERE id=?1",
            [&execution_id],
        )
        .unwrap();
    database
        .execute(
            "UPDATE codex_execution_usage_state SET turn_id='deadline-turn' WHERE execution_id=?1",
            [&execution_id],
        )
        .unwrap();
    store
        .enter_codex_usage_terminal_grace(
            execution_id.clone(),
            "deadline-runtime".into(),
            "deadline-thread".into(),
            "deadline-turn".into(),
            1,
        )
        .await
        .unwrap();
    store
        .project_execution_usage(crate::agent::provider::telemetry::UsageEvent::cumulative(
            execution_id.clone(),
            crate::agent::provider::ProviderId::new("codex".into()).unwrap(),
            100,
            None,
            None,
            None,
            None,
            None,
            None,
            2,
        ))
        .await
        .unwrap();
    let row = store
        .execution(execution_id.clone())
        .await
        .unwrap()
        .unwrap();
    let (wire, mut server) = tokio::io::duplex(1024);
    let (read, write) = tokio::io::split(wire);
    let client = super::super::app_server::Client::transport(
        "deadline-runtime".into(),
        read,
        write,
        tokio::io::empty(),
    );
    server
        .write_all(
            &super::super::protocol::encode(&json!({
                "method":"thread/tokenUsage/updated",
                "params":{"threadId":"deadline-thread","turnId":"deadline-turn","tokenUsage":{
                    "total":{"totalTokens":150},"last":{"totalTokens":50}
                }}
            }))
            .unwrap(),
        )
        .await
        .unwrap();
    codex(store.clone())
        .drain_terminal_usage(
            &execution_id,
            &client,
            &NoopEventSink,
            "deadline-thread",
            &row,
            tokio::time::Instant::now(),
        )
        .await;
    assert_eq!(
        database
            .query_row(
                "SELECT telemetry_state FROM codex_execution_usage_state WHERE execution_id=?1",
                [&execution_id],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        "frozen"
    );
    let public = store.execution_usage(execution_id).await.unwrap().unwrap();
    assert_eq!((public.total_tokens, public.revision), (Some(100), 1));
}

#[test]
fn activity_adapter_publishes_only_after_validation_without_direct_store_projection() {
    let source = include_str!("../provider.rs");
    // 只忽略 rustfmt 与 CRLF/LF 的空白差异，仍验证私有绑定校验先于 telemetry 发布。
    let compact = source.split_whitespace().collect::<String>();
    assert!(!source.contains(".store.execution_activity("));
    let validation = compact.find("fnactivity_telemetry_event").unwrap();
    let publish = compact.find("telemetry.publish").unwrap();
    assert!(validation < publish);
    assert!(compact.contains("returnErr(\"PROVIDER_RUNTIME_MISMATCH\".into());"));
}

#[test]
fn usage_adapter_publishes_only_after_binding_and_late_turn_invalidation() {
    let source = include_str!("../provider.rs");
    assert!(!source.contains(".store.execution_usage("));
    let binding = source.find("fn usage_telemetry_event").unwrap();
    let usage = &source[source.find("Notification::Usage(usage) => {").unwrap()..];
    let publish = usage.find("AgentTelemetryEvent::Usage(event)").unwrap();
    assert!(binding < source.find("Notification::Usage(usage) => {").unwrap());
    assert!(publish > 0);
    assert!(!usage[..publish].contains("?"));
    assert!(usage[..publish].contains("invalidate_codex_usage_baseline"));
}

#[test]
fn successful_discovery_registers_one_available_codex_provider() {
    run(async {
        let (_directory, store) = store().await;
        let mut registry = ProviderRegistry::new();
        register_codex_provider_with_discovery(
            &mut registry,
            store,
            "adapter-test".into(),
            Default::default(),
            Ok("C:/fake/codex.exe".into()),
        )
        .unwrap();

        let id = ProviderId::new("codex".into()).unwrap();
        assert_eq!(registry.health(&id).unwrap(), ProviderHealth::Available);
        assert_eq!(
            registry.list_descriptors(),
            vec![registry.get(&id).unwrap().descriptor()]
        );
        assert_eq!(registry.list_descriptors()[0].id.as_str(), "codex");
    });
}

#[test]
fn failed_discovery_registers_unavailable_without_exposing_the_error() {
    run(async {
        let (_directory, store) = store().await;
        let mut registry = ProviderRegistry::new();
        register_codex_provider_with_discovery(
            &mut registry,
            store,
            "adapter-test".into(),
            Default::default(),
            Err("private discovery failure".into()),
        )
        .unwrap();

        let id = ProviderId::new("codex".into()).unwrap();
        assert_eq!(registry.health(&id).unwrap(), ProviderHealth::Unavailable);
        let descriptor = &registry.list_descriptors()[0];
        assert_eq!(descriptor.id.as_str(), "codex");
        assert_eq!(descriptor.display_name, "Codex");
        assert_eq!(
            descriptor.version.as_deref(),
            Some(super::super::protocol::VERSION)
        );
        assert_eq!(
            registry.capabilities(&id).unwrap(),
            ProviderCapabilities {
                can_execute: true,
                can_continue: true,
                can_cancel: true,
                can_recover: true,
                activity: true,
                token_usage: false,
            }
        );
        let error = match registry.get(&id) {
            Ok(_) => panic!("unavailable provider resolved for execution"),
            Err(error) => error,
        };
        assert_eq!(error.code, ProviderErrorCode::AgentProviderUnavailable);
    });
}

#[test]
fn duplicate_registration_keeps_the_original_codex_provider() {
    run(async {
        let (_directory, store) = store().await;
        let mut registry = ProviderRegistry::new();
        register_codex_provider_with_discovery(
            &mut registry,
            store.clone(),
            "first".into(),
            Default::default(),
            Ok("C:/fake/first.exe".into()),
        )
        .unwrap();
        let id = ProviderId::new("codex".into()).unwrap();
        let original = registry.get(&id).unwrap();

        let error = register_codex_provider_with_discovery(
            &mut registry,
            store,
            "second".into(),
            Default::default(),
            Ok("C:/fake/second.exe".into()),
        )
        .unwrap_err();

        assert_eq!(error.code, ProviderErrorCode::AgentProviderContractError);
        assert!(Arc::ptr_eq(&original, &registry.get(&id).unwrap()));
    });
}

#[test]
fn trait_cancel_and_execute_reuse_the_store_cancel_authority() {
    run(async {
        let (directory, store) = store().await;
        let manager = AgentTaskManager::new(store.clone(), "must-not-launch.exe".into());
        let created = manager.create(input(directory.path())).await.unwrap();
        let provider: Arc<dyn AgentProvider> = Arc::new(codex(store.clone()));

        provider
            .cancel(ProviderCancelContext {
                execution_id: created.execution_id.clone(),
            })
            .await
            .unwrap();

        let cancelled = store
            .execution(created.execution_id.clone())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(cancelled.status, "cancelled");
        assert_eq!(cancelled.dispatch_state, "not_dispatched");
        assert!(cancelled.runtime_instance_id.is_none());
        assert!(
            store
                .workspace_claim(cancelled.canonical_workspace_root.clone())
                .await
                .unwrap()
                .is_none()
        );

        let result = provider
            .execute(
                ProviderExecutionContext {
                    execution_id: created.execution_id,
                },
                Arc::new(NoopAcceptanceSink),
                Arc::new(NoopEventSink),
            )
            .await
            .unwrap();
        assert_eq!(result.execution_id, cancelled.id);
        assert_eq!(result.outcome, ProviderOutcome::Cancelled);
        assert_eq!(result.result, None);
        assert_eq!(
            result.result_completeness,
            ProviderResultCompleteness::Unknown
        );
        assert_eq!(result.diagnostic_code, None);
    });
}

#[test]
fn projection_accepts_only_frozen_terminal_rows_and_json() {
    let result = provider_run_result(row(
        "completed",
        "complete",
        Some(json!({ "answer": 42 }).to_string()),
    ))
    .unwrap();
    assert_eq!(result.execution_id, "execution-1");
    assert_eq!(result.outcome, ProviderOutcome::Completed);
    assert_eq!(result.result, Some(json!({ "answer": 42 })));
    assert_eq!(
        result.result_completeness,
        ProviderResultCompleteness::Complete
    );
    assert_eq!(result.diagnostic_code.as_deref(), Some("STABLE_DIAGNOSTIC"));

    let wire = serde_json::to_value(result).unwrap();
    for private in [
        "errorMessage",
        "interruptDiagnostic",
        "releaseEvidence",
        "runtimeId",
        "threadId",
        "turnId",
        "job",
    ] {
        assert!(
            wire.get(private).is_none(),
            "projected private field {private}"
        );
    }
    assert_eq!(wire["result"], json!({ "answer": 42 }));

    for invalid in [
        row("running", "complete", Some("{}".into())),
        row("completed", "future", Some("{}".into())),
        row("completed", "complete", Some("{broken".into())),
    ] {
        assert_eq!(
            provider_run_result(invalid).unwrap_err().code,
            ProviderErrorCode::AgentProviderContractError
        );
    }
}

#[test]
fn trait_startup_reconcile_uses_shared_recovery_for_pending_claim() {
    run(async {
        let (directory, store) = store().await;
        let manager = AgentTaskManager::new(store.clone(), "must-not-launch.exe".into());
        let created = manager.create(input(directory.path())).await.unwrap();
        let provider: Arc<dyn AgentProvider> = Arc::new(codex(store));

        assert!(provider.capabilities().can_recover);
        assert_eq!(
            provider
                .startup_reconcile(ProviderStartupContext {})
                .await
                .unwrap(),
            ProviderReconcileSummary {
                items: vec![ProviderReconcileItem {
                    subject_id: created.execution_id,
                    kind: ProviderReconcileKind::ExecutionPendingExplicitResume,
                }],
            }
        );
    });
}

#[test]
fn trait_startup_reconcile_projects_shared_recovery_errors_to_operation_failed() {
    run(async {
        let (directory, store) = store().await;
        let database = rusqlite::Connection::open(directory.path().join("agent-state.db")).unwrap();
        database.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
        database
            .execute(
                "INSERT INTO workspace_claims VALUES ('missing-root','missing-execution','exclusive_execution',1)",
                [],
            )
            .unwrap();
        let provider: Arc<dyn AgentProvider> = Arc::new(codex(store));

        assert_eq!(
            provider
                .startup_reconcile(ProviderStartupContext {})
                .await
                .unwrap_err(),
            ProviderError {
                code: ProviderErrorCode::AgentProviderOperationFailed,
            }
        );
    });
}

#[test]
fn registered_backend_diagnostics_reach_startup_recovery_without_summary_leakage() {
    run(async {
        let source = include_str!("../provider.rs");
        assert!(
            source.contains(
                "Err(error) => (PathBuf::new(), Some(error), ProviderHealth::Unavailable)"
            )
        );
        assert!(source.contains("self.backend_error.as_deref()"));
        assert!(!source.contains("then_some(\"BACKEND_UNAVAILABLE\")"));

        {
            let diagnostic = "CODEX_APP_SERVER_INCOMPATIBLE: unsupported protocol";
            let (directory, store) = store().await;
            prepare_backend_recovery(&store, directory.path()).await;
            let mut registry = ProviderRegistry::new();
            register_codex_provider_with_discovery(
                &mut registry,
                store,
                "adapter-test".into(),
                Default::default(),
                Err(diagnostic.into()),
            )
            .unwrap();
            let id = ProviderId::new("codex".into()).unwrap();
            assert_eq!(
                registry
                    .get_registered(&id)
                    .unwrap()
                    .startup_reconcile(ProviderStartupContext {})
                    .await
                    .unwrap_err(),
                ProviderError {
                    code: ProviderErrorCode::AgentProviderCapabilityUnsupported,
                }
            );
        }

        let diagnostic = "BACKEND_UNAVAILABLE: discovery failed";
        let (directory, store) = store().await;
        let execution_id = prepare_backend_recovery(&store, directory.path()).await;
        let mut registry = ProviderRegistry::new();
        register_codex_provider_with_discovery(
            &mut registry,
            store,
            "adapter-test".into(),
            Default::default(),
            Err(diagnostic.into()),
        )
        .unwrap();
        let id = ProviderId::new("codex".into()).unwrap();
        let summary = registry
            .get_registered(&id)
            .unwrap()
            .startup_reconcile(ProviderStartupContext {})
            .await
            .unwrap();
        assert_eq!(
            summary,
            ProviderReconcileSummary {
                items: vec![ProviderReconcileItem {
                    subject_id: execution_id,
                    kind: ProviderReconcileKind::ExecutionProviderFailure,
                }],
            }
        );
        let projected = format!("{summary:?}");
        assert!(!projected.contains(diagnostic));
        assert!(!projected.to_ascii_lowercase().contains("backend"));
    });
}
