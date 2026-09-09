//! Minimal internal entry point. No scheduler, cancellation or recovery manager.
use super::{
    codex::provider::CodexProvider,
    coordinator::WorkspaceExecutionCoordinator,
    execution::{CreateExecutionInput, ExecutionMode, canonicalize_request},
    store::{StateStore, transactions::CreateOutcome},
};
use std::path::PathBuf;

pub struct AgentTaskManager {
    store: StateStore,
    executable: PathBuf,
    owner: String,
}
impl AgentTaskManager {
    pub fn new(store: StateStore, executable: PathBuf) -> Self {
        Self {
            store,
            executable,
            owner: Self::id("host"),
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
        outcome.execution = CodexProvider {
            store: self.store.clone(),
            executable: self.executable.clone(),
            owner: self.owner.clone(),
        }
        .execute(&outcome.execution_id)
        .await?;
        Ok(outcome)
    }
}
