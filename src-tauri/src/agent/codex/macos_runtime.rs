use super::macos_launcher::{
    self, MacosLaunchRequest, MacosProcessIdentityAdapter, ProcessIdentity, process_group_members,
};
use std::{
    fmt, io, thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// 单个 shutdown 阶段允许等待的最大时长，避免调用方传入无界超时。
const MAX_PHASE_TIMEOUT: Duration = Duration::from_secs(30);
/// Process Group 与直接 child 状态的固定轮询间隔。
const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// 当前 Host 从创建起连续持有 ownership 的 macOS Codex Runtime。
pub(crate) struct MacosRuntime {
    id: String,
    child: macos_launcher::CreatedChild,
    identity: ProcessIdentity,
}

impl fmt::Debug for MacosRuntime {
    /// 调试输出只暴露稳定标识，不展开 stdio 或 child handle。
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MacosRuntime")
            .field("id", &self.id)
            .field("leader_pid", &self.identity.pid)
            .field("pgid", &self.identity.pgid)
            .finish_non_exhaustive()
    }
}

/// 当前 Host live ownership 下完成 Process Group 收口的完整证据。
#[derive(Debug)]
pub(crate) struct MacosTerminationEvidence {
    runtime_id: String,
    leader_pid: libc::pid_t,
    pgid: libc::pid_t,
    process_start_token: macos_launcher::ProcessStartToken,
    observed_at: i64,
    host_continuous_ownership: bool,
    direct_child_reaped: bool,
}

/// shutdown 失败及仍可继续收口的完整 Runtime ownership。
#[derive(Debug)]
pub(crate) struct MacosRuntimeFailure {
    pub code: &'static str,
    pub message: String,
    pub runtime: Box<MacosRuntime>,
}

/// 一次有序观测中直接 child 与 Process Group 的退出状态。
struct ExitObservation {
    direct_child_reaped: bool,
    members: Vec<libc::pid_t>,
}

impl ExitObservation {
    /// 只有直接 child 已回收且原 Process Group 为空才算完整退出。
    fn is_complete(&self) -> bool {
        self.direct_child_reaped && self.members.is_empty()
    }
}

/// 向当前 Host 持有的 Process Group 发送指定信号。
fn signal_group(pgid: libc::pid_t, signal: libc::c_int) -> io::Result<()> {
    // SAFETY: pgid 来自 launcher 已验证的独立 Session，signal 使用系统定义常量。
    if unsafe { libc::killpg(pgid, signal) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// 判断系统错误是否表示本次观测目标已经不存在。
fn is_esrch(error: &io::Error) -> bool {
    error.raw_os_error() == Some(libc::ESRCH)
}

/// 在 deadline 前休眠不超过一个轮询间隔，避免超时后额外等待。
fn sleep_until_next_poll(deadline: Instant) {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if !remaining.is_zero() {
        thread::sleep(remaining.min(POLL_INTERVAL));
    }
}

impl MacosRuntime {
    /// 从已验证的 launcher ownership 构造 live-host Runtime。
    pub(crate) fn create(
        request: MacosLaunchRequest,
    ) -> Result<Self, macos_launcher::MacosLaunchFailure> {
        let id = request.runtime_instance_id.clone();
        let launched = macos_launcher::launch(&request)?;
        Ok(Self {
            id,
            child: launched.child,
            identity: launched.identity,
        })
    }

    /// 仅供单元测试篡改创建时身份，用于验证 mismatch 必须 fail closed。
    #[cfg(test)]
    fn replace_identity_for_test(&mut self, identity: ProcessIdentity) {
        self.identity = identity;
    }

    /// 依次执行 SIGTERM、有限 grace 与必要时的 SIGKILL，并只返回完整退出证据。
    pub(crate) fn shutdown(
        mut self,
        grace: Duration,
        kill_wait: Duration,
    ) -> Result<MacosTerminationEvidence, MacosRuntimeFailure> {
        let grace = grace.min(MAX_PHASE_TIMEOUT);
        let kill_wait = kill_wait.min(MAX_PHASE_TIMEOUT);

        // 先固定 child/group 状态，再验证 leader 身份；身份不足时绝不发送信号。
        let initial_result = self.observe_exit();
        let initial = match initial_result {
            Ok(observation) => observation,
            Err(error) => {
                return Err(self.unknown(format!("初始退出状态观测失败: {error}")));
            }
        };
        let mut group_continuously_observed_nonempty = !initial.members.is_empty();

        let identity_result = MacosProcessIdentityAdapter::observe(self.identity.pid);
        match identity_result {
            Ok(observed) if self.identity.matches(&observed) => {}
            Ok(_) => {
                return Err(self.unknown("leader 身份与创建时 PID、PGID、SID 或启动令牌不匹配"));
            }
            Err(error) if is_esrch(&error) && initial.is_complete() => {
                return Ok(self.complete_evidence());
            }
            Err(error) if is_esrch(&error) => {
                return Err(self.unknown("leader 已不可观测且直接 child 或 Process Group 仍未收口"));
            }
            Err(error) => {
                return Err(self.unknown(format!("leader 身份观测失败: {error}")));
            }
        }
        if initial.is_complete() {
            return Ok(self.complete_evidence());
        }

        let term_result = signal_group(self.identity.pgid, libc::SIGTERM);
        if let Err(error) = term_result
            && !is_esrch(&error)
        {
            return Err(self.failure(
                "CODEX_PROCESS_GROUP_SIGNAL_FAILED",
                format!("向 Process Group 发送 SIGTERM 失败: {error}"),
            ));
        }

        // 即使 grace 为零也立即观测一次；只有双重退出事实才能完成。
        let grace_deadline = Instant::now() + grace;
        loop {
            let observation_result = self.observe_exit();
            let observation = match observation_result {
                Ok(observation) => observation,
                Err(error) => {
                    return Err(self.unknown(format!("SIGTERM 后退出状态观测失败: {error}")));
                }
            };
            if observation.is_complete() {
                return Ok(self.complete_evidence());
            }
            if observation.members.is_empty() {
                group_continuously_observed_nonempty = false;
            }

            if Instant::now() >= grace_deadline {
                break;
            }
            sleep_until_next_poll(grace_deadline);
        }

        // grace 后重新验证 leader；leader 已退出时只接受此前组成员连续非空的 live 路径。
        let identity_result = MacosProcessIdentityAdapter::observe(self.identity.pid);
        match identity_result {
            Ok(observed) if self.identity.matches(&observed) => {}
            Ok(_) => {
                return Err(self.unknown("SIGKILL 前 leader 身份不再匹配创建时身份"));
            }
            Err(error) if is_esrch(&error) && group_continuously_observed_nonempty => {}
            Err(error) if is_esrch(&error) => {
                return Err(
                    self.unknown("leader 已退出，但 Process Group 未保持连续、成功且非空的观测")
                );
            }
            Err(error) => {
                return Err(self.unknown(format!("SIGKILL 前 leader 身份观测失败: {error}")));
            }
        }

        let kill_result = signal_group(self.identity.pgid, libc::SIGKILL);
        if let Err(error) = kill_result
            && !is_esrch(&error)
        {
            return Err(self.failure(
                "CODEX_PROCESS_GROUP_SIGNAL_FAILED",
                format!("向 Process Group 发送 SIGKILL 失败: {error}"),
            ));
        }

        // kill_wait 为零时同样至少立即观测一次，不把 killpg 成功或 ESRCH 当作完成证据。
        let kill_deadline = Instant::now() + kill_wait;
        loop {
            let observation_result = self.observe_exit();
            let observation = match observation_result {
                Ok(observation) => observation,
                Err(error) => {
                    return Err(self.unknown(format!("SIGKILL 后退出状态观测失败: {error}")));
                }
            };
            if observation.is_complete() {
                return Ok(self.complete_evidence());
            }

            if Instant::now() >= kill_deadline {
                break;
            }
            sleep_until_next_poll(kill_deadline);
        }

        Err(self.unknown("SIGKILL 等待结束后仍无法确认直接 child 已回收且 Process Group 为空"))
    }

    /// 先回收直接 child，再查询 Process Group，形成同一轮退出观测。
    fn observe_exit(&mut self) -> io::Result<ExitObservation> {
        let direct_child_reaped = self.child.process.try_wait()?.is_some();
        let members = process_group_members(self.identity.pgid)?;
        Ok(ExitObservation {
            direct_child_reaped,
            members,
        })
    }

    /// 构造保留完整 Runtime ownership 的稳定失败。
    fn failure(self, code: &'static str, message: impl Into<String>) -> MacosRuntimeFailure {
        MacosRuntimeFailure {
            code,
            message: message.into(),
            runtime: Box::new(self),
        }
    }

    /// 将证据不足统一映射为可重试或人工收口的 unknown。
    fn unknown(self, message: impl Into<String>) -> MacosRuntimeFailure {
        self.failure("CODEX_RUNTIME_TERMINATION_UNCONFIRMED", message)
    }

    /// 只在调用点已经确认 child reaped 且 group empty 时生成完整 live-host 证据。
    fn complete_evidence(&self) -> MacosTerminationEvidence {
        MacosTerminationEvidence {
            runtime_id: self.id.clone(),
            leader_pid: self.identity.pid,
            pgid: self.identity.pgid,
            process_start_token: self.identity.start_token.clone(),
            observed_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as i64,
            host_continuous_ownership: true,
            direct_child_reaped: true,
        }
    }
}

#[cfg(test)]
mod tests;
