//! macOS ARM64 App Server ownership bridge；同步 OS 生命周期只在 blocking worker 执行。
use super::*;
use crate::agent::{
    codex::{
        macos_discovery,
        macos_runtime_adapter::{LaunchRequest, Runtime, RuntimeError, RuntimeFailure},
    },
    store::StateStore,
    task_manager::ProbeContext,
};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    future::Future,
    io::Read,
    path::{Path, PathBuf},
    process::ExitStatus,
    sync::atomic::{AtomicU64, Ordering},
};
use tokio::io::AsyncReadExt;

#[cfg(test)]
use crate::agent::codex::compatibility::MACOS_ARM64;

static NEXT_PROBE: AtomicU64 = AtomicU64::new(1);

/// 完整通过 macOS ARM64 preflight 与共享 schema 契约的候选证据。
pub struct CompatibilityEvidence {
    pub identity: CompatibilityIdentity,
    pub executable: PathBuf,
    pub source_commit: &'static str,
    pub wire_contract: &'static str,
    _binary: File,
}

/// Probe cleanup 失败的 typed ownership；runtime_id 与 live owner 不得被压成字符串。
#[derive(Debug)]
pub(crate) struct ProbeRuntimeFailure {
    pub(crate) runtime_id: String,
    pub(crate) failure: RuntimeFailure,
}

/// Compatibility 拒绝可在完整 cleanup 后继续；Runtime failure 必须停止 selection。
#[derive(Debug)]
pub(crate) enum ProbeFailure {
    Compatibility(ProtocolError),
    Runtime(ProbeRuntimeFailure),
}

type ProbeResult<T> = std::result::Result<T, ProbeFailure>;

/// cancellation cleanup 的保留域；Probe 使用全局 key，业务 Runtime 使用原 Workspace key。
#[derive(Clone)]
enum RetentionAuthority {
    Probe(ProbeContext),
    Business {
        store: StateStore,
        runtime_pool: std::sync::Arc<crate::agent::codex::pool::CodexRuntimePool>,
        workspace: String,
    },
}

/// 将 Runtime id 与既有 Pool authority 绑定，Drop worker 不依赖 async 调用栈存活。
#[derive(Clone)]
struct RuntimeRetention {
    authority: RetentionAuthority,
    runtime_id: String,
}

impl RuntimeRetention {
    /// Probe cleanup failure 只能进入现有 Pool 的全局 quarantine。
    fn probe(context: ProbeContext, runtime_id: String) -> Self {
        Self {
            authority: RetentionAuthority::Probe(context),
            runtime_id,
        }
    }

    /// 业务 Runtime cleanup failure 保留到原 Workspace quarantine。
    fn business(
        store: StateStore,
        runtime_pool: std::sync::Arc<crate::agent::codex::pool::CodexRuntimePool>,
        workspace: String,
        runtime_id: String,
    ) -> Self {
        Self {
            authority: RetentionAuthority::Business {
                store,
                runtime_pool,
                workspace,
            },
            runtime_id,
        }
    }

    /// 将 termination failure 同步交给既有 Pool；返回值仅供显式调用方诊断。
    fn retain(&self, failure: RuntimeFailure) -> RuntimeFailure {
        match &self.authority {
            RetentionAuthority::Probe(context) => macos_discovery::retain_probe_failure(
                context,
                ProbeRuntimeFailure {
                    runtime_id: self.runtime_id.clone(),
                    failure,
                },
            ),
            RetentionAuthority::Business {
                store,
                runtime_pool,
                workspace,
            } => runtime_pool.retain_failure(store, workspace, &self.runtime_id, failure),
        }
    }
}

/// 已创建 Runtime 的 cancellation guard；显式转移前始终拥有 bounded cleanup 责任。
struct OwnedRuntimeGuard {
    runtime: Option<Runtime>,
    retention: RuntimeRetention,
}

impl OwnedRuntimeGuard {
    /// 从 handoff 结果建立唯一 owner。
    fn new(runtime: Runtime, retention: RuntimeRetention) -> Self {
        Self {
            runtime: Some(runtime),
            retention,
        }
    }

    /// 只借用 Runtime 执行 stdio/initialized 操作，不提前解除 cancellation ownership。
    fn runtime(&self) -> &Runtime {
        self.runtime.as_ref().expect("owned Runtime exists")
    }

    /// CLI probe 的直接 child 状态只能在 owner 仍由 guard 持有时观测。
    fn runtime_mut(&mut self) -> &mut Runtime {
        self.runtime.as_mut().expect("owned Runtime exists")
    }

    /// 显式 termination 通过独立 handoff worker 执行，调用 future 被取消也不会丢 owner。
    async fn terminate(
        mut self,
        timeout: std::time::Duration,
    ) -> std::result::Result<(), RuntimeFailure> {
        let runtime = self.runtime.take().expect("owned Runtime exists");
        let retention = self.retention.clone();
        let result = handoff_termination(runtime, retention.clone(), timeout)
            .await
            .map_err(|error| {
                RuntimeFailure::from(RuntimeError::new(
                    "CODEX_RUNTIME_WORKER_FAILED",
                    error.to_string(),
                ))
            })?
            .take();
        // 显式调用方随后被取消或其 JoinHandle 被放弃时，也不能丢失 failure 内的 owner。
        result.map_err(|failure| retention.retain(failure))
    }
}

impl Drop for OwnedRuntimeGuard {
    /// async local 被取消时立即转移到 bounded cleanup worker；失败必须保留到原 Pool。
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            spawn_retained_termination(runtime, self.retention.clone());
        }
    }
}

/// termination worker 的结果信封；receiver abandonment 时失败仍会进入 quarantine。
struct TerminationHandoff {
    result: Option<std::result::Result<(), RuntimeFailure>>,
    retention: RuntimeRetention,
}

impl TerminationHandoff {
    /// 显式调用方接管结果后，错误由上层既有路径处理。
    fn take(mut self) -> std::result::Result<(), RuntimeFailure> {
        self.result
            .take()
            .expect("termination handoff result exists")
    }
}

impl Drop for TerminationHandoff {
    /// 未消费的 termination failure 不得丢弃其中的 live Runtime owner。
    fn drop(&mut self) {
        if let Some(Err(failure)) = self.result.take() {
            self.retention.retain(failure);
        }
    }
}

/// 启动独立 bounded termination，并通过信封处理 receiver abandonment。
fn handoff_termination(
    runtime: Runtime,
    retention: RuntimeRetention,
    timeout: std::time::Duration,
) -> oneshot::Receiver<TerminationHandoff> {
    let (sender, receiver) = oneshot::channel();
    tokio::spawn(async move {
        let result = runtime.terminate(timeout).await;
        let _ = sender.send(TerminationHandoff {
            result: Some(result),
            retention,
        });
    });
    receiver
}

/// Drop 路径不等待 worker，但 bounded failure 必须同步交给已有 quarantine。
fn spawn_retained_termination(runtime: Runtime, retention: RuntimeRetention) {
    tokio::spawn(async move {
        if let Err(failure) = runtime.terminate(INIT_TIMEOUT).await {
            retention.retain(failure);
        }
    });
}

/// 测试只在唯一 executable + checkpoint 上暂停，不影响并行 fixture。
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
enum ProbeCheckpoint {
    Created,
    Read,
}

#[cfg(test)]
struct ProbePause {
    entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
}

/// 测试暂停表按 canonical fixture path 与唯一 checkpoint 隔离。
#[cfg(test)]
type ProbePauseKey = (PathBuf, ProbeCheckpoint);
#[cfg(test)]
type ProbePauseMap = std::collections::HashMap<ProbePauseKey, std::sync::Arc<ProbePause>>;

#[cfg(test)]
fn probe_pauses() -> &'static std::sync::Mutex<ProbePauseMap> {
    static PAUSES: std::sync::OnceLock<std::sync::Mutex<ProbePauseMap>> =
        std::sync::OnceLock::new();
    PAUSES.get_or_init(Default::default)
}

#[cfg(test)]
fn install_probe_pause(
    executable: &Path,
    checkpoint: ProbeCheckpoint,
) -> std::sync::Arc<ProbePause> {
    let pause = std::sync::Arc::new(ProbePause {
        entered: Default::default(),
        release: Default::default(),
    });
    probe_pauses()
        .lock()
        .unwrap()
        .insert((executable.to_owned(), checkpoint), pause.clone());
    pause
}

#[cfg(test)]
fn remove_probe_pause(executable: &Path, checkpoint: ProbeCheckpoint) {
    if let Some(pause) = probe_pauses()
        .lock()
        .unwrap()
        .remove(&(executable.to_owned(), checkpoint))
    {
        pause.release.notify_waiters();
    }
}

#[cfg(test)]
async fn probe_checkpoint(executable: &Path, checkpoint: ProbeCheckpoint) {
    let pause = probe_pauses()
        .lock()
        .unwrap()
        .get(&(executable.to_owned(), checkpoint))
        .cloned();
    if let Some(pause) = pause {
        pause.entered.notify_one();
        pause.release.notified().await;
    }
}

#[cfg(not(test))]
async fn probe_checkpoint(_executable: &Path, _checkpoint: ProbeCheckpoint) {}

impl From<ProtocolError> for ProbeFailure {
    /// 非 Runtime ownership 错误保持原协议类别。
    fn from(error: ProtocolError) -> Self {
        Self::Compatibility(error)
    }
}

/// 将任意本地 I/O 失败转换为稳定 compatibility 错误。
fn io_error(error: impl std::fmt::Display) -> ProtocolError {
    ProtocolError::incompatible(error.to_string())
}

/// 把 Runtime 失败与其唯一 probe id 绑定，供原 Pool 接管 ownership。
fn probe_runtime_failure(runtime_id: String, failure: RuntimeFailure) -> ProbeFailure {
    ProbeFailure::Runtime(ProbeRuntimeFailure {
        runtime_id,
        failure,
    })
}

/// 读取完整文件并计算大写 SHA-256。
fn digest(mut file: File) -> Result<String> {
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(io_error)?;
        if read == 0 {
            break;
        }
        hash.update(&buffer[..read]);
    }
    Ok(hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect())
}

/// 最终 executable 必须已经 canonicalize 为 absolute path。
fn validate_executable_path(executable: &Path) -> Result<()> {
    if !executable.is_absolute() {
        return Err(ProtocolError::new(
            "CODEX_EXECUTABLE_NOT_RUNNABLE",
            "Absolute executable required",
        ));
    }
    let canonical = executable.canonicalize().map_err(io_error)?;
    if canonical != executable {
        return Err(ProtocolError::new(
            "CODEX_EXECUTABLE_NOT_RUNNABLE",
            "Executable path must be canonical",
        ));
    }
    macos_discovery::preflight(executable)
        .map_err(|error| ProtocolError::new(error.code, error.message))
}

/// 通过 cancellation-safe handoff 创建正式 probe Runtime；receiver abandonment 仍会 bounded cleanup。
async fn create_probe_runtime(
    context: &ProbeContext,
    executable: &Path,
    cwd: &Path,
    args: Vec<&str>,
    runtime_id: String,
) -> ProbeResult<OwnedRuntimeGuard> {
    let retention = RuntimeRetention::probe(context.clone(), runtime_id.clone());
    let checkpoint_path = executable.to_owned();
    let create = Runtime::create(
        context.store(),
        context.owner().into(),
        LaunchRequest {
            executable: executable.to_owned(),
            current_dir: cwd.to_owned(),
            args: args.into_iter().map(Into::into).collect(),
            runtime_instance_id: runtime_id.clone(),
        },
        INIT_TIMEOUT,
    );
    let create = async move {
        let runtime = create.await?;
        probe_checkpoint(&checkpoint_path, ProbeCheckpoint::Created).await;
        Ok(runtime)
    };
    handoff_creation(create, retention)
        .await
        .map_err(|error| {
            probe_runtime_failure(
                runtime_id.clone(),
                RuntimeFailure::from(RuntimeError::new(
                    "CODEX_RUNTIME_WORKER_FAILED",
                    error.to_string(),
                )),
            )
        })?
        .take()
        .map_err(|failure| probe_runtime_failure(runtime_id, failure))
}

/// 创建隔离 probe Runtime，直接执行 Mach-O + argv 并保证 Process Group 收口。
async fn cli(
    context: &ProbeContext,
    executable: &Path,
    cwd: &Path,
    args: Vec<&str>,
) -> ProbeResult<String> {
    let runtime_id = format!(
        "macos-contract-{}-{}",
        std::process::id(),
        NEXT_PROBE.fetch_add(1, Ordering::Relaxed)
    );
    let mut runtime =
        create_probe_runtime(context, executable, cwd, args, runtime_id.clone()).await?;
    #[cfg(test)]
    if executable
        .file_name()
        .is_some_and(|name| name.to_string_lossy().starts_with("probe-ownership-child"))
        && let Err(error) = runtime
            .runtime_mut()
            .resume_stopped_probe_for_test(Duration::from_secs(2))
    {
        terminate_probe(runtime_id, runtime).await?;
        return Err(io_error(format!("Probe fixture resume failed: {error}")).into());
    }
    let (stdin, stdout, stderr) = match runtime.runtime().clone_stdio() {
        Ok(pipes) => pipes,
        Err(error) => {
            terminate_probe(runtime_id, runtime).await?;
            return Err(io_error(error).into());
        }
    };
    probe_checkpoint(executable, ProbeCheckpoint::Read).await;
    drop(stdin);
    let mut stdout = tokio::fs::File::from_std(stdout).take((MAX_MESSAGE + 1) as u64);
    let mut stderr = tokio::fs::File::from_std(stderr).take((MAX_MESSAGE + 1) as u64);
    let output = tokio::time::timeout(INIT_TIMEOUT, async {
        let mut out = Vec::new();
        let mut err = Vec::new();
        tokio::try_join!(stdout.read_to_end(&mut out), stderr.read_to_end(&mut err))?;
        Ok::<_, std::io::Error>((out, err))
    })
    .await;
    // stdio 关闭不等价于退出成功；有界轮询真实 direct child ExitStatus。
    let exit_status = if matches!(&output, Ok(Ok(_))) {
        Some(
            tokio::time::timeout(INIT_TIMEOUT, async {
                loop {
                    match runtime.runtime_mut().probe_exit_status() {
                        Ok(Some(status)) => break Ok::<ExitStatus, std::io::Error>(status),
                        Ok(None) => tokio::time::sleep(std::time::Duration::from_millis(10)).await,
                        Err(error) => break Err(error),
                    }
                }
            })
            .await,
        )
    } else {
        None
    };
    // 无论读取成功、失败或超时都先完成 Process Group 收口，避免 `?` 提前丢失 live child。
    terminate_probe(runtime_id, runtime).await?;
    let (output, diagnostic) = output
        .map_err(|_| ProbeFailure::from(io_error("Compatibility CLI timed out")))?
        .map_err(|error| ProbeFailure::from(io_error(error)))?;
    if output.len() > MAX_MESSAGE || diagnostic.len() > MAX_MESSAGE {
        return Err(io_error("Compatibility CLI output exceeded bound").into());
    }
    let status = exit_status
        .expect("successful CLI output must have an exit observation")
        .map_err(|_| ProbeFailure::from(io_error("Compatibility CLI exit status timed out")))?
        .map_err(io_error)
        .map_err(ProbeFailure::from)?;
    if !status.success() {
        return Err(io_error(format!("Compatibility CLI exited with status {status}")).into());
    }
    String::from_utf8(output)
        .map_err(io_error)
        .map_err(Into::into)
}

/// 唯一结束 probe Runtime 的路径；失败时保留 id 和完整 live ownership。
async fn terminate_probe(runtime_id: String, runtime: OwnedRuntimeGuard) -> ProbeResult<()> {
    runtime
        .terminate(INIT_TIMEOUT)
        .await
        .map_err(|failure| probe_runtime_failure(runtime_id, failure))
}

/// 对 canonical ARM64 Mach-O 执行共享 schema compatibility selection contract。
pub(crate) async fn verify(
    context: &ProbeContext,
    executable: PathBuf,
) -> ProbeResult<CompatibilityEvidence> {
    validate_executable_path(&executable).map_err(ProbeFailure::from)?;
    let temp = tempfile::tempdir()
        .map_err(io_error)
        .map_err(ProbeFailure::from)?;
    // 版本和 binary digest 只形成 identity 证据，不参与兼容性准入。
    let version = cli(context, &executable, temp.path(), vec!["--version"])
        .await?
        .trim()
        .to_owned();
    let binary = File::open(&executable)
        .map_err(io_error)
        .map_err(ProbeFailure::from)?;
    let binary_for_hash = binary
        .try_clone()
        .map_err(io_error)
        .map_err(ProbeFailure::from)?;
    let hash = tokio::task::spawn_blocking(move || digest(binary_for_hash))
        .await
        .map_err(io_error)
        .map_err(ProbeFailure::from)?
        .map_err(ProbeFailure::from)?;
    // 兼容检测只导出并读取 schema，不启动 app-server 或发送 JSON-RPC。
    let schema_dir = temp.path().join("schema");
    let schema_path = schema_dir
        .to_str()
        .ok_or_else(|| ProbeFailure::from(io_error("Schema path is not Unicode")))?;
    cli(
        context,
        &executable,
        temp.path(),
        vec![
            "app-server",
            "generate-json-schema",
            "--experimental",
            "--out",
            schema_path,
        ],
    )
    .await?;
    let schema_path = schema_dir.join("codex_app_server_protocol.schemas.json");
    let schema_file = File::open(&schema_path)
        .map_err(io_error)
        .map_err(ProbeFailure::from)?;
    let schema = tokio::task::spawn_blocking(move || digest(schema_file))
        .await
        .map_err(io_error)
        .map_err(ProbeFailure::from)?
        .map_err(ProbeFailure::from)?;
    let schema_bytes = tokio::fs::read(schema_path)
        .await
        .map_err(io_error)
        .map_err(ProbeFailure::from)?;
    crate::agent::codex::compatibility::validate_schema(&schema_bytes)
        .map_err(ProbeFailure::from)?;
    let identity = CompatibilityIdentity {
        version,
        binary_sha256: hash,
        protocol_schema_sha256: schema,
    };
    Ok(CompatibilityEvidence {
        identity,
        executable,
        source_commit: SOURCE_COMMIT,
        wire_contract: WIRE_CONTRACT,
        _binary: binary,
    })
}

/// Runtime monitor 与 Client 的单一所有权组合。
pub struct ManagedClient {
    pub client: Client,
    pub compatibility: CompatibilityEvidence,
    reconciliation: tokio::task::JoinHandle<std::result::Result<(), RuntimeFailure>>,
}

impl ManagedClient {
    /// Provider fixture 使用的 owned Client，不创建真实进程。
    #[cfg(test)]
    pub(crate) fn test_owned(
        client: Client,
        reconciliation: tokio::task::JoinHandle<std::result::Result<(), RuntimeFailure>>,
    ) -> Self {
        Self {
            client,
            reconciliation,
            compatibility: CompatibilityEvidence {
                identity: CompatibilityIdentity {
                    version: MACOS_ARM64.codex_version.into(),
                    binary_sha256: MACOS_ARM64.binary_sha256.into(),
                    protocol_schema_sha256: MACOS_ARM64.protocol_schema_sha256.into(),
                },
                executable: PathBuf::from("fixture"),
                source_commit: SOURCE_COMMIT,
                wire_contract: WIRE_CONTRACT,
                _binary: tempfile::tempfile().unwrap(),
            },
        }
    }

    /// 请求 Client 停止后等待独立 Runtime monitor 完成真实 Process Group 收口。
    pub async fn shutdown(self) -> std::result::Result<(), RuntimeFailure> {
        self.client.cancel();
        self.wait_for_reconciliation().await
    }

    /// 等待 reconciliation worker，Join 失败使用稳定 Runtime 错误。
    pub async fn wait_for_reconciliation(self) -> std::result::Result<(), RuntimeFailure> {
        self.reconciliation.await.map_err(|error| {
            RuntimeFailure::from(RuntimeError::new(
                "CODEX_RUNTIME_WORKER_FAILED",
                error.to_string(),
            ))
        })?
    }
}

/// create future 的结果信封；receiver 取消后仍会主动收口其中的 Runtime。
struct RuntimeHandoff {
    result: Option<std::result::Result<Runtime, RuntimeFailure>>,
    retention: RuntimeRetention,
}

impl RuntimeHandoff {
    /// 唯一取出创建结果，并立即建立 cancellation guard。
    fn take(mut self) -> std::result::Result<OwnedRuntimeGuard, RuntimeFailure> {
        self.result
            .take()
            .expect("Runtime handoff result exists")
            .map(|runtime| OwnedRuntimeGuard::new(runtime, self.retention.clone()))
    }
}

impl Drop for RuntimeHandoff {
    /// 未消费信封时启动独立 bounded shutdown，避免取消丢失 ownership。
    fn drop(&mut self) {
        let runtime = match self.result.take() {
            Some(Ok(runtime)) => Some(runtime),
            Some(Err(failure)) => failure.runtime.map(|runtime| *runtime),
            None => None,
        };
        if let Some(runtime) = runtime {
            spawn_retained_termination(runtime, self.retention.clone());
        }
    }
}

/// 不可取消的 blocking create 始终把结果交给显式 owner。
fn handoff_creation(
    create: impl Future<Output = std::result::Result<Runtime, RuntimeFailure>> + Send + 'static,
    retention: RuntimeRetention,
) -> oneshot::Receiver<RuntimeHandoff> {
    let (sender, receiver) = oneshot::channel();
    tokio::spawn(async move {
        let result = RuntimeHandoff {
            result: Some(create.await),
            retention,
        };
        let _ = sender.send(result);
    });
    receiver
}

/// Runtime attempt 对应 dispatch 或 recovery 的既有持久化入口。
pub(crate) enum RuntimeAttempt {
    Dispatch(String),
    Recovery(String),
}

/// 验证候选后创建真实 App Server，stdio/RPC 全程保持 Tokio async。
pub async fn connect(
    probe_context: ProbeContext,
    store: StateStore,
    owner: String,
    runtime_id: String,
    executable: PathBuf,
    cwd: PathBuf,
    attempt: Option<RuntimeAttempt>,
) -> std::result::Result<ManagedClient, RuntimeFailure> {
    let compatibility = match verify(&probe_context, executable.clone()).await {
        Ok(compatibility) => compatibility,
        Err(ProbeFailure::Compatibility(error)) => {
            return Err(RuntimeFailure::from(RuntimeError::new(
                error.code,
                error.message,
            )));
        }
        Err(ProbeFailure::Runtime(failure)) => {
            // Provider reconnect 的 probe 与 startup discovery 使用同一全局 quarantine。
            return Err(macos_discovery::retain_probe_failure(
                &probe_context,
                failure,
            ));
        }
    };
    let now = crate::agent::coordinator::now();
    match attempt {
        Some(RuntimeAttempt::Dispatch(id)) => {
            store
                .reserve_runtime_attempt(id, runtime_id.clone(), now)
                .await
        }
        Some(RuntimeAttempt::Recovery(id)) => {
            store
                .reserve_recovery_attempt(id, runtime_id.clone(), now)
                .await
        }
        None => Ok(()),
    }
    .map_err(|error| RuntimeError::new("CODEX_RUNTIME_STORE_FAILED", error))?;
    let retention = RuntimeRetention::business(
        store.clone(),
        probe_context.runtime_pool(),
        cwd.to_string_lossy().into_owned(),
        runtime_id.clone(),
    );
    let runtime = handoff_creation(
        Runtime::create(
            store,
            owner,
            LaunchRequest {
                executable,
                current_dir: cwd,
                args: vec!["app-server".into(), "--listen".into(), "stdio://".into()],
                runtime_instance_id: runtime_id.clone(),
            },
            INIT_TIMEOUT,
        ),
        retention,
    )
    .await
    .map_err(|error| {
        RuntimeFailure::from(RuntimeError::new(
            "CODEX_RUNTIME_WORKER_FAILED",
            error.to_string(),
        ))
    })?
    .take()?;
    let (stdin, stdout, stderr) = match runtime.runtime().clone_stdio() {
        Ok(pipes) => pipes,
        Err(error) => {
            runtime.terminate(INIT_TIMEOUT).await?;
            return Err(RuntimeFailure::from(RuntimeError::new(
                "CODEX_STDIO_FAILED",
                error.to_string(),
            )));
        }
    };
    let stdout = tokio::fs::File::from_std(stdout);
    #[cfg(test)]
    let stdout = super::tests::record_output(stdout, &runtime_id, "stdout");
    let stderr = tokio::fs::File::from_std(stderr);
    #[cfg(test)]
    let stderr = super::tests::record_output(stderr, &runtime_id, "stderr");
    let client = Client::transport(
        runtime_id,
        tokio::io::BufReader::new(stdout),
        tokio::fs::File::from_std(stdin),
        stderr,
    );
    let identity = compatibility.identity.clone();
    let mut failure = client.failure();
    let (ready_tx, ready_rx) = oneshot::channel::<()>();
    let (persist_tx, persist_rx) = oneshot::channel();
    let reconciliation = tokio::spawn(async move {
        let initialize = tokio::select! {
            biased;
            _ = async {
                loop {
                    if failure.borrow().is_some() { break; }
                    if failure.changed().await.is_err() { break; }
                }
            } => false,
            result = ready_rx => result.is_ok(),
        };
        if initialize {
            let persisted = runtime.runtime().initialized(&identity).await;
            let ready = persisted.is_ok();
            let _ = persist_tx.send(persisted);
            if ready {
                loop {
                    if failure.borrow().is_some() || failure.changed().await.is_err() {
                        break;
                    }
                }
            }
        }
        runtime.terminate(INIT_TIMEOUT).await
    });
    if let Err(error) = client.initialize().await {
        drop(ready_tx);
        reconciliation.await.map_err(|join| {
            RuntimeFailure::from(RuntimeError::new(
                "CODEX_RUNTIME_WORKER_FAILED",
                join.to_string(),
            ))
        })??;
        return Err(RuntimeFailure::from(RuntimeError::new(
            error.code,
            error.message,
        )));
    }
    if ready_tx.send(()).is_err() {
        client.cancel();
        reconciliation.await.map_err(|join| {
            RuntimeFailure::from(RuntimeError::new(
                "CODEX_RUNTIME_WORKER_FAILED",
                join.to_string(),
            ))
        })??;
        return Err(RuntimeFailure::from(RuntimeError::new(
            "CODEX_RUNTIME_RECONCILIATION_STARTED",
            "Runtime reconciliation began during initialization",
        )));
    }
    let persisted = persist_rx.await.map_err(|_| {
        RuntimeFailure::from(RuntimeError::new(
            "CODEX_RUNTIME_WORKER_FAILED",
            "Runtime initialization worker closed",
        ))
    });
    if !matches!(persisted, Ok(Ok(()))) {
        client.cancel();
        reconciliation.await.map_err(|join| {
            RuntimeFailure::from(RuntimeError::new(
                "CODEX_RUNTIME_WORKER_FAILED",
                join.to_string(),
            ))
        })??;
        return Err(match persisted {
            Ok(Err(error)) => error.into(),
            Err(error) => error,
            _ => unreachable!(),
        });
    }
    Ok(ManagedClient {
        client,
        compatibility,
        reconciliation,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{
        codex::{
            compatibility::MACOS_ARM64,
            macos_launcher::{self, MacosLaunchRequest},
            macos_recovery, macos_runtime_store,
        },
        task_manager::AgentTaskManager,
    };
    use std::os::unix::fs::PermissionsExt;

    /// 编译固定 macOS child fixture，整个测试路径不经过 shell。
    fn probe_fixture(directory: &Path) -> PathBuf {
        let executable = directory.join("probe-ownership-child");
        let output = std::process::Command::new("rustc")
            .args(["--edition=2024", "--crate-name", "probe_ownership_child"])
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

    /// 等待指定 Runtime 行到达目标状态，避免用固定 sleep 掩盖 cancellation ownership 竞态。
    async fn wait_runtime_state(store: &StateStore, runtime_id: &str, expected: &str) {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            if store
                .runtime(runtime_id.into())
                .await
                .unwrap()
                .is_some_and(|row| row.state == expected)
            {
                return;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "Runtime state 等待超时"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    /// 返回正式 Store 中唯一未终止 probe id，测试不依赖内部 PID 或 Process Group。
    fn active_probe_id(root: &Path) -> String {
        let database = rusqlite::Connection::open(root.join("agent-state.db")).unwrap();
        database
            .query_row(
                "SELECT id FROM runtime_instances WHERE state != 'terminated' ORDER BY created_at DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap()
    }

    /// version/schema 两类 CLI probe 必须共用 Manager 的正式 Store/owner，且不创建业务行。
    #[tokio::test]
    async fn probe_ownership_uses_formal_store_owner_without_business_state() {
        let directory = tempfile::tempdir().unwrap();
        let store = StateStore::open(directory.path().join("formal-store"))
            .await
            .unwrap();
        let manager = AgentTaskManager::new(store.clone(), PathBuf::new());
        let context = manager.probe_context();
        let executable = probe_fixture(directory.path());

        assert_eq!(
            cli(
                &context,
                &executable,
                directory.path(),
                vec!["probe-output", "codex-cli 0.153.4"],
            )
            .await
            .unwrap()
            .trim(),
            "codex-cli 0.153.4"
        );
        cli(
            &context,
            &executable,
            directory.path(),
            vec!["probe-output", "schema-ok"],
        )
        .await
        .unwrap();
        let database =
            rusqlite::Connection::open(directory.path().join("formal-store/agent-state.db"))
                .unwrap();
        let probe_rows: i64 = database
            .query_row(
                "SELECT count(*) FROM runtime_instances WHERE owner_host_instance_id=?1 AND state='terminated' AND termination_evidence_state='complete'",
                [context.owner()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(probe_rows, 2);
        for table in ["executions", "workspace_claims"] {
            let count: i64 = database
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(count, 0, "probe must not create {table}");
        }
    }

    /// create await 被放弃后，guard 必须继续有界收口并写入 complete evidence。
    #[tokio::test]
    async fn probe_ownership_create_cancellation_keeps_runtime_owned() {
        let directory = tempfile::tempdir().unwrap();
        let state_root = directory.path().join("formal-store");
        let store = StateStore::open(state_root.clone()).await.unwrap();
        let manager = AgentTaskManager::new(store.clone(), PathBuf::new());
        let context = manager.probe_context();
        let executable = probe_fixture(directory.path()).canonicalize().unwrap();
        let pause = install_probe_pause(&executable, ProbeCheckpoint::Created);
        let cwd = directory.path().to_owned();
        let task = tokio::spawn({
            let context = context.clone();
            let executable = executable.clone();
            async move { cli(&context, &executable, &cwd, vec!["report", "create"]).await }
        });
        pause.entered.notified().await;
        let runtime_id = active_probe_id(&state_root);
        task.abort();
        let _ = task.await;
        remove_probe_pause(&executable, ProbeCheckpoint::Created);
        wait_runtime_state(&store, &runtime_id, "terminated").await;
        assert_eq!(
            store
                .runtime(runtime_id)
                .await
                .unwrap()
                .unwrap()
                .termination_evidence_state,
            "complete"
        );
    }

    /// stdout/stderr read await 被放弃后，guard 必须保留并收口已经创建的 Runtime。
    #[tokio::test]
    async fn probe_ownership_read_cancellation_keeps_runtime_owned() {
        let directory = tempfile::tempdir().unwrap();
        let state_root = directory.path().join("formal-store");
        let store = StateStore::open(state_root.clone()).await.unwrap();
        let manager = AgentTaskManager::new(store.clone(), PathBuf::new());
        let context = manager.probe_context();
        let executable = probe_fixture(directory.path()).canonicalize().unwrap();
        let pause = install_probe_pause(&executable, ProbeCheckpoint::Read);
        let cwd = directory.path().to_owned();
        let task = tokio::spawn({
            let context = context.clone();
            let executable = executable.clone();
            async move { cli(&context, &executable, &cwd, vec!["report", "read"]).await }
        });
        pause.entered.notified().await;
        let runtime_id = active_probe_id(&state_root);
        task.abort();
        let _ = task.await;
        remove_probe_pause(&executable, ProbeCheckpoint::Read);
        wait_runtime_state(&store, &runtime_id, "terminated").await;
        assert_eq!(
            store
                .runtime(runtime_id)
                .await
                .unwrap()
                .unwrap()
                .termination_evidence_state,
            "complete"
        );
    }

    /// cancellation cleanup 失败必须保留 Runtime owner 并进入 probe 全局 quarantine。
    #[tokio::test]
    async fn probe_ownership_cancellation_failure_enters_global_quarantine() {
        let directory = tempfile::tempdir().unwrap();
        let state_root = directory.path().join("formal-store");
        let store = StateStore::open(state_root.clone()).await.unwrap();
        let manager = AgentTaskManager::new(store.clone(), PathBuf::new());
        let context = manager.probe_context();
        let executable = probe_fixture(directory.path()).canonicalize().unwrap();
        let pause = install_probe_pause(&executable, ProbeCheckpoint::Read);
        let cwd = directory.path().to_owned();
        let task = tokio::spawn({
            let context = context.clone();
            let executable = executable.clone();
            async move { cli(&context, &executable, &cwd, vec!["report", "failure"]).await }
        });
        pause.entered.notified().await;
        let runtime_id = active_probe_id(&state_root);
        let database = rusqlite::Connection::open(state_root.join("agent-state.db")).unwrap();
        database
            .execute(
                "UPDATE runtime_instances SET state='terminated' WHERE id=?1",
                [&runtime_id],
            )
            .unwrap();
        task.abort();
        let _ = task.await;
        remove_probe_pause(&executable, ProbeCheckpoint::Read);
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !context.runtime_pool().retains_runtime("", &runtime_id) {
            assert!(
                std::time::Instant::now() < deadline,
                "probe quarantine 等待超时"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(
            context
                .runtime_pool()
                .check_workspace("/workspace")
                .unwrap_err(),
            "AGENT_RUNTIME_QUARANTINED"
        );
        database
            .execute(
                "UPDATE runtime_instances SET state='starting' WHERE id=?1",
                [&runtime_id],
            )
            .unwrap();
        context
            .runtime_pool()
            .retry_workspace(&store, "")
            .await
            .unwrap();
    }

    /// receiver abandonment 后的业务 termination failure 必须进入原 Workspace quarantine。
    #[tokio::test]
    async fn probe_ownership_business_handoff_abandonment_retains_failure() {
        let directory = tempfile::tempdir().unwrap();
        let state_root = directory.path().join("formal-store");
        let store = StateStore::open(state_root.clone()).await.unwrap();
        let manager = AgentTaskManager::new(store.clone(), PathBuf::new());
        let context = manager.probe_context();
        let executable = probe_fixture(directory.path());
        let runtime_id = "business-handoff-abandoned".to_string();
        let retention = RuntimeRetention::business(
            store.clone(),
            context.runtime_pool(),
            "/workspace".into(),
            runtime_id.clone(),
        );
        let create_store = store.clone();
        let create_id = runtime_id.clone();
        let create_owner = context.owner().to_owned();
        let create_cwd = directory.path().to_owned();
        let create_state_root = state_root.clone();
        let receiver = handoff_creation(
            async move {
                let runtime = Runtime::create(
                    create_store,
                    create_owner,
                    LaunchRequest {
                        executable,
                        current_dir: create_cwd,
                        args: vec!["report".into(), "handoff".into()],
                        runtime_instance_id: create_id.clone(),
                    },
                    INIT_TIMEOUT,
                )
                .await?;
                let database =
                    rusqlite::Connection::open(create_state_root.join("agent-state.db")).unwrap();
                database
                    .execute(
                        "UPDATE runtime_instances SET state='terminated' WHERE id=?1",
                        [&create_id],
                    )
                    .unwrap();
                Ok(runtime)
            },
            retention,
        );
        drop(receiver);
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !context
            .runtime_pool()
            .retains_runtime("/workspace", &runtime_id)
        {
            assert!(
                std::time::Instant::now() < deadline,
                "business quarantine 等待超时"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let database =
            rusqlite::Connection::open(directory.path().join("formal-store/agent-state.db"))
                .unwrap();
        database
            .execute(
                "UPDATE runtime_instances SET state='starting' WHERE id=?1",
                [&runtime_id],
            )
            .unwrap();
        context
            .runtime_pool()
            .retry_workspace(&store, "/workspace")
            .await
            .unwrap();
    }

    /// 显式 termination future 被取消等价于 receiver abandonment，失败 owner 仍必须进入全局 quarantine。
    #[tokio::test]
    async fn probe_ownership_explicit_termination_abandonment_retains_failure() {
        let directory = tempfile::tempdir().unwrap();
        let state_root = directory.path().join("formal-store");
        let store = StateStore::open(state_root.clone()).await.unwrap();
        let manager = AgentTaskManager::new(store.clone(), PathBuf::new());
        let context = manager.probe_context();
        let runtime_id = "probe-explicit-termination-abandoned".to_string();
        let runtime = Runtime::create(
            context.store(),
            context.owner().into(),
            LaunchRequest {
                executable: probe_fixture(directory.path()),
                current_dir: directory.path().to_owned(),
                args: vec!["report".into(), "done".into()],
                runtime_instance_id: runtime_id.clone(),
            },
            INIT_TIMEOUT,
        )
        .await
        .unwrap();
        let database = rusqlite::Connection::open(state_root.join("agent-state.db")).unwrap();
        database
            .execute(
                "UPDATE runtime_instances SET state='terminated' WHERE id=?1",
                [&runtime_id],
            )
            .unwrap();
        let receiver = handoff_termination(
            runtime,
            RuntimeRetention::probe(context.clone(), runtime_id.clone()),
            INIT_TIMEOUT,
        );
        drop(receiver);

        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !context.runtime_pool().retains_runtime("", &runtime_id) {
            assert!(
                std::time::Instant::now() < deadline,
                "显式 termination cancellation quarantine 等待超时"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        database
            .execute(
                "UPDATE runtime_instances SET state='starting' WHERE id=?1",
                [&runtime_id],
            )
            .unwrap();
        context
            .runtime_pool()
            .retry_workspace(&store, "")
            .await
            .unwrap();
    }

    /// 候选必须完成 version 与 schema 两个 CLI probe，再按共享契约判定兼容性。
    #[tokio::test]
    async fn probe_ownership_compatibility_runs_version_and_schema_before_decision() {
        let directory = tempfile::tempdir().unwrap();
        let state_root = directory.path().join("formal-store");
        let store = StateStore::open(state_root.clone()).await.unwrap();
        let manager = AgentTaskManager::new(store, PathBuf::new());
        let context = manager.probe_context();
        let executable = probe_fixture(directory.path()).canonicalize().unwrap();
        assert!(matches!(
            verify(&context, executable).await,
            Err(ProbeFailure::Compatibility(_))
        ));
        let database = rusqlite::Connection::open(state_root.join("agent-state.db")).unwrap();
        let complete: i64 = database
            .query_row(
                "SELECT count(*) FROM runtime_instances WHERE state='terminated' AND termination_evidence_state='complete'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(complete, 2);
    }

    /// 合法 stdout 或已生成 schema 都不能覆盖非零退出码，且必须完成 probe cleanup。
    async fn assert_nonzero_cli_rejected(suffix: &str, exit_code: &str, expected_probes: i64) {
        let directory = tempfile::tempdir().unwrap();
        let state_root = directory.path().join("formal-store");
        let store = StateStore::open(state_root.clone()).await.unwrap();
        let manager = AgentTaskManager::new(store, PathBuf::new());
        let source = probe_fixture(directory.path());
        let executable = directory
            .path()
            .join(format!("probe-ownership-child-{suffix}"));
        std::fs::copy(source, &executable).unwrap();
        let executable = executable.canonicalize().unwrap();
        let failure = match verify(&manager.probe_context(), executable).await {
            Ok(_) => panic!("nonzero compatibility CLI exit was accepted"),
            Err(failure) => failure,
        };
        match failure {
            ProbeFailure::Compatibility(error) => {
                assert_eq!(error.code, "CODEX_APP_SERVER_INCOMPATIBLE");
                assert!(error.message.contains(exit_code), "{}", error.message);
            }
            ProbeFailure::Runtime(error) => panic!("probe cleanup failed: {error:?}"),
        }
        let database = rusqlite::Connection::open(state_root.join("agent-state.db")).unwrap();
        let complete: i64 = database
            .query_row(
                "SELECT count(*) FROM runtime_instances WHERE state='terminated' AND termination_evidence_state='complete'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(complete, expected_probes);
    }

    /// version 有合法 stdout 但退出码非零时不得接受 identity。
    #[tokio::test]
    async fn version_stdout_with_nonzero_exit_is_rejected() {
        assert_nonzero_cli_rejected("version-nonzero", "7", 1).await;
    }

    /// schema 文件已经写出但导出命令退出码非零时不得接受 schema。
    #[tokio::test]
    async fn generated_schema_with_nonzero_exit_is_rejected() {
        assert_nonzero_cli_rejected("schema-nonzero", "9", 2).await;
    }

    /// Compatibility 拒绝只有在该候选的 Runtime evidence complete 后才能继续下一候选。
    #[tokio::test]
    async fn probe_ownership_compatibility_continues_after_complete_cleanup() {
        let directory = tempfile::tempdir().unwrap();
        let store = StateStore::open(directory.path().join("formal-store"))
            .await
            .unwrap();
        let manager = AgentTaskManager::new(store.clone(), PathBuf::new());
        let context = manager.probe_context();
        let first = probe_fixture(directory.path());
        let second = directory.path().join("probe-ownership-child-2");
        std::fs::copy(&first, &second).unwrap();
        std::fs::set_permissions(&second, std::fs::Permissions::from_mode(0o755)).unwrap();
        let first = first.canonicalize().unwrap();
        let expected = second.canonicalize().unwrap();
        let first_for_verify = first.clone();
        let probe_cwd = directory.path().to_owned();
        let selected = macos_discovery::select_owned(vec![first, second], move |path| {
            let context = context.clone();
            let first = first_for_verify.clone();
            let probe_cwd = probe_cwd.clone();
            async move {
                cli(
                    &context,
                    &path,
                    &probe_cwd,
                    vec!["probe-output", "probe-ok"],
                )
                .await?;
                if path == first {
                    Err(ProbeFailure::Compatibility(ProtocolError::incompatible(
                        "fixture mismatch",
                    )))
                } else {
                    Ok(())
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(selected, expected);

        let database =
            rusqlite::Connection::open(directory.path().join("formal-store/agent-state.db"))
                .unwrap();
        let complete: i64 = database
            .query_row(
                "SELECT count(*) FROM runtime_instances WHERE state='terminated' AND termination_evidence_state='complete'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(complete, 2);
    }

    /// cleanup failure 必须保留 typed owner/runtime_id，并由既有 Pool 的全局 key 阻断 Workspace。
    #[tokio::test]
    async fn probe_ownership_cleanup_failure_enters_global_quarantine() {
        let directory = tempfile::tempdir().unwrap();
        let store = StateStore::open(directory.path().join("formal-store"))
            .await
            .unwrap();
        let manager = AgentTaskManager::new(store.clone(), PathBuf::new());
        let context = manager.probe_context();
        let executable = probe_fixture(directory.path());
        let runtime_id = "probe-cleanup-failure".to_string();
        let runtime = Runtime::create(
            context.store(),
            context.owner().into(),
            LaunchRequest {
                executable,
                current_dir: directory.path().to_owned(),
                args: vec!["report".into(), "done".into()],
                runtime_instance_id: runtime_id.clone(),
            },
            INIT_TIMEOUT,
        )
        .await
        .unwrap();
        let database =
            rusqlite::Connection::open(directory.path().join("formal-store/agent-state.db"))
                .unwrap();
        database
            .execute(
                "UPDATE runtime_instances SET state='terminated' WHERE id=?1",
                [&runtime_id],
            )
            .unwrap();
        let runtime = OwnedRuntimeGuard::new(
            runtime,
            RuntimeRetention::probe(context.clone(), runtime_id.clone()),
        );
        let failure = terminate_probe(runtime_id.clone(), runtime)
            .await
            .unwrap_err();
        let ProbeFailure::Runtime(failure) = failure else {
            panic!("cleanup failure must remain typed")
        };
        assert_eq!(failure.runtime_id, runtime_id);
        assert!(failure.failure.runtime.is_none());
        assert!(context.runtime_pool().retains_runtime("", &runtime_id));
        assert_eq!(
            context
                .runtime_pool()
                .check_workspace("/workspace")
                .unwrap_err(),
            "AGENT_RUNTIME_QUARANTINED"
        );

        // 恢复测试注入的状态后由原 Pool 显式收口，不留孤儿进程。
        database
            .execute(
                "UPDATE runtime_instances SET state='starting' WHERE id=?1",
                [&runtime_id],
            )
            .unwrap();
        context
            .runtime_pool()
            .retry_workspace(&store, "")
            .await
            .unwrap();
    }

    /// 重启后的 probe orphan 只委托既有 macos_recovery，不生成 Execution/Claim。
    #[tokio::test]
    async fn probe_ownership_restart_uses_existing_macos_recovery() {
        let directory = tempfile::tempdir().unwrap();
        let store = StateStore::open(directory.path().join("formal-store"))
            .await
            .unwrap();
        let executable = probe_fixture(directory.path());
        let marker = directory.path().join("probe-recovery-marker");
        let request = MacosLaunchRequest {
            executable: executable.clone(),
            current_dir: directory.path().to_owned(),
            args: vec!["ignore-tree".into(), marker.clone().into_os_string()],
            runtime_instance_id: "probe-recovery-runtime".into(),
        };
        let launched = macos_launcher::launch(&request).unwrap();
        macos_runtime_store::prepare(
            &store,
            "probe-recovery-runtime",
            "old-host",
            &executable.to_string_lossy(),
            1,
        )
        .unwrap();
        macos_runtime_store::start(&store, "probe-recovery-runtime", &launched.identity, 2)
            .unwrap();
        macos_runtime_store::initialized(
            &store,
            "probe-recovery-runtime",
            MACOS_ARM64.codex_version,
            MACOS_ARM64.protocol_schema_sha256,
            3,
        )
        .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let ready = PathBuf::from(format!("{}.ready", marker.display()));
        while !ready.exists() && std::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(ready.exists());
        // 同进程测试必须模拟系统 reaper，否则 SIGKILL 后 zombie 会污染 group-empty 观测。
        let mut child = launched.child;
        let waiter = std::thread::spawn(move || child.process.wait().unwrap());

        let summary = macos_recovery::recover_startup(&store, "new-host")
            .await
            .unwrap();
        waiter.join().unwrap();
        assert_eq!(summary.items.len(), 1);
        let row = store
            .runtime("probe-recovery-runtime".into())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.termination_evidence_state, "complete");
        let database =
            rusqlite::Connection::open(directory.path().join("formal-store/agent-state.db"))
                .unwrap();
        for table in ["executions", "workspace_claims"] {
            let count: i64 = database
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(count, 0);
        }
    }

    /// Runtime 最终 executable 必须是 canonical absolute path。
    #[test]
    fn executable_path_must_be_absolute_and_canonical() {
        assert_eq!(
            validate_executable_path(std::path::Path::new("relative/codex"))
                .unwrap_err()
                .code,
            "CODEX_EXECUTABLE_NOT_RUNNABLE"
        );
    }

    /// 真实兼容检测只导出 schema，不启动 app-server 或发送 JSON-RPC。
    #[tokio::test]
    #[ignore = "requires SERENA_CODEX_SMOKE pointing to a compatible macOS ARM64 binary"]
    async fn real_compatible_macos_arm64_schema_smoke() {
        let executable = std::env::var_os("SERENA_CODEX_SMOKE")
            .map(PathBuf::from)
            .expect("SERENA_CODEX_SMOKE must be set")
            .canonicalize()
            .unwrap();
        let state = tempfile::tempdir().unwrap();
        let store = StateStore::open(state.path().join("state")).await.unwrap();
        let manager = AgentTaskManager::new(store, PathBuf::new());
        let evidence = verify(&manager.probe_context(), executable).await.unwrap();
        assert!(!evidence.identity.version.is_empty());
        assert!(!evidence.identity.binary_sha256.is_empty());
        assert!(!evidence.identity.protocol_schema_sha256.is_empty());
    }

    /// 正式 connect 仍执行 initialize，并验证完整 Runtime 生命周期收口。
    #[tokio::test]
    #[ignore = "requires SERENA_CODEX_SMOKE pointing to a compatible macOS ARM64 binary"]
    async fn real_compatible_macos_arm64_lifecycle_smoke() {
        let executable = std::env::var_os("SERENA_CODEX_SMOKE")
            .map(PathBuf::from)
            .expect("SERENA_CODEX_SMOKE must be set")
            .canonicalize()
            .unwrap();
        let workspace = tempfile::tempdir().unwrap();
        std::fs::set_permissions(workspace.path(), std::fs::Permissions::from_mode(0o555)).unwrap();
        let state = tempfile::tempdir().unwrap();
        let store = StateStore::open(state.path().join("state")).await.unwrap();
        let manager = AgentTaskManager::new(store.clone(), PathBuf::new());
        let probe_context = manager.probe_context();
        let runtime_id = format!("macos-real-smoke-{}", std::process::id());
        let managed = connect(
            probe_context.clone(),
            store.clone(),
            probe_context.owner().into(),
            runtime_id.clone(),
            executable,
            workspace.path().to_owned(),
            None,
        )
        .await
        .unwrap();
        assert!(!managed.compatibility.identity.version.is_empty());
        managed.shutdown().await.unwrap();
        let record = store.runtime(runtime_id).await.unwrap().unwrap();
        assert_eq!(record.state, "terminated");
        assert_eq!(record.runtime_platform, "macos");
        assert_eq!(record.termination_evidence_state, "complete");
        assert_eq!(
            record.termination_evidence_type.as_deref(),
            Some("macos_live_process_group_empty")
        );
        let database =
            rusqlite::Connection::open(state.path().join("state/agent-state.db")).unwrap();
        let incomplete: i64 = database
            .query_row(
                "SELECT count(*) FROM runtime_instances WHERE owner_host_instance_id != ?1 OR state != 'terminated' OR termination_evidence_state != 'complete'",
                [probe_context.owner()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(incomplete, 0);
    }

    /// 真实不兼容 schema 候选必须经过同一 verifier 并收口为 COMPATIBILITY BLOCKED。
    #[tokio::test]
    #[ignore = "requires SERENA_CODEX_INCOMPATIBLE_SMOKE pointing to an incompatible ARM64 Codex"]
    async fn real_incompatible_candidate_is_compatibility_blocked() {
        let executable = std::env::var_os("SERENA_CODEX_INCOMPATIBLE_SMOKE")
            .map(PathBuf::from)
            .expect("SERENA_CODEX_INCOMPATIBLE_SMOKE must be set")
            .canonicalize()
            .unwrap();
        let state = tempfile::tempdir().unwrap();
        let store = StateStore::open(state.path().join("state")).await.unwrap();
        let manager = AgentTaskManager::new(store, PathBuf::new());
        let probe_context = manager.probe_context();
        let result = macos_discovery::select_owned(vec![executable], move |path| {
            let probe_context = probe_context.clone();
            async move { verify(&probe_context, path).await.map(|_| ()) }
        })
        .await;
        assert!(matches!(
            result,
            Err(macos_discovery::SelectionFailure::Discovery(
                macos_discovery::DiscoveryError {
                    code: "CODEX_COMPATIBILITY_BLOCKED",
                    ..
                }
            ))
        ));
    }
}
