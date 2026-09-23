//! Windows 私有转发 adapter；既有 Runtime/Job Object 契约保持不变。

use crate::agent::store::RuntimeRecord;
use crate::agent::{
    provider::port::{ProviderReconcileItem, ProviderReconcileKind, ProviderReconcileSummary},
    task_manager::recovery::{RecoveryOutcome, StartupRecoveryFailure},
};

#[cfg(test)]
pub(crate) use super::runtime::RuntimeError;
pub(crate) use super::runtime::{RuntimeFailure, recover};

/// 精确复用 Windows 既有两类完整 Job termination evidence。
pub(crate) fn is_complete_termination(record: &RuntimeRecord) -> bool {
    record.state == "terminated"
        && record.termination_evidence_state == "complete"
        && matches!(
            record.termination_evidence_type.as_deref(),
            Some("job_active_processes_zero" | "managed_job_destroyed")
        )
        && record.termination_evidence_at.is_some()
}

/// Windows 继续调用既有 startup recovery，并只在 adapter 边界投影 Provider summary。
pub(crate) async fn recover_startup(
    store: &crate::agent::store::StateStore,
    executable: &std::path::Path,
    owner: &str,
    runtime_pool: &std::sync::Arc<crate::agent::codex::pool::CodexRuntimePool>,
    backend_error: Option<&str>,
) -> Result<ProviderReconcileSummary, StartupRecoveryFailure> {
    let outcomes = crate::agent::task_manager::recovery::recover_startup_with_authority(
        store,
        executable,
        owner,
        runtime_pool,
        backend_error,
    )
    .await?;
    Ok(ProviderReconcileSummary {
        items: outcomes.iter().map(provider_reconcile_item).collect(),
    })
}

/// 保持既有 Windows RecoveryOutcome 到 Provider kind 的精确映射。
fn provider_reconcile_item(outcome: &RecoveryOutcome) -> ProviderReconcileItem {
    let (subject_id, kind) = match outcome {
        RecoveryOutcome::OrphanRuntime {
            runtime_id,
            failure: None,
        } => (
            runtime_id.clone(),
            ProviderReconcileKind::OrphanResourceRecovered,
        ),
        RecoveryOutcome::OrphanRuntime {
            runtime_id,
            failure: Some(_),
        } => (
            runtime_id.clone(),
            ProviderReconcileKind::OrphanResourceUnknown,
        ),
        RecoveryOutcome::Released { execution_id } => (
            execution_id.clone(),
            ProviderReconcileKind::ExecutionReleased,
        ),
        RecoveryOutcome::Inconsistent { execution_id, .. } => (
            execution_id.clone(),
            ProviderReconcileKind::ExecutionInconsistent,
        ),
        RecoveryOutcome::PendingExplicitResume { execution_id } => (
            execution_id.clone(),
            ProviderReconcileKind::ExecutionPendingExplicitResume,
        ),
        RecoveryOutcome::Unknown { execution_id, .. } => (
            execution_id.clone(),
            ProviderReconcileKind::ExecutionUnknown,
        ),
        RecoveryOutcome::RuntimeFailure { execution_id, .. } => (
            execution_id.clone(),
            ProviderReconcileKind::ExecutionProviderFailure,
        ),
        RecoveryOutcome::Interrupted { execution, .. } => (
            execution.id.clone(),
            ProviderReconcileKind::ExecutionInterrupted,
        ),
    };
    ProviderReconcileItem { subject_id, kind }
}
