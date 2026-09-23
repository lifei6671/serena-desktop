use super::{
    macos_launcher::{CreatedChild, MacosLaunchRequest},
    macos_recovery::{self, RecoveryStatus},
    macos_runtime::{MacosRuntime, MacosRuntimeCreateFailure, MacosRuntimeFailure},
    protocol::CompatibilityIdentity,
};
use crate::agent::store::{RuntimeRecord, StateStore};
use crate::agent::{
    provider::port::ProviderReconcileSummary, task_manager::recovery::StartupRecoveryFailure,
};
use std::{ffi::OsString, fmt, fs::File, path::PathBuf, process::ExitStatus, time::Duration};

/// 共享 managed 层传入的 concrete macOS launch 参数。
pub(crate) struct LaunchRequest {
    pub(crate) executable: PathBuf,
    pub(crate) args: Vec<OsString>,
    pub(crate) current_dir: PathBuf,
    pub(crate) runtime_instance_id: String,
}

/// macOS product adapter 持有 live Runtime 或 identity 失败后仍不能丢弃的 child ownership。
enum RuntimeOwnership {
    Managed(MacosRuntime),
    Created(CreatedChild),
}

/// 共享 Provider 可见的 macOS concrete Runtime wrapper；它不是公共 trait。
pub(crate) struct Runtime {
    ownership: RuntimeOwnership,
}

impl fmt::Debug for Runtime {
    /// 调试信息只暴露 ownership 类别，不展开 stdio 或系统 handle。
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Runtime")
            .field(
                "ownership",
                &match self.ownership {
                    RuntimeOwnership::Managed(_) => "managed",
                    RuntimeOwnership::Created(_) => "created-unverified",
                },
            )
            .finish()
    }
}

/// Adapter 的稳定 Runtime 错误。
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct RuntimeError {
    pub(crate) code: &'static str,
    pub(crate) message: String,
}

impl RuntimeError {
    /// 构造共享 Provider/Pool 可识别的稳定错误。
    pub(in crate::agent) fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

/// 失败收口必须把仍存在的 live ownership 交还调用方。
pub(crate) struct RuntimeFailure {
    pub(crate) code: &'static str,
    pub(crate) message: String,
    pub(crate) runtime: Option<Box<Runtime>>,
}

impl fmt::Debug for RuntimeFailure {
    /// 调试信息只显示稳定错误和 ownership 是否保留。
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RuntimeFailure")
            .field("code", &self.code)
            .field("message", &self.message)
            .field("retains_runtime", &self.runtime.is_some())
            .finish()
    }
}

impl From<RuntimeError> for RuntimeFailure {
    /// 没有 live owner 的 adapter 错误保持 runtime=None。
    fn from(error: RuntimeError) -> Self {
        Self {
            code: error.code,
            message: error.message,
            runtime: None,
        }
    }
}

impl Runtime {
    /// 只观测 probe 的 direct child 状态；调用方仍须独立完成 group termination。
    pub(crate) fn probe_exit_status(&mut self) -> std::io::Result<Option<ExitStatus>> {
        match &mut self.ownership {
            RuntimeOwnership::Managed(runtime) => runtime.probe_exit_status(),
            RuntimeOwnership::Created(_) => Err(std::io::Error::other(
                "Unverified created child has no probe exit status",
            )),
        }
    }

    /// 仅供 macOS managed compatibility 测试恢复已经自停的短命 probe fixture。
    #[cfg(test)]
    pub(crate) fn resume_stopped_probe_for_test(
        &mut self,
        timeout: Duration,
    ) -> std::io::Result<()> {
        match &mut self.ownership {
            RuntimeOwnership::Managed(runtime) => {
                runtime.resume_stopped_probe_for_test(timeout)
            }
            RuntimeOwnership::Created(_) => Err(std::io::Error::other(
                "Unverified created child cannot resume probe fixture",
            )),
        }
    }

    /// 在 blocking worker 创建 setsid Runtime，并完整转移任何失败 ownership。
    pub(crate) async fn create(
        store: StateStore,
        owner: String,
        request: LaunchRequest,
        _timeout: Duration,
    ) -> Result<Self, RuntimeFailure> {
        tokio::task::spawn_blocking(move || {
            MacosRuntime::create(
                store,
                owner,
                MacosLaunchRequest {
                    executable: request.executable,
                    args: request.args,
                    current_dir: request.current_dir,
                    runtime_instance_id: request.runtime_instance_id,
                },
            )
        })
        .await
        .map_err(|error| {
            RuntimeFailure::from(RuntimeError::new(
                "CODEX_RUNTIME_WORKER_FAILED",
                error.to_string(),
            ))
        })?
        .map(|runtime| Self {
            ownership: RuntimeOwnership::Managed(runtime),
        })
        .map_err(map_create_failure)
    }

    /// 只从已验证 managed Runtime 复制 stdio；未验证 child 永不进入协议层。
    pub(crate) fn clone_stdio(&self) -> std::io::Result<(File, File, File)> {
        match &self.ownership {
            RuntimeOwnership::Managed(runtime) => runtime.clone_stdio(),
            RuntimeOwnership::Created(_) => Err(std::io::Error::other(
                "Unverified created child has no protocol stdio handoff",
            )),
        }
    }

    /// 在 blocking worker 提交 initialize identity，不阻塞 Tokio async executor。
    pub(crate) async fn initialized(
        &self,
        identity: &CompatibilityIdentity,
    ) -> Result<(), RuntimeError> {
        let RuntimeOwnership::Managed(runtime) = &self.ownership else {
            return Err(RuntimeError::new(
                "CODEX_PROCESS_IDENTITY_FAILED",
                "Unverified created child cannot become initialized",
            ));
        };
        let (store, runtime_id) = runtime.initialization_context();
        let identity = identity.clone();
        tokio::task::spawn_blocking(move || {
            super::macos_runtime_store::initialized(
                &store,
                &runtime_id,
                &identity.version,
                &identity.protocol_schema_sha256,
                crate::agent::coordinator::now(),
            )
            .map_err(|error| RuntimeError::new(error.code, error.message))
        })
        .await
        .map_err(|error| RuntimeError::new("CODEX_RUNTIME_WORKER_FAILED", error.to_string()))?
    }

    /// 在 blocking worker 执行有界 SIGTERM→SIGKILL 收口并返回持久化 evidence 结果。
    pub(crate) async fn terminate(self, timeout: Duration) -> Result<(), RuntimeFailure> {
        tokio::task::spawn_blocking(move || match self.ownership {
            RuntimeOwnership::Managed(runtime) => {
                let grace = timeout / 2;
                let kill_wait = timeout.saturating_sub(grace);
                runtime
                    .shutdown(grace, kill_wait)
                    .map(|_| ())
                    .map_err(map_shutdown_failure)
            }
            RuntimeOwnership::Created(created) => Err(RuntimeFailure {
                code: "CODEX_RUNTIME_TERMINATION_UNCONFIRMED",
                message:
                    "Process identity was not verified; process-group termination is forbidden"
                        .into(),
                runtime: Some(Box::new(Runtime {
                    ownership: RuntimeOwnership::Created(created),
                })),
            }),
        })
        .await
        .map_err(|error| {
            RuntimeFailure::from(RuntimeError::new(
                "CODEX_RUNTIME_WORKER_FAILED",
                error.to_string(),
            ))
        })?
    }
}

/// 把 Phase 2A create failure 转换为共享 Pool 可保留的 ownership。
fn map_create_failure(mut failure: MacosRuntimeCreateFailure) -> RuntimeFailure {
    let runtime = failure
        .runtime
        .take()
        .map(|runtime| Runtime {
            ownership: RuntimeOwnership::Managed(*runtime),
        })
        .or_else(|| {
            failure.created.take().map(|created| Runtime {
                ownership: RuntimeOwnership::Created(*created),
            })
        });
    RuntimeFailure {
        code: failure.code,
        message: failure.message,
        runtime: runtime.map(Box::new),
    }
}

/// 把 Phase 2A shutdown failure 转换为共享 Pool 可重试的 ownership。
fn map_shutdown_failure(failure: MacosRuntimeFailure) -> RuntimeFailure {
    RuntimeFailure {
        code: failure.code,
        message: failure.message,
        runtime: Some(Box::new(Runtime {
            ownership: RuntimeOwnership::Managed(*failure.runtime),
        })),
    }
}

/// 无 live owner 的恢复直接复用既有 macos_recovery，unknown 必须 fail closed。
pub(crate) async fn recover(
    store: StateStore,
    runtime_id: String,
    timeout: Duration,
) -> Result<(), RuntimeFailure> {
    let status = tokio::task::spawn_blocking(move || {
        tauri::async_runtime::block_on(macos_recovery::recover_runtime(
            &store,
            &runtime_id,
            timeout / 2,
            timeout.saturating_sub(timeout / 2),
        ))
    })
    .await
    .map_err(|error| {
        RuntimeFailure::from(RuntimeError::new(
            "CODEX_RUNTIME_WORKER_FAILED",
            error.to_string(),
        ))
    })?
    .map_err(|message| {
        RuntimeFailure::from(RuntimeError::new("CODEX_RUNTIME_STORE_FAILED", message))
    })?;
    match status {
        RecoveryStatus::Recovered => Ok(()),
        RecoveryStatus::Unknown => Err(RuntimeFailure::from(RuntimeError::new(
            "CODEX_RUNTIME_TERMINATION_UNCONFIRMED",
            "macOS Runtime recovery did not produce complete group-empty evidence",
        ))),
    }
}

/// 只接受 Phase 2A/2B 已冻结的两种 macOS group-empty evidence。
pub(crate) fn is_complete_termination(record: &RuntimeRecord) -> bool {
    record.state == "terminated"
        && record.runtime_platform == "macos"
        && record.containment_type == "macos_process_group"
        && record.process_identity_scheme == "darwin_proc_bsd_start_v1"
        && record.codex_pid.is_some()
        && record
            .codex_process_start_token
            .as_deref()
            .is_some_and(|token| super::macos_launcher::ProcessStartToken::decode(token).is_ok())
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

/// Provider startup 直接委托 Phase 2B macos_recovery，不复制 Claim/recovery 状态机。
pub(crate) async fn recover_startup(
    store: &StateStore,
    _executable: &std::path::Path,
    owner: &str,
    _runtime_pool: &std::sync::Arc<crate::agent::codex::pool::CodexRuntimePool>,
    _backend_error: Option<&str>,
) -> Result<ProviderReconcileSummary, StartupRecoveryFailure> {
    macos_recovery::recover_startup(store, owner)
        .await
        .map_err(StartupRecoveryFailure::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::codex::{
        macos_launcher::process_group_members, protocol::CompatibilityIdentity,
    };
    use std::{path::Path, process::Command, time::Duration};

    /// 直接编译固定 fixture，避免通过 shell 创建 Runtime。
    fn fixture(directory: &Path) -> std::path::PathBuf {
        let executable = directory.join("macos-runtime-adapter-child");
        let output = Command::new("rustc")
            .args([
                "--edition=2024",
                "--crate-name",
                "macos_runtime_adapter_child",
            ])
            .arg(
                Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/macos_runtime_child.rs"),
            )
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        executable
    }

    /// Adapter 必须在 blocking worker 创建进程，并把 stdio 交给 Tokio 层。
    #[tokio::test]
    async fn create_hands_off_stdio_persists_identity_and_shuts_down() {
        let directory = tempfile::tempdir().unwrap();
        let store = crate::agent::store::StateStore::open(directory.path().join("state"))
            .await
            .unwrap();
        let runtime_id = "adapter-runtime".to_string();
        let runtime = Runtime::create(
            store.clone(),
            "test-host".into(),
            LaunchRequest {
                executable: fixture(directory.path()),
                args: vec!["report".into(), "带 空格".into()],
                current_dir: directory.path().to_owned(),
                runtime_instance_id: runtime_id.clone(),
            },
            Duration::from_secs(2),
        )
        .await
        .unwrap();
        let (_stdin, _stdout, _stderr) = runtime.clone_stdio().unwrap();
        runtime
            .initialized(&CompatibilityIdentity {
                version: "codex-cli 0.153.4".into(),
                binary_sha256: crate::agent::codex::compatibility::MACOS_ARM64
                    .binary_sha256
                    .into(),
                protocol_schema_sha256: crate::agent::codex::compatibility::MACOS_ARM64
                    .protocol_schema_sha256
                    .into(),
            })
            .await
            .unwrap();
        runtime.terminate(Duration::from_secs(2)).await.unwrap();

        let mut row = store.runtime(runtime_id).await.unwrap().unwrap();
        assert!(is_complete_termination(&row));
        assert!(
            process_group_members(row.codex_pid.unwrap() as libc::pid_t)
                .unwrap()
                .is_empty()
        );
        row.codex_process_start_token = None;
        assert!(!is_complete_termination(&row));
        row.codex_process_start_token = Some("darwin_proc_bsd_start_v1:1:1000000".into());
        assert!(!is_complete_termination(&row));
    }

    /// spawn 后 Store start 失败时，Adapter failure 必须继续持有可收口 owner。
    #[tokio::test]
    async fn create_failure_retains_runtime_owner() {
        let directory = tempfile::tempdir().unwrap();
        let store = crate::agent::store::StateStore::open(directory.path().join("state"))
            .await
            .unwrap();
        store
            .write_blocking(|transaction| {
                transaction
                    .execute_batch(
                        "CREATE TRIGGER reject_adapter_start BEFORE UPDATE OF state ON runtime_instances
                         WHEN NEW.state='starting'
                         BEGIN SELECT RAISE(ABORT,'fixture start failure'); END;",
                    )
                    .map_err(|error| error.to_string())?;
                Ok(())
            })
            .unwrap();
        let failure = Runtime::create(
            store,
            "test-host".into(),
            LaunchRequest {
                executable: fixture(directory.path()),
                args: vec![
                    "ignore-tree".into(),
                    directory.path().join("leaf.pid").into_os_string(),
                ],
                current_dir: directory.path().to_owned(),
                runtime_instance_id: "adapter-start-failure".into(),
            },
            Duration::from_secs(2),
        )
        .await
        .unwrap_err();
        assert_eq!(failure.code, "CODEX_RUNTIME_STORE_FAILED");
        failure
            .runtime
            .expect("spawn 后 failure 必须保留 Runtime ownership")
            .terminate(Duration::from_secs(2))
            .await
            .unwrap();
    }

    /// 缺失 Runtime 的恢复不能伪造成 complete termination。
    #[tokio::test]
    async fn recovery_delegates_fail_closed_for_missing_runtime() {
        let directory = tempfile::tempdir().unwrap();
        let store = crate::agent::store::StateStore::open(directory.path().join("state"))
            .await
            .unwrap();
        let failure = recover(store, "missing-runtime".into(), Duration::from_millis(10))
            .await
            .unwrap_err();
        assert_eq!(failure.code, "CODEX_RUNTIME_TERMINATION_UNCONFIRMED");
        assert!(failure.runtime.is_none());
    }
}
