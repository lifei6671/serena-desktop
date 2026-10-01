//! macOS Runtime 持久化边界；只接受调用方已经观察到的身份和 containment 事实。

use super::macos_launcher::ProcessIdentity;
use crate::agent::store::StateStore;
use rusqlite::{Transaction, params};

/// macOS Runtime Store 的稳定失败，不复用 Windows Runtime 错误类型。
#[derive(Debug, PartialEq, Eq)]
pub(super) struct MacosRuntimeStoreError {
    pub(super) code: &'static str,
    pub(super) message: String,
}

impl MacosRuntimeStoreError {
    /// 构造带稳定错误码的 Store 失败。
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

/// Store 只接受由 live 或跨 Host recovery 已封闭确认的 group-empty 事实。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MacosEvidenceKind {
    LiveGroupEmpty,
    RecoveredGroupEmpty,
}

impl MacosEvidenceKind {
    /// 返回 schema v10 冻结的 evidence type。
    fn as_str(self) -> &'static str {
        match self {
            Self::LiveGroupEmpty => "macos_live_process_group_empty",
            Self::RecoveredGroupEmpty => "macos_recovered_process_group_empty",
        }
    }
}

/// 在单个即时事务中执行恰好影响一行的 Runtime 状态写入。
fn write_one(
    store: &StateStore,
    operation: impl FnOnce(&Transaction<'_>) -> rusqlite::Result<usize>,
) -> Result<(), MacosRuntimeStoreError> {
    store
        .write_blocking(|transaction| {
            let changed = operation(transaction).map_err(|error| error.to_string())?;
            if changed != 1 {
                return Err("CODEX_RUNTIME_STATE_CONFLICT".into());
            }
            Ok(())
        })
        .map_err(|message| {
            let code = if message == "CODEX_RUNTIME_STATE_CONFLICT" {
                "CODEX_RUNTIME_STATE_CONFLICT"
            } else {
                "CODEX_RUNTIME_STORE_FAILED"
            };
            MacosRuntimeStoreError::new(code, message)
        })
}

/// 在 spawn 前登记无身份的 macOS preparing Runtime。
pub(super) fn prepare(
    store: &StateStore,
    id: &str,
    owner: &str,
    executable: &str,
    now: i64,
) -> Result<(), MacosRuntimeStoreError> {
    write_one(store, |transaction| {
        transaction.execute(
            "INSERT INTO runtime_instances(
                id,owner_host_instance_id,executable_path,provider,state,created_at,updated_at,
                runtime_platform,containment_type,process_identity_scheme)
             VALUES(?1,?2,?3,'codex','preparing',?4,?4,'macos','macos_process_group',
                    'darwin_proc_bsd_start_v1')",
            params![id, owner, executable, now],
        )
    })
}

/// 原子写入 launcher 已验证的 PID、PGID、SID 与版本化 start token。
pub(super) fn start(
    store: &StateStore,
    id: &str,
    identity: &ProcessIdentity,
    now: i64,
) -> Result<(), MacosRuntimeStoreError> {
    write_one(store, |transaction| {
        transaction.execute(
            "UPDATE runtime_instances SET state='starting',process_id=?2,
                process_start_token=?3,containment_process_group_id=?4,
                containment_session_id=?5,containment_verified_at=?6,
                started_at=?6,updated_at=?6
             WHERE id=?1 AND state='preparing'",
            params![
                id,
                i64::from(identity.pid),
                identity.start_token.encode(),
                i64::from(identity.pgid),
                i64::from(identity.sid),
                now,
            ],
        )
    })
}

/// 保存协议初始化结果并进入 running。
pub(super) fn initialized(
    store: &StateStore,
    id: &str,
    version: &str,
    schema: &str,
    now: i64,
) -> Result<(), MacosRuntimeStoreError> {
    write_one(store, |transaction| {
        transaction.execute(
            "UPDATE runtime_instances SET state='running',executable_version=?2,
                protocol_contract_sha256=?3,updated_at=?4
             WHERE id=?1 AND state='starting' AND containment_verified_at IS NOT NULL",
            params![id, version, schema, now],
        )
    })
}

/// 标记 Host 已开始执行 Runtime shutdown。
pub(super) fn terminating(
    store: &StateStore,
    id: &str,
    now: i64,
) -> Result<(), MacosRuntimeStoreError> {
    write_one(store, |transaction| {
        transaction.execute(
            "UPDATE runtime_instances SET state='terminating',updated_at=?2
             WHERE id=?1 AND state != 'terminated'",
            params![id, now],
        )
    })
}

/// 将不足以形成完整 evidence 的 Runtime 固定为 unknown。
pub(super) fn unknown(
    store: &StateStore,
    id: &str,
    code: &str,
    message: &str,
    now: i64,
) -> Result<(), MacosRuntimeStoreError> {
    write_one(store, |transaction| {
        transaction.execute(
            "UPDATE runtime_instances SET state='unknown',last_error_code=?2,
                last_error_message=?3,updated_at=?4
             WHERE id=?1 AND termination_evidence_state != 'complete'",
            params![id, code, message, now],
        )
    })
}

/// 原子提交平台对应的 group-empty evidence，并冻结同 Runtime Usage。
pub(super) fn complete(
    store: &StateStore,
    id: &str,
    kind: MacosEvidenceKind,
    at: i64,
) -> Result<(), MacosRuntimeStoreError> {
    store
        .write_blocking(|transaction| {
            let changed = transaction
                .execute(
                    "UPDATE runtime_instances SET state='terminated',stopped_at=?3,
                        termination_evidence_type=?2,termination_evidence_at=?3,
                        termination_evidence_state='complete',last_error_code=NULL,
                        last_error_message=NULL,updated_at=?3
                     WHERE id=?1 AND state != 'terminated'",
                    params![id, kind.as_str(), at],
                )
                .map_err(|error| error.to_string())?;
            if changed != 1 {
                return Err("CODEX_RUNTIME_STATE_CONFLICT".into());
            }
            transaction
                .execute(
                    "UPDATE codex_execution_usage_state
                     SET telemetry_state='frozen',freeze_at=?2
                     WHERE runtime_instance_id=?1 AND telemetry_state != 'frozen'",
                    params![id, at],
                )
                .map_err(|error| error.to_string())?;
            Ok(())
        })
        .map_err(|message| {
            let code = if message == "CODEX_RUNTIME_STATE_CONFLICT" {
                "CODEX_RUNTIME_STATE_CONFLICT"
            } else {
                "CODEX_RUNTIME_STORE_FAILED"
            };
            MacosRuntimeStoreError::new(code, message)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{
        codex::macos_launcher::{ProcessIdentity, ProcessStartToken},
        store::StateStore,
    };

    /// 构造隔离 Store，避免 Runtime 状态测试共享数据库。
    fn store() -> StateStore {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.keep();
        tauri::async_runtime::block_on(StateStore::open(path)).unwrap()
    }

    /// macOS Runtime 状态写入必须保存完整身份，并冻结 complete 时的 usage。
    #[test]
    fn macos_runtime_store_persists_identity_and_complete_evidence() {
        let store = store();
        prepare(&store, "runtime", "host", "/bin/test", 1).unwrap();
        let prepared = tauri::async_runtime::block_on(store.runtime("runtime".into()))
            .unwrap()
            .unwrap();
        assert_eq!(prepared.runtime_platform, "macos");
        assert_eq!(prepared.state, "preparing");
        assert_eq!(prepared.codex_pid, None);

        let identity = ProcessIdentity {
            pid: 91,
            pgid: 91,
            sid: 91,
            start_token: ProcessStartToken {
                seconds: 2,
                microseconds: 3,
            },
        };
        start(&store, "runtime", &identity, 4).unwrap();
        initialized(&store, "runtime", "1.0", "schema", 5).unwrap();
        terminating(&store, "runtime", 6).unwrap();
        complete(&store, "runtime", MacosEvidenceKind::LiveGroupEmpty, 7).unwrap();

        let complete_record = tauri::async_runtime::block_on(store.runtime("runtime".into()))
            .unwrap()
            .unwrap();
        assert_eq!(complete_record.state, "terminated");
        assert_eq!(complete_record.codex_pid, Some(91));
        assert_eq!(complete_record.containment_process_group_id, Some(91));
        assert_eq!(complete_record.containment_session_id, Some(91));
        assert_eq!(
            complete_record.codex_process_start_token.as_deref(),
            Some("darwin_proc_bsd_start_v1:2:3")
        );
        assert_eq!(
            complete_record.termination_evidence_type.as_deref(),
            Some("macos_live_process_group_empty")
        );
        assert!(prepare(&store, "runtime", "host", "/bin/test", 8).is_err());
    }
}
