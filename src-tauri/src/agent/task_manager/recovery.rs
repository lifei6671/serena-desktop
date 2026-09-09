//! Startup-only orchestration. The previous Host must have exited before entry.
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
    /// Internal recovery context only; not wired to UI, startup hooks or a scheduler.
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
            let evidence = self
                .store
                .runtime(original.clone())
                .await?
                .ok_or("RUNTIME_NOT_FOUND")?;
            if row.status == "unknown" {
                let event = Transition::ResumeRecovery(RecoveryBasis::RuntimeTermination {
                    runtime_id: original,
                    evidence_at: evidence
                        .termination_evidence_at
                        .ok_or("RUNTIME_EVIDENCE_REQUIRED")?,
                });
                if let Err(error) = self.store.provider_event(id.clone(), event, now()).await {
                    if error != "NEW_RECOVERY_EVIDENCE_REQUIRED" {
                        return Err(error);
                    }
                    outcomes.push(RecoveryOutcome::Unknown {
                        execution_id: id,
                        failure: None,
                    });
                    continue;
                }
            } else if row.status != "reconciling" {
                self.store
                    .provider_event(id.clone(), Transition::Reconcile, now())
                    .await?;
            }
            let row = self
                .store
                .execution(id.clone())
                .await?
                .ok_or("EXECUTION_NOT_FOUND")?;
            let mut result = None;
            let mut diagnostic = Some("Exact persisted Thread/Turn unavailable".to_string());
            // R2 is read-only and cannot be created until original Job recovery succeeded.
            if row.thread_id.is_some() && row.turn_id.is_some() {
                let recovery_id = Self::id("recovery-runtime");
                let scope =
                    RecoveryScope::after_termination_for_execution(&self.store, &id, &recovery_id)
                        .await;
                match scope {
                    Err(error) => diagnostic = Some(error.to_string()),
                    Ok(scope) => {
                        if let Some(error) = &self.backend_error {
                            outcomes.push(RecoveryOutcome::RuntimeFailure {
                                execution_id: id,
                                failure: runtime::RuntimeFailure {
                                    code: if error.starts_with("CODEX_APP_SERVER_INCOMPATIBLE") {
                                        "CODEX_APP_SERVER_INCOMPATIBLE"
                                    } else {
                                        "BACKEND_UNAVAILABLE"
                                    },
                                    message: error.clone(),
                                    runtime: None,
                                },
                            });
                            continue;
                        }
                        let managed = match managed::connect(
                            self.store.clone(),
                            self.owner.clone(),
                            recovery_id,
                            self.executable.clone(),
                            PathBuf::from(&row.canonical_workspace_root),
                        )
                        .await
                        {
                            Ok(client) => client,
                            Err(failure) => {
                                outcomes.push(RecoveryOutcome::RuntimeFailure {
                                    execution_id: id,
                                    failure,
                                });
                                continue;
                            }
                        };
                        match managed.client.recover_result(scope).await {
                            Ok(recovered) => {
                                result = Some(recovered);
                                diagnostic = None;
                            }
                            Err(error) => diagnostic = Some(error.to_string()),
                        }
                        if let Err(failure) = managed.shutdown().await {
                            outcomes.push(RecoveryOutcome::RuntimeFailure {
                                execution_id: id,
                                failure,
                            });
                            continue;
                        }
                    }
                }
            }
            let execution = WorkspaceExecutionCoordinator {
                store: self.store.clone(),
            }
            .finish_runtime_terminated(&id, row.revision, result)
            .await?;
            outcomes.push(RecoveryOutcome::Interrupted {
                execution: Box::new(execution),
                result_diagnostic: diagnostic,
            });
        }
        Ok(outcomes)
    }
}

#[cfg(test)]
mod tests;
