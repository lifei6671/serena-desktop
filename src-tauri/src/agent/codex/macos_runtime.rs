use super::macos_launcher::{
    self, MacosLaunchRequest, MacosProcessIdentityAdapter, ProcessIdentity, process_group_members,
};
use super::macos_runtime_store::{self, MacosEvidenceKind};
use crate::agent::store::StateStore;
use std::{
    fmt,
    fs::File,
    io,
    os::fd::{AsRawFd, FromRawFd},
    process::ExitStatus,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// 单个 shutdown 阶段允许等待的最大时长，避免调用方传入无界超时。
const MAX_PHASE_TIMEOUT: Duration = Duration::from_secs(30);
/// Process Group 与直接 child 状态的固定轮询间隔。
const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// 当前 Host 从创建起连续持有 ownership 的 macOS Codex Runtime。
pub(crate) struct MacosRuntime {
    id: String,
    store: StateStore,
    store_ready: bool,
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

/// create 失败时保留 spawn 后已取得的 Runtime 或 launcher child ownership。
pub(crate) struct MacosRuntimeCreateFailure {
    pub code: &'static str,
    pub message: String,
    pub runtime: Option<Box<MacosRuntime>>,
    pub created: Option<Box<macos_launcher::CreatedChild>>,
}

impl fmt::Debug for MacosRuntimeCreateFailure {
    /// 调试信息仅展示稳定诊断与 ownership 类型。
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MacosRuntimeCreateFailure")
            .field("code", &self.code)
            .field("message", &self.message)
            .field("retains_runtime", &self.runtime.is_some())
            .field("retains_created_child", &self.created.is_some())
            .finish()
    }
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
    /// 仅供 compatibility CLI 读取 direct child 真实退出状态，不推断 Process Group 已清空。
    pub(crate) fn probe_exit_status(&mut self) -> io::Result<Option<ExitStatus>> {
        self.child.process.try_wait()
    }

    /// 复制三条 stdio fd，把异步读写所有权交给 Tokio，同时保留 Runtime child ownership。
    pub(crate) fn clone_stdio(&self) -> io::Result<(File, File, File)> {
        /// 使用 dup 创建独立 owned fd；File Drop 只关闭复制出的描述符。
        fn duplicate(fd: libc::c_int) -> io::Result<File> {
            // SAFETY: fd 来自当前 Runtime 持有的有效 stdio，成功结果由 File 独占接管。
            let duplicated = unsafe { libc::dup(fd) };
            if duplicated < 0 {
                Err(io::Error::last_os_error())
            } else {
                // SAFETY: duplicated 是本函数刚取得且尚未交给其他 owner 的有效 fd。
                Ok(unsafe { File::from_raw_fd(duplicated) })
            }
        }

        Ok((
            duplicate(self.child.stdin.as_raw_fd())?,
            duplicate(self.child.stdout.as_raw_fd())?,
            duplicate(self.child.stderr.as_raw_fd())?,
        ))
    }

    /// 复制初始化写入所需的稳定 Store/id，避免 blocking worker 借用 live Runtime。
    pub(crate) fn initialization_context(&self) -> (StateStore, String) {
        (self.store.clone(), self.id.clone())
    }

    /// 在 spawn 前准备 Store，并在 spawn 后持久化已验证的完整进程身份。
    pub(crate) fn create(
        store: StateStore,
        owner: String,
        request: MacosLaunchRequest,
    ) -> Result<Self, MacosRuntimeCreateFailure> {
        let id = request.runtime_instance_id.clone();
        let prepared_at = now();
        macos_runtime_store::prepare(
            &store,
            &id,
            &owner,
            &request.executable.to_string_lossy(),
            prepared_at,
        )
        .map_err(|error| MacosRuntimeCreateFailure {
            code: error.code,
            message: error.message,
            runtime: None,
            created: None,
        })?;
        let launched =
            macos_launcher::launch(&request).map_err(|error| MacosRuntimeCreateFailure {
                code: error.code,
                message: error.message,
                runtime: None,
                created: error.created,
            })?;
        let mut runtime = Self {
            id,
            store,
            store_ready: false,
            child: launched.child,
            identity: launched.identity,
        };
        if let Err(error) =
            macos_runtime_store::start(&runtime.store, &runtime.id, &runtime.identity, now())
        {
            let _ = macos_runtime_store::unknown(
                &runtime.store,
                &runtime.id,
                error.code,
                &error.message,
                now(),
            );
            return Err(MacosRuntimeCreateFailure {
                code: error.code,
                message: error.message,
                runtime: Some(Box::new(runtime)),
                created: None,
            });
        }
        runtime.store_ready = true;
        Ok(runtime)
    }

    /// 仅供 macOS managed compatibility fixture：等待短命 probe 自身进入 SIGSTOP，
    /// 再由已验证 leader PID 发送 SIGCONT，消除测试进程在父侧身份采集前退出的竞态。
    #[cfg(test)]
    pub(crate) fn resume_stopped_probe_for_test(&mut self, timeout: Duration) -> io::Result<()> {
        let deadline = Instant::now() + timeout.min(Duration::from_secs(5));
        loop {
            let mut status = 0;
            // SAFETY: identity.pid 是当前 Runtime 持有的直接 child；WUNTRACED 只读取 stop 状态，不回收进程。
            let waited = unsafe {
                libc::waitpid(
                    self.identity.pid,
                    &mut status,
                    libc::WNOHANG | libc::WUNTRACED,
                )
            };
            if waited == self.identity.pid {
                if libc::WIFSTOPPED(status) {
                    break;
                }
                if libc::WIFEXITED(status) || libc::WIFSIGNALED(status) {
                    return Err(io::Error::from_raw_os_error(libc::ESRCH));
                }
                return Err(io::Error::from_raw_os_error(libc::EPROTO));
            }
            if waited < 0 {
                return Err(io::Error::last_os_error());
            }
            if Instant::now() >= deadline {
                return Err(io::Error::from_raw_os_error(libc::ETIMEDOUT));
            }
            sleep_until_next_poll(deadline);
        }

        // SAFETY: 只恢复上面已经由 waitpid 证明处于 stopped 的同一直接 child。
        if unsafe { libc::kill(self.identity.pid, libc::SIGCONT) } == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
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
        // 已持久化 Runtime 只有先固定 terminating 才可发信号；start 写入失败的 live ownership 仅做本机收口。
        if self.store_ready
            && let Err(error) = macos_runtime_store::terminating(&self.store, &self.id, now())
        {
            return Err(self.failure(error.code, error.message));
        }

        // 先固定 child/group 状态，再验证 leader 身份；身份不足时绝不发送信号。
        let initial_result = self.observe_exit();
        let initial = match initial_result {
            Ok(observation) => observation,
            Err(error) => {
                return Err(self.unknown(format!("初始退出状态观测失败: {error}")));
            }
        };
        let identity_result = MacosProcessIdentityAdapter::observe(self.identity.pid);
        match identity_result {
            Ok(observed) if self.identity.matches(&observed) => {}
            Ok(_) => {
                return Err(self.unknown("leader 身份与创建时 PID、PGID、SID 或启动令牌不匹配"));
            }
            Err(error) if is_esrch(&error) && initial.is_complete() => {
                return self.finish_complete();
            }
            Err(error) if is_esrch(&error) => {
                return Err(self.unknown("leader 已不可观测且直接 child 或 Process Group 仍未收口"));
            }
            Err(error) => {
                return Err(self.unknown(format!("leader 身份观测失败: {error}")));
            }
        }
        if initial.is_complete() {
            return self.finish_complete();
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
                return self.finish_complete();
            }
            if Instant::now() >= grace_deadline {
                break;
            }
            sleep_until_next_poll(grace_deadline);
        }

        // grace 后必须重新证明同一 leader；leader 已退出时旧 PGID 已可能被复用，绝不升级信号。
        let identity_result = MacosProcessIdentityAdapter::observe(self.identity.pid);
        match identity_result {
            Ok(observed) if self.identity.matches(&observed) => {}
            Ok(_) => {
                return Err(self.unknown("SIGKILL 前 leader 身份不再匹配创建时身份"));
            }
            Err(error) if is_esrch(&error) => {
                return Err(self.unknown("SIGKILL 前 leader 已退出，拒绝向可能复用的 PGID 发信号"));
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
                return self.finish_complete();
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
        let mut message = message.into();
        if self.store_ready
            && let Err(error) =
                macos_runtime_store::unknown(&self.store, &self.id, code, &message, now())
        {
            message.push_str(&format!("; Store unknown 写入失败: {}", error.message));
        }
        MacosRuntimeFailure {
            code,
            message,
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

    /// 先提交 sealed live group-empty evidence；提交失败时保留 Runtime ownership。
    fn finish_complete(self) -> Result<MacosTerminationEvidence, MacosRuntimeFailure> {
        let evidence = self.complete_evidence();
        // start 身份写入失败的 Runtime 只能收口进程，不能补造可供 Claim release 使用的持久化 evidence。
        if self.store_ready
            && let Err(error) = macos_runtime_store::complete(
                &self.store,
                &self.id,
                MacosEvidenceKind::LiveGroupEmpty,
                evidence.observed_at,
            )
        {
            return Err(self.failure(error.code, error.message));
        }
        Ok(evidence)
    }
}

/// 返回当前 Unix epoch 毫秒，作为 Store 状态和 evidence 时间。
fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[cfg(test)]
mod tests;
