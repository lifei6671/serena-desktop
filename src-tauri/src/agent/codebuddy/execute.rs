//! Fresh/Continue Execute 的单一 owner：exact terminal 后停止整个 Job，再消费 durable authority。
use super::{discovery::ResolvedLaunchSpec, fresh::DesiredConfiguration, protocol::Limits};
use crate::agent::{
    coordinator::now,
    execution::state::{ResultCompleteness, Status, Transition},
    provider::{
        ProviderOutcome, ProviderResultCompleteness, ProviderRunResult,
        port::{AgentEventSink, ProviderAcceptanceSink, ProviderExecutionFailure},
    },
    store::StateStore,
};
use std::sync::Arc;
use tokio::sync::oneshot;

/// prepare/prompt/finalize 均由本 task 持有；不读取当前 admission policy，也不重放有副作用请求。
pub(super) async fn run(
    store: StateStore,
    owner: String,
    resolved: ResolvedLaunchSpec,
    id: String,
    acceptance: Arc<dyn ProviderAcceptanceSink>,
    telemetry: Arc<dyn AgentEventSink>,
    cancelled: oneshot::Receiver<()>,
) -> Result<ProviderRunResult, ProviderExecutionFailure> {
    // 拒绝无关 Execution 后不运行 cleanup，避免绕过 admission 终止另一条生命周期。
    let row = store
        .execution(id.clone())
        .await?
        .ok_or("EXECUTION_NOT_FOUND".to_owned())?;
    if row.provider != "codebuddy"
        || row.status != "dispatch_pending"
        || row.dispatch_state != "not_dispatched"
        || row.runtime_instance_id.is_some()
    {
        return Err(ProviderExecutionFailure::State(
            "CODEBUDDY_EXECUTION_CONTEXT_INVALID".into(),
        ));
    }
    let attempt_id = format!("codebuddy-{}", super::store::new_conversation_id()?);
    let result = async {
        let session = match row.parent_execution_id.clone() {
            Some(source_execution_id) => {
                super::continued::prepare_owned(
                    store.clone(),
                    owner.clone(),
                    id.clone(),
                    source_execution_id,
                    &resolved,
                    DesiredConfiguration::default(),
                    Limits::default(),
                    attempt_id.clone(),
                )
                .await
            }
            None => {
                super::fresh::prepare_owned(
                    store.clone(),
                    owner.clone(),
                    id.clone(),
                    &resolved,
                    DesiredConfiguration::default(),
                    Limits::default(),
                    attempt_id.clone(),
                )
                .await
            }
        }
        .map_err(|e| e.code().to_owned())?;
        let completion =
            super::prompt::run(session, store.clone(), acceptance, telemetry, cancelled).await;
        let result = completion.result.map_err(|e| e.code().to_owned());
        let staged = if let Ok(result) = &result {
            let terminal = match result.outcome {
                ProviderOutcome::Completed => Status::Completed,
                ProviderOutcome::Failed => Status::Failed,
                ProviderOutcome::Cancelled => Status::Cancelled,
                ProviderOutcome::Interrupted => Status::Interrupted,
            };
            let completeness = match result.result_completeness {
                ProviderResultCompleteness::Complete => ResultCompleteness::Complete,
                ProviderResultCompleteness::Partial => ResultCompleteness::Partial,
                ProviderResultCompleteness::Unknown => ResultCompleteness::Unknown,
            };
            store
                .provider_event(
                    id.clone(),
                    Transition::ProviderTerminalResult {
                        runtime_id: completion
                            .session
                            .private
                            .runtime_instance_id
                            .clone()
                            .ok_or("RUNTIME_REQUIRED")?,
                        status: terminal,
                        result: result.result.clone(),
                        completeness,
                    },
                    now(),
                )
                .await
        } else {
            Ok(())
        };
        // shutdown 返回值不是释放授权；无论结果如何都重新读取 Store evidence。
        let _ = completion.session.shutdown().await;
        staged?;
        result
    }
    .await;
    let row = store
        .execution(id.clone())
        .await?
        .ok_or("EXECUTION_NOT_FOUND".to_owned())?;
    if row.runtime_instance_id.as_deref() == Some(attempt_id.as_str()) {
        if let Err(error) = super::recovery::reconcile_execution_with_launch(
            &store,
            &id,
            false,
            &owner,
            Some(&resolved),
        )
        .await
        {
            super::recovery::mark_unknown(&store, &id).await?;
            return Err(ProviderExecutionFailure::State(error));
        }
        let final_row = store
            .execution(id.clone())
            .await?
            .ok_or("EXECUTION_NOT_FOUND".to_owned())?;
        if final_row.release_evidence_state != "complete" {
            return Err(ProviderExecutionFailure::State(
                "RUNTIME_TERMINATION_EVIDENCE_REQUIRED".into(),
            ));
        }
    }
    if row.runtime_instance_id.is_none()
        && store.runtime_workspace(attempt_id).await?.as_deref()
            == Some(row.canonical_workspace_root.as_str())
    {
        super::recovery::mark_unknown(&store, &id).await?;
    }
    result.map_err(ProviderExecutionFailure::State)
}

#[cfg(test)]
mod tests;
