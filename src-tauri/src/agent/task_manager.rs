//! Internal execution and exact-id cancellation entry points. No scheduler.
use super::{
    codex::provider::CodexProvider,
    coordinator::WorkspaceExecutionCoordinator,
    execution::{CreateExecutionInput, ExecutionMode, canonicalize_request},
    store::{StateStore, transactions::CreateOutcome},
};
use std::path::PathBuf;

pub mod recovery;

pub struct AgentTaskManager {
    store: StateStore,
    executable: PathBuf,
    owner: String,
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
            owner: Self::id("host"),
            #[cfg(test)]
            test_client: None,
        }
    }
    fn id(prefix: &str) -> String {
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
    async fn dispatch_pending_execution(
        &self,
        execution_id: &str,
    ) -> Result<super::store::ExecutionRecord, super::codex::provider::ExecutionFailure> {
        let provider = CodexProvider {
            store: self.store.clone(),
            executable: self.executable.clone(),
            owner: self.owner.clone(),
        };
        let id = execution_id.to_owned();
        #[cfg(test)]
        let test_client = self.test_client.clone();
        // The owned worker retains the permit even if its caller stops waiting.
        // Provider/ManagedClient continue to own Runtime and Job convergence.
        tokio::spawn(async move {
            let _permit = provider.store.guard_pending_dispatch(id.clone()).await?;
            #[cfg(test)]
            if let Some((client, database)) = test_client {
                // Test-only Runtime creation boundary; reuse the TASK-006 Fake wire pipeline.
                rusqlite::Connection::open(database).unwrap().execute(
                    "INSERT INTO runtime_instances(id,owner_host_instance_id,state,created_at,updated_at) VALUES (?1,'fixture','running',1,1)",
                    [client.runtime_id()],
                ).unwrap();
                client.initialize().await.map_err(|e| super::codex::provider::ExecutionFailure::State(e.to_string()))?;
                return provider.run_client(&id, &client).await.map_err(super::codex::provider::ExecutionFailure::State);
            }
            provider.execute(&id).await
        }).await.map_err(|e| super::codex::provider::ExecutionFailure::State(e.to_string()))?
    }
}
