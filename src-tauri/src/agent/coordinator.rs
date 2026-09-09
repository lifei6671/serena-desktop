//! Internal finalization coordination; all Claim writes remain in StateStore.
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
    pub(crate) async fn finish_runtime_terminated(
        &self,
        id: &str,
        revision: i64,
        result: Option<RecoveredResult>,
    ) -> Result<ExecutionRecord, String> {
        let row = self
            .store
            .execution(id.into())
            .await?
            .ok_or("EXECUTION_NOT_FOUND")?;
        if row.revision != revision {
            return Err("EXECUTION_REVISION_CONFLICT".into());
        }
        let value = result
            .map(serde_json::to_value)
            .transpose()
            .map_err(|e| e.to_string())?;
        if let Some(value) = &value
            && (value["executionId"] != id
                || value["executionRevision"] != row.revision
                || value["sourceRuntimeId"].as_str() != row.runtime_instance_id.as_deref()
                || value["recoveredByRuntimeId"] == value["sourceRuntimeId"]
                || value["threadId"].as_str() != row.thread_id.as_deref()
                || value["turnId"].as_str() != row.turn_id.as_deref()
                || value["resultCompleteness"] != "complete"
                || row
                    .provider_terminal_status
                    .as_deref()
                    .is_some_and(|status| value["terminalTurn"]["status"] != status))
        {
            return Err("EXECUTION_RESULT_IDENTITY_MISMATCH".into());
        }
        let completeness = if value.is_some() {
            ResultCompleteness::Complete
        } else {
            ResultCompleteness::Unknown
        };
        self.store
            .finalize_and_release_execution(
                id.into(),
                revision,
                Finalization {
                    terminal: Status::Interrupted,
                    basis: ReleaseBasis::RuntimeTerminated,
                    result: value,
                    completeness,
                },
                now(),
            )
            .await?;
        self.store
            .execution(id.into())
            .await?
            .ok_or_else(|| "EXECUTION_NOT_FOUND".into())
    }
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
        let terminal = match row.provider_terminal_status.as_deref() {
            Some("completed") => Status::Completed,
            Some("failed") => Status::Failed,
            Some("interrupted") if row.interrupt_requested_at.is_some() => Status::Cancelled,
            Some("interrupted") => Status::Interrupted,
            _ => return Err("EXECUTION_RESULT_IDENTITY_MISMATCH".into()),
        };
        if value["executionId"] != id
            || value["executionRevision"] != row.revision
            || value["threadId"].as_str() != row.thread_id.as_deref()
            || value["turnId"].as_str() != row.turn_id.as_deref()
            || value["sourceRuntimeId"] != runtime
            || value["recoveredByRuntimeId"] != runtime
            || value["terminalTurn"]["status"].as_str() != row.provider_terminal_status.as_deref()
            || value["resultCompleteness"] != "complete"
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
                    terminal,
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
