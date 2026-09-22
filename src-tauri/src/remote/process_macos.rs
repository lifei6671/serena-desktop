//! Quick Tunnel 在 macOS 上的独立 Session/Process Group owner。

use crate::macos_process::{self, Identity};
use std::{io, time::Duration};

const TERM_GRACE: Duration = Duration::from_secs(1);
const KILL_WAIT: Duration = Duration::from_secs(5);

/// 持有直接 child 与创建时内核身份；失败时可由 `pending_child` 原样保留。
pub(crate) struct ManagedChild {
    child: tokio::process::Child,
    identity: Identity,
    termination_started: Option<std::time::Instant>,
    kill_sent: bool,
}

impl ManagedChild {
    /// 在 exec 前建立独立 Session，spawn 后由父侧冻结完整身份。
    pub(crate) fn spawn(command: &mut tokio::process::Command) -> Result<Self, String> {
        macos_process::configure_tokio_command(command);
        let mut child = command
            .spawn()
            .map_err(|_| "QUICK_TUNNEL_START_FAILED".to_string())?;
        let identity = Identity::capture(
            child
                .id()
                .ok_or_else(|| "QUICK_TUNNEL_START_FAILED".to_string())?,
        )
        .map_err(|_| {
            // 身份验证失败时不猜测 PGID，只请求 Tokio 回收直接 child。
            let _ = child.start_kill();
            "QUICK_TUNNEL_START_FAILED".to_string()
        })?;
        Ok(Self {
            child,
            identity,
            termination_started: None,
            kill_sent: false,
        })
    }

    /// 返回创建时直接 child PID。
    pub(crate) fn id(&self) -> Option<u32> {
        self.child.id()
    }

    /// 只有直接 child 已回收且原 process group 为空才报告完成。
    pub(crate) fn try_wait(&mut self) -> io::Result<Option<()>> {
        let child_exited = self.child.try_wait()?.is_some();
        let group_empty = macos_process::group_is_empty(self.identity.pgid())?;
        Ok((child_exited && group_empty).then_some(()))
    }

    /// 等待自然退出；显式停止后负责有限 grace、身份复核与必要的 SIGKILL。
    pub(crate) async fn wait(&mut self) -> io::Result<()> {
        loop {
            if self.try_wait()?.is_some() {
                return Ok(());
            }
            if let Some(started) = self.termination_started {
                let elapsed = started.elapsed();
                if !self.kill_sent && elapsed >= TERM_GRACE {
                    self.identity
                        .signal(libc::SIGKILL)
                        .map_err(io::Error::other)?;
                    self.kill_sent = true;
                }
                if elapsed >= TERM_GRACE + KILL_WAIT {
                    return Err(io::Error::other(
                        "Quick Tunnel Process Group 未在有界停止窗口内清空",
                    ));
                }
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }

    /// 启动完整 process group 的有界 SIGTERM→SIGKILL 收口。
    pub(crate) fn start_kill(&mut self) -> io::Result<()> {
        if self.try_wait()?.is_some() || self.termination_started.is_some() {
            return Ok(());
        }
        self.identity
            .signal(libc::SIGTERM)
            .map_err(io::Error::other)?;
        self.termination_started = Some(std::time::Instant::now());
        Ok(())
    }
}

impl std::ops::Deref for ManagedChild {
    type Target = tokio::process::Child;

    /// 复用既有 stderr/stdout ownership API，不暴露 process-group identity。
    fn deref(&self) -> &Self::Target {
        &self.child
    }
}

impl std::ops::DerefMut for ManagedChild {
    /// 复用既有 child pipe 与等待接口。
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.child
    }
}

impl Drop for ManagedChild {
    /// 非正常 Drop 只在 leader 身份仍匹配时 best-effort 终止其独占 group。
    fn drop(&mut self) {
        let _ = self.identity.signal(libc::SIGKILL);
    }
}
