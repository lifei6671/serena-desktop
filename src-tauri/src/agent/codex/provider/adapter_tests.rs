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
    },
    task_manager::AgentTaskManager,
};
use serde_json::json;

struct NoopEventSink;

impl AgentEventSink for NoopEventSink {}

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
    let outcomes = vec![
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

#[test]
fn activity_adapter_publishes_only_after_validation_without_direct_store_projection() {
    let source = include_str!("../provider.rs");
    assert!(!source.contains(".store.execution_activity("));
    let validation = source.find("fn activity_telemetry_event").unwrap();
    let publish = source
        .find("telemetry\n                                .publish")
        .unwrap();
    assert!(validation < publish);
    assert!(source.contains("return Err(\"PROVIDER_RUNTIME_MISMATCH\".into());"));
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
