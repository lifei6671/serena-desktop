//! Host-owned workspace-exclusive Runtime ownership and fail-closed quarantine.
use super::{
    app_server::managed::ManagedClient,
    runtime::{self, RuntimeFailure},
};
use crate::agent::store::{RuntimeRecord, StateStore};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard, OwnedRwLockReadGuard, RwLock};
use tokio_util::sync::CancellationToken;

struct Entry {
    store: StateStore,
    client: Arc<AsyncMutex<Option<ManagedClient>>>,
}
struct RetainedRuntimeFailure {
    store: StateStore,
    runtime_id: String,
    failure: RuntimeFailure,
}
#[derive(Default)]
pub(crate) struct CodexRuntimePool {
    #[cfg(test)]
    pub test_connect: Mutex<Option<TestConnect>>,
    entries: Mutex<HashMap<String, Entry>>,
    workers: Arc<RwLock<()>>,
    pub stop: CancellationToken,
    // Key is canonical workspace; empty key only for unassignable legacy orphans.
    // An empty Vec still means quarantined while recovery owns the failures.
    failures: Mutex<HashMap<String, Vec<RetainedRuntimeFailure>>>,
    shutdown: AsyncMutex<()>,
}
#[cfg(test)]
pub(crate) type TestConnect = Arc<
    dyn Fn(
            String,
            std::path::PathBuf,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<ManagedClient, RuntimeFailure>> + Send>,
        > + Send
        + Sync,
>;

fn terminated(row: &RuntimeRecord) -> bool {
    row.state == "terminated"
        && row.termination_evidence_state == "complete"
        && matches!(
            row.termination_evidence_type.as_deref(),
            Some("job_active_processes_zero" | "managed_job_destroyed")
        )
        && row.termination_evidence_at.is_some()
}
impl CodexRuntimePool {
    pub async fn enter(&self) -> Result<OwnedRwLockReadGuard<()>, String> {
        let guard = self.workers.clone().read_owned().await;
        if self.stop.is_cancelled() {
            return Err("AGENT_SHUTTING_DOWN".into());
        }
        Ok(guard)
    }
    pub fn check_workspace(&self, workspace: &str) -> Result<(), String> {
        let failures = self.failures.lock().unwrap();
        if failures.contains_key(workspace) || failures.contains_key("") {
            Err("AGENT_RUNTIME_QUARANTINED".into())
        } else {
            Ok(())
        }
    }
    pub fn retains_runtime(&self, workspace: &str, runtime_id: &str) -> bool {
        self.failures
            .lock()
            .unwrap()
            .get(workspace)
            .is_some_and(|items| items.iter().any(|f| f.runtime_id == runtime_id))
    }
    async fn lock_entry(
        &self,
        store: &StateStore,
        workspace: &str,
    ) -> OwnedMutexGuard<Option<ManagedClient>> {
        let client = self
            .entries
            .lock()
            .unwrap()
            .entry(workspace.into())
            .or_insert_with(|| Entry {
                store: store.clone(),
                client: Arc::new(AsyncMutex::new(None)),
            })
            .client
            .clone();
        client.lock_owned().await
    }
    pub async fn lease(
        &self,
        store: &StateStore,
        workspace: &str,
    ) -> Result<OwnedMutexGuard<Option<ManagedClient>>, String> {
        let lease = self.lock_entry(store, workspace).await;
        self.check_workspace(workspace)?;
        Ok(lease)
    }
    pub fn retain_failure(
        &self,
        store: &StateStore,
        workspace: &str,
        runtime_id: &str,
        failure: RuntimeFailure,
    ) -> RuntimeFailure {
        let diagnostic = RuntimeFailure {
            code: failure.code,
            message: failure.message.clone(),
            runtime: None,
        };
        let mut all = self.failures.lock().unwrap();
        let retained = all.entry(workspace.into()).or_default();
        if let Some(prior) = retained.iter_mut().find(|f| f.runtime_id == runtime_id) {
            if failure.runtime.is_some() {
                assert!(
                    prior.failure.runtime.is_none(),
                    "Runtime already has a retained owner"
                );
                prior.failure.runtime = failure.runtime;
            }
            prior.failure.code = failure.code;
            prior.failure.message = failure.message;
        } else {
            retained.push(RetainedRuntimeFailure {
                store: store.clone(),
                runtime_id: runtime_id.into(),
                failure,
            });
        }
        diagnostic
    }
    pub async fn retain_attempt_failure(
        &self,
        store: &StateStore,
        workspace: &str,
        runtime_id: &str,
        failure: RuntimeFailure,
    ) -> RuntimeFailure {
        if failure.runtime.is_none() {
            match store.runtime(runtime_id.into()).await {
                Ok(None) => return failure, // No Runtime creation row: no app-server could exist.
                Ok(Some(row)) if terminated(&row) => return failure,
                _ => {}
            }
        }
        self.retain_failure(store, workspace, runtime_id, failure)
    }
    /// Explicit convergence only. Does not dispatch or replay any Execution.
    pub async fn retry_workspace(&self, store: &StateStore, workspace: &str) -> Result<(), String> {
        let _worker = self.enter().await?;
        let _lease = self.lock_entry(store, workspace).await;
        self.reconcile_quarantine(workspace).await
    }
    async fn reconcile_quarantine(&self, workspace: &str) -> Result<(), String> {
        let ids: Vec<String> = self
            .failures
            .lock()
            .unwrap()
            .get(workspace)
            .map(|items| items.iter().map(|f| f.runtime_id.clone()).collect())
            .unwrap_or_default();
        for runtime_id in ids {
            // Keep recovery identity in quarantine across every await (including
            // cancellation/panic); only move the optional physical owner out.
            let (store, owner) = {
                let mut all = self.failures.lock().unwrap();
                let retained = all
                    .get_mut(workspace)
                    .unwrap()
                    .iter_mut()
                    .find(|f| f.runtime_id == runtime_id)
                    .unwrap();
                (retained.store.clone(), retained.failure.runtime.take())
            };
            let result = if let Some(owner) = owner {
                owner.terminate(super::protocol::INIT_TIMEOUT).await
            } else {
                match store.runtime(runtime_id.clone()).await {
                    Ok(Some(row)) if terminated(&row) => Ok(()),
                    _ => {
                        runtime::recover(
                            store.clone(),
                            runtime_id.clone(),
                            super::protocol::INIT_TIMEOUT,
                        )
                        .await
                    }
                }
            };
            let failure = match result {
                Err(failure) => Some(failure),
                Ok(()) => {
                    if matches!(store.runtime(runtime_id.clone()).await, Ok(Some(row)) if terminated(&row))
                    {
                        None
                    } else {
                        Some(RuntimeFailure {
                            code: "CODEX_RUNTIME_EVIDENCE_REQUIRED",
                            message: "Authoritative termination evidence required".into(),
                            runtime: None,
                        })
                    }
                }
            };
            let mut all = self.failures.lock().unwrap();
            let retained = all.get_mut(workspace).unwrap();
            if let Some(failure) = failure {
                retained
                    .iter_mut()
                    .find(|f| f.runtime_id == runtime_id)
                    .unwrap()
                    .failure = failure;
            } else {
                retained.retain(|f| f.runtime_id != runtime_id);
            }
        }
        let mut all = self.failures.lock().unwrap();
        if all.get(workspace).is_some_and(Vec::is_empty) {
            all.remove(workspace);
        }
        if all.contains_key(workspace) {
            Err("AGENT_RUNTIME_QUARANTINED".into())
        } else {
            Ok(())
        }
    }
    pub async fn shutdown(&self) -> Result<(), String> {
        self.stop.cancel();
        let _shutdown = self.shutdown.lock().await;
        let _workers = self.workers.write().await;
        let entries: Vec<_> = self
            .entries
            .lock()
            .unwrap()
            .iter()
            .map(|(w, e)| (w.clone(), e.store.clone(), e.client.clone()))
            .collect();
        for (workspace, store, client) in entries {
            if let Some(managed) = client.lock().await.take() {
                let runtime_id = managed.client.runtime_id().to_owned();
                self.retain_failure(
                    &store,
                    &workspace,
                    &runtime_id,
                    RuntimeFailure {
                        code: "CODEX_RUNTIME_SHUTDOWN_PENDING",
                        message: "Awaiting owned monitor termination".into(),
                        runtime: None,
                    },
                );
                if let Err(failure) = managed.shutdown().await {
                    let mut all = self.failures.lock().unwrap();
                    all.get_mut(&workspace)
                        .unwrap()
                        .iter_mut()
                        .find(|f| f.runtime_id == runtime_id)
                        .unwrap()
                        .failure = failure;
                }
            }
        }
        self.entries.lock().unwrap().clear();
        let workspaces: Vec<_> = self.failures.lock().unwrap().keys().cloned().collect();
        for workspace in workspaces {
            let _ = self.reconcile_quarantine(&workspace).await;
        }
        let failures = self.failures.lock().unwrap();
        if failures.is_empty() {
            Ok(())
        } else {
            Err(failures
                .iter()
                .flat_map(|(w, failures)| {
                    failures.iter().map(move |f| {
                        format!(
                            "{}: {} [{} / {}]",
                            f.failure.code, f.failure.message, w, f.runtime_id
                        )
                    })
                })
                .collect::<Vec<_>>()
                .join("; "))
        }
    }
    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.entries.lock().unwrap().is_empty()
    }
}
