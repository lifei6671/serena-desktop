//! 明确 Provider failed 终态后的只读恢复判定；不创建或派发 Execution。
use crate::agent::store::{ExecutionRecord, WorkExecutionLinkRecord, WorkRunRecord};
use serde::{Deserialize, Serialize};

const MAX_AUTOMATIC_RECOVERY_ATTEMPTS: u8 = 2;
const RECOVERY_KIND: &str = "auto_recovery";

/// 自动恢复控制面可观察的纯持久化判定。
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum AutoRecoveryDecision {
    Eligible {
        work_run_id: String,
        root_execution_id: String,
        parent_execution_id: String,
        next_attempt: u8,
    },
    NotEligible(AutoRecoveryIneligibleReason),
}

/// Worker 对一次投递的稳定结果；跳过不是错误，也不会修改父 Execution。
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum AutoRecoverySchedule {
    Scheduled { execution_id: String },
    Skipped(AutoRecoveryIneligibleReason),
}

/// 交给既有 Continue 管线的最小恢复输入；不包含 Provider 原始错误或父 prompt。
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct AutoRecoveryPlan {
    pub(crate) request_key: String,
    pub(crate) prompt: String,
    pub(crate) delegation_context_json: String,
}

/// 首版只返回稳定分类，不泄露 Provider 原始诊断。
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum AutoRecoveryIneligibleReason {
    ExecutionMissing,
    StatusNotFailed,
    ProviderTerminalNotFailed,
    DispatchNotDispatched,
    ReleaseEvidenceIncomplete,
    InterruptRequested,
    WorkspaceClaimPresent,
    WorkLinkMissing,
    WorkRunMissing,
    WorkRunNotActive,
    LineageMalformed,
    LineageInconsistent,
    BudgetExhausted,
}

/// 判定当前 marker 时所需的父 Work lineage 快照。
pub(crate) struct AutoRecoveryParent {
    pub(crate) execution_id: String,
    pub(crate) work_run_id: String,
    pub(crate) parent_execution_id: Option<String>,
    pub(crate) delegation_context_json: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AutoRecoveryMarker {
    kind: String,
    root_execution_id: String,
    attempt: u8,
}

/// 由 Eligible 判定和当前 Work 事实生成固定、可幂等的 Continue 输入。
pub(crate) fn build_plan(
    decision: &AutoRecoveryDecision,
    work: &WorkRunRecord,
) -> Option<AutoRecoveryPlan> {
    let AutoRecoveryDecision::Eligible {
        root_execution_id,
        parent_execution_id,
        next_attempt,
        ..
    } = decision
    else {
        return None;
    };
    let marker = AutoRecoveryMarker {
        kind: RECOVERY_KIND.into(),
        root_execution_id: root_execution_id.clone(),
        attempt: *next_attempt,
    };
    Some(AutoRecoveryPlan {
        request_key: format!("auto-recovery:{root_execution_id}:{next_attempt}"),
        prompt: format!(
            "Auto recovery attempt: {next_attempt}.\nWork title: {}\nWork goal: {}\nRoot Execution: {root_execution_id}\nParent Execution: {parent_execution_id}\nFailure category: provider_terminal_failed\nCheck the current Workspace's existing changes, repair the failure, and run necessary validation.",
            work.title,
            work.goal.as_deref().unwrap_or("")
        ),
        delegation_context_json: serde_json::to_string(&marker)
            .expect("fixed auto-recovery marker must serialize"),
    })
}

/// 只根据已持久化的 Execution、Claim 和 Work lineage 计算下一次恢复资格。
pub(crate) fn evaluate(
    execution: &ExecutionRecord,
    link: &WorkExecutionLinkRecord,
    work: &WorkRunRecord,
    claim_absent: bool,
    parent: Option<&AutoRecoveryParent>,
) -> AutoRecoveryDecision {
    if execution.status != "failed" {
        return AutoRecoveryDecision::NotEligible(AutoRecoveryIneligibleReason::StatusNotFailed);
    }
    if execution.provider_terminal_status.as_deref() != Some("failed") {
        return AutoRecoveryDecision::NotEligible(
            AutoRecoveryIneligibleReason::ProviderTerminalNotFailed,
        );
    }
    if execution.dispatch_state != "dispatched" {
        return AutoRecoveryDecision::NotEligible(
            AutoRecoveryIneligibleReason::DispatchNotDispatched,
        );
    }
    if execution.release_evidence_state != "complete" {
        return AutoRecoveryDecision::NotEligible(
            AutoRecoveryIneligibleReason::ReleaseEvidenceIncomplete,
        );
    }
    if execution.interrupt_requested_at.is_some() {
        return AutoRecoveryDecision::NotEligible(AutoRecoveryIneligibleReason::InterruptRequested);
    }
    if !claim_absent {
        return AutoRecoveryDecision::NotEligible(
            AutoRecoveryIneligibleReason::WorkspaceClaimPresent,
        );
    }
    if work.status != "active" {
        return AutoRecoveryDecision::NotEligible(AutoRecoveryIneligibleReason::WorkRunNotActive);
    }
    let (root_execution_id, previous_attempt) = match recovery_lineage(execution, link, parent) {
        Ok(lineage) => lineage,
        Err(reason) => return AutoRecoveryDecision::NotEligible(reason),
    };
    let Some(next_attempt) = previous_attempt.checked_add(1) else {
        return AutoRecoveryDecision::NotEligible(AutoRecoveryIneligibleReason::BudgetExhausted);
    };
    if next_attempt > MAX_AUTOMATIC_RECOVERY_ATTEMPTS {
        return AutoRecoveryDecision::NotEligible(AutoRecoveryIneligibleReason::BudgetExhausted);
    }
    AutoRecoveryDecision::Eligible {
        work_run_id: link.work_run_id.clone(),
        root_execution_id,
        parent_execution_id: execution.id.clone(),
        next_attempt,
    }
}

/// 普通 Continue 没有此 marker，因而不会被计入自动恢复预算。
fn recovery_lineage(
    execution: &ExecutionRecord,
    link: &WorkExecutionLinkRecord,
    parent: Option<&AutoRecoveryParent>,
) -> Result<(String, u8), AutoRecoveryIneligibleReason> {
    let Some(context) = link.delegation_context_json.as_deref() else {
        return Ok((execution.id.clone(), 0));
    };
    let value: serde_json::Value = serde_json::from_str(context)
        .map_err(|_| AutoRecoveryIneligibleReason::LineageMalformed)?;
    if value.get("kind").and_then(serde_json::Value::as_str) != Some(RECOVERY_KIND) {
        return Ok((execution.id.clone(), 0));
    }
    let marker: AutoRecoveryMarker = serde_json::from_value(value)
        .map_err(|_| AutoRecoveryIneligibleReason::LineageMalformed)?;
    if marker.kind != RECOVERY_KIND
        || marker.root_execution_id.trim().is_empty()
        || !(1..=MAX_AUTOMATIC_RECOVERY_ATTEMPTS).contains(&marker.attempt)
    {
        return Err(AutoRecoveryIneligibleReason::LineageMalformed);
    }
    let Some(parent) = parent else {
        return Err(AutoRecoveryIneligibleReason::LineageInconsistent);
    };
    if link.parent_execution_id.as_deref() != execution.parent_execution_id.as_deref()
        || link.parent_execution_id.as_deref() != Some(parent.execution_id.as_str())
        || parent.work_run_id != link.work_run_id
    {
        return Err(AutoRecoveryIneligibleReason::LineageInconsistent);
    }
    match marker.attempt {
        1 if parent.execution_id == marker.root_execution_id => {}
        2 => {
            let Some(parent_context) = parent.delegation_context_json.as_deref() else {
                return Err(AutoRecoveryIneligibleReason::LineageInconsistent);
            };
            let parent_marker: AutoRecoveryMarker = serde_json::from_str(parent_context)
                .map_err(|_| AutoRecoveryIneligibleReason::LineageInconsistent)?;
            if parent_marker.kind != RECOVERY_KIND
                || parent_marker.attempt != 1
                || parent_marker.root_execution_id != marker.root_execution_id
                || parent.parent_execution_id.as_deref() != Some(marker.root_execution_id.as_str())
            {
                return Err(AutoRecoveryIneligibleReason::LineageInconsistent);
            }
        }
        _ => return Err(AutoRecoveryIneligibleReason::LineageInconsistent),
    }
    Ok((marker.root_execution_id, marker.attempt))
}
