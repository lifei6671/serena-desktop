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
                return Err(ExecutionFailure::Runtime(error));
            }
        };
        let result = self
            .run_client_with_acceptance(id, &managed.client, acceptance)
            .await;
        managed
            .shutdown()
            .await
            .map_err(ExecutionFailure::Runtime)?;
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
        if row.status != "reconciling" && row.provider_terminal_status.is_none() {
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
                return outcome;
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
        if outcome.is_err() {
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
                .thread_start(
                    &row.canonical_workspace_root,
                    serde_json::from_value(serde_json::json!(row.mode))
                        .map_err(|e| e.to_string())?,
                )
                .await
                .map_err(|e| e.to_string())?
        };
        self.bind(id, client, &thread.id, None).await?;
        // Product continue is accepted only after exact managed Thread validation.
        if let Some(receipt) = acceptance.take() {
            let _ = receipt.send(Ok(()));
        }
        let (flushed_tx, mut flushed_rx) = tokio::sync::oneshot::channel();
        let request = client.turn_start_observed(&thread.id, id, &row.prompt, flushed_tx);
        tokio::pin!(request);
        let (mut flushed, mut acknowledged, mut terminal) = (false, false, false);
        // One owner, one interrupt future. The DB is authoritative; polling also
        // observes intent committed before this Provider began receiving events.
        let mut interrupt: Option<
            std::pin::Pin<
                Box<dyn std::future::Future<Output = super::protocol::Result<()>> + Send + '_>,
            >,
        > = None;
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
                event = client.receive_event() => {
                    let event = event.map_err(|e| e.to_string())?;
                    if event.runtime_id != client.runtime_id() { return Err("PROVIDER_RUNTIME_MISMATCH".into()); }
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
                        Notification::ThreadStarted(t) if t.id != thread.id => return Err("PROVIDER_THREAD_MISMATCH".into()),
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
