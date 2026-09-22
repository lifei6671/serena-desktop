use std::{
    ffi::{OsStr, OsString},
    fmt, io,
    os::unix::ffi::OsStrExt,
    os::unix::process::CommandExt,
    path::PathBuf,
    process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio},
    ptr,
};

/// executable、argv 及各自终止 NUL 的最大总字节数。
pub(super) const MAX_COMMAND_BYTES: usize = 128 * 1024;
/// Runtime ID 的最大 UTF-8 字节数。
pub(super) const MAX_RUNTIME_ID_BYTES: usize = 128;

/// macOS Codex launcher 在创建进程前需要的固定输入。
#[derive(Debug)]
pub(crate) struct MacosLaunchRequest {
    pub executable: PathBuf,
    pub args: Vec<OsString>,
    pub current_dir: PathBuf,
    pub runtime_instance_id: String,
}

/// macOS Codex launcher 的稳定错误信息。
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct MacosLaunchError {
    pub code: &'static str,
    pub message: String,
}

/// macOS 内核进程启动时间令牌，只在当前私有进程契约内比较。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ProcessStartToken {
    pub(super) seconds: u64,
    pub(super) microseconds: u64,
}

impl ProcessStartToken {
    /// 编码为持久化层唯一接受的 Darwin BSD 启动时间版本格式。
    pub(super) fn encode(&self) -> String {
        format!(
            "darwin_proc_bsd_start_v1:{}:{}",
            self.seconds, self.microseconds
        )
    }

    /// 严格解码版本化启动令牌，拒绝未知版本、字段数量和无效时间值。
    pub(super) fn decode(encoded: &str) -> Result<Self, &'static str> {
        let mut fields = encoded.split(':');
        if fields.next() != Some("darwin_proc_bsd_start_v1") {
            return Err("CODEX_PROCESS_IDENTITY_FAILED");
        }
        let seconds = fields
            .next()
            .ok_or("CODEX_PROCESS_IDENTITY_FAILED")?
            .parse::<u64>()
            .map_err(|_| "CODEX_PROCESS_IDENTITY_FAILED")?;
        let microseconds = fields
            .next()
            .ok_or("CODEX_PROCESS_IDENTITY_FAILED")?
            .parse::<u64>()
            .map_err(|_| "CODEX_PROCESS_IDENTITY_FAILED")?;
        if fields.next().is_some() || microseconds >= 1_000_000 {
            return Err("CODEX_PROCESS_IDENTITY_FAILED");
        }
        Ok(Self {
            seconds,
            microseconds,
        })
    }
}

/// launcher 创建时冻结的 leader 身份与 containment 信息。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ProcessIdentity {
    pub(super) pid: libc::pid_t,
    pub(super) pgid: libc::pid_t,
    pub(super) sid: libc::pid_t,
    pub(super) start_token: ProcessStartToken,
}

impl ProcessIdentity {
    /// 只有 PID、PGID、SID 和启动令牌全部一致时才匹配原进程。
    pub(super) fn matches(&self, observed: &Self) -> bool {
        self == observed
    }
}

/// 隐藏 Darwin libproc 结构的私有进程身份适配器。
pub(super) struct MacosProcessIdentityAdapter;

impl MacosProcessIdentityAdapter {
    /// 从 Darwin 内核读取 leader 身份，不向上层暴露 libproc 结构。
    pub(super) fn observe(pid: libc::pid_t) -> io::Result<ProcessIdentity> {
        // SAFETY: proc_bsdinfo 是 C POD 输出缓冲区，零初始化后再交给内核完整填充。
        let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of_val(&info) as libc::c_int;

        clear_errno();
        // SAFETY: 缓冲区指向有效且足长的局部 proc_bsdinfo，调用期间保持独占可写。
        let read = unsafe {
            libc::proc_pidinfo(
                pid,
                libc::PROC_PIDTBSDINFO,
                0,
                (&mut info as *mut libc::proc_bsdinfo).cast(),
                size,
            )
        };
        classify_proc_pidinfo_read(read, size, current_errno())?;

        // SAFETY: getpgid 只读取指定 PID 的内核进程组状态。
        let pgid = unsafe { libc::getpgid(pid) };
        if pgid < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: getsid 只读取指定 PID 的内核 Session 状态。
        let sid = unsafe { libc::getsid(pid) };
        if sid < 0 {
            return Err(io::Error::last_os_error());
        }
        if info.pbi_pid != pid as u32 || info.pbi_pgid != pgid as u32 {
            return Err(io::Error::from_raw_os_error(libc::EPROTO));
        }

        Ok(ProcessIdentity {
            pid,
            pgid,
            sid,
            start_token: ProcessStartToken {
                seconds: info.pbi_start_tvsec,
                microseconds: info.pbi_start_tvusec,
            },
        })
    }
}

/// 已成功创建且仍由调用方完整持有的 child 与三条 stdio pipe。
pub(crate) struct CreatedChild {
    pub(super) stdin: ChildStdin,
    pub(super) stdout: ChildStdout,
    pub(super) stderr: ChildStderr,
    pub(super) process: Child,
    pub(super) pid: libc::pid_t,
    pub(super) pgid: libc::pid_t,
}

/// 已完成父子双侧身份验证的 launcher 结果。
pub(super) struct LaunchedChild {
    pub(super) child: CreatedChild,
    pub(super) identity: ProcessIdentity,
}

/// launcher 失败及创建后仍需收口的 ownership。
pub(crate) struct MacosLaunchFailure {
    pub code: &'static str,
    pub message: String,
    pub created: Option<Box<CreatedChild>>,
}

impl fmt::Debug for MacosLaunchFailure {
    /// 调试信息只展示稳定诊断与 ownership 是否存在，不展开进程句柄。
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MacosLaunchFailure")
            .field("code", &self.code)
            .field("message", &self.message)
            .field("has_created_ownership", &self.created.is_some())
            .finish()
    }
}

/// 构造统一的 launcher 输入错误。
fn invalid(message: impl Into<String>) -> MacosLaunchError {
    MacosLaunchError {
        code: "CODEX_LAUNCH_INPUT_INVALID",
        message: message.into(),
    }
}

/// 按 Unix 原始字节判断 OS 字符串是否包含 NUL。
fn contains_nul(value: &OsStr) -> bool {
    value.as_bytes().contains(&0)
}

/// 清除当前线程的 errno，使 libproc 的空结果与失败可以区分。
fn clear_errno() {
    // SAFETY: macOS 的 __error 返回当前线程有效的 errno 存储地址。
    unsafe {
        *libc::__error() = 0;
    }
}

/// 读取当前线程 errno 的整数值，不触发其他系统调用。
fn current_errno() -> libc::c_int {
    // SAFETY: macOS 的 __error 返回当前线程有效的 errno 存储地址。
    unsafe { *libc::__error() }
}

/// 将 proc_pidinfo 的读取字节数和已清零后的 errno 转换为稳定结果。
fn classify_proc_pidinfo_read(
    read: libc::c_int,
    expected_size: libc::c_int,
    errno: libc::c_int,
) -> io::Result<()> {
    if read == expected_size {
        return Ok(());
    }
    // 正数表示内核写入了不完整结构；无论 errno 值如何都属于协议错误。
    if read > 0 {
        return Err(io::Error::from_raw_os_error(libc::EPROTO));
    }

    // Apple API 以零表示失败；清零后的 errno 决定是进程消失还是具体系统错误。
    Err(if errno == 0 {
        io::Error::from_raw_os_error(libc::ESRCH)
    } else {
        io::Error::from_raw_os_error(errno)
    })
}

/// 在创建任何进程前验证 launcher 输入边界。
pub(super) fn validate(request: &MacosLaunchRequest) -> Result<(), MacosLaunchError> {
    if request.runtime_instance_id.is_empty()
        || request.runtime_instance_id.len() > MAX_RUNTIME_ID_BYTES
        || request.runtime_instance_id.contains(['/', '\\', '\0'])
        || !request.executable.is_absolute()
        || !request.current_dir.is_absolute()
        || contains_nul(request.executable.as_os_str())
        || contains_nul(request.current_dir.as_os_str())
        || !request.current_dir.is_dir()
    {
        return Err(invalid("Runtime ID、绝对 executable 和现有 cwd 必填"));
    }

    // 每一项都计入 exec 所需的终止 NUL，并显式拒绝算术溢出。
    let mut command_bytes = request
        .executable
        .as_os_str()
        .as_bytes()
        .len()
        .checked_add(1)
        .ok_or_else(|| invalid("executable 长度溢出"))?;
    for argument in &request.args {
        if contains_nul(argument) {
            return Err(invalid("argv 不能包含 NUL"));
        }
        let argument_bytes = argument
            .as_bytes()
            .len()
            .checked_add(1)
            .ok_or_else(|| invalid("argv 长度溢出"))?;
        command_bytes = command_bytes
            .checked_add(argument_bytes)
            .ok_or_else(|| invalid("argv 长度溢出"))?;
    }
    if command_bytes > MAX_COMMAND_BYTES {
        return Err(invalid("executable 与 argv 超过 128 KiB"));
    }

    Ok(())
}

/// 枚举指定 Process Group 的当前成员；空 Vec 是唯一的 group-empty 结果。
pub(super) fn process_group_members(pgid: libc::pid_t) -> io::Result<Vec<libc::pid_t>> {
    clear_errno();
    // SAFETY: null buffer 与零长度是 libproc 查询成员数量的约定调用方式。
    let count = unsafe { libc::proc_listpgrppids(pgid, ptr::null_mut(), 0) };
    if count <= 0 {
        let errno = current_errno();
        return if count == 0 && errno == 0 {
            Ok(Vec::new())
        } else if errno != 0 {
            Err(io::Error::from_raw_os_error(errno))
        } else {
            Err(io::Error::from_raw_os_error(libc::EPROTO))
        };
    }

    let initial_count =
        usize::try_from(count).map_err(|_| io::Error::from_raw_os_error(libc::EOVERFLOW))?;
    let mut capacity = initial_count
        .checked_add(16)
        .ok_or_else(|| io::Error::from_raw_os_error(libc::EOVERFLOW))?;

    // 成员可能在计数和填充之间增长，最多扩容重试三次后明确失败。
    for _ in 0..3 {
        let byte_capacity = capacity
            .checked_mul(std::mem::size_of::<libc::pid_t>())
            .and_then(|bytes| libc::c_int::try_from(bytes).ok())
            .ok_or_else(|| io::Error::from_raw_os_error(libc::EOVERFLOW))?;
        let mut members = vec![0; capacity];
        clear_errno();
        // SAFETY: members 提供 byte_capacity 对应的连续可写 pid_t 缓冲区。
        let listed =
            unsafe { libc::proc_listpgrppids(pgid, members.as_mut_ptr().cast(), byte_capacity) };
        if listed <= 0 {
            let errno = current_errno();
            return if listed == 0 && errno == 0 {
                Ok(Vec::new())
            } else if errno != 0 {
                Err(io::Error::from_raw_os_error(errno))
            } else {
                Err(io::Error::from_raw_os_error(libc::EPROTO))
            };
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

/// 使用固定 executable 与逐项 argv 创建独立 macOS Session，并在父侧验证身份。
pub(super) fn launch(request: &MacosLaunchRequest) -> Result<LaunchedChild, MacosLaunchFailure> {
    validate(request).map_err(|error| MacosLaunchFailure {
        code: error.code,
        message: error.message,
        created: None,
    })?;

    let mut command = Command::new(&request.executable);
    command
        .args(&request.args)
        .current_dir(&request.current_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // SAFETY: 闭包只调用 setsid/getpid/getpgid/getsid 并构造固定 errno，不访问其他线程状态。
    unsafe {
        command.pre_exec(|| {
            let session = libc::setsid();
            if session < 0 {
                return Err(io::Error::last_os_error());
            }
            let pid = libc::getpid();
            let pgid = libc::getpgid(0);
            let sid = libc::getsid(0);
            if session != pid || pgid != pid || sid != pid {
                return Err(io::Error::from_raw_os_error(libc::EPROTO));
            }
            Ok(())
        });
    }

    let mut process = command.spawn().map_err(|error| MacosLaunchFailure {
        code: "CODEX_PROCESS_CREATE_FAILED",
        message: error.to_string(),
        created: None,
    })?;
    let pid = process.id() as libc::pid_t;

    // Stdio::piped 在 spawn 成功时由标准库保证三个句柄均存在；先完整取得后再构造 ownership。
    let stdin = process
        .stdin
        .take()
        .expect("piped child stdin must exist after successful spawn");
    let stdout = process
        .stdout
        .take()
        .expect("piped child stdout must exist after successful spawn");
    let stderr = process
        .stderr
        .take()
        .expect("piped child stderr must exist after successful spawn");
    let mut created = Box::new(CreatedChild {
        stdin,
        stdout,
        stderr,
        process,
        pid,
        pgid: pid,
    });

    let identity = match MacosProcessIdentityAdapter::observe(pid) {
        Ok(identity) => identity,
        Err(error) => {
            return Err(MacosLaunchFailure {
                code: "CODEX_PROCESS_IDENTITY_FAILED",
                message: error.to_string(),
                created: Some(created),
            });
        }
    };
    if identity.pid != identity.pgid || identity.pid != identity.sid {
        return Err(MacosLaunchFailure {
            code: "CODEX_PROCESS_IDENTITY_FAILED",
            message: "父侧观测到的 PID、PGID 与 SID 不一致".into(),
            created: Some(created),
        });
    }
    created.pgid = identity.pgid;

    Ok(LaunchedChild {
        child: *created,
        identity,
    })
}

#[cfg(test)]
mod tests;
