use super::automatic_recovery::{AutoRecoveryDecision, AutoRecoveryIneligibleReason};
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
use crate::agent::store::transactions::product::{WorkExecutionContext, WorkspaceSnapshot};
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
    state_failure: Option<&'static str>,
    terminal_failed_db: Option<std::path::PathBuf>,
    terminal_failed_once: AtomicBool,
    child_completed: Option<Arc<tokio::sync::Notify>>,
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
            state_failure: None,
            terminal_failed_db: None,
            terminal_failed_once: AtomicBool::new(false),
            child_completed: None,
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

    fn with_state_failure(mut self, code: &'static str) -> Self {
        self.state_failure = Some(code);
        self
    }

    /// 仅模拟 Provider 已安全写入 failed 终态后的稳定错误码。
    fn with_terminal_failed_once(mut self, database: std::path::PathBuf) -> Self {
        self.terminal_failed_db = Some(database);
        self.terminal_failed_once.store(true, Ordering::SeqCst);
        self
    }

    fn with_child_completion(mut self, completed: Arc<tokio::sync::Notify>) -> Self {
        self.child_completed = Some(completed);
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
            if let Some(code) = self.state_failure {
                return Err(ProviderExecutionFailure::State(code.into()));
            }
            if self.terminal_failed_once.swap(false, Ordering::SeqCst) {
                let database = self.terminal_failed_db.as_ref().unwrap();
                let connection = rusqlite::Connection::open(database).unwrap();
                mark_safe_failed(&connection, &context.execution_id);
                return Err(ProviderExecutionFailure::State(
                    "PROVIDER_TERMINAL_failed".into(),
                ));
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
            if let Some(completed) = &self.child_completed {
                completed.notify_waiters();
            }
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

/// 建立可由纯控制面判定的 Work-linked Execution；不会启动 Provider。
async fn automatic_recovery_root(store: &StateStore, root: &std::path::Path, id: &str) {
    store
        .create_work_run(
            "work".into(),
            "workspace".into(),
            root.to_string_lossy().into(),
            1,
            "goal".into(),
            Some("recover goal".into()),
            1,
        )
        .await
        .unwrap();
    store
        .product_create_fresh_with_work(
            id.into(),
            "agent".into(),
            "initial".into(),
            "original prompt must not be read by decision".into(),
            "workspace".into(),
            Some(WorkspaceSnapshot {
                id: "workspace".into(),
                root: root.to_string_lossy().into(),
                generation: 1,
            }),
            Some(WorkExecutionContext {
                work_run_id: "work".into(),
                parent_execution_id: None,
                delegation_context_json: None,
            }),
            1,
        )
        .await
        .unwrap();
}

/// 测试 fixture 直接写入已完成的安全终态，避免把 Provider/Runtime 逻辑混入判定单测。
fn mark_safe_failed(db: &rusqlite::Connection, id: &str) {
    db.execute(
        "UPDATE executions SET status='failed', dispatch_state='dispatched', \
         provider_terminal_status='failed', release_evidence_state='complete', \
         release_evidence_kind='same_runtime_cleanup', release_evidence_json='{}', \
         completed_at=2, interrupt_requested_at=NULL WHERE id=?1",
        [id],
    )
    .unwrap();
    db.execute("DELETE FROM workspace_claims WHERE execution_id=?1", [id])
        .unwrap();
}

fn auto_marker(root: &str, attempt: u8) -> String {
    format!(
        "{{\"kind\":\"auto_recovery\",\"rootExecutionId\":{},\"attempt\":{attempt}}}",
        serde_json::to_string(root).unwrap()
    )
}

async fn automatic_recovery_child(
    store: &StateStore,
    id: &str,
    parent: &str,
    context: Option<String>,
) {
    store
        .product_create_continuation_with_work(
            id.into(),
            parent.into(),
            format!("continue-{id}"),
            "unused continuation prompt".into(),
            Some(WorkExecutionContext {
                work_run_id: "work".into(),
                parent_execution_id: Some(parent.into()),
                delegation_context_json: context,
            }),
            None,
            3,
        )
        .await
        .unwrap();
}

/// 等待已有 Continue handoff 完成创建，避免以时间睡眠假设调度顺序。
async fn wait_for_work_links(store: &StateStore, expected: usize) {
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            if store
                .work_execution_links("work".into())
                .await
                .unwrap()
                .len()
                >= expected
            {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

#[test]
fn automatic_recovery_plan_is_deterministic_and_safe() {
    let work = crate::agent::store::WorkRunRecord {
        id: "work".into(),
        workspace_id: "workspace".into(),
        canonical_workspace_root: "root".into(),
        workspace_generation: 1,
        title: "Recover title".into(),
        goal: Some("Recover goal".into()),
        status: "active".into(),
        revision: 1,
        acceptance_json: None,
        created_at: 1,
        updated_at: 1,
        completed_at: None,
    };
    let decision = AutoRecoveryDecision::Eligible {
        work_run_id: "work".into(),
        root_execution_id: "E1".into(),
        parent_execution_id: "E2".into(),
        next_attempt: 2,
    };
    let first = super::automatic_recovery::build_plan(&decision, &work).unwrap();
    let second = super::automatic_recovery::build_plan(&decision, &work).unwrap();
    assert_eq!(first, second);
    assert_eq!(first.request_key, "auto-recovery:E1:2");
    assert_eq!(first.delegation_context_json, auto_marker("E1", 2));
    assert!(first.prompt.contains("Recover title"));
    assert!(first.prompt.contains("Recover goal"));
    assert!(first.prompt.contains("provider_terminal_failed"));
    assert!(!first.prompt.contains("original prompt"));
    assert!(!first.prompt.contains("raw stderr"));
}

#[tokio::test]
async fn automatic_recovery_decision_uses_persisted_lineage_and_survives_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let store = StateStore::open(directory.path().into()).await.unwrap();
    automatic_recovery_root(&store, directory.path(), "E1").await;
    let db = rusqlite::Connection::open(directory.path().join("agent-state.db")).unwrap();
    mark_safe_failed(&db, "E1");
    let manager = AgentTaskManager::new(store.clone(), "unused".into());
    assert_eq!(
        manager.evaluate_auto_recovery("E1").await.unwrap(),
        AutoRecoveryDecision::Eligible {
            work_run_id: "work".into(),
            root_execution_id: "E1".into(),
            parent_execution_id: "E1".into(),
            next_attempt: 1,
        }
    );

    automatic_recovery_child(&store, "E2", "E1", Some(auto_marker("E1", 1))).await;
    mark_safe_failed(&db, "E2");
    let reopened = StateStore::open(directory.path().into()).await.unwrap();
    let reopened_manager = AgentTaskManager::new(reopened.clone(), "unused".into());
    assert_eq!(
        reopened_manager.evaluate_auto_recovery("E2").await.unwrap(),
        AutoRecoveryDecision::Eligible {
            work_run_id: "work".into(),
            root_execution_id: "E1".into(),
            parent_execution_id: "E2".into(),
            next_attempt: 2,
        }
    );
    db.execute(
        "UPDATE work_execution_links SET delegation_context_json=?1 WHERE execution_id='E2'",
        [auto_marker("forged-root", 1)],
    )
    .unwrap();
    assert_eq!(
        reopened_manager.evaluate_auto_recovery("E2").await.unwrap(),
        AutoRecoveryDecision::NotEligible(AutoRecoveryIneligibleReason::LineageInconsistent)
    );
    db.execute(
        "UPDATE work_execution_links SET delegation_context_json=?1 WHERE execution_id='E2'",
        [auto_marker("E1", 1)],
    )
    .unwrap();

    automatic_recovery_child(&reopened, "E3", "E2", Some(auto_marker("E1", 2))).await;
    mark_safe_failed(&db, "E3");
    assert_eq!(
        reopened_manager.evaluate_auto_recovery("E3").await.unwrap(),
        AutoRecoveryDecision::NotEligible(AutoRecoveryIneligibleReason::BudgetExhausted)
    );
}

#[tokio::test]
async fn automatic_recovery_decision_does_not_count_manual_continuation() {
    let directory = tempfile::tempdir().unwrap();
    let store = StateStore::open(directory.path().into()).await.unwrap();
    automatic_recovery_root(&store, directory.path(), "E1").await;
    let db = rusqlite::Connection::open(directory.path().join("agent-state.db")).unwrap();
    mark_safe_failed(&db, "E1");
    automatic_recovery_child(&store, "manual-E2", "E1", None).await;
    mark_safe_failed(&db, "manual-E2");
    let manager = AgentTaskManager::new(store, "unused".into());
    assert_eq!(
        manager.evaluate_auto_recovery("manual-E2").await.unwrap(),
        AutoRecoveryDecision::Eligible {
            work_run_id: "work".into(),
            root_execution_id: "manual-E2".into(),
            parent_execution_id: "manual-E2".into(),
            next_attempt: 1,
        }
    );
}

#[tokio::test]
async fn automatic_recovery_decision_rejects_incomplete_or_malformed_facts() {
    let cases = [
        (
            "status='completed'",
            AutoRecoveryIneligibleReason::StatusNotFailed,
        ),
        (
            "status='cancelled'",
            AutoRecoveryIneligibleReason::StatusNotFailed,
        ),
        (
            "status='interrupted'",
            AutoRecoveryIneligibleReason::StatusNotFailed,
        ),
        (
            "status='reconciling'",
            AutoRecoveryIneligibleReason::StatusNotFailed,
        ),
        (
            "status='unknown'",
            AutoRecoveryIneligibleReason::StatusNotFailed,
        ),
        (
            "provider_terminal_status='completed'",
            AutoRecoveryIneligibleReason::ProviderTerminalNotFailed,
        ),
        (
            "dispatch_state='uncertain'",
            AutoRecoveryIneligibleReason::DispatchNotDispatched,
        ),
        (
            "release_evidence_state='incomplete'",
            AutoRecoveryIneligibleReason::ReleaseEvidenceIncomplete,
        ),
        (
            "interrupt_requested_at=9",
            AutoRecoveryIneligibleReason::InterruptRequested,
        ),
    ];
    for (change, expected) in cases {
        let directory = tempfile::tempdir().unwrap();
        let store = StateStore::open(directory.path().into()).await.unwrap();
        automatic_recovery_root(&store, directory.path(), "E1").await;
        let db = rusqlite::Connection::open(directory.path().join("agent-state.db")).unwrap();
        mark_safe_failed(&db, "E1");
        db.execute(&format!("UPDATE executions SET {change} WHERE id='E1'"), [])
            .unwrap();
        let manager = AgentTaskManager::new(store, "unused".into());
        assert_eq!(
            manager.evaluate_auto_recovery("E1").await.unwrap(),
            AutoRecoveryDecision::NotEligible(expected),
            "{change}"
        );
    }

    let directory = tempfile::tempdir().unwrap();
    let store = StateStore::open(directory.path().into()).await.unwrap();
    automatic_recovery_root(&store, directory.path(), "E1").await;
    let db = rusqlite::Connection::open(directory.path().join("agent-state.db")).unwrap();
    mark_safe_failed(&db, "E1");
    db.execute(
        "INSERT INTO workspace_claims(canonical_workspace_root,execution_id,claim_type,acquired_at) \
         VALUES (?1,'E1','exclusive_execution',3)",
        [directory.path().to_string_lossy().as_ref()],
    )
    .unwrap();
    let manager = AgentTaskManager::new(store.clone(), "unused".into());
    assert_eq!(
        manager.evaluate_auto_recovery("E1").await.unwrap(),
        AutoRecoveryDecision::NotEligible(AutoRecoveryIneligibleReason::WorkspaceClaimPresent)
    );
    db.execute("DELETE FROM workspace_claims WHERE execution_id='E1'", [])
        .unwrap();
    db.execute(
        "UPDATE work_runs SET status='completed' WHERE id='work'",
        [],
    )
    .unwrap();
    assert_eq!(
        manager.evaluate_auto_recovery("E1").await.unwrap(),
        AutoRecoveryDecision::NotEligible(AutoRecoveryIneligibleReason::WorkRunNotActive)
    );
    db.execute("UPDATE work_runs SET status='active' WHERE id='work'", [])
        .unwrap();
    db.execute(
        "UPDATE work_execution_links SET delegation_context_json='{\"kind\":\"auto_recovery\",\"attempt\":\"bad\"}' WHERE execution_id='E1'",
        [],
    )
    .unwrap();
    assert_eq!(
        manager.evaluate_auto_recovery("E1").await.unwrap(),
        AutoRecoveryDecision::NotEligible(AutoRecoveryIneligibleReason::LineageMalformed)
    );
    db.execute(
        "DELETE FROM work_execution_links WHERE execution_id='E1'",
        [],
    )
    .unwrap();
    assert_eq!(
        manager.evaluate_auto_recovery("E1").await.unwrap(),
        AutoRecoveryDecision::NotEligible(AutoRecoveryIneligibleReason::WorkLinkMissing)
    );
}

#[tokio::test]
async fn automatic_recovery_worker_notifies_terminal_failure_and_reuses_continue() {
    let directory = tempfile::tempdir().unwrap();
    let store = StateStore::open(directory.path().into()).await.unwrap();
    automatic_recovery_root(&store, directory.path(), "E1").await;
    let child_completed = Arc::new(tokio::sync::Notify::new());
    let provider = Arc::new(
        FakeProvider::new(store.clone(), "codex", true, true)
            .with_terminal_failed_once(directory.path().join("agent-state.db"))
            .with_child_completion(child_completed.clone()),
    );
    let manager = manager_with_provider(store.clone(), provider, ProviderHealth::Available);
    assert!(manager.start_auto_recovery_worker());
    assert!(!manager.start_auto_recovery_worker());

    let first_child = child_completed.notified();
    tokio::pin!(first_child);
    assert_eq!(
        manager
            .dispatch_with_receipt("E1", None, false)
            .await
            .unwrap_err(),
        ProviderExecutionFailure::State("PROVIDER_TERMINAL_failed".into())
    );
    first_child.await;
    wait_for_work_links(&store, 2).await;
    let parent = store.execution("E1".into()).await.unwrap().unwrap();
    assert_eq!(parent.status, "failed");
    assert_eq!(parent.release_evidence_state, "complete");
    let links = store.work_execution_links("work".into()).await.unwrap();
    let child_id = links[1].execution_id.clone();
    let child = store.execution(child_id.clone()).await.unwrap().unwrap();
    assert_eq!(child.workspace_id, parent.workspace_id);
    assert_eq!(
        child.canonical_workspace_root,
        parent.canonical_workspace_root
    );
    assert_eq!(child.workspace_generation, parent.workspace_generation);
    assert_eq!(links[1].parent_execution_id.as_deref(), Some("E1"));
    assert_eq!(links[1].delegation_context_json, Some(auto_marker("E1", 1)));

    // 同一终态重复投递仍复用 requestKey 对应的同一个 child。
    manager.notify_auto_recovery_for_test("E1");
    tokio::task::yield_now().await;
    wait_for_work_links(&store, 2).await;
    assert_eq!(
        store
            .work_execution_links("work".into())
            .await
            .unwrap()
            .len(),
        2
    );

    let db = rusqlite::Connection::open(directory.path().join("agent-state.db")).unwrap();
    mark_safe_failed(&db, &child_id);
    let second_child = child_completed.notified();
    tokio::pin!(second_child);
    manager.notify_auto_recovery_for_test(&child_id);
    second_child.await;
    wait_for_work_links(&store, 3).await;
    let third_id = store.work_execution_links("work".into()).await.unwrap()[2]
        .execution_id
        .clone();
    let third_link = store
        .work_execution_link(third_id.clone())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        third_link.parent_execution_id.as_deref(),
        Some(child_id.as_str())
    );
    assert_eq!(
        third_link.delegation_context_json,
        Some(auto_marker("E1", 2))
    );

    mark_safe_failed(&db, &third_id);
    assert_eq!(
        manager.schedule_auto_recovery(&third_id).await.unwrap(),
        super::automatic_recovery::AutoRecoverySchedule::Skipped(
            AutoRecoveryIneligibleReason::BudgetExhausted
        )
    );
    manager.notify_auto_recovery_for_test(&third_id);
    tokio::task::yield_now().await;
    assert_eq!(
        store
            .work_execution_links("work".into())
            .await
            .unwrap()
            .len(),
        3
    );

    manager.runtime_pool.stop.cancel();
    manager.wait_for_auto_recovery_worker_for_test().await;
}

#[tokio::test]
async fn automatic_recovery_worker_rejects_non_terminal_and_shutdown_notifications() {
    for kind in ["runtime", "protocol", "unavailable"] {
        let directory = tempfile::tempdir().unwrap();
        let store = StateStore::open(directory.path().into()).await.unwrap();
        automatic_recovery_root(&store, directory.path(), "E1").await;
        let provider = match kind {
            "runtime" => FakeProvider::new(store.clone(), "codex", true, true)
                .with_runtime_failure("CODEX_RUNTIME_TEST_FAILED", "safe"),
            "protocol" => FakeProvider::new(store.clone(), "codex", true, true)
                .with_state_failure("CODEX_PROTOCOL_INVALID_MESSAGE"),
            _ => FakeProvider::new(store.clone(), "codex", true, true),
        };
        let health = if kind == "unavailable" {
            ProviderHealth::Unavailable
        } else {
            ProviderHealth::Available
        };
        let manager = manager_with_provider(store.clone(), Arc::new(provider), health);
        assert!(manager.start_auto_recovery_worker());
        assert!(
            manager
                .dispatch_with_receipt("E1", None, false)
                .await
                .is_err()
        );
        tokio::task::yield_now().await;
        assert_eq!(
            store
                .work_execution_links("work".into())
                .await
                .unwrap()
                .len(),
            1
        );
        manager.runtime_pool.stop.cancel();
        manager.wait_for_auto_recovery_worker_for_test().await;
    }

    let directory = tempfile::tempdir().unwrap();
    let store = StateStore::open(directory.path().into()).await.unwrap();
    automatic_recovery_root(&store, directory.path(), "E1").await;
    let db = rusqlite::Connection::open(directory.path().join("agent-state.db")).unwrap();
    mark_safe_failed(&db, "E1");
    let manager = manager_with_provider(
        store.clone(),
        Arc::new(FakeProvider::new(store.clone(), "codex", true, true)),
        ProviderHealth::Available,
    );
    assert!(manager.start_auto_recovery_worker());
    manager.runtime_pool.stop.cancel();
    manager.notify_auto_recovery_for_test("E1");
    manager.wait_for_auto_recovery_worker_for_test().await;
    assert_eq!(
        store
            .work_execution_links("work".into())
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn automatic_recovery_schedule_failure_preserves_parent_facts() {
    let directory = tempfile::tempdir().unwrap();
    let store = StateStore::open(directory.path().into()).await.unwrap();
    automatic_recovery_root(&store, directory.path(), "E1").await;
    let db = rusqlite::Connection::open(directory.path().join("agent-state.db")).unwrap();
    mark_safe_failed(&db, "E1");
    let provider = Arc::new(
        FakeProvider::new(store.clone(), "codex", true, true).with_ineligible_continuation(),
    );
    let manager = manager_with_provider(store.clone(), provider.clone(), ProviderHealth::Available);
    let parent = store.execution("E1".into()).await.unwrap().unwrap();
    assert!(manager.start_auto_recovery_worker());
    manager.notify_auto_recovery_for_test("E1");
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while provider.continuation_calls.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(store.execution("E1".into()).await.unwrap().unwrap(), parent);
    assert!(
        store
            .workspace_claim(parent.canonical_workspace_root.clone())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        store
            .work_execution_links("work".into())
            .await
            .unwrap()
            .len(),
        1
    );
    manager.runtime_pool.stop.cancel();
    manager.wait_for_auto_recovery_worker_for_test().await;
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
