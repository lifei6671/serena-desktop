use super::*;
use crate::agent::provider::{
    ProviderCapabilities, ProviderDescriptor, ProviderOutcome, ProviderResultCompleteness,
    ProviderRunResult, ProviderStartupContext,
    port::{
        AgentEventSink, AgentProvider, ProviderContinuationContext, ProviderContinuationDecision,
        ProviderExecutionFailure, ProviderFuture, ProviderReconcileItem, ProviderReconcileKind,
        ProviderReconcileSummary,
    },
    registry::ProviderHealth,
};
use serde_json::json;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

struct FakeProvider {
    id: ProviderId,
    store: StateStore,
    accept: bool,
    can_continue: bool,
    continuation_eligible: AtomicBool,
    can_cancel: bool,
    can_recover: bool,
    runtime_failure: Option<(&'static str, &'static str)>,
    recovery_result: Option<Result<ProviderReconcileSummary, ProviderError>>,
    release: Option<Arc<tokio::sync::Notify>>,
    execute_calls: AtomicUsize,
    continuation_calls: AtomicUsize,
    cancel_calls: AtomicUsize,
    reconcile_calls: AtomicUsize,
    terminal: AtomicBool,
}

impl FakeProvider {
    fn new(store: StateStore, id: &str, accept: bool, can_cancel: bool) -> Self {
        Self {
            id: ProviderId::new(id.into()).unwrap(),
            store,
            accept,
            can_continue: true,
            continuation_eligible: AtomicBool::new(true),
            can_cancel,
            can_recover: false,
            runtime_failure: None,
            recovery_result: None,
            release: None,
            execute_calls: AtomicUsize::new(0),
            continuation_calls: AtomicUsize::new(0),
            cancel_calls: AtomicUsize::new(0),
            reconcile_calls: AtomicUsize::new(0),
            terminal: AtomicBool::new(false),
        }
    }

    fn with_runtime_failure(mut self, code: &'static str, message: &'static str) -> Self {
        self.runtime_failure = Some((code, message));
        self
    }

    fn with_recovery(mut self, result: Result<ProviderReconcileSummary, ProviderError>) -> Self {
        self.can_recover = true;
        self.recovery_result = Some(result);
        self
    }

    fn without_continuation(mut self) -> Self {
        self.can_continue = false;
        self
    }

    fn with_ineligible_continuation(self) -> Self {
        self.continuation_eligible.store(false, Ordering::SeqCst);
        self
    }
}

impl AgentProvider for FakeProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: self.id.clone(),
            display_name: "Routing Fake".into(),
            version: None,
        }
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            can_execute: true,
            can_continue: self.can_continue,
            can_cancel: self.can_cancel,
            can_recover: self.can_recover,
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
            self.execute_calls.fetch_add(1, Ordering::SeqCst);
            if self.accept {
                acceptance.accepted();
                acceptance.accepted();
            }
            if let Some((code, message)) = self.runtime_failure {
                return Err(ProviderExecutionFailure::Runtime {
                    code: code.into(),
                    message: message.into(),
                });
            }
            if let Some(release) = &self.release {
                release.notified().await;
            }
            self.store
                .request_cancel(
                    context.execution_id.clone(),
                    super::super::coordinator::now(),
                )
                .await
                .map_err(ProviderExecutionFailure::State)?;
            self.terminal.store(true, Ordering::SeqCst);
            Ok(ProviderRunResult {
                execution_id: context.execution_id,
                outcome: ProviderOutcome::Cancelled,
                result: None,
                result_completeness: ProviderResultCompleteness::Unknown,
                diagnostic_code: None,
            })
        })
    }

    fn cancel<'a>(
        &'a self,
        context: ProviderCancelContext,
    ) -> ProviderFuture<'a, Result<(), ProviderError>> {
        Box::pin(async move {
            self.cancel_calls.fetch_add(1, Ordering::SeqCst);
            self.store
                .request_cancel(context.execution_id, super::super::coordinator::now())
                .await
                .map(|_| ())
                .map_err(|_| ProviderError {
                    code: ProviderErrorCode::AgentProviderOperationFailed,
                })
        })
    }

    fn validate_continuation<'a>(
        &'a self,
        _context: ProviderContinuationContext,
    ) -> ProviderFuture<'a, Result<ProviderContinuationDecision, ProviderError>> {
        Box::pin(async move {
            self.continuation_calls.fetch_add(1, Ordering::SeqCst);
            Ok(if self.continuation_eligible.load(Ordering::SeqCst) {
                ProviderContinuationDecision::Eligible
            } else {
                ProviderContinuationDecision::Ineligible
            })
        })
    }

    fn startup_reconcile<'a>(
        &'a self,
        _context: ProviderStartupContext,
    ) -> ProviderFuture<'a, Result<ProviderReconcileSummary, ProviderError>> {
        Box::pin(async move {
            self.reconcile_calls.fetch_add(1, Ordering::SeqCst);
            self.recovery_result.clone().unwrap_or(Err(ProviderError {
                code: ProviderErrorCode::AgentProviderCapabilityUnsupported,
            }))
        })
    }
}

fn input(root: &std::path::Path, request_key: &str) -> CreateExecutionInput {
    serde_json::from_value(json!({
        "agent_id": "routing-agent",
        "request_key": request_key,
        "prompt": "route through registry",
        "execution_profile": {},
        "workspace_id": "routing-workspace",
        "canonical_workspace_root": root.to_str().unwrap(),
        "provider": "codex",
        "mode": "read_only"
    }))
    .unwrap()
}

fn manager_with_provider(
    store: StateStore,
    provider: Arc<dyn AgentProvider>,
    health: ProviderHealth,
) -> AgentTaskManager {
    let mut registry = ProviderRegistry::new();
    registry.register(provider, health).unwrap();
    let mut manager = AgentTaskManager::new(store, "must-not-launch.exe".into());
    manager.use_registry(registry);
    manager
}

#[tokio::test]
async fn startup_reconcile_uses_registered_providers_and_isolates_failures() {
    let directory = tempfile::tempdir().unwrap();
    let store = StateStore::open(directory.path().into()).await.unwrap();
    let unavailable_failure = Arc::new(
        FakeProvider::new(store.clone(), "alpha", true, true).with_recovery(Err(ProviderError {
            code: ProviderErrorCode::AgentProviderOperationFailed,
        })),
    );
    let skipped = Arc::new(FakeProvider::new(store.clone(), "middle", true, true));
    let recovered = Arc::new(
        FakeProvider::new(store.clone(), "zeta", true, true).with_recovery(Ok(
            ProviderReconcileSummary {
                items: vec![
                    ProviderReconcileItem {
                        subject_id: "opaque-first".into(),
                        kind: ProviderReconcileKind::ExecutionUnknown,
                    },
                    ProviderReconcileItem {
                        subject_id: "opaque-second".into(),
                        kind: ProviderReconcileKind::ExecutionInterrupted,
                    },
                ],
            },
        )),
    );
    let mut registry = ProviderRegistry::new();
    registry
        .register(unavailable_failure.clone(), ProviderHealth::Unavailable)
        .unwrap();
    registry
        .register(skipped.clone(), ProviderHealth::Available)
        .unwrap();
    registry
        .register(recovered.clone(), ProviderHealth::Available)
        .unwrap();
    let mut manager = AgentTaskManager::new(store, "must-not-launch.exe".into());
    manager.use_registry(registry);

    assert_eq!(
        manager.reconcile_startup().await.unwrap(),
        vec![
            ProviderReconcileItem {
                subject_id: "opaque-first".into(),
                kind: ProviderReconcileKind::ExecutionUnknown,
            },
            ProviderReconcileItem {
                subject_id: "opaque-second".into(),
                kind: ProviderReconcileKind::ExecutionInterrupted,
            },
        ]
    );
    assert_eq!(
        unavailable_failure.reconcile_calls.load(Ordering::SeqCst),
        1
    );
    assert_eq!(skipped.reconcile_calls.load(Ordering::SeqCst), 0);
    assert_eq!(recovered.reconcile_calls.load(Ordering::SeqCst), 1);
    let registry = manager.registry().unwrap();
    let failed_id = ProviderId::new("alpha".into()).unwrap();
    assert_eq!(
        registry.health(&failed_id).unwrap(),
        ProviderHealth::Unavailable
    );
    assert!(matches!(
        registry.get(&failed_id),
        Err(ProviderError {
            code: ProviderErrorCode::AgentProviderUnavailable,
        })
    ));
}

#[tokio::test]
async fn host_acceptance_is_one_shot_and_receiver_drop_does_not_close_the_sink() {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let sink = HostAcceptanceSink::new(Some(tx));
    sink.accepted();
    sink.accepted();
    assert_eq!(rx.await.unwrap(), Ok(()));
    assert!(sink.is_accepted());

    let (tx, rx) = tokio::sync::oneshot::channel();
    drop(rx);
    let dropped_receiver = HostAcceptanceSink::new(Some(tx));
    dropped_receiver.accepted();
    assert!(dropped_receiver.is_accepted());

    let (tx, rx) = tokio::sync::oneshot::channel();
    let rejected = HostAcceptanceSink::new(Some(tx));
    rejected.reject("AGENT_PROVIDER_OPERATION_FAILED".into());
    rejected.accepted();
    assert_eq!(
        rx.await.unwrap(),
        Err("AGENT_PROVIDER_OPERATION_FAILED".into())
    );
    assert!(!rejected.is_accepted());
}

#[test]
fn production_task_manager_has_only_registry_provider_routes() {
    let source = include_str!("../task_manager.rs");

    assert!(source.contains(".get(&provider_id)"));
    assert!(source.contains(".get_registered(&provider_id)"));
    assert!(source.contains("ProviderExecutionContext"));
    assert!(source.contains("ProviderCancelContext"));
    assert!(source.contains("ExecutionTelemetryProjector::new"));
    assert!(source.contains("run_client_with_acceptance_and_telemetry"));
    assert!(!source.contains("NoopEventSink"));
    assert!(!source.contains(".execute_with_acceptance("));
    assert!(!source.contains(".request_cancel("));
}

#[tokio::test]
async fn persisted_provider_routes_through_registry_trait_object() {
    let directory = tempfile::tempdir().unwrap();
    let store = StateStore::open(directory.path().into()).await.unwrap();
    let fake = Arc::new(FakeProvider::new(store.clone(), "codex", true, true));
    let manager = manager_with_provider(store, fake.clone(), ProviderHealth::Available);

    let outcome = manager
        .execute(input(directory.path(), "registry-route"))
        .await
        .unwrap();

    assert_eq!(outcome.execution.provider, "codex");
    assert_eq!(outcome.execution.status, "cancelled");
    assert_eq!(fake.execute_calls.load(Ordering::SeqCst), 1);
    assert_eq!(fake.cancel_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn fresh_create_rejects_generic_parent_before_persistence_or_dispatch() {
    let directory = tempfile::tempdir().unwrap();
    let store = StateStore::open(directory.path().into()).await.unwrap();
    let fake = Arc::new(FakeProvider::new(store.clone(), "codex", true, true));
    let manager = manager_with_provider(store.clone(), fake.clone(), ProviderHealth::Available);
    let mut rejected = input(directory.path(), "forged-parent");
    rejected.parent_execution_id = Some("source".into());

    assert_eq!(
        manager.create(rejected).await.unwrap_err(),
        "TASK006_REQUIRES_FRESH_READ_ONLY_DEFAULT_PROFILE"
    );
    assert!(
        store
            .product_history_ids(None, None)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        store
            .workspace_claim(directory.path().to_string_lossy().into())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(fake.execute_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn continuation_validation_uses_registration_not_execute_health() {
    let directory = tempfile::tempdir().unwrap();
    let store = StateStore::open(directory.path().into()).await.unwrap();
    let available_for_read = Arc::new(FakeProvider::new(store.clone(), "codex", true, true));
    let manager = manager_with_provider(
        store.clone(),
        available_for_read.clone(),
        ProviderHealth::Unavailable,
    );
    assert!(manager.can_continue("source".into(), "codex".into()).await);
    assert_eq!(
        available_for_read.continuation_calls.load(Ordering::SeqCst),
        1
    );
    assert_eq!(available_for_read.execute_calls.load(Ordering::SeqCst), 0);

    let unsupported =
        Arc::new(FakeProvider::new(store.clone(), "codex", true, true).without_continuation());
    let manager = manager_with_provider(
        store.clone(),
        unsupported.clone(),
        ProviderHealth::Available,
    );
    assert!(!manager.can_continue("source".into(), "codex".into()).await);
    assert_eq!(unsupported.continuation_calls.load(Ordering::SeqCst), 0);

    let ineligible = Arc::new(
        FakeProvider::new(store.clone(), "codex", true, true).with_ineligible_continuation(),
    );
    let manager = manager_with_provider(store, ineligible.clone(), ProviderHealth::Available);
    assert!(!manager.can_continue("source".into(), "codex".into()).await);
    assert_eq!(ineligible.continuation_calls.load(Ordering::SeqCst), 1);
    assert_eq!(ineligible.execute_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn receipt_precedes_provider_terminal_and_duplicate_acceptance() {
    let directory = tempfile::tempdir().unwrap();
    let store = StateStore::open(directory.path().into()).await.unwrap();
    let mut fake = FakeProvider::new(store.clone(), "codex", true, true);
    let release = Arc::new(tokio::sync::Notify::new());
    fake.release = Some(release.clone());
    let fake = Arc::new(fake);
    let manager = manager_with_provider(store.clone(), fake.clone(), ProviderHealth::Available);

    let execution_id = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        manager.product_submit(
            super::super::product::Action::Start {
                workspace_id: "routing-workspace".into(),
                agent_id: "routing-agent".into(),
                request_key: "acceptance-before-terminal".into(),
                prompt: "wait".into(),
            },
            Some(
                super::super::store::transactions::product::WorkspaceSnapshot {
                    id: "routing-workspace".into(),
                    root: directory.path().to_string_lossy().into(),
                    generation: 1,
                },
            ),
        ),
    )
    .await
    .unwrap()
    .unwrap();

    assert!(!fake.terminal.load(Ordering::SeqCst));
    assert_eq!(fake.execute_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        store
            .execution(execution_id.clone())
            .await
            .unwrap()
            .unwrap()
            .status,
        "dispatch_pending"
    );
    release.notify_one();
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while !fake.terminal.load(Ordering::SeqCst) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        store.execution(execution_id).await.unwrap().unwrap().status,
        "cancelled"
    );
}

#[tokio::test]
async fn terminal_success_without_acceptance_is_a_host_contract_failure() {
    let directory = tempfile::tempdir().unwrap();
    let store = StateStore::open(directory.path().into()).await.unwrap();
    let fake = Arc::new(FakeProvider::new(store.clone(), "codex", false, true));
    let manager = manager_with_provider(store, fake, ProviderHealth::Available);

    match manager
        .execute(input(directory.path(), "missing-acceptance"))
        .await
        .unwrap_err()
    {
        ProviderExecutionFailure::State(error) => {
            assert_eq!(error, "AGENT_PROVIDER_CONTRACT_ERROR")
        }
        error => panic!("unexpected failure: {error:?}"),
    }
}

#[tokio::test]
async fn runtime_failure_keeps_classification_and_safe_receipt_diagnostic() {
    let directory = tempfile::tempdir().unwrap();
    let store = StateStore::open(directory.path().into()).await.unwrap();
    let fake = Arc::new(
        FakeProvider::new(store.clone(), "codex", false, true)
            .with_runtime_failure("CODEX_RUNTIME_TEST_FAILED", "safe diagnostic"),
    );
    let manager = manager_with_provider(store, fake, ProviderHealth::Available);
    let created = manager
        .create(input(directory.path(), "runtime-failure"))
        .await
        .unwrap();
    let (tx, rx) = tokio::sync::oneshot::channel();

    assert_eq!(
        manager
            .dispatch_with_receipt(&created.execution_id, Some(tx), false)
            .await
            .unwrap_err(),
        ProviderExecutionFailure::Runtime {
            code: "CODEX_RUNTIME_TEST_FAILED".into(),
            message: "safe diagnostic".into(),
        }
    );
    assert_eq!(
        rx.await.unwrap(),
        Err("CODEX_RUNTIME_TEST_FAILED: safe diagnostic".into())
    );
}

#[tokio::test]
async fn unavailable_provider_cancel_uses_registration_and_preserves_manual_resolution() {
    let directory = tempfile::tempdir().unwrap();
    let store = StateStore::open(directory.path().into()).await.unwrap();
    let fake = Arc::new(FakeProvider::new(store.clone(), "codex", true, true));
    let manager = manager_with_provider(store.clone(), fake.clone(), ProviderHealth::Unavailable);
    let created = manager
        .create(input(directory.path(), "unavailable-cancel"))
        .await
        .unwrap();

    match manager
        .resume_pending_execution(&created.execution_id)
        .await
        .unwrap_err()
    {
        ProviderExecutionFailure::State(error) => {
            assert_eq!(error, "AGENT_PROVIDER_UNAVAILABLE")
        }
        error => panic!("unexpected failure: {error:?}"),
    }
    assert_eq!(fake.execute_calls.load(Ordering::SeqCst), 0);

    let cancelled = manager.cancel(&created.execution_id).await.unwrap();
    assert_eq!(cancelled.status, "cancelled");
    assert_eq!(fake.cancel_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        manager
            .registry()
            .unwrap()
            .health(&ProviderId::new("codex".into()).unwrap())
            .unwrap(),
        ProviderHealth::Unavailable
    );
    match manager
        .registry()
        .unwrap()
        .get(&ProviderId::new("codex".into()).unwrap())
    {
        Err(error) => assert_eq!(
            provider_error_code(error.code),
            "AGENT_PROVIDER_UNAVAILABLE"
        ),
        Ok(_) => panic!("unavailable provider resolved for execution"),
    }

    let manual = manager
        .create(input(directory.path(), "manual-resolution"))
        .await
        .unwrap();
    rusqlite::Connection::open(directory.path().join("agent-state.db"))
        .unwrap()
        .execute(
            "UPDATE executions SET status='unknown',dispatch_state='uncertain' WHERE id=?1",
            [&manual.execution_id],
        )
        .unwrap();
    let unchanged = manager.cancel(&manual.execution_id).await.unwrap();
    assert_eq!(unchanged.status, "unknown");
    assert_eq!(fake.cancel_calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn cancel_capability_and_unknown_registration_fail_closed() {
    let directory = tempfile::tempdir().unwrap();
    let store = StateStore::open(directory.path().into()).await.unwrap();
    let unsupported = Arc::new(FakeProvider::new(store.clone(), "codex", true, false));
    let manager = manager_with_provider(
        store.clone(),
        unsupported.clone(),
        ProviderHealth::Unavailable,
    );
    let created = manager
        .create(input(directory.path(), "unsupported-cancel"))
        .await
        .unwrap();
    assert_eq!(
        manager.cancel(&created.execution_id).await.unwrap_err(),
        "AGENT_PROVIDER_CAPABILITY_UNSUPPORTED"
    );
    assert_eq!(unsupported.cancel_calls.load(Ordering::SeqCst), 0);

    let other = Arc::new(FakeProvider::new(store.clone(), "other", true, true));
    let unknown_manager = manager_with_provider(store, other, ProviderHealth::Available);
    assert_eq!(
        unknown_manager
            .cancel(&created.execution_id)
            .await
            .unwrap_err(),
        "AGENT_PROVIDER_NOT_FOUND"
    );
}
