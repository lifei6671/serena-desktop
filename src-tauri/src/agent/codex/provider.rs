//! Maps one persisted Execution to one original Runtime/Thread/Turn.
use super::{
    app_server::{CleanupScope, Client, managed, recovery::RecoveryScope},
    protocol::{Notification, TurnStatus},
};
use crate::agent::{
    coordinator::{WorkspaceExecutionCoordinator, now},
    execution::state::{DispatchState, Status, Transition},
    provider::{
        ProviderCancelContext, ProviderCapabilities, ProviderDescriptor, ProviderError,
        ProviderErrorCode, ProviderExecutionContext, ProviderId, ProviderOutcome,
        ProviderResultCompleteness, ProviderRunResult, ProviderStartupContext,
        port::{
            AgentEventSink, AgentProvider, ProviderAcceptanceSink, ProviderContinuationContext,
            ProviderContinuationDecision, ProviderExecutionFailure, ProviderFuture,
            ProviderReconcileItem, ProviderReconcileKind, ProviderReconcileSummary,
        },
        registry::{ProviderHealth, ProviderRegistry},
    },
    store::{ExecutionRecord, StateStore},
    task_manager::recovery::{RecoveryOutcome, recover_startup_with_authority},
};
use std::{path::PathBuf, sync::Arc, time::Duration};

pub(crate) struct CodexProvider {
    pub store: StateStore,
    pub executable: PathBuf,
    pub(crate) backend_error: Option<String>,
    pub owner: String,
    pub runtime_pool: std::sync::Arc<super::pool::CodexRuntimePool>,
}
#[derive(Debug)]
pub enum ExecutionFailure {
    State(String),
    Runtime(super::runtime::RuntimeFailure),
}

fn provider_execution_failure(error: ExecutionFailure) -> ProviderExecutionFailure {
    match error {
        ExecutionFailure::State(value) => ProviderExecutionFailure::State(value),
        ExecutionFailure::Runtime(failure) => ProviderExecutionFailure::Runtime {
            code: failure.code.to_string(),
            message: failure.message,
        },
    }
}

/// Codex-private managed continuation provenance. This is deliberately shared
/// by the public eligibility projection and the runtime parent resolver so a
/// child can only resume the exact Thread that was validated for its source.
fn managed_continuation_thread(row: &ExecutionRecord) -> Option<&str> {
    let thread = row.thread_id.as_deref().filter(|value| !value.is_empty())?;
    let turn = row.turn_id.as_deref().filter(|value| !value.is_empty())?;
    let runtime = row
        .runtime_instance_id
        .as_deref()
        .filter(|value| !value.is_empty())?;
    let result = row
        .final_result_json
        .as_deref()
        .and_then(|result| serde_json::from_str::<serde_json::Value>(result).ok())?;
    (result["historyMode"] == "paginated"
        && result["executionId"].as_str() == Some(row.id.as_str())
        && result["turnId"].as_str() == Some(turn)
        && result["threadId"].as_str() == Some(thread)
        && result["sourceRuntimeId"].as_str() == Some(runtime))
    .then_some(thread)
}

fn activity_telemetry_event(
    execution_id: &str,
    envelope_runtime_id: &str,
    client_runtime_id: &str,
    root_thread_id: &str,
    row: &ExecutionRecord,
    activity: &super::protocol::Activity,
) -> Option<crate::agent::provider::telemetry::AgentActivityEvent> {
    if !has_exact_telemetry_binding(
        execution_id,
        envelope_runtime_id,
        client_runtime_id,
        root_thread_id,
        row,
        &activity.thread_id,
        &activity.turn_id,
    ) {
        return None;
    }
    match (activity.phase, activity.tool_category) {
        (crate::agent::activity::ActivityPhase::Provider, None) => Some(
            crate::agent::provider::telemetry::AgentActivityEvent::provider(
                execution_id.into(),
                activity.observed_at,
            ),
        ),
        (crate::agent::activity::ActivityPhase::Tool, Some(category)) => {
            Some(crate::agent::provider::telemetry::AgentActivityEvent::tool(
                execution_id.into(),
                category,
                activity.observed_at,
            ))
        }
        _ => None,
    }
}

/// 复用 Activity 与 Usage 的唯一 private execution/runtime/thread/turn 绑定权威。
fn has_exact_telemetry_binding(
    execution_id: &str,
    envelope_runtime_id: &str,
    client_runtime_id: &str,
    root_thread_id: &str,
    row: &ExecutionRecord,
    notification_thread_id: &str,
    notification_turn_id: &str,
) -> bool {
    row.id == execution_id
        && envelope_runtime_id == client_runtime_id
        && row.runtime_instance_id.as_deref() == Some(client_runtime_id)
        && notification_thread_id == root_thread_id
        && row.thread_id.as_deref() == Some(root_thread_id)
        && row.thread_id.as_deref() == Some(notification_thread_id)
        && row.turn_id.as_deref() == Some(notification_turn_id)
}

/// 将已严格绑定的 Codex private cumulative snapshot 映射为 Provider-Agnostic event。
fn usage_telemetry_event(
    execution_id: &str,
    envelope_runtime_id: &str,
    client_runtime_id: &str,
    root_thread_id: &str,
    row: &ExecutionRecord,
    usage: &super::protocol::Usage,
) -> Option<crate::agent::provider::telemetry::UsageEvent> {
    if !has_exact_telemetry_binding(
        execution_id,
        envelope_runtime_id,
        client_runtime_id,
        root_thread_id,
        row,
        &usage.thread_id,
        &usage.turn_id,
    ) {
        return None;
    }
    Some(crate::agent::provider::telemetry::UsageEvent::cumulative(
        execution_id.into(),
        ProviderId::new("codex".into()).ok()?,
        usage.total.total_tokens,
        usage.total.input_tokens,
        usage.total.cached_input_tokens,
        usage.total.cache_write_input_tokens,
        usage.total.output_tokens,
        usage.total.reasoning_output_tokens,
        usage.model_context_window,
        usage.observed_at,
    ))
}

/// 将 Thread 来源与 warm reuse 事实收敛为唯一的 Provider-private baseline 意图。
fn usage_baseline_intent(
    continuation_thread: Option<&str>,
    warm_observed_same_epoch: bool,
) -> crate::agent::store::CodexUsageBaselineIntent {
    match (continuation_thread, warm_observed_same_epoch) {
        (None, _) => crate::agent::store::CodexUsageBaselineIntent::FreshZero,
        (Some(_), true) => crate::agent::store::CodexUsageBaselineIntent::WarmObservedSameEpoch,
        (Some(_), false) => crate::agent::store::CodexUsageBaselineIntent::Unknown,
    }
}

/// 识别同 Root/runtime 的前一 Turn Usage；调用者已在 event loop 验证 runtime。
fn is_late_usage_turn(
    row: &ExecutionRecord,
    root_thread_id: &str,
    usage: &super::protocol::Usage,
) -> bool {
    usage.thread_id == root_thread_id
        && row.thread_id.as_deref() == Some(root_thread_id)
        && row.turn_id.as_deref() != Some(usage.turn_id.as_str())
}

impl From<String> for ExecutionFailure {
    fn from(error: String) -> Self {
        Self::State(error)
    }
}
struct NoopAcceptanceSink;
impl ProviderAcceptanceSink for NoopAcceptanceSink {
    fn accepted(&self) {}
}
struct NoopEventSink;
impl AgentEventSink for NoopEventSink {}
impl CodexProvider {
    async fn continuation_runtime_thread(
        &self,
        parent_execution_id: &str,
    ) -> Result<String, String> {
        let source = self.row(parent_execution_id).await?;
        managed_continuation_thread(&source)
            .map(str::to_owned)
            .ok_or_else(|| "AGENT_CONTINUE_NOT_ALLOWED".into())
    }

    async fn runtime_continuation_target(
        &self,
        row: &ExecutionRecord,
    ) -> Result<Option<String>, String> {
        match row.parent_execution_id.as_deref() {
            Some(parent_execution_id) => self
                .continuation_runtime_thread(parent_execution_id)
                .await
                .map(Some),
            // Bounded compatibility for pre-C2 and previously bound rows only.
            None => Ok(row.thread_id.clone()),
        }
    }

    pub async fn execute(&self, id: &str) -> Result<ExecutionRecord, ExecutionFailure> {
        self.execute_with_acceptance_and_telemetry(
            id,
            Arc::new(NoopAcceptanceSink),
            Arc::new(NoopEventSink),
        )
        .await
    }
    pub(crate) async fn execute_with_acceptance(
        &self,
        id: &str,
        acceptance: Arc<dyn ProviderAcceptanceSink>,
    ) -> Result<ExecutionRecord, ExecutionFailure> {
        self.execute_with_acceptance_and_telemetry(id, acceptance, Arc::new(NoopEventSink))
            .await
    }
    async fn execute_with_acceptance_and_telemetry(
        &self,
        id: &str,
        acceptance: Arc<dyn ProviderAcceptanceSink>,
        telemetry: Arc<dyn AgentEventSink>,
    ) -> Result<ExecutionRecord, ExecutionFailure> {
        let _worker = self.runtime_pool.enter().await?;
        let row = self.row(id).await?;
        if row.status == "cancelled" && row.dispatch_state == "not_dispatched" {
            return Ok(row);
        }
        let mut lease = self
            .runtime_pool
            .lease(&self.store, &row.canonical_workspace_root)
            .await?;
        if self.runtime_pool.stop.is_cancelled() {
            return Err("AGENT_SHUTTING_DOWN".to_string().into());
        }
        if lease.as_ref().is_some_and(|m| !m.client.reusable()) {
            let stale = lease.take().unwrap();
            let runtime_id = stale.client.runtime_id().to_owned();
            stale.shutdown().await.map_err(|failure| {
                ExecutionFailure::Runtime(self.runtime_pool.retain_failure(
                    &self.store,
                    &row.canonical_workspace_root,
                    &runtime_id,
                    failure,
                ))
            })?;
        }
        if lease.is_none() {
            let runtime_id = crate::agent::task_manager::AgentTaskManager::id("runtime");
            let executable = if self.executable.as_os_str().is_empty() {
                match super::discovery::discover().await {
                    Ok(path) => path,
                    Err(error) => {
                        let diagnostic = match self.failed(id).await {
                            Ok(()) => error,
                            Err(state) => format!("{error}; reconciliation persistence: {state}"),
                        };
                        return Err(diagnostic.into());
                    }
                }
            } else {
                self.executable.clone()
            };
            let managed = match self
                .connect(
                    id,
                    self.store.clone(),
                    self.owner.clone(),
                    runtime_id.clone(),
                    executable,
                    PathBuf::from(&row.canonical_workspace_root),
                )
                .await
            {
                Ok(managed) => managed,
                Err(error) => {
                    let mut error = self
                        .runtime_pool
                        .retain_attempt_failure(
                            &self.store,
                            &row.canonical_workspace_root,
                            &runtime_id,
                            error,
                        )
                        .await;
                    if let Err(state) = self.failed(id).await {
                        error
                            .message
                            .push_str(&format!("; reconciliation persistence: {state}"));
                    }
                    // connect has already converged its owner, or returns that owner
                    // in RuntimeFailure. An unbound attempt cannot be replayed.
                    let marked = async {
                        if self.row(id).await?.status != "dispatch_pending" {
                            crate::agent::task_manager::recovery::mark_unknown(&self.store, id)
                                .await?;
                        }
                        Ok::<(), String>(())
                    }
                    .await;
                    if let Err(state) = marked {
                        error.message.push_str(&format!("; mark unknown: {state}"));
                    }
                    return Err(ExecutionFailure::Runtime(error));
                }
            };
            *lease = Some(managed);
        }
        self.run_leased(id, &mut lease, acceptance.as_ref(), telemetry.as_ref())
            .await
    }
    async fn connect(
        &self,
        id: &str,
        store: StateStore,
        owner: String,
        runtime_id: String,
        executable: PathBuf,
        workspace: PathBuf,
    ) -> Result<managed::ManagedClient, super::runtime::RuntimeFailure> {
        #[cfg(test)]
        {
            let connect = self.runtime_pool.test_connect.lock().unwrap().clone();
            if let Some(connect) = connect {
                store
                    .reserve_runtime_attempt(id.into(), runtime_id.clone(), now())
                    .await
                    .map_err(|e| {
                        super::runtime::RuntimeError::new("CODEX_RUNTIME_STORE_FAILED", e)
                    })?;
                return connect(runtime_id, workspace).await;
            }
        }
        managed::connect(
            store,
            owner,
            runtime_id,
            executable,
            workspace,
            Some(managed::RuntimeAttempt::Dispatch(id.into())),
        )
        .await
    }
    #[cfg(test)]
    async fn run_managed(
        &self,
        id: &str,
        managed: managed::ManagedClient,
        acceptance: Arc<dyn ProviderAcceptanceSink>,
    ) -> Result<ExecutionRecord, ExecutionFailure> {
        let _worker = self.runtime_pool.enter().await?;
        let mut lease = self
            .runtime_pool
            .lease(&self.store, &self.row(id).await?.canonical_workspace_root)
            .await?;
        assert!(lease.is_none());
        *lease = Some(managed);
        self.run_leased(id, &mut lease, acceptance.as_ref(), &NoopEventSink)
            .await
    }
    async fn run_leased(
        &self,
        id: &str,
        lease: &mut Option<managed::ManagedClient>,
        acceptance: &dyn ProviderAcceptanceSink,
        telemetry: &dyn AgentEventSink,
    ) -> Result<ExecutionRecord, ExecutionFailure> {
        let client = &lease.as_ref().unwrap().client;
        let result = tokio::select! {
            biased;
            _ = self.runtime_pool.stop.cancelled() => {
                self.failed(id).await?;
                Err("AGENT_SHUTTING_DOWN".into())
            },
            result = self.run_client_with_acceptance_and_telemetry(id, client, acceptance, telemetry) => result,
        };
        let row = self.row(id).await;
        if let Ok(row) = &row
            && matches!(
                row.status.as_str(),
                "completed" | "failed" | "cancelled" | "interrupted"
            )
            && row.release_evidence_kind.as_deref() == Some("same_runtime_cleanup")
            && row.release_evidence_state == "complete"
            && row.result_completeness == "complete"
            && client
                .finish_execution(
                    &self.store,
                    row.thread_id.as_deref(),
                    row.turn_id.as_deref(),
                )
                .await
                .is_ok()
            && !self.runtime_pool.stop.is_cancelled()
        {
            return result.map_err(ExecutionFailure::State);
        }
        let runtime_id = client.runtime_id().to_owned();
        let workspace = self.row(id).await?.canonical_workspace_root;
        let termination = lease.take().unwrap().shutdown().await.map_err(|failure| {
            self.runtime_pool
                .retain_failure(&self.store, &workspace, &runtime_id, failure)
        });
        self.finish_after_shutdown(id, result, termination).await
    }
    /// Only called after the ManagedClient monitor has returned ownership/evidence.
    async fn finish_after_shutdown(
        &self,
        id: &str,
        result: Result<ExecutionRecord, String>,
        termination: Result<(), super::runtime::RuntimeFailure>,
    ) -> Result<ExecutionRecord, ExecutionFailure> {
        use crate::agent::task_manager::recovery::{
            RecoveryOutcome, mark_unknown, reconcile_execution_after_runtime_end,
        };
        if let Err(mut failure) = termination {
            if let Err(error) = mark_unknown(&self.store, id).await {
                failure
                    .message
                    .push_str(&format!("; mark unknown: {error}"));
            }
            return Err(ExecutionFailure::Runtime(failure));
        }
        let row = self.row(id).await?;
        if result.is_err()
            && !matches!(
                row.status.as_str(),
                "completed" | "failed" | "cancelled" | "interrupted"
            )
        {
            match reconcile_execution_after_runtime_end(
                &self.store,
                &self.executable,
                &self.owner,
                &self.runtime_pool,
                None,
                id,
            )
            .await
            {
                Ok(RecoveryOutcome::RuntimeFailure { failure, .. }) => {
                    return Err(ExecutionFailure::Runtime(failure));
                }
                Ok(_) => {}
                Err(error) => {
                    mark_unknown(&self.store, id).await?;
                    return Err(ExecutionFailure::State(format!(
                        "{}; reconciliation: {error}",
                        result.unwrap_err()
                    )));
                }
            }
        }
        result.map_err(ExecutionFailure::State)
    }
    async fn row(&self, id: &str) -> Result<ExecutionRecord, String> {
        self.store
            .execution(id.into())
            .await?
            .ok_or_else(|| "EXECUTION_NOT_FOUND".into())
    }
    async fn event(&self, id: &str, event: Transition) -> Result<(), String> {
        self.store.provider_event(id.into(), event, now()).await
    }
    pub(crate) async fn failed(&self, id: &str) -> Result<(), String> {
        let row = self.row(id).await?;
        if matches!(
            row.status.as_str(),
            "completed" | "failed" | "cancelled" | "interrupted"
        ) {
            return Ok(());
        }
        // No Provider boundary was crossed. Keep the existing pending identity and
        // Claim; explicit resume or cancel-before-dispatch remains available.
        if row.status == "dispatch_pending"
            && row.dispatch_state == "not_dispatched"
            && row.runtime_instance_id.is_none()
            && row.provider_terminal_status.is_none()
            // Persisted pre-bind attempts are never safe to replay.
            && !self.store.has_runtime_attempt(id.into()).await?
            && self.store.workspace_claim(row.canonical_workspace_root.clone()).await?
                .is_some_and(|claim| claim.execution_id == row.id)
        {
            return Ok(());
        }
        if row.dispatch_state == "dispatching" {
            self.event(
                id,
                Transition::Dispatch {
                    to: DispatchState::Uncertain,
                    runtime_id: None,
                },
            )
            .await?;
        }
        let row = self.row(id).await?;
        if !matches!(row.status.as_str(), "reconciling" | "unknown")
            && row.provider_terminal_status.is_none()
        {
            self.event(id, Transition::Reconcile).await?;
        }
        Ok(())
    }
    async fn bind(
        &self,
        id: &str,
        client: &Client,
        thread: &str,
        turn: Option<String>,
    ) -> Result<(), String> {
        loop {
            let outcome = self
                .store
                .bind_protocol_identity(
                    id.into(),
                    self.row(id).await?.revision,
                    client.runtime_id().into(),
                    thread.into(),
                    turn.clone(),
                    now(),
                )
                .await;
            if outcome.as_ref().err().map(String::as_str) != Some("EXECUTION_REVISION_CONFLICT") {
                outcome?;
                client
                    .bind_observability_scope(thread, turn.as_deref())
                    .map_err(|error| error.to_string())?;
                return Ok(());
            }
        }
    }
    pub(crate) async fn run_client(
        &self,
        id: &str,
        client: &Client,
    ) -> Result<ExecutionRecord, String> {
        self.run_client_with_acceptance(id, client, &NoopAcceptanceSink)
            .await
    }
    pub(crate) async fn run_client_with_acceptance(
        &self,
        id: &str,
        client: &Client,
        acceptance: &dyn ProviderAcceptanceSink,
    ) -> Result<ExecutionRecord, String> {
        self.run_client_with_acceptance_and_telemetry(id, client, acceptance, &NoopEventSink)
            .await
    }
    pub(crate) async fn run_client_with_acceptance_and_telemetry(
        &self,
        id: &str,
        client: &Client,
        acceptance: &dyn ProviderAcceptanceSink,
        telemetry: &dyn AgentEventSink,
    ) -> Result<ExecutionRecord, String> {
        let outcome = async {
            let row = self.row(id).await?;
            if row.status == "cancelled" && row.dispatch_state == "not_dispatched" {
                return Ok(row);
            }
            client
                .prepare_execution(&self.store)
                .await
                .map_err(|e| e.to_string())?;
            self.run_active_client(id, client, acceptance, telemetry)
                .await
        }
        .await;
        if let Err(error) = &outcome {
            self.store
                .execution_diagnostic(
                    id.into(),
                    "CODEX_PROVIDER_FAILURE".into(),
                    error.clone(),
                    now(),
                )
                .await?;
            self.failed(id).await?;
        }
        outcome
    }
    async fn run_active_client(
        &self,
        id: &str,
        client: &Client,
        acceptance: &dyn ProviderAcceptanceSink,
        telemetry: &dyn AgentEventSink,
    ) -> Result<ExecutionRecord, String> {
        let row = self.row(id).await?;
        if row.status == "cancelled" && row.dispatch_state == "not_dispatched" {
            return Ok(row);
        }
        let dispatch = self
            .event(
                id,
                Transition::Dispatch {
                    to: DispatchState::Dispatching,
                    runtime_id: Some(client.runtime_id().into()),
                },
            )
            .await;
        if let Err(error) = dispatch {
            let row = self.row(id).await?;
            if row.status == "cancelled" && row.dispatch_state == "not_dispatched" {
                return Ok(row);
            }
            return Err(error);
        }
        client
            .enable_root_title(self.store.clone(), id)
            .map_err(|e| e.to_string())?;
        let mode =
            serde_json::from_value(serde_json::json!(row.mode)).map_err(|e| e.to_string())?;
        // A parent is the current continuation authority. A persisted child
        // thread is only the bounded pre-C2 fallback when there is no parent.
        let continuation_thread = self.runtime_continuation_target(&row).await?;
        let warm = continuation_thread
            .as_deref()
            .is_some_and(|thread_id| client.loaded_thread(thread_id).is_some());
        let thread = if let Some(thread_id) = continuation_thread.as_deref() {
            let thread = if let Some(thread) = client.loaded_thread(thread_id) {
                thread
            } else {
                client
                    .thread_resume(thread_id)
                    .await
                    .map_err(|e| e.to_string())?
            };
            if thread.history_mode != super::protocol::HistoryMode::Paginated {
                return Err(
                    "CODEX_APP_SERVER_INCOMPATIBLE: continuation requires paginated history".into(),
                );
            }
            thread
        } else {
            client
                .thread_start(&row.canonical_workspace_root, mode)
                .await
                .map_err(|e| e.to_string())?
        };
        self.bind(id, client, &thread.id, None).await?;
        // Thread bind 完成后、创建 Turn 副作用前冻结 Usage epoch baseline；遥测失败不能改变 Execution 结果。
        let baseline_intent = usage_baseline_intent(continuation_thread.as_deref(), warm);
        if let Err(error) = self
            .store
            .prepare_codex_usage_baseline(
                id.into(),
                client.runtime_id().into(),
                thread.id.clone(),
                baseline_intent,
                now(),
            )
            .await
        {
            eprintln!("Codex usage baseline preparation dropped: {error}");
        }
        if !warm {
            self.store
                .save_thread_name(thread.id.clone(), thread.name.clone())
                .await?;
        }
        // Product continue is accepted only after exact managed Thread validation.
        acceptance.accepted();
        let (flushed_tx, mut flushed_rx) = tokio::sync::oneshot::channel();
        let request = client.turn_start_observed(
            &thread.id,
            id,
            &row.prompt,
            mode,
            &row.canonical_workspace_root,
            flushed_tx,
        );
        tokio::pin!(request);
        let (mut flushed, mut acknowledged, mut terminal) = (false, false, false);
        // Provider terminal 只固定一次 Usage grace deadline；后续 recover/cleanup/finish 不得重置它。
        let mut usage_grace_deadline = None;
        // One owner, one interrupt future. The DB is authoritative; polling also
        // observes intent committed before this Provider began receiving events.
        let mut interrupt: Option<
            std::pin::Pin<
                Box<dyn std::future::Future<Output = super::protocol::Result<()>> + Send + '_>,
            >,
        > = None;
        // Only a non-retry error starts this bounded terminal grace period.
        // Ordinary long-running turns and retry notifications have no deadline.
        let mut error_deadline = None;
        let mut interrupt_sent = false;
        let mut interrupt_done = false;
        let mut cancel_poll = tokio::time::interval(Duration::from_millis(100));
        cancel_poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        // Exact persisted terminal identity supersedes the turn/start ACK. Keep the
        // request future alive through cleanup; dropping it cancels the Client.
        while !(flushed && terminal && (!interrupt_sent || interrupt_done)) {
            let row = self.row(id).await?;
            if !interrupt_sent
                && row.interrupt_requested_at.is_some()
                && row.provider_terminal_status.is_none()
                && matches!(
                    row.status.as_str(),
                    "dispatch_pending" | "running" | "cancel_requested"
                )
                && let (Some(thread), Some(turn)) = (row.thread_id, row.turn_id)
            {
                if row.runtime_instance_id.as_deref() != Some(client.runtime_id()) {
                    return Err("PROVIDER_RUNTIME_MISMATCH".into());
                }
                if row.status == "dispatch_pending" {
                    self.event(id, Transition::Running).await?;
                }
                self.store.request_cancel(id.into(), now()).await?;
                interrupt_sent = true;
                interrupt = Some(Box::pin(async move {
                    client.turn_interrupt(&thread, &turn).await
                }));
            }
            tokio::select! {
                biased;
                outcome = async { interrupt.as_mut().unwrap().await }, if interrupt_sent && !interrupt_done => {
                    interrupt_done = true;
                    match outcome {
                        Ok(()) => self.event(id, Transition::InterruptAck).await?,
                        Err(error) if error.code == "CODEX_RPC_TIMEOUT" => {
                            self.event(id, Transition::InterruptTimeout {diagnostic: error.to_string()}).await?;
                            if self.row(id).await?.provider_terminal_status.is_some() { return Err("INTERRUPT_TIMEOUT_AFTER_TERMINAL".into()); }
                            return Err(error.to_string());
                        }
                        Err(error) => return Err(error.to_string()),
                    }
                }
                outcome = &mut flushed_rx, if !flushed => {
                    outcome.map_err(|_| "TURN_FLUSH_NOT_CONFIRMED")?;
                    self.event(id, Transition::Dispatch {to: DispatchState::Dispatched, runtime_id: None}).await?;
                    flushed = true;
                }
                outcome = &mut request, if !acknowledged && (!terminal || !flushed) => {
                    let turn = outcome.map_err(|e| e.to_string())?;
                    self.bind(id, client, &thread.id, Some(turn.id)).await?;
                    acknowledged = true;
                    // Late ACK fills identity only; it cannot undo finalizing.
                }
                _ = async { match error_deadline { Some(deadline) => tokio::time::sleep_until(deadline).await, None => std::future::pending().await } }, if !terminal => {
                    return Err("CODEX_TURN_ERROR_TERMINAL_TIMEOUT: non-retry error without turn/completed".into());
                }
                event = client.receive_event() => {
                    let event = event.map_err(|e| e.to_string())?;
                    if event.runtime_id != client.runtime_id() {
                        if matches!(&event.notification, Notification::Activity(_) | Notification::Usage(_)) {
                            eprintln!("Codex telemetry hint dropped");
                            continue;
                        }
                        return Err("PROVIDER_RUNTIME_MISMATCH".into());
                    }
                    let event_thread = match &event.notification {
                        Notification::ThreadStarted(t) => Some(&t.id),
                        Notification::TurnStarted {thread_id,..} | Notification::TurnCompleted {thread_id,..}
                        | Notification::TurnError {thread_id,..} | Notification::ThreadNameUpdated {thread_id,..}
                        | Notification::SubAgentStarted {thread_id,..}
                        | Notification::PermissionDenied {thread_id,..}
                        | Notification::Usage(super::protocol::Usage { thread_id, .. }) => Some(thread_id),
                        Notification::Activity(activity) => Some(&activity.thread_id),
                        _ => None,
                    };
                    // Only the Root owns lifecycle authority. Validate wire shapes
                    // before this point, but never bind or persist unowned events.
                    if event_thread.is_some_and(|id| id != &thread.id) { continue; }
                    let terminal_event = matches!(&event.notification, Notification::TurnCompleted { .. });
                    match event.notification {
                        Notification::TurnStarted { thread_id, turn } | Notification::TurnCompleted { thread_id, turn } => {
                            if thread_id != thread.id { return Err("PROVIDER_THREAD_MISMATCH".into()); }
                            self.bind(id, client, &thread.id, Some(turn.id.clone())).await?;
                            if !terminal_event {
                                if turn.status != TurnStatus::InProgress { return Err("PROVIDER_STARTED_STATUS_INVALID".into()); }
                                if self.row(id).await?.status == "dispatch_pending" { self.event(id, Transition::Running).await?; }
                            } else {
                                    let status = match turn.status {TurnStatus::Completed => Status::Completed, TurnStatus::Failed => Status::Failed, TurnStatus::Interrupted => Status::Interrupted, _ => return Err("PROVIDER_TERMINAL_STATUS_INVALID".into())};
                                    self.event(id, Transition::ProviderTerminal {runtime_id: client.runtime_id().into(), status}).await?;
                                    let terminal_at = now();
                                    // deadline 从 ProviderTerminal 立即开始；Store I/O 不得借机延长本次 drain 窗口。
                                    let grace_deadline = tokio::time::Instant::now()
                                        + Duration::from_millis(crate::agent::store::USAGE_TERMINAL_GRACE_MS as u64);
                                    match self.store.enter_codex_usage_terminal_grace(
                                        id.into(),
                                        client.runtime_id().into(),
                                        thread.id.clone(),
                                        turn.id.clone(),
                                        terminal_at,
                                    ).await {
                                        Ok(()) => usage_grace_deadline = Some(grace_deadline),
                                        Err(error) => eprintln!("Codex usage terminal grace dropped: {error}"),
                                    }
                                    terminal = true;
                            }
                        }
                        Notification::TurnError { thread_id, turn_id, error, will_retry } => {
                            if thread_id != thread.id { return Err("PROVIDER_THREAD_MISMATCH".into()); }
                            self.bind(id, client, &thread_id, Some(turn_id.clone())).await?;
                            self.store.execution_diagnostic(id.into(), "CODEX_TURN_ERROR".into(),
                                serde_json::json!({"runtimeId": client.runtime_id(), "threadId": thread_id,
                                    "turnId": turn_id, "willRetry": will_retry, "error": error}).to_string(), now()).await?;
                            if will_retry {
                                error_deadline = None;
                            } else if error_deadline.is_none() {
                                error_deadline = Some(tokio::time::Instant::now() + super::protocol::RPC_TIMEOUT);
                            }
                        }
                        Notification::ThreadStarted(t) if t.id != thread.id => return Err("PROVIDER_THREAD_MISMATCH".into()),
                        Notification::ThreadNameUpdated { thread_id, name } => {
                            if thread_id != thread.id { return Err("PROVIDER_THREAD_MISMATCH".into()); }
                            self.store.save_thread_name(thread_id, name).await?;
                        }
                        Notification::Activity(activity) => {
                            let row = match self.row(id).await {
                                Ok(row) => row,
                                Err(error) => {
                                    eprintln!("Codex activity hint dropped: {error}");
                                    continue;
                                }
                            };
                            let Some(event) = activity_telemetry_event(
                                id,
                                &event.runtime_id,
                                client.runtime_id(),
                                &thread.id,
                                &row,
                                &activity,
                            ) else {
                                eprintln!("Codex activity hint dropped");
                                continue;
                            };
                            telemetry
                                .publish(crate::agent::provider::telemetry::AgentTelemetryEvent::Activity(event))
                                .await;
                        }
                        Notification::Usage(usage) => {
                            let row = match self.row(id).await {
                                Ok(row) => row,
                                Err(error) => {
                                    eprintln!("Codex usage hint dropped: {error}");
                                    continue;
                                }
                            };
                            // 同 Root/runtime 的旧 Turn 累计值会污染 Continue 基线；先使本 Execution 的 Usage 降级，随后绝不发布该事件。
                            if is_late_usage_turn(&row, &thread.id, &usage) {
                                if let Err(error) = self
                                    .store
                                    .invalidate_codex_usage_baseline(
                                        id.into(),
                                        client.runtime_id().into(),
                                        thread.id.clone(),
                                        usage.observed_at,
                                    )
                                    .await
                                {
                                    eprintln!("Codex usage baseline invalidation dropped: {error}");
                                }
                                continue;
                            }
                            let Some(event) = usage_telemetry_event(
                                id,
                                &event.runtime_id,
                                client.runtime_id(),
                                &thread.id,
                                &row,
                                &usage,
                            ) else {
                                eprintln!("Codex usage hint dropped");
                                continue;
                            };
                            telemetry
                                .publish(crate::agent::provider::telemetry::AgentTelemetryEvent::Usage(event))
                                .await;
                        }
                        Notification::PermissionDenied { thread_id, turn_id, kind } => {
                            let row = match self.row(id).await {
                                Ok(row) => row,
                                Err(error) => {
                                    eprintln!("Codex permission hint dropped: {error}");
                                    continue;
                                }
                            };
                            if row.thread_id.as_deref() == Some(thread_id.as_str())
                                && row.turn_id.as_deref() == Some(turn_id.as_str())
                                && let Err(error) = self.store.execution_diagnostic(
                                    id.into(),
                                    "CODEX_PERMISSION_DENIED".into(),
                                    kind.as_str().into(),
                                    now(),
                                ).await
                            {
                                eprintln!("Codex permission hint dropped: {error}");
                            }
                        }
                        _ => {}
                    }
                }
                _ = cancel_poll.tick() => {},
            }
        }
        let scope = RecoveryScope::same_runtime_for_execution(&self.store, id, client.runtime_id())
            .await
            .map_err(|e| e.to_string())?;
        let result = client
            .recover_result(scope)
            .await
            .map_err(|e| e.to_string())?;
        let scope = CleanupScope::for_execution(&self.store, id)
            .await
            .map_err(|e| e.to_string())?;
        let empty = client.cleanup(scope).await.map_err(|e| e.to_string())?;
        let row = WorkspaceExecutionCoordinator {
            store: self.store.clone(),
        }
        .finish(id, result, empty)
        .await?;
        if let Some(deadline) = usage_grace_deadline {
            self.drain_terminal_usage(id, client, telemetry, &thread.id, &row, deadline)
                .await;
        } else if let Err(error) = self.store.freeze_codex_usage(id.into(), now()).await {
            // Grace 无法建立时只做 fail-safe telemetry freeze；Execution 已终态，不能升级为 Provider failure。
            eprintln!("Codex usage fail-safe freeze dropped: {error}");
        }
        // Terminal evidence can safely finish the Execution before the ACK. Only
        // a settled request can keep the transport reusable; never wait/replay it
        // to manufacture success. Dropping an unresolved request cancels Client.
        if !acknowledged {
            use std::future::Future;
            let _ =
                std::future::poll_fn(|cx| std::task::Poll::Ready(request.as_mut().poll(cx))).await;
        }
        if row.status == "failed" {
            return Err("PROVIDER_TERMINAL_failed".into());
        }
        Ok(row)
    }

    /// 在 Execution/Claim 已终态后，按原 terminal deadline 仅接收当前 Root 的 exact Usage。
    async fn drain_terminal_usage(
        &self,
        execution_id: &str,
        client: &Client,
        telemetry: &dyn AgentEventSink,
        root_thread_id: &str,
        terminal_row: &ExecutionRecord,
        deadline: tokio::time::Instant,
    ) {
        if tokio::time::Instant::now() >= deadline {
            if let Err(error) = self
                .store
                .freeze_codex_usage(execution_id.into(), now())
                .await
            {
                eprintln!("Codex usage grace freeze dropped: {error}");
            }
            return;
        }
        loop {
            tokio::select! {
                biased;
                _ = tokio::time::sleep_until(deadline) => break,
                incoming = client.receive_event() => {
                    let event = match incoming {
                        Ok(event) => event,
                        Err(error) => {
                            eprintln!("Codex usage grace receive closed: {error}");
                            break;
                        }
                    };
                    if event.runtime_id != client.runtime_id() {
                        continue;
                    }
                    match event.notification {
                        // cleanup RPC 与其通知是独立帧；grace 若丢弃该通知会让下一次 warm turn
                        // 读取到旧标题。终态后只允许持久化同一 Root 的公共标题，不改变 Execution 生命周期。
                        Notification::ThreadNameUpdated { thread_id, name }
                            if thread_id == root_thread_id =>
                        {
                            if let Err(error) = self.store.save_thread_name(thread_id, name).await {
                                eprintln!("Codex terminal-grace title dropped: {error}");
                            }
                        }
                        Notification::Usage(usage) => {
                            // terminal grace 不再运行 old-turn baseline invalidation：非精确 identity 直接丢弃。
                            let Some(usage) = usage_telemetry_event(
                                execution_id,
                                &event.runtime_id,
                                client.runtime_id(),
                                root_thread_id,
                                terminal_row,
                                &usage,
                            ) else {
                                continue;
                            };
                            telemetry
                                .publish(crate::agent::provider::telemetry::AgentTelemetryEvent::Usage(usage))
                                .await;
                        }
                        _ => continue,
                    }
                }
            }
        }
        if let Err(error) = self
            .store
            .freeze_codex_usage(execution_id.into(), now())
            .await
        {
            eprintln!("Codex usage grace freeze dropped: {error}");
        }
    }
}

pub(crate) async fn register_codex_provider(
    registry: &mut ProviderRegistry,
    store: StateStore,
    owner: String,
    runtime_pool: Arc<super::pool::CodexRuntimePool>,
) -> Result<(), ProviderError> {
    register_codex_provider_with_discovery(
        registry,
        store,
        owner,
        runtime_pool,
        super::discovery::discover().await,
    )
}

pub(crate) fn register_codex_provider_with_discovery(
    registry: &mut ProviderRegistry,
    store: StateStore,
    owner: String,
    runtime_pool: Arc<super::pool::CodexRuntimePool>,
    discovery: Result<PathBuf, String>,
) -> Result<(), ProviderError> {
    let (executable, backend_error, health) = match discovery {
        Ok(executable) => (executable, None, ProviderHealth::Available),
        Err(error) => (PathBuf::new(), Some(error), ProviderHealth::Unavailable),
    };
    registry.register(
        Arc::new(CodexProvider {
            store,
            executable,
            backend_error,
            owner,
            runtime_pool,
        }),
        health,
    )
}

fn provider_run_result(row: ExecutionRecord) -> Result<ProviderRunResult, ProviderError> {
    let outcome = match row.status.as_str() {
        "completed" => ProviderOutcome::Completed,
        "failed" => ProviderOutcome::Failed,
        "cancelled" => ProviderOutcome::Cancelled,
        "interrupted" => ProviderOutcome::Interrupted,
        _ => {
            return Err(ProviderError {
                code: ProviderErrorCode::AgentProviderContractError,
            });
        }
    };
    let result_completeness = match row.result_completeness.as_str() {
        "unknown" => ProviderResultCompleteness::Unknown,
        "partial" => ProviderResultCompleteness::Partial,
        "complete" => ProviderResultCompleteness::Complete,
        _ => {
            return Err(ProviderError {
                code: ProviderErrorCode::AgentProviderContractError,
            });
        }
    };
    let result = row
        .final_result_json
        .map(|value| serde_json::from_str(&value))
        .transpose()
        .map_err(|_| ProviderError {
            code: ProviderErrorCode::AgentProviderContractError,
        })?;
    Ok(ProviderRunResult {
        execution_id: row.id,
        outcome,
        result,
        result_completeness,
        diagnostic_code: row.error_code,
    })
}

fn provider_reconcile_item(outcome: &RecoveryOutcome) -> ProviderReconcileItem {
    let (subject_id, kind) = match outcome {
        RecoveryOutcome::OrphanRuntime {
            runtime_id,
            failure: None,
        } => (
            runtime_id.clone(),
            ProviderReconcileKind::OrphanResourceRecovered,
        ),
        RecoveryOutcome::OrphanRuntime {
            runtime_id,
            failure: Some(_),
        } => (
            runtime_id.clone(),
            ProviderReconcileKind::OrphanResourceUnknown,
        ),
        RecoveryOutcome::Released { execution_id } => (
            execution_id.clone(),
            ProviderReconcileKind::ExecutionReleased,
        ),
        RecoveryOutcome::Inconsistent { execution_id, .. } => (
            execution_id.clone(),
            ProviderReconcileKind::ExecutionInconsistent,
        ),
        RecoveryOutcome::PendingExplicitResume { execution_id } => (
            execution_id.clone(),
            ProviderReconcileKind::ExecutionPendingExplicitResume,
        ),
        RecoveryOutcome::Unknown { execution_id, .. } => (
            execution_id.clone(),
            ProviderReconcileKind::ExecutionUnknown,
        ),
        RecoveryOutcome::RuntimeFailure { execution_id, .. } => (
            execution_id.clone(),
            ProviderReconcileKind::ExecutionProviderFailure,
        ),
        RecoveryOutcome::Interrupted { execution, .. } => (
            execution.id.clone(),
            ProviderReconcileKind::ExecutionInterrupted,
        ),
    };
    ProviderReconcileItem { subject_id, kind }
}

impl AgentProvider for CodexProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: ProviderId::new("codex".into()).expect("static Codex provider id is valid"),
            display_name: "Codex".into(),
            version: Some(super::protocol::VERSION.into()),
        }
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            can_execute: true,
            can_continue: true,
            can_cancel: true,
            can_recover: true,
            activity: true,
            token_usage: false,
        }
    }

    fn execute<'a>(
        &'a self,
        context: ProviderExecutionContext,
        acceptance: Arc<dyn ProviderAcceptanceSink>,
        telemetry: Arc<dyn AgentEventSink>,
    ) -> ProviderFuture<'a, Result<ProviderRunResult, ProviderExecutionFailure>> {
        Box::pin(async move {
            let row = self
                .execute_with_acceptance_and_telemetry(&context.execution_id, acceptance, telemetry)
                .await
                .map_err(provider_execution_failure)?;
            provider_run_result(row).map_err(|_| {
                ProviderExecutionFailure::State("AGENT_PROVIDER_CONTRACT_ERROR".into())
            })
        })
    }

    fn cancel<'a>(
        &'a self,
        context: ProviderCancelContext,
    ) -> ProviderFuture<'a, Result<(), ProviderError>> {
        Box::pin(async move {
            self.store
                .request_cancel(context.execution_id, now())
                .await
                .map(|_| ())
                .map_err(|_| ProviderError {
                    code: ProviderErrorCode::AgentProviderOperationFailed,
                })
        })
    }

    fn validate_continuation<'a>(
        &'a self,
        context: ProviderContinuationContext,
    ) -> ProviderFuture<'a, Result<ProviderContinuationDecision, ProviderError>> {
        Box::pin(async move {
            let row = self
                .store
                .execution(context.source_execution_id)
                .await
                .map_err(|_| ProviderError {
                    code: ProviderErrorCode::AgentProviderOperationFailed,
                })?
                .ok_or(ProviderError {
                    code: ProviderErrorCode::AgentProviderOperationFailed,
                })?;
            let eligible = managed_continuation_thread(&row).is_some();
            Ok(if eligible {
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
            let outcomes = recover_startup_with_authority(
                &self.store,
                &self.executable,
                &self.owner,
                &self.runtime_pool,
                self.backend_error.as_deref(),
            )
            .await
            .map_err(|failure| ProviderError {
                code: if failure.code == "CODEX_APP_SERVER_INCOMPATIBLE" {
                    ProviderErrorCode::AgentProviderCapabilityUnsupported
                } else {
                    ProviderErrorCode::AgentProviderOperationFailed
                },
            })?;
            Ok(ProviderReconcileSummary {
                items: outcomes.iter().map(provider_reconcile_item).collect(),
            })
        })
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod cancellation_tests;

#[cfg(test)]
mod adapter_tests;
