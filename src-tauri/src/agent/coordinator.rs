//! Internal success-path coordination; all Claim writes remain in StateStore.
use super::{
    codex::app_server::{EmptyEvidence, recovery::RecoveredResult},
    execution::{CanonicalRequest, state::*},
    store::{ExecutionRecord, StateStore, transactions::CreateOutcome},
};

pub(crate) fn now() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

pub(crate) struct WorkspaceExecutionCoordinator {
    pub store: StateStore,
}
impl WorkspaceExecutionCoordinator {
    pub async fn create(
        &self,
        id: String,
        request: CanonicalRequest,
    ) -> Result<CreateOutcome, String> {
        self.store.create_execution(id, request, now()).await
    }
    pub async fn finish(
        &self,
        id: &str,
        result: RecoveredResult,
        empty: EmptyEvidence,
    ) -> Result<ExecutionRecord, String> {
        let row = self
            .store
            .execution(id.into())
            .await?
            .ok_or("EXECUTION_NOT_FOUND")?;
        let value = serde_json::to_value(result).map_err(|e| e.to_string())?;
        let scope = empty.scope();
        let runtime = row
            .runtime_instance_id
            .as_deref()
            .ok_or("RUNTIME_REQUIRED")?;
        if value["executionId"] != id
            || value["executionRevision"] != row.revision
            || value["threadId"].as_str() != row.thread_id.as_deref()
            || value["turnId"].as_str() != row.turn_id.as_deref()
            || value["sourceRuntimeId"] != runtime
            || value["recoveredByRuntimeId"] != runtime
            || value["terminalTurn"]["status"] != "completed"
            || value["resultCompleteness"] != "complete"
            || row.provider_terminal_status.as_deref() != Some("completed")
            || row
                .provider_terminal_evidence_runtime_instance_id
                .as_deref()
                != Some(runtime)
            || scope.execution_id() != id
            || scope.revision() != Some(row.revision)
            || scope.runtime_id() != runtime
            || Some(scope.thread_id()) != row.thread_id.as_deref()
        {
            return Err("EXECUTION_RESULT_IDENTITY_MISMATCH".into());
        }
        self.store
            .transition_execution(
                id.into(),
                row.revision,
                Transition::CleanupEmpty {
                    runtime_id: runtime.into(),
                },
                now(),
            )
            .await?;
        self.store
            .finalize_and_release_execution(
                id.into(),
                row.revision + 1,
                Finalization {
                    terminal: Status::Completed,
                    basis: ReleaseBasis::SameRuntimeCleanup,
                    result: Some(value),
                    completeness: ResultCompleteness::Complete,
                },
                now(),
            )
            .await?;
        self.store
            .execution(id.into())
            .await?
            .ok_or_else(|| "EXECUTION_NOT_FOUND".into())
    }
}
