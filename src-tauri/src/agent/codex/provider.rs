//! Maps one persisted Execution to one original Runtime/Thread/Turn.
use super::{
    app_server::{CleanupScope, Client, managed, recovery::RecoveryScope},
    protocol::{Notification, TurnStatus},
};
use crate::agent::{
    coordinator::{WorkspaceExecutionCoordinator, now},
    execution::state::{DispatchState, Status, Transition},
    store::{ExecutionRecord, StateStore},
};
use std::{path::PathBuf, time::Duration};

pub(crate) struct CodexProvider {
    pub store: StateStore,
    pub executable: PathBuf,
    pub owner: String,
}
#[derive(Debug)]
pub enum ExecutionFailure {
    State(String),
    Runtime(super::runtime::RuntimeFailure),
}
impl From<String> for ExecutionFailure {
    fn from(error: String) -> Self {
        Self::State(error)
    }
}
impl CodexProvider {
    pub async fn execute(&self, id: &str) -> Result<ExecutionRecord, ExecutionFailure> {
        self.execute_with_acceptance(id, &mut None).await
    }
    pub(crate) async fn execute_with_acceptance(
        &self,
        id: &str,
        acceptance: &mut Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
    ) -> Result<ExecutionRecord, ExecutionFailure> {
        let row = self.row(id).await?;
        if row.status == "cancelled" && row.dispatch_state == "not_dispatched" {
            return Ok(row);
        }
        let managed = match managed::connect(
            self.store.clone(),
            self.owner.clone(),
            format!("runtime-{id}"),
            self.executable.clone(),
            PathBuf::from(&row.canonical_workspace_root),
        )
        .await
        {
            Ok(managed) => managed,
            Err(mut error) => {
                if let Err(state) = self.failed(id).await {
                    error
                        .message
                        .push_str(&format!("; reconciliation persistence: {state}"));
                }
                // connect has already converged its owner, or returns that owner
                // in RuntimeFailure. An unbound attempt cannot be replayed.
                let marked = async {
                    if self.row(id).await?.status != "dispatch_pending" {
                        crate::agent::task_manager::recovery::mark_unknown(&self.store, id).await?;
                    }
                    Ok::<(), String>(())
                }.await;
                if let Err(state) = marked {
                    error.message.push_str(&format!("; mark unknown: {state}"));
                }
                return Err(ExecutionFailure::Runtime(error));
            }
        };
        self.run_managed(id, managed, acceptance).await
    }
    async fn run_managed(
        &self, id: &str, managed: managed::ManagedClient,
        acceptance: &mut Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
    ) -> Result<ExecutionRecord, ExecutionFailure> {
        let result = self
            .run_client_with_acceptance(id, &managed.client, acceptance)
            .await;
        let termination = managed.shutdown().await;
        self.finish_after_shutdown(id, result, termination).await
    }
    /// Only called after the ManagedClient monitor has returned ownership/evidence.
    async fn finish_after_shutdown(
        &self,
        id: &str,
        result: Result<ExecutionRecord, String>,
        termination: Result<(), super::runtime::RuntimeFailure>,
    ) -> Result<ExecutionRecord, ExecutionFailure> {
        use crate::agent::task_manager::recovery::{mark_unknown, reconcile_execution_after_runtime_end, RecoveryOutcome};
        if let Err(mut failure) = termination {
            if let Err(error) = mark_unknown(&self.store, id).await {
                failure.message.push_str(&format!("; mark unknown: {error}"));
            }
            return Err(ExecutionFailure::Runtime(failure));
        }
        let row = self.row(id).await?;
        if result.is_err() && !matches!(row.status.as_str(), "completed" | "failed" | "cancelled" | "interrupted") {
            match reconcile_execution_after_runtime_end(&self.store, &self.executable, &self.owner, None, id).await {
                Ok(RecoveryOutcome::RuntimeFailure { failure, .. }) => return Err(ExecutionFailure::Runtime(failure)),
                Ok(_) => {},
                Err(error) => {
                    mark_unknown(&self.store, id).await?;
                    return Err(ExecutionFailure::State(format!("{}; reconciliation: {error}", result.unwrap_err())));
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
            // A created but unbound Runtime cannot reuse this immutable attempt ID.
            && self.store.runtime(format!("runtime-{id}")).await?.is_none()
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
        if !matches!(row.status.as_str(), "reconciling" | "unknown") && row.provider_terminal_status.is_none() {
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
        self.run_client_with_acceptance(id, client, &mut None).await
    }
    pub(crate) async fn run_client_with_acceptance(
        &self,
        id: &str,
        client: &Client,
        acceptance: &mut Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
    ) -> Result<ExecutionRecord, String> {
        let outcome = self.run_active_client(id, client, acceptance).await;
        if let Err(error) = &outcome {
            self.store.execution_diagnostic(id.into(), "CODEX_PROVIDER_FAILURE".into(), error.clone(), now()).await?;
            self.failed(id).await?;
        }
        outcome
    }
    async fn run_active_client(
        &self,
        id: &str,
        client: &Client,
        acceptance: &mut Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
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
        client.enable_root_title(self.store.clone(), id).map_err(|e| e.to_string())?;
        let mode =
            serde_json::from_value(serde_json::json!(row.mode)).map_err(|e| e.to_string())?;
        let thread = if let Some(thread_id) = &row.thread_id {
            let thread = client
                .thread_resume(thread_id)
                .await
                .map_err(|e| e.to_string())?;
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
        self.store.save_thread_name(thread.id.clone(), thread.name.clone()).await?;
        // Product continue is accepted only after exact managed Thread validation.
        if let Some(receipt) = acceptance.take() {
            let _ = receipt.send(Ok(()));
        }
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
                    if event.runtime_id != client.runtime_id() { return Err("PROVIDER_RUNTIME_MISMATCH".into()); }
                    let event_thread = match &event.notification {
                        Notification::ThreadStarted(t) => Some(&t.id),
                        Notification::TurnStarted {thread_id,..} | Notification::TurnCompleted {thread_id,..}
                        | Notification::TurnError {thread_id,..} | Notification::ThreadNameUpdated {thread_id,..}
                        | Notification::SubAgentStarted {thread_id,..}
                        | Notification::PermissionDenied {thread_id,..} => Some(thread_id),
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
                            self.bind(id, client, &thread.id, Some(turn.id)).await?;
                            if !terminal_event {
                                if turn.status != TurnStatus::InProgress { return Err("PROVIDER_STARTED_STATUS_INVALID".into()); }
                                if self.row(id).await?.status == "dispatch_pending" { self.event(id, Transition::Running).await?; }
                            } else {
                                    let status = match turn.status {TurnStatus::Completed => Status::Completed, TurnStatus::Failed => Status::Failed, TurnStatus::Interrupted => Status::Interrupted, _ => return Err("PROVIDER_TERMINAL_STATUS_INVALID".into())};
                                    self.event(id, Transition::ProviderTerminal {runtime_id: client.runtime_id().into(), status}).await?;
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
                            if row.thread_id.as_deref() == Some(activity.thread_id.as_str())
                                && row.turn_id.as_deref() == Some(activity.turn_id.as_str())
                                && let Err(error) = self.store.execution_activity(
                                    id.into(),
                                    activity.thread_id,
                                    activity.turn_id,
                                    activity.phase,
                                    activity.tool_category,
                                    activity.observed_at,
                                ).await
                            {
                                eprintln!("Codex activity hint dropped: {error}");
                            }
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
        if row.status == "failed" {
            return Err("PROVIDER_TERMINAL_failed".into());
        }
        Ok(row)
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod cancellation_tests;
