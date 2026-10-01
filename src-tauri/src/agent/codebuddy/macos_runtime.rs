//! CodeBuddy ACP 的 macOS 平台适配；进程身份及终止算法复用 Codex containment。
use super::{
    client::{Handshake, ManagedClient},
    macos_launcher::LaunchRequest,
    protocol::{Failure, Limits, StderrTail},
};
use crate::agent::{
    codex::{
        macos_launcher::{CreatedChild, process_group_members},
        macos_runtime::MacosRuntime,
    },
    coordinator::now,
    store::{StateStore, codebuddy_runtime::CodeBuddyRuntimeUpdate},
};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, LazyLock, Mutex},
    time::Duration,
};
use tokio::{io::AsyncReadExt, task::JoinHandle};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

#[path = "macos_version.rs"]
mod version;
pub(super) use version::probe_product_version;

/// 与 Codex Pool 一致保留失败 ownership；未确认终止前同一 Workspace 禁止新进程。
static QUARANTINE: LazyLock<Mutex<HashMap<PathBuf, Vec<RetainedOwnership>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// 字段本身持有 child/fd；生产路径只保留，不自动重试、不生成 termination evidence。
#[allow(dead_code)]
enum RetainedOwnership {
    Managed(MacosRuntime),
    Created(CreatedChild),
}

/// 保留未知身份的 spawn 结果；只有直接 child 已回收且原组为空才可丢弃句柄。
fn retain_created(workspace: PathBuf, mut child: CreatedChild) {
    if child.process.try_wait().ok().flatten().is_some()
        && process_group_members(child.pgid).is_ok_and(|members| members.is_empty())
    {
        return;
    }
    QUARANTINE
        .lock()
        .unwrap()
        .entry(workspace)
        .or_default()
        .push(RetainedOwnership::Created(child));
}

/// 即使启动 future 被取消，也由唯一 owner 持有完整 containment。
struct Owner {
    core: Mutex<Option<MacosRuntime>>,
    workspace: PathBuf,
    durable: Option<(StateStore, String)>,
    outcome: Mutex<Option<Result<(), Failure>>>,
    cleanup_on_drop: bool,
}
impl Owner {
    /// 只能在 blocking worker 中调用；真实 group-empty 后才提交 Provider evidence。
    fn cleanup(&self) -> Result<(), Failure> {
        let mut slot = self.core.lock().unwrap();
        let Some(core) = slot.take() else {
            return self.outcome.lock().unwrap().unwrap_or(Ok(()));
        };
        let original = if let Some((store, id)) = &self.durable {
            tauri::async_runtime::block_on(store.runtime(id.clone()))
                .ok()
                .flatten()
        } else {
            None
        };
        match core.shutdown(Duration::from_millis(500), Duration::from_secs(2)) {
            Ok(evidence) => {
                let result = (|| {
                    if let Some((store, _)) = &self.durable {
                        let original = original.ok_or(Failure::Cleanup)?;
                        let proof = super::macos_recovery::live_evidence(original, evidence)
                            .map_err(|_| Failure::Cleanup)?;
                        tauri::async_runtime::block_on(store.complete_codebuddy_runtime(proof))
                            .map_err(|_| Failure::Cleanup)?;
                    }
                    Ok(())
                })();
                if result.is_err()
                    && let Some((store, id)) = &self.durable
                {
                    let _ = tauri::async_runtime::block_on(store.update_codebuddy_runtime(
                        id.clone(),
                        CodeBuddyRuntimeUpdate::Unknown,
                        now(),
                    ));
                }
                *self.outcome.lock().unwrap() = Some(result);
                result
            }
            Err(error) => {
                if let Some((store, id)) = &self.durable {
                    let _ = tauri::async_runtime::block_on(store.update_codebuddy_runtime(
                        id.clone(),
                        CodeBuddyRuntimeUpdate::Unknown,
                        now(),
                    ));
                }
                *slot = Some(*error.runtime);
                Err(Failure::Cleanup)
            }
        }
    }
}
impl Drop for Owner {
    /// Drop 不在 Tokio worker 上 block_on；将剩余 ownership 交给独立收口线程。
    fn drop(&mut self) {
        if !self.cleanup_on_drop {
            if let Some(core) = self.core.get_mut().unwrap().take() {
                QUARANTINE
                    .lock()
                    .unwrap()
                    .entry(self.workspace.clone())
                    .or_default()
                    .push(RetainedOwnership::Managed(core));
            }
            return;
        }
        if let Some(core) = self.core.get_mut().unwrap().take() {
            let cleanup = Self {
                core: Mutex::new(Some(core)),
                workspace: self.workspace.clone(),
                durable: self.durable.clone(),
                outcome: Mutex::new(None),
                cleanup_on_drop: false,
            };
            std::thread::spawn(move || {
                let _ = cleanup.cleanup();
            });
        }
    }
}

/// ACP 资源与 containment 分离；业务层只持有 ManagedClient。
pub(crate) struct Runtime {
    pub(crate) client: Option<ManagedClient>,
    owner: Arc<Owner>,
    monitor: Option<JoinHandle<()>>,
    stderr: Option<JoinHandle<()>>,
}
impl Runtime {
    /// 返回当前真实 owner 的 durable ID。
    pub(super) fn runtime_id(&self) -> Option<&str> {
        self.owner.durable.as_ref().map(|(_, id)| id.as_str())
    }

    /// catalog 使用同一个受管 Runtime，但不创建 durable Execution/Claim。
    pub(crate) async fn start(
        request: LaunchRequest,
        limits: Limits,
    ) -> Result<(Self, Handshake), Failure> {
        Self::start_owned(request, limits, None).await
    }

    /// 私有任务保证 caller drop 不会中断 spawn 与身份写入之间的 ownership。
    pub(crate) async fn start_persisted(
        request: LaunchRequest,
        limits: Limits,
        store: StateStore,
        id: String,
    ) -> Result<(Self, Handshake), Failure> {
        if request.runtime_instance_id() != id {
            return Err(Failure::Launch);
        }
        tokio::spawn(async move {
            let record = store
                .runtime(id.clone())
                .await
                .map_err(|_| Failure::Cleanup)?
                .ok_or(Failure::Cleanup)?;
            if record.provider != "codebuddy"
                || record.state != "preparing"
                || record.runtime_platform != "macos"
                || !store
                    .codebuddy_runtime_binding_valid(id.clone())
                    .await
                    .map_err(|_| Failure::Cleanup)?
            {
                return Err(Failure::Cleanup);
            }
            Self::start_owned(request, limits, Some((store, id))).await
        })
        .await
        .map_err(|_| Failure::Cleanup)?
    }

    /// 在 blocking spawn 返回之前建立 RAII owner，避免 JoinHandle 被取消造成裸 child。
    async fn start_owned(
        request: LaunchRequest,
        limits: Limits,
        durable: Option<(StateStore, String)>,
    ) -> Result<(Self, Handshake), Failure> {
        let (owner, stdin, stdout, stderr) = tokio::task::spawn_blocking(move || {
            let workspace = request.inner.current_dir.clone();
            if QUARANTINE.lock().unwrap().contains_key(&workspace) {
                return Err(Failure::Cleanup);
            }
            let core = match MacosRuntime::create_external(request.inner, &request.path) {
                Ok(core) => core,
                Err(error) => {
                    if let Some(child) = error.created {
                        retain_created(workspace, *child);
                    }
                    if let Some((store, id)) = &durable {
                        let _ = tauri::async_runtime::block_on(store.update_codebuddy_runtime(
                            id.clone(),
                            CodeBuddyRuntimeUpdate::Unknown,
                            now(),
                        ));
                    }
                    return Err(Failure::Launch);
                }
            };
            let owner = Arc::new(Owner {
                core: Mutex::new(Some(core)),
                workspace,
                durable,
                outcome: Mutex::new(None),
                cleanup_on_drop: true,
            });
            let files = {
                let slot = owner.core.lock().unwrap();
                let core = slot.as_ref().unwrap();
                if let Some((store, id)) = &owner.durable {
                    let identity = core.process_identity();
                    tauri::async_runtime::block_on(store.update_codebuddy_runtime(
                        id.clone(),
                        CodeBuddyRuntimeUpdate::MacosProcessStarted {
                            pid: identity.pid as u32,
                            start_token: identity.start_token.encode(),
                        },
                        now(),
                    ))
                    .map_err(|_| Failure::Cleanup)?;
                }
                core.clone_stdio().map_err(|_| Failure::Io)?
            };
            Ok((owner, files.0, files.1, files.2))
        })
        .await
        .map_err(|_| Failure::Launch)??;
        let tail = Arc::new(Mutex::new(StderrTail::new(limits.stderr_bytes)));
        let mut stderr = tokio::fs::File::from_std(stderr);
        let drain = tokio::spawn(async move {
            let mut bytes = [0; 4096];
            while let Ok(count) = stderr.read(&mut bytes).await {
                if count == 0 {
                    break;
                }
                tail.lock().unwrap().push(&bytes[..count]);
            }
        });
        let mut runtime = Self {
            client: None,
            owner,
            monitor: None,
            stderr: Some(drain),
        };
        let client = match ManagedClient::connect(
            tokio::fs::File::from_std(stdin).compat_write(),
            tokio::fs::File::from_std(stdout).compat(),
            limits,
        )
        .await
        {
            Ok(client) => client,
            Err(error) => {
                runtime.shutdown().await?;
                return Err(error);
            }
        };
        let mut stopped = client.requests.shared.stop.subscribe();
        let monitor_owner = runtime.owner.clone();
        runtime.monitor = Some(tokio::spawn(async move {
            let _ = stopped.wait_for(|failure| failure.is_some()).await;
            let _ = tokio::task::spawn_blocking(move || monitor_owner.cleanup()).await;
        }));
        runtime.client = Some(client);
        let result = runtime.client.as_ref().unwrap().requests.initialize().await;
        match result {
            Ok(handshake) => {
                if let Some((store, id)) = &runtime.owner.durable
                    && store
                        .update_codebuddy_runtime(
                            id.clone(),
                            CodeBuddyRuntimeUpdate::Initialized,
                            now(),
                        )
                        .await
                        .is_err()
                {
                    runtime.shutdown().await?;
                    return Err(Failure::Cleanup);
                }
                Ok((runtime, handshake))
            }
            Err(error) => {
                runtime.shutdown().await?;
                Err(error)
            }
        }
    }

    /// 先收口整组解开 pipe，再等待 ACP 和 stderr 有界结束。
    pub(crate) async fn shutdown(mut self) -> Result<(), Failure> {
        let owner = self.owner.clone();
        let mut result = tokio::task::spawn_blocking(move || owner.cleanup())
            .await
            .map_err(|_| Failure::Cleanup)?;
        if let Some(client) = self.client.take() {
            client.shutdown().await;
        }
        for task in [self.monitor.take(), self.stderr.take()]
            .into_iter()
            .flatten()
        {
            result = finish_task(task).await.and(result);
        }
        result
    }
}
impl Drop for Runtime {
    /// caller drop 唤醒 ACP 并由独立 blocking owner 完成同一收口。
    fn drop(&mut self) {
        if let Some(client) = &self.client {
            client.requests.shared.fail(Failure::Closed);
        }
        let owner = self.owner.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let _ = owner.cleanup();
        });
    }
}
/// pipe/drain 未按时结束时保留失败，禁止把 timeout 当成功。
async fn finish_task(mut task: JoinHandle<()>) -> Result<(), Failure> {
    match tokio::time::timeout(Duration::from_secs(2), &mut task).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(_)) => Err(Failure::Io),
        Err(_) => {
            task.abort();
            Err(Failure::Cleanup)
        }
    }
}

#[cfg(test)]
#[path = "macos_runtime_tests.rs"]
mod tests;
