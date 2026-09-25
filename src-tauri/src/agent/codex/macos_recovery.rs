//! macOS 跨 Host Runtime 与 Claim 恢复；不复用 live direct-child evidence。

use super::{
    macos_launcher::{
        MacosProcessIdentityAdapter, ProcessIdentity, ProcessStartToken, process_group_members,
    },
    macos_runtime_store::{self, MacosEvidenceKind},
};
use crate::agent::store::{RuntimeRecord, StateStore};
use crate::agent::{
    coordinator::WorkspaceExecutionCoordinator,
    execution::state::{RecoveryBasis, Transition},
    provider::port::{ProviderReconcileItem, ProviderReconcileKind, ProviderReconcileSummary},
    store::transactions::ClaimRecovery,
};
use std::{
    io, thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const MAX_PHASE_TIMEOUT: Duration = Duration::from_secs(30);
const POLL_INTERVAL: Duration = Duration::from_millis(10);

#[cfg(test)]
static GROUP_OBSERVATION_FAILURES: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashSet<libc::pid_t>>,
> = std::sync::OnceLock::new();

/// 单条跨 Host Runtime recovery 的可持久化结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RecoveryStatus {
    Recovered,
    Unknown,
}

/// 从 Store 投影恢复并验证完整的 macOS leader 身份。
fn persisted_identity(record: &RuntimeRecord) -> Result<ProcessIdentity, &'static str> {
    if record.runtime_platform != "macos"
        || record.containment_type != "macos_process_group"
        || record.process_identity_scheme != "darwin_proc_bsd_start_v1"
        || record.containment_verified_at.is_none()
    {
        return Err("CODEX_PROCESS_IDENTITY_FAILED");
    }
    let pid = record
        .codex_pid
        .and_then(|value| libc::pid_t::try_from(value).ok())
        .ok_or("CODEX_PROCESS_IDENTITY_FAILED")?;
    let pgid = record
        .containment_process_group_id
        .and_then(|value| libc::pid_t::try_from(value).ok())
        .ok_or("CODEX_PROCESS_IDENTITY_FAILED")?;
    let sid = record
        .containment_session_id
        .and_then(|value| libc::pid_t::try_from(value).ok())
        .ok_or("CODEX_PROCESS_IDENTITY_FAILED")?;
    if pid != pgid || pid != sid {
        return Err("CODEX_PROCESS_IDENTITY_FAILED");
    }
    let start_token = ProcessStartToken::decode(
        record
            .codex_process_start_token
            .as_deref()
            .ok_or("CODEX_PROCESS_IDENTITY_FAILED")?,
    )?;
    Ok(ProcessIdentity {
        pid,
        pgid,
        sid,
        start_token,
    })
}

/// 向已通过身份与 containment 验证的 Process Group 发送信号。
fn signal_group(pgid: libc::pid_t, signal: libc::c_int) -> io::Result<()> {
    // SAFETY: 调用点只传入已与持久化 leader 身份精确匹配的正 PGID。
    if unsafe { libc::killpg(pgid, signal) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// 查询 Process Group；测试可只对指定 PGID 注入观测失败而不影响并行 fixture。
fn observe_group(pgid: libc::pid_t) -> io::Result<Vec<libc::pid_t>> {
    #[cfg(test)]
    if GROUP_OBSERVATION_FAILURES
        .get_or_init(Default::default)
        .lock()
        .expect("group observation fault mutex poisoned")
        .contains(&pgid)
    {
        return Err(io::Error::from_raw_os_error(libc::EIO));
    }
    process_group_members(pgid)
}

/// 仅供当前 PGID 的 recovery 测试注入或清除 group 查询失败。
#[cfg(test)]
fn set_group_observation_failure(pgid: libc::pid_t, enabled: bool) {
    let mut failures = GROUP_OBSERVATION_FAILURES
        .get_or_init(Default::default)
        .lock()
        .expect("group observation fault mutex poisoned");
    if enabled {
        failures.insert(pgid);
    } else {
        failures.remove(&pgid);
    }
}

/// 在 bounded deadline 内等待下一轮 group 观测。
fn sleep_until_next_poll(deadline: Instant) {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if !remaining.is_zero() {
        thread::sleep(remaining.min(POLL_INTERVAL));
    }
}

/// 将证据不足稳定持久化为 unknown；Store 失败向上传播以阻止后续 Claim release。
fn persist_unknown(
    store: &StateStore,
    id: &str,
    message: impl Into<String>,
) -> Result<RecoveryStatus, String> {
    let message = message.into();
    macos_runtime_store::unknown(
        store,
        id,
        "CODEX_RUNTIME_TERMINATION_UNCONFIRMED",
        &message,
        now(),
    )
    .map_err(|error| error.message)?;
    Ok(RecoveryStatus::Unknown)
}

/// 重新验证 leader 身份；不存在、不匹配和查询失败都不能降级为匹配。
fn identity_matches(expected: &ProcessIdentity) -> Result<bool, io::Error> {
    MacosProcessIdentityAdapter::observe(expected.pid).map(|observed| expected.matches(&observed))
}

/// 恢复并有界终止一条跨 Host macOS Runtime，仅 group-empty 可形成 complete evidence。
pub(crate) async fn recover_runtime(
    store: &StateStore,
    runtime_id: &str,
    grace: Duration,
    kill_wait: Duration,
) -> Result<RecoveryStatus, String> {
    let Some(record) = store.runtime(runtime_id.to_owned()).await? else {
        return Ok(RecoveryStatus::Unknown);
    };
    let expected = match persisted_identity(&record) {
        Ok(identity) => identity,
        Err(_) => {
            return persist_unknown(
                store,
                runtime_id,
                "持久化 macOS 身份或 containment 证据不完整",
            );
        }
    };

    // 恢复前 leader 必须仍以同一 PID/PGID/SID/token 存活，且明确属于原组。
    match identity_matches(&expected) {
        Ok(true) => {}
        Ok(false) => return persist_unknown(store, runtime_id, "leader 身份与持久化证据不匹配"),
        Err(error) => {
            return persist_unknown(store, runtime_id, format!("leader 身份不可观测: {error}"));
        }
    }
    let members = match observe_group(expected.pgid) {
        Ok(members) => members,
        Err(error) => {
            return persist_unknown(
                store,
                runtime_id,
                format!("Process Group 查询失败: {error}"),
            );
        }
    };
    if !members.contains(&expected.pid) {
        return persist_unknown(store, runtime_id, "leader 不属于持久化 Process Group");
    }

    if let Err(error) = macos_runtime_store::terminating(store, runtime_id, now()) {
        return persist_unknown(
            store,
            runtime_id,
            format!("Runtime terminating 写入失败: {}", error.message),
        );
    }
    if let Err(error) = signal_group(expected.pgid, libc::SIGTERM) {
        return persist_unknown(store, runtime_id, format!("发送 SIGTERM 失败: {error}"));
    }

    let grace_deadline = Instant::now() + grace.min(MAX_PHASE_TIMEOUT);
    loop {
        let members = match observe_group(expected.pgid) {
            Ok(members) => members,
            Err(error) => {
                return persist_unknown(
                    store,
                    runtime_id,
                    format!("SIGTERM 后 group 查询失败: {error}"),
                );
            }
        };
        if members.is_empty() {
            return complete_recovered(store, runtime_id);
        }
        // 跨 Host 恢复没有连续 child ownership；leader 消失后不得向残留 group 升级 KILL。
        match identity_matches(&expected) {
            Ok(true) => {}
            Ok(false) => {
                return persist_unknown(
                    store,
                    runtime_id,
                    "SIGTERM 后 leader 身份发生变化且 group 非空",
                );
            }
            Err(error) => {
                return persist_unknown(
                    store,
                    runtime_id,
                    format!("SIGTERM 后 leader 消失且 group 非空: {error}"),
                );
            }
        }
        if Instant::now() >= grace_deadline {
            break;
        }
        sleep_until_next_poll(grace_deadline);
    }

    // grace 到期时再次精确匹配原 leader，匹配失败绝不发送 SIGKILL。
    match identity_matches(&expected) {
        Ok(true) => {}
        Ok(false) => return persist_unknown(store, runtime_id, "SIGKILL 前 leader 身份不匹配"),
        Err(error) => {
            return persist_unknown(
                store,
                runtime_id,
                format!("SIGKILL 前 leader 不可观测: {error}"),
            );
        }
    }
    if let Err(error) = signal_group(expected.pgid, libc::SIGKILL) {
        return persist_unknown(store, runtime_id, format!("发送 SIGKILL 失败: {error}"));
    }

    let kill_deadline = Instant::now() + kill_wait.min(MAX_PHASE_TIMEOUT);
    loop {
        match observe_group(expected.pgid) {
            Ok(members) if members.is_empty() => return complete_recovered(store, runtime_id),
            Ok(_) => {}
            Err(error) => {
                return persist_unknown(
                    store,
                    runtime_id,
                    format!("SIGKILL 后 group 查询失败: {error}"),
                );
            }
        }
        if Instant::now() >= kill_deadline {
            return persist_unknown(store, runtime_id, "SIGKILL 等待结束后 Process Group 仍非空");
        }
        sleep_until_next_poll(kill_deadline);
    }
}

/// 原子提交 recovered group-empty evidence，提交失败时绝不报告 recovered。
fn complete_recovered(store: &StateStore, runtime_id: &str) -> Result<RecoveryStatus, String> {
    if let Err(error) = macos_runtime_store::complete(
        store,
        runtime_id,
        MacosEvidenceKind::RecoveredGroupEmpty,
        now(),
    ) {
        return persist_unknown(
            store,
            runtime_id,
            format!("recovered evidence 提交失败: {}", error.message),
        );
    }
    Ok(RecoveryStatus::Recovered)
}

/// 返回当前 Unix epoch 毫秒。
fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

/// 验证一条 Runtime 已具备 macOS 平台对应的完整 group-empty release evidence。
fn complete_macos_evidence(record: &RuntimeRecord) -> bool {
    record.state == "terminated"
        && record.runtime_platform == "macos"
        && record.containment_type == "macos_process_group"
        && record.process_identity_scheme == "darwin_proc_bsd_start_v1"
        && record.codex_pid.is_some()
        && record
            .codex_process_start_token
            .as_deref()
            .is_some_and(|token| ProcessStartToken::decode(token).is_ok())
        && record.containment_process_group_id == record.codex_pid.map(i64::from)
        && record.containment_session_id == record.codex_pid.map(i64::from)
        && record.containment_verified_at.is_some()
        && record.termination_evidence_state == "complete"
        && record.termination_evidence_at.is_some()
        && matches!(
            record.termination_evidence_type.as_deref(),
            Some("macos_live_process_group_empty" | "macos_recovered_process_group_empty")
        )
}

/// 将 Execution 收口为 unknown，但不删除其 Claim。
async fn mark_execution_unknown(store: &StateStore, execution_id: &str) -> Result<(), String> {
    let row = store
        .execution(execution_id.to_owned())
        .await?
        .ok_or("EXECUTION_NOT_FOUND")?;
    if matches!(
        row.status.as_str(),
        "unknown" | "completed" | "failed" | "cancelled" | "interrupted"
    ) {
        return Ok(());
    }
    if row.status != "reconciling" {
        store
            .provider_event(execution_id.to_owned(), Transition::Reconcile, now())
            .await?;
    }
    store
        .provider_event(execution_id.to_owned(), Transition::MarkUnknown, now())
        .await
}

/// 使用既有 ResumeRecovery 与 RuntimeTerminated authority 原子完成 interrupted + Claim release。
async fn release_recovered_execution(
    store: &StateStore,
    execution_id: &str,
    runtime: &RuntimeRecord,
) -> Result<(), String> {
    let execution = store
        .execution(execution_id.to_owned())
        .await?
        .ok_or("EXECUTION_NOT_FOUND")?;
    if execution.provider != runtime.provider {
        mark_execution_unknown(store, execution_id).await?;
        return Err("RUNTIME_PROVIDER_MISMATCH".into());
    }
    if !complete_macos_evidence(runtime) {
        mark_execution_unknown(store, execution_id).await?;
        return Err("RUNTIME_TERMINATION_EVIDENCE_REQUIRED".into());
    }
    let mut row = store
        .execution(execution_id.to_owned())
        .await?
        .ok_or("EXECUTION_NOT_FOUND")?;
    if row.status == "unknown" {
        store
            .provider_event(
                execution_id.to_owned(),
                Transition::ResumeRecovery(RecoveryBasis::RuntimeTermination {
                    runtime_id: runtime.id.clone(),
                    evidence_at: runtime
                        .termination_evidence_at
                        .ok_or("RUNTIME_EVIDENCE_REQUIRED")?,
                }),
                now(),
            )
            .await?;
        row = store
            .execution(execution_id.to_owned())
            .await?
            .ok_or("EXECUTION_NOT_FOUND")?;
    } else if row.status != "reconciling" {
        store
            .provider_event(execution_id.to_owned(), Transition::Reconcile, now())
            .await?;
        row = store
            .execution(execution_id.to_owned())
            .await?
            .ok_or("EXECUTION_NOT_FOUND")?;
    }
    WorkspaceExecutionCoordinator {
        store: store.clone(),
    }
    .finish_runtime_terminated(execution_id, row.revision, None)
    .await?;
    Ok(())
}

/// 在 Provider 发布前恢复 orphan Runtime 与所有 durable Claim。
pub(crate) async fn recover_startup(
    store: &StateStore,
    owner: &str,
) -> Result<ProviderReconcileSummary, String> {
    recover_startup_with_timeouts(
        store,
        owner,
        Duration::from_secs(10),
        Duration::from_secs(10),
    )
    .await
}

/// 使用明确的有界等待恢复 startup 状态；测试以较短 deadline 覆盖相同状态机。
async fn recover_startup_with_timeouts(
    store: &StateStore,
    owner: &str,
    grace: Duration,
    kill_wait: Duration,
) -> Result<ProviderReconcileSummary, String> {
    let claims = store.recover_claims(now()).await?;
    let mut items = Vec::new();

    // orphan Runtime 没有关联 Claim，只收口 containment，不改变无关 Execution。
    for runtime_id in store.orphan_runtimes(owner.to_owned()).await? {
        let Some(runtime) = store.runtime(runtime_id.clone()).await? else {
            items.push(ProviderReconcileItem {
                subject_id: runtime_id,
                kind: ProviderReconcileKind::OrphanResourceUnknown,
            });
            continue;
        };
        // macOS provider 不重写历史 Windows orphan；其 Named Job 证据只能由 Windows recovery 解释。
        if runtime.runtime_platform != "macos" {
            items.push(ProviderReconcileItem {
                subject_id: runtime_id,
                kind: ProviderReconcileKind::OrphanResourceUnknown,
            });
            continue;
        }
        let status = recover_runtime(store, &runtime_id, grace, kill_wait).await?;
        items.push(ProviderReconcileItem {
            subject_id: runtime_id,
            kind: match status {
                RecoveryStatus::Recovered => ProviderReconcileKind::OrphanResourceRecovered,
                RecoveryStatus::Unknown => ProviderReconcileKind::OrphanResourceUnknown,
            },
        });
    }

    for claim in claims {
        let execution_id = match claim {
            ClaimRecovery::Released { execution_id } => {
                items.push(ProviderReconcileItem {
                    subject_id: execution_id,
                    kind: ProviderReconcileKind::ExecutionReleased,
                });
                continue;
            }
            ClaimRecovery::Inconsistent { execution_id, .. } => {
                items.push(ProviderReconcileItem {
                    subject_id: execution_id,
                    kind: ProviderReconcileKind::ExecutionInconsistent,
                });
                continue;
            }
            ClaimRecovery::PendingExplicitResume { execution_id } => {
                items.push(ProviderReconcileItem {
                    subject_id: execution_id,
                    kind: ProviderReconcileKind::ExecutionPendingExplicitResume,
                });
                continue;
            }
            ClaimRecovery::Pending { execution_id } | ClaimRecovery::Unknown { execution_id } => {
                execution_id
            }
        };
        let execution = store
            .execution(execution_id.clone())
            .await?
            .ok_or("EXECUTION_NOT_FOUND")?;
        let Some(runtime_id) = execution.runtime_instance_id else {
            mark_execution_unknown(store, &execution_id).await?;
            items.push(ProviderReconcileItem {
                subject_id: execution_id,
                kind: ProviderReconcileKind::ExecutionUnknown,
            });
            continue;
        };
        let Some(mut runtime) = store.runtime(runtime_id.clone()).await? else {
            mark_execution_unknown(store, &execution_id).await?;
            items.push(ProviderReconcileItem {
                subject_id: execution_id,
                kind: ProviderReconcileKind::ExecutionUnknown,
            });
            continue;
        };
        if !complete_macos_evidence(&runtime) {
            if runtime.runtime_platform != "macos"
                || recover_runtime(store, &runtime_id, grace, kill_wait).await?
                    != RecoveryStatus::Recovered
            {
                mark_execution_unknown(store, &execution_id).await?;
                items.push(ProviderReconcileItem {
                    subject_id: execution_id,
                    kind: ProviderReconcileKind::ExecutionUnknown,
                });
                continue;
            }
            runtime = store
                .runtime(runtime_id)
                .await?
                .ok_or("RUNTIME_NOT_FOUND")?;
        }
        if release_recovered_execution(store, &execution_id, &runtime)
            .await
            .is_err()
        {
            // ResumeRecovery 已提交的 evidence 必须保留；Claim release 失败时留在 reconciling 供下次幂等重试。
            items.push(ProviderReconcileItem {
                subject_id: execution_id,
                kind: ProviderReconcileKind::ExecutionProviderFailure,
            });
            continue;
        }
        items.push(ProviderReconcileItem {
            subject_id: execution_id,
            kind: ProviderReconcileKind::ExecutionInterrupted,
        });
    }
    Ok(ProviderReconcileSummary { items })
}

#[cfg(test)]
#[path = "macos_recovery/tests.rs"]
mod tests;
