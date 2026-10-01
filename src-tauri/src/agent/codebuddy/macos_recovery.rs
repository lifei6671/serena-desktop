//! CodeBuddy macOS 原 Runtime 身份验证；共享 Darwin 终止算法但不借用 Codex 持久化证据。
use super::recovery::TerminationEvidence;
use crate::agent::{
    codex::{macos_recovery, macos_runtime::MacosTerminationEvidence},
    coordinator::now,
    store::RuntimeRecord,
};
use std::time::Duration;

/// 原 Runtime 必须为 CodeBuddy macOS，且绝不能混入 Windows Job 字段。
pub(crate) fn valid_identity(record: &RuntimeRecord) -> Result<(), String> {
    if record.provider != "codebuddy"
        || record.id.is_empty()
        || record.id.contains(['\\', '/', '\0'])
        || record.owner_host_instance_id.is_empty()
        || record.job_name.is_some()
        || record.job_session_id.is_some()
        || record.job_creation_mode.is_some()
        || record.job_handle_inheritable.is_some()
        || record.job_kill_on_close.is_some()
        || record.job_breakaway_allowed.is_some()
        || record.job_policy_verified_at.is_some()
        || record
            .codex_pid
            .is_none_or(|pid| pid == 0 || pid > i32::MAX as u32)
    {
        return Err("CODEBUDDY_RUNTIME_IDENTITY_INVALID".into());
    }
    macos_recovery::persisted_identity(record)
        .map(|_| ())
        .map_err(|_| "CODEBUDDY_RUNTIME_IDENTITY_INVALID".into())
}

/// 只接受本 Provider 原身份所对应的两种 group-empty evidence。
pub(crate) fn complete(record: &RuntimeRecord) -> bool {
    valid_identity(record).is_ok()
        && record.state == "terminated"
        && record.termination_evidence_state == "complete"
        && record.termination_evidence_at.is_some()
        && matches!(
            record.termination_evidence_type.as_deref(),
            Some("macos_live_process_group_empty" | "macos_recovered_process_group_empty")
        )
}

/// live Runtime 的封闭 group-empty 证据必须与原持久化身份逐字段匹配。
pub(crate) fn live_evidence(
    original: RuntimeRecord,
    evidence: MacosTerminationEvidence,
) -> Result<TerminationEvidence, String> {
    valid_identity(&original)?;
    if !evidence.matches_runtime(&original) {
        return Err("CODEBUDDY_RUNTIME_EVIDENCE_CONFLICT".into());
    }
    Ok(TerminationEvidence {
        original,
        kind: "macos_live_process_group_empty",
        at: now(),
    })
}

/// 跨 Host 只在原 leader PID/PGID/SID/start-token 仍匹配时允许发信号。
pub(super) fn observe(
    original: RuntimeRecord,
    timeout: Duration,
) -> Result<TerminationEvidence, String> {
    valid_identity(&original)?;
    let identity = macos_recovery::persisted_identity(&original).map_err(str::to_owned)?;
    macos_recovery::terminate_observed_group(&identity, timeout, timeout)?;
    Ok(TerminationEvidence {
        original,
        kind: "macos_recovered_process_group_empty",
        at: now(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{
        codebuddy::recovery,
        store::{StateStore, codebuddy_runtime::CodeBuddyRuntimeUpdate},
    };

    /// 使用隔离数据库保存测试身份；不调用外部 CodeBuddy 或任意真实进程。
    async fn prepared() -> (tempfile::TempDir, StateStore) {
        let directory = tempfile::tempdir().unwrap();
        let store = StateStore::open(directory.path().to_owned()).await.unwrap();
        store
            .prepare_codebuddy_macos_runtime(
                "r1".into(),
                "host".into(),
                "/fixture/codebuddy".into(),
                1,
            )
            .await
            .unwrap();
        store
            .update_codebuddy_runtime(
                "r1".into(),
                CodeBuddyRuntimeUpdate::MacosProcessStarted {
                    pid: 91,
                    start_token: "darwin_proc_bsd_start_v1:2:3".into(),
                },
                2,
            )
            .await
            .unwrap();
        (directory, store)
    }

    /// macOS 保存原生 containment，不借用 Windows policy 字段，complete 后才获授权。
    #[tokio::test]
    async fn codebuddy_macos_durable_identity_and_evidence_are_platform_specific() {
        let (_directory, store) = prepared().await;
        store
            .update_codebuddy_runtime("r1".into(), CodeBuddyRuntimeUpdate::Initialized, 3)
            .await
            .unwrap();
        let original = store.runtime("r1".into()).await.unwrap().unwrap();
        valid_identity(&original).unwrap();
        assert_eq!(original.provider, "codebuddy");
        assert_eq!(original.runtime_platform, "macos");
        assert_eq!(original.state, "running");
        assert_eq!(original.containment_process_group_id, Some(91));
        assert_eq!(original.containment_session_id, Some(91));
        assert!(original.job_name.is_none());
        assert!(!recovery::approved_runtime(&store, "r1").await.unwrap());
        // 测试只注入 sealed observation，专门验证 durable 消费边界。
        store
            .complete_codebuddy_runtime(TerminationEvidence {
                original,
                kind: "macos_recovered_process_group_empty",
                at: 4,
            })
            .await
            .unwrap();
        assert!(recovery::approved_runtime(&store, "r1").await.unwrap());
    }

    /// 旧快照不能完成身份被替换后的 Runtime，其他 Provider 同样不能借用原证据。
    #[tokio::test]
    async fn codebuddy_macos_evidence_rejects_changed_original_identity() {
        let (_directory, store) = prepared().await;
        let original = store.runtime("r1".into()).await.unwrap().unwrap();
        store.write_blocking(|tx| { tx.execute("UPDATE runtime_instances SET process_start_token='darwin_proc_bsd_start_v1:9:9' WHERE id='r1'", []).map_err(|e| e.to_string())?; Ok(()) }).unwrap();
        assert_eq!(
            store
                .complete_codebuddy_runtime(TerminationEvidence {
                    original,
                    kind: "macos_live_process_group_empty",
                    at: 4
                })
                .await
                .unwrap_err(),
            "CODEBUDDY_RUNTIME_EVIDENCE_CONFLICT"
        );
        let mut invalid = store.runtime("r1".into()).await.unwrap().unwrap();
        invalid.provider = "codex".into();
        assert!(valid_identity(&invalid).is_err());
        invalid.provider = "codebuddy".into();
        invalid.job_name = Some("injected".into());
        assert!(valid_identity(&invalid).is_err());
        invalid.job_name = None;
        invalid.containment_session_id = Some(92);
        assert!(valid_identity(&invalid).is_err());
    }

    /// 缺少可靠身份时启动恢复不能发信号或伪造 group-empty。
    #[tokio::test]
    async fn codebuddy_macos_recovery_preserves_unknown_without_identity() {
        let directory = tempfile::tempdir().unwrap();
        let store = StateStore::open(directory.path().to_owned()).await.unwrap();
        store
            .prepare_codebuddy_macos_runtime(
                "r1".into(),
                "old-host".into(),
                "/fixture/codebuddy".into(),
                1,
            )
            .await
            .unwrap();
        assert!(
            recovery::recover(&store, "r1".into(), Duration::ZERO)
                .await
                .is_err()
        );
        let record = store.runtime("r1".into()).await.unwrap().unwrap();
        assert_eq!(record.state, "unknown");
        assert_ne!(record.termination_evidence_state, "complete");
        assert!(!recovery::approved_runtime(&store, "r1").await.unwrap());
    }
    /// 真实 Darwin leader 经跨 Host startup 收敛，只把原 Runtime 写为 recovered group-empty。
    #[tokio::test]
    async fn codebuddy_macos_startup_recovers_original_native_process_group() {
        use crate::agent::codex::macos_launcher::{MacosLaunchRequest, launch};
        let directory = tempfile::tempdir().unwrap();
        let store = StateStore::open(directory.path().join("state"))
            .await
            .unwrap();
        let launched = launch(&MacosLaunchRequest {
            executable: "/bin/sleep".into(),
            args: vec!["30".into()],
            current_dir: directory.path().to_owned(),
            runtime_instance_id: "old-runtime".into(),
        })
        .unwrap();
        store
            .prepare_codebuddy_macos_runtime(
                "old-runtime".into(),
                "old-host".into(),
                "/bin/sleep".into(),
                1,
            )
            .await
            .unwrap();
        store
            .update_codebuddy_runtime(
                "old-runtime".into(),
                CodeBuddyRuntimeUpdate::MacosProcessStarted {
                    pid: launched.identity.pid as u32,
                    start_token: launched.identity.start_token.encode(),
                },
                2,
            )
            .await
            .unwrap();
        let mut child = launched.child;
        let waiter = std::thread::spawn(move || child.process.wait().unwrap());
        let summary = recovery::startup(&store, "new-host").await.unwrap();
        waiter.join().unwrap();
        assert_eq!(summary.items.len(), 1);
        assert!(matches!(
            summary.items[0].kind,
            crate::agent::provider::port::ProviderReconcileKind::OrphanResourceRecovered
        ));
        let original = store.runtime("old-runtime".into()).await.unwrap().unwrap();
        assert_eq!(original.owner_host_instance_id, "old-host");
        assert_eq!(
            original.termination_evidence_type.as_deref(),
            Some("macos_recovered_process_group_empty")
        );
        assert!(
            recovery::approved_runtime(&store, "old-runtime")
                .await
                .unwrap()
        );
        recovery::recover(&store, "old-runtime".into(), Duration::ZERO)
            .await
            .unwrap();
    }
}
