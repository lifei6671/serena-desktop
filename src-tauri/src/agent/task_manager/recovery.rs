//! Startup and live reconciliation share only the post-termination phase.
//! Claims select work; Runtime owns Job evidence; no Provider dispatch is replayed.
use super::AgentTaskManager;
use crate::agent::{
    codex::{
        app_server::{managed, recovery::RecoveryScope},
        runtime,
    },
    coordinator::{WorkspaceExecutionCoordinator, now},
    execution::state::{RecoveryBasis, Transition},
    store::{ExecutionRecord, transactions::ClaimRecovery},
};
use std::{path::PathBuf, time::Duration};

#[derive(Debug)]
pub enum RecoveryOutcome {
    Released {
        execution_id: String,
    },
    Inconsistent {
        execution_id: String,
        code: &'static str,
    },
    PendingExplicitResume {
        execution_id: String,
    },
    Unknown {
        execution_id: String,
        failure: Option<runtime::RuntimeFailure>,
    },
    RuntimeFailure {
        execution_id: String,
        failure: runtime::RuntimeFailure,
    },
    Interrupted {
        execution: Box<ExecutionRecord>,
        result_diagnostic: Option<String>,
    },
}

impl AgentTaskManager {
    /// Reconcile durable Claims before the Product service is published at startup.
    pub async fn recover_startup(&self) -> Result<Vec<RecoveryOutcome>, String> {
        let claims = self.store.recover_claims(now()).await?;
        let mut outcomes = Vec::new();
        for claim in claims {
            let id = match claim {
                ClaimRecovery::PendingExplicitResume { execution_id } => {
                    outcomes.push(RecoveryOutcome::PendingExplicitResume { execution_id });
                    continue;
                }
                ClaimRecovery::Released { execution_id } => {
                    outcomes.push(RecoveryOutcome::Released { execution_id });
                    continue;
                }
                ClaimRecovery::Inconsistent { execution_id, code } => {
                    outcomes.push(RecoveryOutcome::Inconsistent { execution_id, code });
                    continue;
                }
                ClaimRecovery::Pending { execution_id }
                | ClaimRecovery::Unknown { execution_id } => execution_id,
            };
            let row = self
                .store
                .execution(id.clone())
                .await?
                .ok_or("EXECUTION_NOT_FOUND")?;
            let Some(original) = row.runtime_instance_id.clone() else {
                if row.status != "unknown" {
                    if row.status != "reconciling" {
                        self.store
                            .provider_event(id.clone(), Transition::Reconcile, now())
                            .await?;
                    }
                    self.store
                        .provider_event(id.clone(), Transition::MarkUnknown, now())
                        .await?;
                }
                outcomes.push(RecoveryOutcome::Unknown {
                    execution_id: id,
                    failure: None,
                });
                continue;
            };
            if let Err(failure) = runtime::recover(
                self.store.clone(),
                original.clone(),
                Duration::from_secs(10),
            )
            .await
            {
                if row.status != "unknown" {
                    if row.status != "reconciling" {
                        self.store
                            .provider_event(id.clone(), Transition::Reconcile, now())
                            .await?;
                    }
                    self.store
                        .provider_event(id.clone(), Transition::MarkUnknown, now())
                        .await?;
                }
                outcomes.push(RecoveryOutcome::Unknown {
                    execution_id: id,
                    failure: Some(failure),
                });
                continue;
            }
            outcomes.push(
                reconcile_execution_after_runtime_end(
                    &self.store,
                    &self.executable,
                    &self.owner,
                    self.backend_error.as_deref(),
                    &id,
                )
                .await?,
            );
        }
        Ok(outcomes)
    }
}

/// No Job acquisition or Provider dispatch here. The caller must first finish its
/// Runtime owner; persisted evidence is checked again before recovery and release.
pub(crate) async fn reconcile_execution_after_runtime_end(
    store: &crate::agent::store::StateStore,
    executable: &std::path::Path,
    owner: &str,
    backend_error: Option<&str>,
    execution_id: &str,
) -> Result<RecoveryOutcome, String> {
    let id = execution_id.to_owned();
    let row = store
        .execution(id.clone())
        .await?
        .ok_or("EXECUTION_NOT_FOUND")?;
    let Some(original) = row.runtime_instance_id.clone() else {
        mark_unknown(store, &id).await?;
        return Ok(RecoveryOutcome::Unknown {
            execution_id: id,
            failure: None,
        });
    };
    let evidence = store.runtime(original.clone()).await?;
    if !evidence.as_ref().is_some_and(|r| {
        r.state == "terminated"
            && r.termination_evidence_state == "complete"
            && matches!(
                r.termination_evidence_type.as_deref(),
                Some("job_active_processes_zero" | "managed_job_destroyed")
            )
            && r.termination_evidence_at.is_some()
    }) {
        mark_unknown(store, &id).await?;
        return Ok(RecoveryOutcome::Unknown {
            execution_id: id,
            failure: None,
        });
    }
    let evidence = evidence.unwrap();
    if row.status == "unknown" {
        let event = Transition::ResumeRecovery(RecoveryBasis::RuntimeTermination {
            runtime_id: original,
            evidence_at: evidence
                .termination_evidence_at
                .ok_or("RUNTIME_EVIDENCE_REQUIRED")?,
        });
        if let Err(error) = store.provider_event(id.clone(), event, now()).await {
            if error != "NEW_RECOVERY_EVIDENCE_REQUIRED" {
                return Err(error);
            }
            return Ok(RecoveryOutcome::Unknown {
                execution_id: id,
                failure: None,
            });
        }
    } else if row.status != "reconciling" {
        store
            .provider_event(id.clone(), Transition::Reconcile, now())
            .await?;
    }
    let row = store
        .execution(id.clone())
        .await?
        .ok_or("EXECUTION_NOT_FOUND")?;
    let mut result = None;
    let mut diagnostic = Some("Exact persisted Thread/Turn unavailable".to_string());
    // R2 is read-only and cannot be created until original Job recovery succeeded.
    if row.thread_id.is_some() && row.turn_id.is_some() {
        let recovery_id = AgentTaskManager::id("recovery-runtime");
        let scope = RecoveryScope::after_termination_for_execution(&store, &id, &recovery_id).await;
        match scope {
            Err(error) => diagnostic = Some(error.to_string()),
            Ok(scope) => {
                let connected = if let Some(error) = backend_error {
                    Err(runtime::RuntimeFailure {
                        code: if error.starts_with("CODEX_APP_SERVER_INCOMPATIBLE") { "CODEX_APP_SERVER_INCOMPATIBLE" } else { "BACKEND_UNAVAILABLE" },
                        message: error.to_owned(),
                        runtime: None,
                    })
                } else {
                    managed::connect(
                        store.clone(),
                        owner.to_owned(),
                        recovery_id,
                        executable.to_owned(),
                        PathBuf::from(&row.canonical_workspace_root),
                    )
                    .await
                };
                match connected {
                    Err(mut failure) => {
                        if let Err(error) = mark_unknown(store, &id).await {
                            failure.message.push_str(&format!("; mark unknown: {error}"));
                        }
                        return Ok(RecoveryOutcome::RuntimeFailure {
                            execution_id: id,
                            failure,
                        });
                    }
                    Ok(managed) => {
                        match managed.client.recover_result(scope).await {
                            Ok(recovered) => {
                                result = Some(recovered);
                                diagnostic = None;
                            }
                            Err(error) => diagnostic = Some(error.to_string()),
                        }
                        if let Err(mut failure) = managed.shutdown().await {
                            if let Err(error) = mark_unknown(store, &id).await {
                                failure.message.push_str(&format!("; mark unknown: {error}"));
                            }
                            return Ok(RecoveryOutcome::RuntimeFailure {
                                execution_id: id,
                                failure,
                            });
                        }
                    }
                }
            }
        }
    }
    let execution = WorkspaceExecutionCoordinator {
        store: store.clone(),
    }
    .finish_runtime_terminated(&id, row.revision, result)
    .await?;
    Ok(RecoveryOutcome::Interrupted {
        execution: Box::new(execution),
        result_diagnostic: diagnostic,
    })
}

pub(crate) async fn mark_unknown(
    store: &crate::agent::store::StateStore,
    id: &str,
) -> Result<(), String> {
    let row = store
        .execution(id.into())
        .await?
        .ok_or("EXECUTION_NOT_FOUND")?;
    if matches!(
        row.status.as_str(),
        "unknown" | "completed" | "failed" | "cancelled" | "interrupted"
    ) {
        return Ok(());
    }
    if row.status != "reconciling" {
        store
            .provider_event(id.into(), Transition::Reconcile, now())
            .await?;
    }
    store
        .provider_event(id.into(), Transition::MarkUnknown, now())
        .await
}

#[cfg(test)]
mod tests;
