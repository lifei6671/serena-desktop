//! Internal execution and exact-id cancellation entry points. No scheduler.
use super::{
    codex::provider::CodexProvider,
    coordinator::WorkspaceExecutionCoordinator,
    execution::{CreateExecutionInput, ExecutionMode, canonicalize_request},
    store::{StateStore, transactions::CreateOutcome},
};
use std::path::PathBuf;

pub mod recovery;

#[derive(Clone)]
pub struct AgentTaskManager {
    store: StateStore,
    executable: PathBuf,
    pub(crate) backend_error: Option<String>,
    owner: String,
    pub(crate) runtime_pool: std::sync::Arc<super::codex::pool::CodexRuntimePool>,
    #[cfg(test)]
    pub(crate) test_handoff: Option<std::sync::Arc<(tokio::sync::Notify, tokio::sync::Notify)>>,
    #[cfg(test)]
    pub(crate) test_client: Option<(std::sync::Arc<super::codex::app_server::Client>, PathBuf)>,
}
impl AgentTaskManager {
    pub async fn cancel(
        &self,
        execution_id: &str,
    ) -> Result<super::store::ExecutionRecord, String> {
        self.store
            .request_cancel(execution_id.into(), super::coordinator::now())
            .await
    }
    pub fn new(store: StateStore, executable: PathBuf) -> Self {
        Self {
            store,
            executable,
            backend_error: None,
            owner: Self::id("host"),
            runtime_pool: Default::default(),
            #[cfg(test)]
            test_handoff: None,
            #[cfg(test)]
            test_client: None,
        }
    }
    pub(crate) fn id(prefix: &str) -> String {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        format!(
            "{prefix}-{}-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_micros(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        )
    }
    pub(crate) async fn create(
        &self,
        input: CreateExecutionInput,
    ) -> Result<CreateOutcome, String> {
        // One fresh read-only slice uses the already accepted client policy.
        if input.mode != ExecutionMode::ReadOnly
            || input.thread_id.is_some()
            || input.execution_profile != serde_json::json!({})
        {
            return Err("TASK006_REQUIRES_FRESH_READ_ONLY_DEFAULT_PROFILE".into());
        }
        WorkspaceExecutionCoordinator {
            store: self.store.clone(),
        }
        .create(Self::id("execution"), canonicalize_request(input)?)
        .await
    }
    pub async fn execute(
        &self,
        input: CreateExecutionInput,
    ) -> Result<CreateOutcome, super::codex::provider::ExecutionFailure> {
        let mut outcome = self.create(input).await?;
        if !outcome.created {
            return Ok(outcome);
        }
        outcome.execution = self
            .dispatch_pending_execution(&outcome.execution_id)
            .await?;
        Ok(outcome)
    }
    pub async fn resume_pending_execution(
        &self,
        execution_id: &str,
    ) -> Result<super::store::ExecutionRecord, super::codex::provider::ExecutionFailure> {
        self.dispatch_pending_execution(execution_id).await
    }
    pub(crate) async fn product_submit(
        &self,
        action: super::product::Action,
        workspace: Option<super::store::transactions::product::WorkspaceSnapshot>,
    ) -> Result<String, super::product::ProductError> {
        self.product_submit_with_work(action, workspace, None).await
    }

    pub(crate) async fn product_submit_with_work(
        &self,
        action: super::product::Action,
        workspace: Option<super::store::transactions::product::WorkspaceSnapshot>,
        work: Option<super::store::transactions::product::WorkExecutionContext>,
    ) -> Result<String, super::product::ProductError> {
        let manager = self.clone();
        // Host owns creation through handoff even if the adapter drops its wait.
        tokio::spawn(async move {
            use super::product::Action;
            if manager.runtime_pool.stop.is_cancelled() {
                return Err("AGENT_SHUTTING_DOWN".to_string().into());
            }
            let continuation = matches!(&action, Action::Continue { .. });
            let outcome = match action {
                Action::Start {
                    workspace_id,
                    agent_id,
                    request_key,
                    prompt,
                } => Some(
                    manager
                        .store
                        .product_create_fresh_with_work(
                            Self::id("execution"),
                            agent_id,
                            request_key,
                            prompt,
                            workspace_id,
                            workspace,
                            work,
                            super::coordinator::now(),
                        )
                        .await?,
                ),
                Action::Continue {
                    execution_id,
                    request_key,
                    prompt,
                } => Some(
                    manager
                        .store
                        .product_create_continuation_with_work(
                            Self::id("execution"),
                            execution_id,
                            request_key,
                            prompt,
                            work,
                            super::coordinator::now(),
                        )
                        .await?,
                ),
                Action::ResumePending { execution_id } => {
                    if let Some(error) = &manager.backend_error {
                        return Err(super::product::ProductError::new(
                            error.clone(),
                            Some(execution_id),
                        ));
                    }
                    let (tx, rx) = tokio::sync::oneshot::channel();
                    let id = execution_id.clone();
                    tokio::spawn(async move {
                        manager.dispatch_with_receipt(&id, Some(tx), false).await
                    });
                    rx.await.map_err(|e| e.to_string())?.map_err(|e| {
                        super::product::ProductError::new(
                            if e == "WORKSPACE_CLAIM_INCONSISTENT" {
                                "AGENT_RESUME_NOT_ALLOWED".into()
                            } else {
                                e
                            },
                            Some(execution_id.clone()),
                        )
                    })?;
                    return Ok(execution_id);
                }
                _ => return Err("AGENT_INVALID_ARGUMENT".to_string().into()),
            }
            .unwrap();
            if outcome.created {
                #[cfg(test)]
                if let Some(hook) = &manager.test_handoff {
                    hook.0.notify_one();
                    hook.1.notified().await;
                }
                let (tx, rx) = tokio::sync::oneshot::channel();
                let id = outcome.execution_id.clone();
                tokio::spawn(async move {
                    manager
                        .dispatch_with_receipt(&id, Some(tx), continuation)
                        .await
                });
                rx.await
                    .map_err(|e| {
                        super::product::ProductError::accepted(
                            e.to_string(),
                            outcome.execution_id.clone(),
                        )
                    })?
                    .map_err(|e| {
                        super::product::ProductError::accepted(e, outcome.execution_id.clone())
                    })?;
            }
            Ok(outcome.execution_id)
        })
        .await
        .map_err(|e| e.to_string())?
    }
    async fn dispatch_pending_execution(
        &self,
        execution_id: &str,
    ) -> Result<super::store::ExecutionRecord, super::codex::provider::ExecutionFailure> {
        self.dispatch_with_receipt(execution_id, None, false).await
    }
    async fn dispatch_with_receipt(
        &self,
        execution_id: &str,
        mut receipt: Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
        continuation: bool,
    ) -> Result<super::store::ExecutionRecord, super::codex::provider::ExecutionFailure> {
        let mut provider = CodexProvider {
            store: self.store.clone(),
            executable: self.executable.clone(),
            owner: self.owner.clone(),
            runtime_pool: self.runtime_pool.clone(),
        };
        let backend_error = self.backend_error.clone();
        let id = execution_id.to_owned();
        #[cfg(test)]
        let test_client = self.test_client.clone();
        // The owned worker retains the permit even if its caller stops waiting.
        // Provider/ManagedClient continue to own Runtime and Job convergence.
        tokio::spawn(async move {
            let admission = async {
                let row = provider.store.execution(id.clone()).await?.ok_or("EXECUTION_NOT_FOUND")?;
                provider.runtime_pool.check_workspace(&row.canonical_workspace_root)?;
                provider.store.guard_pending_dispatch(id.clone()).await
            }.await;
            let permit = admission;
            let _permit = match permit {
                Ok(p)=>p,
                Err(e)=>{if let Some(receipt)=receipt {let _=receipt.send(Err(e.clone()));}return Err(e.into());}
            };
            if let Some(error) = backend_error {
                provider.failed(&id).await?;
                if let Some(receipt) = receipt { let _ = receipt.send(Err(error.clone())); }
                return Err(error.into());
            }
            if provider.executable.as_os_str().is_empty() {
                match super::codex::discovery::discover().await {
                    Ok(path)=>provider.executable=path,
                    Err(e)=>{
                        let diagnostic=match provider.failed(&id).await{Ok(())=>e,Err(state)=>format!("{e}; reconciliation persistence: {state}")};
                        if let Some(receipt)=receipt {let _=receipt.send(Err(diagnostic.clone()));}
                        return Err(diagnostic.into());
                    }
                }
            }
            if !continuation && let Some(receipt)=receipt.take() {let _=receipt.send(Ok(()));}
            let result = async {
            #[cfg(test)]
            if let Some((client, database)) = test_client {
                // Test-only Runtime creation boundary; reuse the TASK-006 Fake wire pipeline.
                rusqlite::Connection::open(database).unwrap().execute(
                    "INSERT INTO runtime_instances(id,owner_host_instance_id,state,created_at,updated_at) VALUES (?1,'fixture','running',1,1)",
                    [client.runtime_id()],
                ).unwrap();
                client.initialize().await.map_err(|e| super::codex::provider::ExecutionFailure::State(e.to_string()))?;
                return provider.run_client_with_acceptance(&id, &client, &mut receipt).await.map_err(super::codex::provider::ExecutionFailure::State);
            }
            provider.execute_with_acceptance(&id, &mut receipt).await
            }.await;
            if let Some(receipt) = receipt {
                let error = match &result {
                    Err(super::codex::provider::ExecutionFailure::State(e)) => e.clone(),
                    Err(super::codex::provider::ExecutionFailure::Runtime(e)) => format!("{}: {}", e.code, e.message),
                    Ok(_) => "AGENT_CONTINUE_NOT_ALLOWED: execution ended before acceptance".into(),
                };
                let _ = receipt.send(Err(error));
            }
            result
        }).await.map_err(|e| super::codex::provider::ExecutionFailure::State(e.to_string()))?
    }
}
