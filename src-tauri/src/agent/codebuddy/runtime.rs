//! Windows 受管 process + initialize ownership；durable cleanup 使用 CB6-005 Job evidence。

use super::{
    client::{Handshake, ManagedClient},
    protocol::{Failure, Limits, StderrTail},
    windows_launcher::{self, CreatedChild, LaunchError, LaunchRequest},
};
use crate::agent::{
    coordinator::now,
    store::{StateStore, codebuddy_runtime::CodeBuddyRuntimeUpdate},
};
use std::{
    os::windows::io::{AsRawHandle, OwnedHandle},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{io::AsyncReadExt, task::JoinHandle};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use windows_sys::Win32::System::JobObjects::TerminateJobObject;

/// Job owner 与管道解耦，保证任意失败和 future drop 都关闭整棵进程树。
struct JobOwner {
    job: Mutex<Option<OwnedHandle>>,
    _process: OwnedHandle,
}
impl JobOwner {
    /// TerminateJobObject 后始终 close；KILL_ON_JOB_CLOSE 是 CB6-002 已验证的内核策略。
    fn terminate(&self) {
        if let Some(job) = self.job.lock().unwrap().take() {
            // SAFETY: 唯一 owner 在调用期间保持 Job handle 有效；失败仍执行 close-on-drop。
            unsafe {
                TerminateJobObject(job.as_raw_handle(), 1);
            }
            drop(job);
        }
    }
}
impl Drop for JobOwner {
    /// 包括 initialize future 被外部取消的路径，不依赖主 PID。
    fn drop(&mut self) {
        self.terminate();
    }
}

/// 持久化 owner 只在关闭 Job 后尝试证据，不释放 execution/claim。
struct DurableRuntime {
    store: StateStore,
    id: String,
    completed: bool,
}
impl DurableRuntime {
    /// 不确定证据明确返回错误，并保留 Store 的恢复入口。
    async fn cleanup(&mut self) -> Result<(), Failure> {
        let result =
            super::recovery::recover(&self.store, self.id.clone(), Duration::from_secs(10)).await;
        self.completed = true;
        result.map_err(|_| Failure::Cleanup)
    }
}
impl Drop for DurableRuntime {
    /// drop 无法 await，调度同一恢复逻辑；Job 由外层同步先关闭。
    fn drop(&mut self) {
        if self.completed {
            return;
        }
        let store = self.store.clone();
        let id = self.id.clone();
        tauri::async_runtime::spawn(async move {
            let _ = super::recovery::recover(&store, id, Duration::from_secs(10)).await;
        });
    }
}

/// Runtime 仅表示当前进程和握手资源，不表示可恢复/可执行的 Provider acceptance。
pub(crate) struct Runtime {
    pub(crate) client: Option<ManagedClient>,
    owner: Arc<JobOwner>,
    durable: Option<DurableRuntime>,
    monitor: Option<JoinHandle<()>>,
    stderr: Option<JoinHandle<()>>,
    _tail: Arc<Mutex<StderrTail>>,
}

impl Runtime {
    /// 返回当前实际受管 owner 的 durable R1，不能由调用方 private 快照替代。
    pub(super) fn runtime_id(&self) -> Option<&str> {
        self.durable.as_ref().map(|runtime| runtime.id.as_str())
    }

    /// launcher 保留 first-runnable Job ownership，SDK 只获得已创建的 streams。
    pub(crate) async fn start(
        request: LaunchRequest,
        limits: Limits,
    ) -> Result<(Self, Handshake), Failure> {
        let launched = tokio::task::spawn_blocking(move || windows_launcher::launch(&request))
            .await
            .map_err(|_| Failure::Launch)?;
        let child = match launched {
            Ok(launched) => launched.child,
            Err(error) => {
                cleanup_launch_error(error);
                return Err(Failure::Launch);
            }
        };
        Self::from_child(child, limits).await
    }

    /// 私有 ownership task 不受 caller 取消影响，禁止 launch 未结束就生成 destroyed 证据。
    pub(crate) async fn start_persisted(
        request: LaunchRequest,
        limits: Limits,
        store: StateStore,
        runtime_id: String,
    ) -> Result<(Self, Handshake), Failure> {
        if request.runtime_instance_id() != runtime_id {
            return Err(Failure::Launch);
        }
        tokio::spawn(async move {
            let record = store
                .runtime(runtime_id.clone())
                .await
                .map_err(|_| Failure::Cleanup)?
                .ok_or(Failure::Cleanup)?;
            if record.provider != "codebuddy"
                || record.state != "preparing"
                || record.job_policy_verified_at.is_some()
                || !store
                    .codebuddy_runtime_binding_valid(runtime_id.clone())
                    .await
                    .map_err(|_| Failure::Cleanup)?
            {
                return Err(Failure::Cleanup);
            }
            let mut durable = DurableRuntime {
                store,
                id: runtime_id,
                completed: false,
            };
            let launched = tokio::task::spawn_blocking(move || windows_launcher::launch(&request))
                .await
                .map_err(|_| Failure::Launch)?;
            let launched = match launched {
                Ok(child) => child,
                Err(error) => {
                    cleanup_launch_error(error);
                    // live policy 尚未验证时只能留下 unknown，不伪造 successful evidence。
                    durable.cleanup().await?;
                    return Err(Failure::Launch);
                }
            };
            let token = launched.process_start_token();
            Self::from_child_owned(launched.child, limits, Some((durable, token))).await
        })
        .await
        .map_err(|_| Failure::Cleanup)?
    }

    /// 接管 CreatedChild 后立刻建立 RAII Job owner，任何 await 之前 ownership 已闭合。
    async fn from_child(child: CreatedChild, limits: Limits) -> Result<(Self, Handshake), Failure> {
        Self::from_child_owned(child, limits, None).await
    }

    /// 在持久化 await 前建立完整 Runtime owner，所有失败共用 shutdown 路径。
    async fn from_child_owned(
        child: CreatedChild,
        limits: Limits,
        persisted: Option<(DurableRuntime, String)>,
    ) -> Result<(Self, Handshake), Failure> {
        let CreatedChild {
            pid,
            stdin,
            stdout,
            stderr,
            process,
            job,
        } = child;
        let owner = Arc::new(JobOwner {
            job: Mutex::new(Some(job)),
            _process: process,
        });
        let tail = Arc::new(Mutex::new(StderrTail::new(limits.stderr_bytes)));
        let drain_tail = tail.clone();
        let mut stderr = tokio::fs::File::from_std(stderr);
        let drain = tokio::spawn(async move {
            let mut buffer = [0; 4096];
            while let Ok(count) = stderr.read(&mut buffer).await {
                if count == 0 {
                    break;
                }
                drain_tail.lock().unwrap().push(&buffer[..count]);
            }
        });
        let (durable, start_token) = match persisted {
            Some((durable, token)) => (Some(durable), Some(token)),
            None => (None, None),
        };
        let mut runtime = Self {
            client: None,
            owner,
            durable,
            monitor: None,
            stderr: Some(drain),
            _tail: tail,
        };
        if let Some(durable) = &runtime.durable {
            let saved = async {
                durable
                    .store
                    .update_codebuddy_runtime(
                        durable.id.clone(),
                        CodeBuddyRuntimeUpdate::PolicyVerified,
                        now(),
                    )
                    .await?;
                durable
                    .store
                    .update_codebuddy_runtime(
                        durable.id.clone(),
                        CodeBuddyRuntimeUpdate::ProcessStarted {
                            pid,
                            start_token: start_token.unwrap(),
                        },
                        now(),
                    )
                    .await
            }
            .await;
            if saved.is_err() {
                runtime.shutdown().await?;
                return Err(Failure::Cleanup);
            }
        }
        let client = ManagedClient::connect(
            tokio::fs::File::from_std(stdin).compat_write(),
            tokio::fs::File::from_std(stdout).compat(),
            limits,
        )
        .await;
        let client = match client {
            Ok(client) => client,
            Err(error) => {
                runtime.shutdown().await?;
                return Err(error);
            }
        };
        let mut stopped = client.requests.shared.stop.subscribe();
        let monitor_owner = runtime.owner.clone();
        let monitor = tokio::spawn(async move {
            let _ = stopped.wait_for(|failure| failure.is_some()).await;
            monitor_owner.terminate();
        });
        runtime.client = Some(client);
        runtime.monitor = Some(monitor);
        match runtime.client.as_ref().unwrap().requests.initialize().await {
            Ok(handshake) => {
                if let Some(durable) = &runtime.durable
                    && durable
                        .store
                        .update_codebuddy_runtime(
                            durable.id.clone(),
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

    /// 先整 Job 终止，解开同步 Win32 pipe read，再等待 SDK/drain/monitor 结束。
    pub(crate) async fn shutdown(mut self) -> Result<(), Failure> {
        self.owner.terminate();
        if let Some(client) = self.client.take() {
            client.shutdown().await;
        }
        let mut result = Ok(());
        if let Some(monitor) = self.monitor.take() {
            result = finish_task(monitor).await;
        }
        if let Some(stderr) = self.stderr.take() {
            result = finish_task(stderr).await.and(result);
        }
        if let Some(mut durable) = self.durable.take() {
            result = durable.cleanup().await.and(result);
        }
        result
    }
}

/// 受管 Job 已关闭后 drain/monitor 仍不结束时明确报告 cleanup failure，不永久等待。
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
impl Drop for Runtime {
    /// drop 先同步关闭整 Job，再由 durable guard 尝试证据；不释放 Claim。
    fn drop(&mut self) {
        if let Some(client) = &self.client {
            client.requests.shared.fail(Failure::Closed);
        }
        self.owner.terminate();
    }
}

/// CreateProcessW 已成功但 acquisition 失败时，也消费完整 CreatedChild owner。
fn cleanup_launch_error(error: LaunchError) {
    if let Some(child) = error.created {
        let CreatedChild { process, job, .. } = *child;
        JobOwner {
            job: Mutex::new(Some(job)),
            _process: process,
        }
        .terminate();
    }
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;
