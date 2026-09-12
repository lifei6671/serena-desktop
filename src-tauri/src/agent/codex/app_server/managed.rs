//! Windows ownership bridge. Blocking pipe/DB/Win32 work never runs on async workers.
use super::*;
use crate::agent::{
    codex::{
        runtime::{Runtime, RuntimeError, RuntimeFailure},
        windows_launcher::{self, CreatedChild, LaunchRequest},
    },
    store::StateStore,
};
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io::Read,
    os::windows::{fs::OpenOptionsExt, io::AsRawHandle},
    path::{Path, PathBuf},
};
use windows_sys::Win32::{
    Storage::FileSystem::FILE_SHARE_READ,
    System::{JobObjects::*, Threading::*},
};

pub struct CompatibilityEvidence {
    pub identity: CompatibilityIdentity,
    pub executable: PathBuf,
    pub source_commit: &'static str,
    pub wire_contract: &'static str,
    // Prevent path replacement/write between hashing, CLI export and launch.
    _binary: File,
}
fn io_error(e: impl std::fmt::Display) -> ProtocolError {
    ProtocolError::incompatible(e.to_string())
}
fn digest(mut file: File) -> Result<String> {
    let mut hash = Sha256::new();
    let mut buf = [0; 64 * 1024];
    loop {
        let n = file.read(&mut buf).map_err(io_error)?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
    }
    Ok(hash.finalize().iter().map(|b| format!("{b:02X}")).collect())
}
fn terminate_probe(child: &CreatedChild) -> Result<()> {
    let job = child.job.as_raw_handle();
    if unsafe { TerminateJobObject(job, 0) } == 0 {
        return Err(io_error(std::io::Error::last_os_error()));
    }
    let end = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { std::mem::zeroed() };
        if unsafe {
            QueryInformationJobObject(
                job,
                JobObjectBasicAccountingInformation,
                (&mut info as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                std::mem::size_of_val(&info) as u32,
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(io_error(std::io::Error::last_os_error()));
        }
        if info.ActiveProcesses == 0 {
            return Ok(());
        }
        if std::time::Instant::now() >= end {
            return Err(io_error(
                "Compatibility probe Job termination not confirmed",
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn cli(executable: &Path, cwd: &Path, args: Vec<String>) -> Result<String> {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let request = LaunchRequest {
        executable: executable.into(),
        current_dir: cwd.into(),
        args: args.into_iter().map(Into::into).collect(),
        runtime_instance_id: format!(
            "contract-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ),
    };
    let launched = match windows_launcher::launch(&request) {
        Ok(v) => v,
        Err(mut e) => {
            if let Some(child) = e.created.take() {
                terminate_probe(&child)?;
            }
            return Err(io_error(format!("{}: Win32 {}", e.code, e.win32_error)));
        }
    };
    let child = launched.child;
    let result = (|| {
        let out = child.stdout.try_clone().map_err(io_error)?;
        let err = child.stderr.try_clone().map_err(io_error)?;
        let (tx, rx) = std::sync::mpsc::sync_channel(2);
        for (is_out, mut file) in [(true, out), (false, err)] {
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut bytes = Vec::new();
                let r = file
                    .by_ref()
                    .take(MAX_MESSAGE as u64)
                    .read_to_end(&mut bytes);
                let _ = tx.send((is_out, r.map(|_| bytes)));
            });
        }
        let end = std::time::Instant::now() + INIT_TIMEOUT;
        let mut output = Vec::new();
        for _ in 0..2 {
            let (is_out, bytes) = rx
                .recv_timeout(end.saturating_duration_since(std::time::Instant::now()))
                .map_err(io_error)?;
            let bytes = bytes.map_err(io_error)?;
            if bytes.len() == MAX_MESSAGE {
                return Err(io_error("CLI output exceeded bound"));
            }
            if is_out {
                output = bytes;
            }
        }
        // Main-process wait is only for this diagnostic CLI exit code, never Job evidence.
        unsafe {
            WaitForSingleObject(child.process.as_raw_handle(), 1000);
        }
        let mut exit = 0;
        if unsafe { GetExitCodeProcess(child.process.as_raw_handle(), &mut exit) } == 0 || exit != 0
        {
            return Err(io_error(format!("Contract CLI failed, exit={exit}")));
        }
        String::from_utf8(output).map_err(io_error)
    })();
    let cleanup = terminate_probe(&child);
    cleanup?;
    result
}
pub async fn verify(executable: PathBuf) -> Result<CompatibilityEvidence> {
    tokio::task::spawn_blocking(move || {
        if !executable.is_absolute() {
            return Err(io_error("Absolute executable required"));
        }
        let binary = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(&executable)
            .map_err(io_error)?;
        let hash = digest(binary.try_clone().map_err(io_error)?)?;
        if hash != BINARY_SHA256 {
            return Err(io_error("Binary hash is not whitelisted"));
        }
        let temp = tempfile::tempdir().map_err(io_error)?;
        let version = cli(&executable, temp.path(), vec!["--version".into()])?
            .trim()
            .to_owned();
        let schema_dir = temp.path().join("schema");
        cli(
            &executable,
            temp.path(),
            vec![
                "app-server".into(),
                "generate-json-schema".into(),
                "--experimental".into(),
                "--out".into(),
                schema_dir
                    .to_str()
                    .ok_or_else(|| io_error("Schema path is not Unicode"))?
                    .into(),
            ],
        )?;
        let schema = digest(
            File::open(schema_dir.join("codex_app_server_protocol.schemas.json"))
                .map_err(io_error)?,
        )?;
        let identity = CompatibilityIdentity {
            version,
            binary_sha256: hash,
            protocol_schema_sha256: schema,
        };
        identity.check()?;
        Ok(CompatibilityEvidence {
            identity,
            executable,
            source_commit: SOURCE_COMMIT,
            wire_contract: WIRE_CONTRACT,
            _binary: binary,
        })
    })
    .await
    .map_err(io_error)?
}
/// Completion retains RuntimeFailure ownership if Job evidence cannot be confirmed.
/// Dropping Client still wakes this independent monitor and reconciles the old Job.
pub struct ManagedClient {
    pub client: Client,
    pub compatibility: CompatibilityEvidence,
    reconciliation: tokio::task::JoinHandle<std::result::Result<(), RuntimeFailure>>,
}
impl ManagedClient {
    #[cfg(test)]
    pub(crate) fn test_owned(client: Client, reconciliation: tokio::task::JoinHandle<std::result::Result<(), RuntimeFailure>>) -> Self {
        Self { client, reconciliation, compatibility: CompatibilityEvidence {
            identity: CompatibilityIdentity { version: VERSION.into(), binary_sha256: BINARY_SHA256.into(), protocol_schema_sha256: SCHEMA_SHA256.into() },
            executable: PathBuf::from("fixture"), source_commit: SOURCE_COMMIT, wire_contract: WIRE_CONTRACT, _binary: tempfile::tempfile().unwrap(),
        } }
    }

    pub async fn shutdown(self) -> std::result::Result<(), RuntimeFailure> {
        self.client.cancel();
        self.wait_for_reconciliation().await
    }
    pub async fn wait_for_reconciliation(self) -> std::result::Result<(), RuntimeFailure> {
        self.reconciliation
            .await
            .map_err(|e| runtime_error(io_error(e)))?
    }
}
/// Retains cleanup responsibility even while buffered in the handoff channel.
pub(super) struct RuntimeHandoff(Option<std::result::Result<Runtime, RuntimeFailure>>);
impl RuntimeHandoff {
    fn take(mut self) -> std::result::Result<Runtime, RuntimeFailure> {
        self.0.take().unwrap()
    }
}
impl Drop for RuntimeHandoff {
    fn drop(&mut self) {
        let runtime = match self.0.take() {
            Some(Ok(runtime)) => Some(runtime),
            Some(Err(failure)) => failure.runtime.map(|runtime| *runtime),
            None => None,
        };
        if let Some(runtime) = runtime {
            // Host shutdown remains TASK-008. During this live executor, a
            // cancelled receiver transfers ownership to explicit Job convergence.
            tokio::spawn(async move {
                let _ = runtime.terminate(INIT_TIMEOUT).await;
            });
        }
    }
}
/// The blocking creation worker cannot be cancelled; its result always has an owner.
pub(super) fn handoff_creation(
    create: impl std::future::Future<Output = std::result::Result<Runtime, RuntimeFailure>>
    + Send
    + 'static,
) -> oneshot::Receiver<RuntimeHandoff> {
    let (sender, receiver) = oneshot::channel();
    tokio::spawn(async move {
        let result = RuntimeHandoff(Some(create.await));
        // On failure the envelope drops here; on success its Drop contract
        // also covers cancellation while queued, before the receiver polls it.
        let _ = sender.send(result);
    });
    receiver
}
pub(crate) enum RuntimeAttempt {
    Dispatch(String),
    Recovery(String),
}

pub async fn connect(
    store: StateStore,
    owner: String,
    runtime_id: String,
    executable: PathBuf,
    cwd: PathBuf,
    attempt: Option<RuntimeAttempt>,
) -> std::result::Result<ManagedClient, RuntimeFailure> {
    let compatibility = verify(executable.clone()).await.map_err(runtime_error)?;
    // Compatibility probes use an isolated temporary cwd and precede the actual
    // managed Runtime attempt. Commit origin identity before Runtime::create can
    // persist its row or enter CreateProcess; never wait for Execution binding.
    let now = crate::agent::coordinator::now();
    match attempt {
        Some(RuntimeAttempt::Dispatch(id)) => store.reserve_runtime_attempt(id, runtime_id.clone(), now).await,
        Some(RuntimeAttempt::Recovery(id)) => store.reserve_recovery_attempt(id, runtime_id.clone(), now).await,
        None => Ok(()), // Protocol-only integration tests have no Execution.
    }.map_err(|e| RuntimeError::new("CODEX_RUNTIME_STORE_FAILED", e))?;
    let runtime = handoff_creation(Runtime::create(
        store,
        owner,
        LaunchRequest {
            executable,
            current_dir: cwd,
            args: vec!["app-server".into(), "--listen".into(), "stdio://".into()],
            runtime_instance_id: runtime_id.clone(),
        },
        INIT_TIMEOUT,
    ))
    .await
    .map_err(|e| runtime_error(io_error(e)))?
    .take()?;
    let pipes = runtime.clone_stdio();
    let (stdin, stdout, stderr) = match pipes {
        Ok(v) => v,
        Err(e) => {
            runtime.terminate(INIT_TIMEOUT).await?;
            return Err(runtime_error(io_error(e)).into());
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
    // The monitor owns the Runtime before the first await in initialize. Cancelling
    // connect drops Client and wakes this monitor; no created ownership is lost.
    let identity = compatibility.identity.clone();
    let mut failure = client.failure();
    let (ready_tx, ready_rx) = oneshot::channel::<()>();
    let (persist_tx, persist_rx) = oneshot::channel();
    let reconciliation = tokio::spawn(async move {
        let initialize = tokio::select! {
            biased;
            _ = async { loop { if failure.borrow().is_some() {break;}
                if failure.changed().await.is_err(){break;} } } => false,
            result = ready_rx => result.is_ok(),
        };
        if initialize {
            let persisted = runtime.initialized(&identity).await;
            let ok = persisted.is_ok();
            let _ = persist_tx.send(persisted);
            if ok {
                loop {
                    if failure.borrow().is_some() {
                        break;
                    }
                    if failure.changed().await.is_err() {
                        break;
                    }
                }
            }
        }
        runtime.terminate(INIT_TIMEOUT).await
    });
    if let Err(error) = client.initialize().await {
        drop(ready_tx);
        reconciliation
            .await
            .map_err(|e| runtime_error(io_error(e)))??;
        return Err(runtime_error(error).into());
    }
    if ready_tx.send(()).is_err() {
        client.cancel();
        reconciliation
            .await
            .map_err(|e| runtime_error(io_error(e)))??;
        return Err(runtime_error(io_error(
            "Runtime reconciliation began during initialization",
        ))
        .into());
    }
    let persisted = persist_rx
        .await
        .map_err(|_| runtime_error(io_error("Runtime initialization worker closed")));
    if !matches!(persisted, Ok(Ok(()))) {
        client.cancel();
        reconciliation
            .await
            .map_err(|e| runtime_error(io_error(e)))??;
        return Err(match persisted {
            Ok(Err(e)) | Err(e) => e.into(),
            _ => unreachable!(),
        });
    }
    Ok(ManagedClient {
        client,
        compatibility,
        reconciliation,
    })
}
fn runtime_error(e: ProtocolError) -> RuntimeError {
    RuntimeError::new(e.code, e.message)
}
