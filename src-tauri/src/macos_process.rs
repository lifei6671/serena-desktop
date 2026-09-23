//! macOS 非 Codex 子进程的私有 Session/Process Group 所有权边界。

use std::{
    io,
    os::unix::process::CommandExt,
    process::{Child, Command},
    ptr, thread,
    time::{Duration, Instant},
};

/// 创建时冻结的 Darwin leader 身份，用于每次信号前排除 PID/PGID 复用。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Identity {
    pid: libc::pid_t,
    pgid: libc::pid_t,
    sid: libc::pid_t,
    start_seconds: u64,
    start_microseconds: u64,
}

impl Identity {
    /// 从直接 child PID 捕获完整 Darwin 身份，并只接受独立 Session leader。
    pub(crate) fn capture(pid: u32) -> io::Result<Self> {
        let pid = libc::pid_t::try_from(pid)
            .map_err(|_| io::Error::from_raw_os_error(libc::EOVERFLOW))?;
        let identity = observe(pid)?;
        if identity.pid != identity.pgid || identity.pid != identity.sid {
            return Err(io::Error::from_raw_os_error(libc::EPROTO));
        }
        Ok(identity)
    }

    /// 返回创建时 leader PID。
    #[cfg(test)]
    pub(crate) fn pid(&self) -> libc::pid_t {
        self.pid
    }

    /// 返回应用独占的 Process Group ID。
    pub(crate) fn pgid(&self) -> libc::pid_t {
        self.pgid
    }

    /// 返回应用独占的 Session ID。
    #[cfg(test)]
    pub(crate) fn sid(&self) -> libc::pid_t {
        self.sid
    }

    /// 重新读取内核事实，并要求 PID、PGID、SID 与启动令牌全部相同。
    pub(crate) fn verify(&self) -> io::Result<()> {
        let observed = observe(self.pid)?;
        if self == &observed {
            Ok(())
        } else {
            Err(io::Error::from_raw_os_error(libc::ESTALE))
        }
    }

    /// 身份匹配后才向完整 owned process group 发送指定信号。
    pub(crate) fn signal(&self, signal: libc::c_int) -> Result<(), String> {
        self.verify()
            .map_err(|error| format!("leader 身份无法重新确认，拒绝发送进程组信号：{error}"))?;
        // SAFETY: PGID 来自已重新匹配完整创建身份的独立 Session leader。
        if unsafe { libc::killpg(self.pgid, signal) } == 0 {
            return Ok(());
        }
        Err(format!(
            "无法向受管 Process Group 发送信号：{}",
            io::Error::last_os_error()
        ))
    }
}

/// 在 exec 前创建独立 Session，避免 spawn 后再归组的竞态窗口。
pub(crate) fn configure_std_command(command: &mut Command) {
    // SAFETY: pre_exec 闭包只调用 async-signal-safe 的 setsid/getpid/getpgid/getsid。
    unsafe {
        command.pre_exec(|| {
            let session = libc::setsid();
            if session < 0 {
                return Err(io::Error::last_os_error());
            }
            let pid = libc::getpid();
            if session != pid || libc::getpgid(0) != pid || libc::getsid(0) != pid {
                return Err(io::Error::from_raw_os_error(libc::EPROTO));
            }
            Ok(())
        });
    }
}

/// 为 Tokio Command 复用同一 exec 前 setsid 契约。
pub(crate) fn configure_tokio_command(command: &mut tokio::process::Command) {
    configure_std_command(command.as_std_mut());
}

/// 枚举指定 Process Group 的当前成员；空集合是唯一完成事实。
fn group_members(pgid: libc::pid_t) -> io::Result<Vec<libc::pid_t>> {
    clear_errno();
    // SAFETY: null 缓冲区与零长度是 libproc 查询成员数量的约定。
    let count = unsafe { libc::proc_listpgrppids(pgid, ptr::null_mut(), 0) };
    if count <= 0 {
        return classify_empty_group_read(count);
    }
    let initial =
        usize::try_from(count).map_err(|_| io::Error::from_raw_os_error(libc::EOVERFLOW))?;
    let mut capacity = initial
        .checked_add(16)
        .ok_or_else(|| io::Error::from_raw_os_error(libc::EOVERFLOW))?;

    // 成员可能在计数与填充之间增加；有限扩容后仍不稳定则明确失败。
    for _ in 0..3 {
        let bytes = capacity
            .checked_mul(std::mem::size_of::<libc::pid_t>())
            .and_then(|value| libc::c_int::try_from(value).ok())
            .ok_or_else(|| io::Error::from_raw_os_error(libc::EOVERFLOW))?;
        let mut members = vec![0; capacity];
        clear_errno();
        // SAFETY: members 是 bytes 对应的连续可写 pid_t 缓冲区。
        let listed = unsafe { libc::proc_listpgrppids(pgid, members.as_mut_ptr().cast(), bytes) };
        if listed <= 0 {
            return classify_empty_group_read(listed);
        }
        let listed =
            usize::try_from(listed).map_err(|_| io::Error::from_raw_os_error(libc::EOVERFLOW))?;
        if listed < capacity {
            members.truncate(listed);
            return Ok(members);
        }
        capacity = listed
            .checked_add(16)
            .ok_or_else(|| io::Error::from_raw_os_error(libc::EOVERFLOW))?;
    }
    Err(io::Error::from_raw_os_error(libc::EOVERFLOW))
}

/// 判断 Process Group 是否已无任何成员。
pub(crate) fn group_is_empty(pgid: libc::pid_t) -> io::Result<bool> {
    group_members(pgid).map(|members| members.is_empty())
}

/// 同步终止 owned child/group，并要求直接 child 已回收且 group 为空。
pub(crate) fn terminate_sync(
    child: &mut Child,
    identity: &Identity,
    grace: Duration,
    kill_wait: Duration,
) -> Result<(), String> {
    if observe_complete(child, identity.pgid)? {
        return Ok(());
    }
    identity.signal(libc::SIGTERM)?;
    if wait_complete(child, identity.pgid, grace)? {
        return Ok(());
    }

    // grace 后重新匹配 leader；若 leader 已退出，旧 PGID 不再足以授权 SIGKILL。
    identity.signal(libc::SIGKILL)?;
    if wait_complete(child, identity.pgid, kill_wait)? {
        return Ok(());
    }
    Err("受管 Process Group 在强制终止后仍未确认清空".into())
}

/// 在有界等待内轮询 child reap 与 group-empty 双重完成事实。
fn wait_complete(child: &mut Child, pgid: libc::pid_t, timeout: Duration) -> Result<bool, String> {
    let deadline = Instant::now() + timeout;
    loop {
        if observe_complete(child, pgid)? {
            return Ok(true);
        }
        if Instant::now() >= deadline {
            return Ok(false);
        }
        thread::sleep(
            Duration::from_millis(10).min(deadline.saturating_duration_since(Instant::now())),
        );
    }
}

/// 在同一轮观测中确认直接 child 已回收且 Process Group 已空。
fn observe_complete(child: &mut Child, pgid: libc::pid_t) -> Result<bool, String> {
    let child_reaped = child
        .try_wait()
        .map_err(|error| format!("无法读取受管 child 状态：{error}"))?
        .is_some();
    let group_empty = group_is_empty(pgid)
        .map_err(|error| format!("无法读取受管 Process Group 状态：{error}"))?;
    Ok(child_reaped && group_empty)
}

/// 读取指定 PID 的 Darwin BSD identity。
fn observe(pid: libc::pid_t) -> io::Result<Identity> {
    // SAFETY: proc_bsdinfo 是零初始化后交给内核完整填充的 POD 缓冲区。
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of_val(&info) as libc::c_int;
    clear_errno();
    // SAFETY: info 在调用期间是有效且足长的独占可写缓冲区。
    let read = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            (&mut info as *mut libc::proc_bsdinfo).cast(),
            size,
        )
    };
    classify_proc_read(read, size, current_errno())?;
    // SAFETY: getpgid/getsid 只读取指定 PID 的内核状态。
    let (pgid, sid) = unsafe { (libc::getpgid(pid), libc::getsid(pid)) };
    if pgid < 0 || sid < 0 {
        return Err(io::Error::last_os_error());
    }
    if info.pbi_pid != pid as u32 || info.pbi_pgid != pgid as u32 {
        return Err(io::Error::from_raw_os_error(libc::EPROTO));
    }
    Ok(Identity {
        pid,
        pgid,
        sid,
        start_seconds: info.pbi_start_tvsec,
        start_microseconds: info.pbi_start_tvusec,
    })
}

/// 清空当前线程 errno，使 libproc 的空结果与系统失败可区分。
fn clear_errno() {
    // SAFETY: macOS __error 返回当前线程有效的 errno 存储地址。
    unsafe {
        *libc::__error() = 0;
    }
}

/// 读取当前线程 errno，不引入其他系统调用。
fn current_errno() -> libc::c_int {
    // SAFETY: macOS __error 返回当前线程有效的 errno 存储地址。
    unsafe { *libc::__error() }
}

/// 将 libproc identity 读取结果转换为标准 io 错误。
fn classify_proc_read(
    read: libc::c_int,
    expected: libc::c_int,
    errno: libc::c_int,
) -> io::Result<()> {
    if read == expected {
        return Ok(());
    }
    if read > 0 {
        return Err(io::Error::from_raw_os_error(libc::EPROTO));
    }
    Err(io::Error::from_raw_os_error(if errno == 0 {
        libc::ESRCH
    } else {
        errno
    }))
}

/// 解释 proc_listpgrppids 的空组或错误结果。
fn classify_empty_group_read(value: libc::c_int) -> io::Result<Vec<libc::pid_t>> {
    let errno = current_errno();
    if value == 0 && errno == 0 {
        Ok(Vec::new())
    } else if errno != 0 {
        Err(io::Error::from_raw_os_error(errno))
    } else {
        Err(io::Error::from_raw_os_error(libc::EPROTO))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        path::{Path, PathBuf},
        process::{Child, Command, Stdio},
        time::{Duration, Instant},
    };

    /// 使用现有固定 Rust fixture，避免测试生产路径依赖 shell command string。
    fn fixture(directory: &Path) -> PathBuf {
        let executable = directory.join("macos-managed-child");
        let output = Command::new("rustc")
            .args(["--edition=2024", "--crate-name", "macos_managed_child"])
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

    /// 创建独立 Session fixture，并返回持续持有的 child 与内核身份。
    fn spawn_fixture(executable: &Path, mode: &str, marker: &Path) -> (Child, Identity) {
        let mut command = Command::new(executable);
        command
            .args([mode])
            .arg(marker)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        configure_std_command(&mut command);
        let mut child = command.spawn().unwrap();
        let identity = match Identity::capture(child.id()) {
            Ok(identity) => identity,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("capture fixture identity: {error}");
            }
        };
        (child, identity)
    }

    /// 等待 fixture 写入 ready marker，证明后代已经进入同一进程组。
    fn wait_file(path: &Path) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !path.exists() {
            assert!(Instant::now() < deadline, "{}", path.display());
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// 测试 teardown 可绕过生产 identity 门，但只操作本测试刚创建的 PGID。
    fn force_cleanup(child: &mut Child, identity: &Identity) {
        // SAFETY: identity 来自当前测试独占创建且尚未交出的 fixture process group。
        unsafe {
            libc::killpg(identity.pgid(), libc::SIGKILL);
        }
        let _ = child.wait();
        let deadline = Instant::now() + Duration::from_secs(2);
        while !group_is_empty(identity.pgid()).unwrap_or(false) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// spawn 必须在 exec 前形成 PID=PGID=SID，并由父进程完整捕获身份。
    #[test]
    fn configured_child_is_verified_session_leader() {
        let directory = tempfile::tempdir().unwrap();
        let executable = fixture(directory.path());
        let marker = directory.path().join("leader.pid");
        let ready = PathBuf::from(format!("{}.ready", marker.display()));
        let (mut child, identity) = spawn_fixture(&executable, "tree", &marker);
        wait_file(&ready);

        assert_eq!(identity.pid(), identity.pgid());
        assert_eq!(identity.pid(), identity.sid());
        assert!(identity.verify().is_ok());
        terminate_sync(
            &mut child,
            &identity,
            Duration::from_secs(1),
            Duration::from_secs(2),
        )
        .unwrap();
        assert!(group_is_empty(identity.pgid()).unwrap());
    }

    /// 终止一个 owned group 不得影响另一个独立 owned group。
    #[test]
    fn termination_is_scoped_to_the_verified_group() {
        let directory = tempfile::tempdir().unwrap();
        let executable = fixture(directory.path());
        let first_marker = directory.path().join("first.pid");
        let second_marker = directory.path().join("second.pid");
        let (mut first, first_identity) = spawn_fixture(&executable, "ignore-tree", &first_marker);
        let (mut second, second_identity) =
            spawn_fixture(&executable, "ignore-tree", &second_marker);
        wait_file(&PathBuf::from(format!("{}.ready", first_marker.display())));
        wait_file(&PathBuf::from(format!("{}.ready", second_marker.display())));

        terminate_sync(
            &mut first,
            &first_identity,
            Duration::from_millis(50),
            Duration::from_secs(2),
        )
        .unwrap();
        assert!(group_is_empty(first_identity.pgid()).unwrap());
        assert!(second.try_wait().unwrap().is_none());
        assert!(second_identity.verify().is_ok());

        force_cleanup(&mut second, &second_identity);
    }

    /// leader 在 grace 内退出后，生产路径必须拒绝向残留 PGID 升级 SIGKILL。
    #[test]
    fn missing_leader_before_sigkill_fails_closed() {
        let directory = tempfile::tempdir().unwrap();
        let executable = fixture(directory.path());
        let marker = directory.path().join("leaf.pid");
        let ready = PathBuf::from(format!("{}.ready", marker.display()));
        let (mut child, identity) = spawn_fixture(&executable, "leader-term-exit", &marker);
        wait_file(&ready);

        let error = terminate_sync(
            &mut child,
            &identity,
            Duration::from_millis(100),
            Duration::from_secs(2),
        )
        .unwrap_err();
        assert!(error.contains("leader"));
        assert!(!group_is_empty(identity.pgid()).unwrap());

        force_cleanup(&mut child, &identity);
    }
}
