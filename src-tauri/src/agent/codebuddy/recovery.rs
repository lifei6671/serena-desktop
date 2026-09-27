//! CodeBuddy 原 Runtime 的 Job evidence；不读取 PID、Session history 或 Codex evidence。
use crate::agent::{
    coordinator::now,
    execution::state::{
        Finalization, RecoveryBasis, ReleaseBasis, ResultCompleteness, Status, Transition,
    },
    provider::port::{
        ProviderReconcileItem, ProviderReconcileKind as Kind, ProviderReconcileSummary,
    },
    store::{
        RuntimeRecord, StateStore, codebuddy_runtime::CodeBuddyRuntimeUpdate,
        transactions::ClaimRecovery,
    },
};
use std::time::Duration;

/// 字段不公开，生产代码只能通过本模块的受验证 Job observation 获得此证据。
pub(crate) struct TerminationEvidence {
    original: RuntimeRecord,
    kind: &'static str,
    at: i64,
}
impl TerminationEvidence {
    /// Store 只读原始 ownership snapshot，不能创建或修改 sealed evidence。
    pub(crate) fn original(&self) -> &RuntimeRecord {
        &self.original
    }
    /// 持久化规范化的 Job evidence kind。
    pub(crate) fn kind(&self) -> &'static str {
        self.kind
    }
    /// 返回实际观察完成的时间。
    pub(crate) fn at(&self) -> i64 {
        self.at
    }
}

/// PID 与 private R2 不参与验证；原 Job 名、Session 和 first-runnable policy 必须全匹配。
pub(crate) fn valid_identity(r: &RuntimeRecord, session: u32) -> Result<(), String> {
    if r.provider != "codebuddy"
        || r.id.is_empty()
        || r.id.contains(['\\', '/', '\0'])
        || r.owner_host_instance_id.is_empty()
        || r.job_name.as_deref()
            != Some(format!("Local\\SerenaDesktop.CodeBuddy.{}", r.id).as_str())
        || r.job_session_id != Some(i64::from(session))
        || r.job_creation_mode.as_deref() != Some("proc_thread_attribute_job_list")
        || r.job_handle_inheritable != Some(false)
        || r.job_kill_on_close != Some(true)
        || r.job_breakaway_allowed != Some(false)
        || r.job_policy_verified_at.is_none()
        || r.runtime_platform != "windows"
        || r.containment_type != "windows_job"
        || r.process_identity_scheme != "windows_filetime_v1"
        || r.containment_process_group_id.is_some()
        || r.containment_session_id.is_some()
        || r.containment_verified_at.is_some()
    {
        return Err("CODEBUDDY_RUNTIME_IDENTITY_INVALID".into());
    }
    Ok(())
}

/// 仅消费 CodeBuddy 已提交的两种 complete evidence；另一 Provider 不能幂等成功。
fn complete(r: &RuntimeRecord) -> bool {
    r.provider == "codebuddy"
        && r.state == "terminated"
        && r.termination_evidence_state == "complete"
        && r.termination_evidence_at.is_some()
        && matches!(
            r.termination_evidence_type.as_deref(),
            Some("job_active_processes_zero" | "managed_job_destroyed")
        )
}

/// 旧 Job 收敛和 durable evidence 提交完成后才返回成功；失败尝试记录 unknown。
pub(crate) async fn recover(
    store: &StateStore,
    id: String,
    timeout: Duration,
) -> Result<(), String> {
    #[cfg(test)]
    let observer = TEST_OBSERVER
        .try_with(|observer| *observer)
        .unwrap_or(observe);
    #[cfg(not(test))]
    let observer = observe;
    recover_using(store, id, timeout, observer).await
}

/// 注入观察边界仅用于确定性故障测试，生产调用固定 Win32 observer。
async fn recover_using(
    store: &StateStore,
    id: String,
    timeout: Duration,
    observer: fn(RuntimeRecord, Duration) -> Result<TerminationEvidence, String>,
) -> Result<(), String> {
    let r = store
        .runtime(id.clone())
        .await?
        .ok_or("CODEBUDDY_RUNTIME_MISSING")?;
    if r.provider != "codebuddy" {
        return Err("CODEBUDDY_RUNTIME_PROVIDER_MISMATCH".into());
    }
    if !store.codebuddy_runtime_binding_valid(id.clone()).await? {
        return Err("CODEBUDDY_RUNTIME_BINDING_CONFLICT".into());
    }
    if complete(&r) {
        return Ok(());
    }
    let result = async {
        let proof = tokio::task::spawn_blocking(move || observer(r, timeout))
            .await
            .map_err(|e| e.to_string())??;
        store.complete_codebuddy_runtime(proof).await
    }
    .await;
    if result.is_err() {
        store
            .update_codebuddy_runtime(id, CodeBuddyRuntimeUpdate::Unknown, now())
            .await?;
    }
    result
}

/// unsupported host 不生成证据或降级为 PID 检查。
#[cfg(not(windows))]
fn observe(_r: RuntimeRecord, _timeout: Duration) -> Result<TerminationEvidence, String> {
    Err("CODEBUDDY_WINDOWS_JOB_REQUIRED".into())
}

#[cfg(windows)]
#[path = "recovery_windows.rs"]
mod windows;
#[cfg(windows)]
use windows::observe;

/// prepare 使用与 recovery 相同的 Local Job namespace authority。
#[cfg(windows)]
pub(crate) fn current_session() -> Result<u32, String> {
    windows::session()
}

/// 每个 execution 不确定映射为 generic unknown；全局 Store 失败才传播到 Provider health。
pub(crate) async fn startup(
    store: &StateStore,
    owner: &str,
) -> Result<ProviderReconcileSummary, String> {
    let claims = store
        .recover_provider_claims("codebuddy".into(), now())
        .await?;
    let mut items = Vec::new();
    for claim in claims {
        let (id, early) = match claim {
            ClaimRecovery::Released { execution_id } => {
                (execution_id, Some(Kind::ExecutionReleased))
            }
            ClaimRecovery::Inconsistent { execution_id, .. } => {
                (execution_id, Some(Kind::ExecutionInconsistent))
            }
            ClaimRecovery::PendingExplicitResume { execution_id } => {
                (execution_id, Some(Kind::ExecutionPendingExplicitResume))
            }
            ClaimRecovery::Pending { execution_id } | ClaimRecovery::Unknown { execution_id } => {
                (execution_id, None)
            }
        };
        let kind = if let Some(kind) = early {
            kind
        } else {
            reconcile_execution(store, &id, true).await?
        };
        items.push(ProviderReconcileItem {
            subject_id: id,
            kind,
        });
    }
    for id in store
        .provider_orphan_runtimes(owner.into(), "codebuddy".into())
        .await?
    {
        let kind = if recover(store, id.clone(), Duration::from_secs(10))
            .await
            .is_ok()
        {
            Kind::OrphanResourceRecovered
        } else {
            Kind::OrphanResourceUnknown
        };
        items.push(ProviderReconcileItem {
            subject_id: id,
            kind,
        });
    }
    Ok(ProviderReconcileSummary { items })
}

/// generic ownership 可用于安全停止 Job；缺失或冲突 private R1 永不授权释放 Claim。
pub(super) async fn reconcile_execution(
    store: &StateStore,
    id: &str,
    recover_runtime: bool,
) -> Result<Kind, String> {
    let row = store
        .execution(id.into())
        .await?
        .ok_or("EXECUTION_NOT_FOUND")?;
    let Some(runtime_id) = row.runtime_instance_id else {
        mark_unknown(store, id).await?;
        return Ok(Kind::ExecutionUnknown);
    };
    let private_exists = store.codebuddy_state_exists(id.into()).await?;
    let private = if private_exists {
        store.read_codebuddy_state(id.into()).await.ok()
    } else {
        None
    };
    let private_valid = private.is_some();
    // 私有 exact terminal 与 generic 暂存结果共同确认；单独 result JSON 永不授权结果恢复。
    let staged = private.as_ref().is_some_and(|p| {
        use agent_client_protocol::schema::v1::StopReason;
        let terminal = match p.terminal_stop_reason {
            Some(StopReason::EndTurn) => "completed",
            Some(StopReason::Refusal) => "failed",
            Some(StopReason::Cancelled) => "cancelled",
            Some(StopReason::MaxTokens | StopReason::MaxTurnRequests) => "interrupted",
            _ => return false,
        };
        p.prompt_state == super::store::PromptState::TerminalObserved
            && p.runtime_instance_id.as_deref() == Some(runtime_id.as_str())
            && p.session_id.is_some()
            && p.terminal_observed_at.is_some()
            && row.provider_terminal_status.as_deref() == Some(terminal)
            && row
                .provider_terminal_evidence_runtime_instance_id
                .as_deref()
                == Some(runtime_id.as_str())
            && row.provider_terminal_evidence_at.is_some()
            && row.final_result_json.is_some()
    });
    // 已有私有身份发生冲突时连 OS mutation 也不执行，避免终止争议绑定。
    if private_exists && !private_valid {
        mark_unknown(store, id).await?;
        return Ok(Kind::ExecutionUnknown);
    }
    // 不推造 Session；generic 原绑定足以停止该 Job，private 缺失仍保留 Claim。
    let recovered = if recover_runtime {
        recover(store, runtime_id.clone(), Duration::from_secs(10)).await
    } else {
        Ok(())
    };
    let durable = store.runtime(runtime_id.clone()).await?;
    #[cfg(windows)]
    let approved = durable.as_ref().is_some_and(|durable| {
        complete(durable)
            && current_session().is_ok_and(|session| valid_identity(durable, session).is_ok())
    });
    #[cfg(not(windows))]
    let approved = false;
    if !private_valid || recovered.is_err() || !approved {
        mark_unknown(store, id).await?;
        return Ok(Kind::ExecutionUnknown);
    }
    let evidence = durable.ok_or("CODEBUDDY_RUNTIME_MISSING")?;
    if row.status == "unknown" {
        if let Err(error) = store
            .provider_event(
                id.into(),
                Transition::ResumeRecovery(RecoveryBasis::RuntimeTermination {
                    runtime_id,
                    evidence_at: evidence
                        .termination_evidence_at
                        .ok_or("RUNTIME_EVIDENCE_REQUIRED")?,
                }),
                now(),
            )
            .await
        {
            if error == "NEW_RECOVERY_EVIDENCE_REQUIRED" {
                return Ok(Kind::ExecutionUnknown);
            }
            return Err(error);
        }
    } else if row.status != "reconciling" && !(staged && row.status == "finalizing") {
        store
            .provider_event(id.into(), Transition::Reconcile, now())
            .await?;
    }
    let mut row = store
        .execution(id.into())
        .await?
        .ok_or("EXECUTION_NOT_FOUND")?;
    if staged && row.status == "reconciling" {
        store
            .provider_event(id.into(), Transition::ResumeStagedTerminal, now())
            .await?;
        row = store
            .execution(id.into())
            .await?
            .ok_or("EXECUTION_NOT_FOUND")?;
    }
    let (terminal, result, completeness) = if staged {
        let terminal = serde_json::from_value(serde_json::Value::String(
            row.provider_terminal_status
                .clone()
                .ok_or("STAGED_TERMINAL_REQUIRED")?,
        ))
        .map_err(|e| e.to_string())?;
        let result = serde_json::from_str(
            row.final_result_json
                .as_deref()
                .ok_or("STAGED_RESULT_REQUIRED")?,
        )
        .map_err(|e| e.to_string())?;
        let completeness = match row.result_completeness.as_str() {
            "complete" => ResultCompleteness::Complete,
            "partial" => ResultCompleteness::Partial,
            "unknown" => ResultCompleteness::Unknown,
            _ => return Err("STAGED_COMPLETENESS_INVALID".into()),
        };
        (terminal, result, completeness)
    } else {
        (Status::Interrupted, None, ResultCompleteness::Unknown)
    };
    // 只使用现有 provider-neutral release 事务，不创建 R2 或恢复 Provider 结果。
    store
        .finalize_and_release_execution(
            id.into(),
            row.revision,
            Finalization {
                terminal,
                basis: ReleaseBasis::RuntimeTerminated,
                result,
                completeness,
            },
            now(),
        )
        .await?;
    Ok(if staged {
        Kind::ExecutionReleased
    } else {
        Kind::ExecutionInterrupted
    })
}

#[cfg(all(test, windows))]
mod tests;

/// 仅投影 generic unknown，不引入 Codex recovery 依赖。
pub(super) async fn mark_unknown(store: &StateStore, id: &str) -> Result<(), String> {
    let row = store
        .execution(id.into())
        .await?
        .ok_or("EXECUTION_NOT_FOUND")?;
    if row.status == "unknown" {
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
tokio::task_local! {
    /// 仅当前测试 future 可注入 Job observer，不影响并发 Provider 或生产路径。
    static TEST_OBSERVER: fn(RuntimeRecord,Duration)->Result<TerminationEvidence,String>;
}
