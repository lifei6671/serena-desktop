//! Windows 受管 process + initialize ownership；不包含持久化、恢复或产品 session。

use super::{
    client::{Handshake, ManagedClient},
    protocol::{Failure, Limits, StderrTail},
    windows_launcher::{self, CreatedChild, LaunchError, LaunchRequest},
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

/// Runtime 仅表示当前进程和握手资源，不表示可恢复/可执行的 Provider acceptance。
pub(crate) struct Runtime {
    pub(crate) client: Option<ManagedClient>,
    owner: Arc<JobOwner>,
    monitor: Option<JoinHandle<()>>,
    stderr: Option<JoinHandle<()>>,
    _tail: Arc<Mutex<StderrTail>>,
}

impl Runtime {
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

    /// 接管 CreatedChild 后立刻建立 RAII Job owner，任何 await 之前 ownership 已闭合。
    async fn from_child(child: CreatedChild, limits: Limits) -> Result<(Self, Handshake), Failure> {
        let CreatedChild {
            stdin,
            stdout,
            stderr,
            process,
            job,
            ..
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
        let client = ManagedClient::connect(
            tokio::fs::File::from_std(stdin).compat_write(),
            tokio::fs::File::from_std(stdout).compat(),
            limits,
        )
        .await;
        let client = match client {
            Ok(client) => client,
            Err(error) => {
                owner.terminate();
                finish_task(drain).await?;
                return Err(error);
            }
        };
        let mut stopped = client.requests.shared.stop.subscribe();
        let monitor_owner = owner.clone();
        let monitor = tokio::spawn(async move {
            let _ = stopped.wait_for(|failure| failure.is_some()).await;
            monitor_owner.terminate();
        });
        let runtime = Self {
            client: Some(client),
            owner,
            monitor: Some(monitor),
            stderr: Some(drain),
            _tail: tail,
        };
        match runtime.client.as_ref().unwrap().requests.initialize().await {
            Ok(handshake) => Ok((runtime, handshake)),
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
    /// drop 与显式 shutdown 共用内核 Job 收敛，不产生恢复证据。
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
