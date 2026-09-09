//! Job-at-creation launcher only. No shell, Runtime state, DB or recovery service.
use std::{
    ffi::{OsStr, OsString},
    fs::File,
    mem::{size_of, zeroed},
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::PathBuf,
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Foundation::*,
    System::{JobObjects::*, Pipes::CreatePipe, Threading::*},
};

#[derive(Debug)]
pub struct LaunchRequest {
    pub executable: PathBuf,
    pub args: Vec<OsString>,
    pub current_dir: PathBuf,
    pub runtime_instance_id: String,
}

/// Unique owner. Dropping closes all handles; KILL_ON_JOB_CLOSE is a kernel policy,
/// not proof of Runtime termination. No Job handle is duplicated or sent to Child.
#[derive(Debug)]
pub struct CreatedChild {
    pub stdin: File,
    pub stdout: File,
    pub stderr: File,
    pub pid: u32,
    pub(super) process: OwnedHandle,
    pub(super) job: OwnedHandle,
}

#[derive(Debug)]
pub struct LaunchedChild {
    pub child: CreatedChild,
    /// Raw Creation FILETIME, not Unix milliseconds; persist as 16 hex digits.
    pub creation_filetime: u64,
}
impl LaunchedChild {
    pub fn process_start_token(&self) -> String {
        format!("{:016x}", self.creation_filetime)
    }
}

#[derive(Debug)]
pub struct LaunchError {
    pub code: &'static str,
    pub win32_error: u32,
    /// Some means CreateProcessW succeeded. The caller owns this process and must
    /// perform TASK-004 termination/evidence handling; never release a Claim here.
    pub created: Option<Box<CreatedChild>>,
}
impl std::fmt::Display for LaunchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} (Win32 {})", self.code, self.win32_error)
    }
}
impl std::error::Error for LaunchError {}
fn failure(code: &'static str, win32_error: u32) -> LaunchError {
    LaunchError {
        code,
        win32_error,
        created: None,
    }
}

fn wide(value: &OsStr) -> Result<Vec<u16>, LaunchError> {
    let mut result: Vec<u16> = value.encode_wide().collect();
    if result.contains(&0) {
        return Err(failure(
            "CODEX_LAUNCH_INPUT_INVALID",
            ERROR_INVALID_PARAMETER,
        ));
    }
    result.push(0);
    Ok(result)
}

/// Microsoft CRT argv convention, applied to each argument including argv[0].
/// Work on UTF-16 so Windows paths are not lossy-converted through UTF-8.
fn command_line(executable: &OsStr, args: &[OsString]) -> Result<Vec<u16>, LaunchError> {
    let mut result = Vec::new();
    for (index, arg) in std::iter::once(executable)
        .chain(args.iter().map(OsString::as_os_str))
        .enumerate()
    {
        if index != 0 {
            result.push(b' ' as u16);
        }
        result.push(b'"' as u16);
        let mut slashes = 0;
        for unit in arg.encode_wide() {
            if unit == 0 {
                return Err(failure(
                    "CODEX_LAUNCH_INPUT_INVALID",
                    ERROR_INVALID_PARAMETER,
                ));
            }
            if unit == b'\\' as u16 {
                slashes += 1;
                continue;
            }
            result.extend(std::iter::repeat_n(
                b'\\' as u16,
                if unit == b'"' as u16 {
                    slashes * 2 + 1
                } else {
                    slashes
                },
            ));
            slashes = 0;
            result.push(unit);
        }
        result.extend(std::iter::repeat_n(b'\\' as u16, slashes * 2));
        result.push(b'"' as u16);
    }
    result.push(0);
    if result.len() > 32767 {
        return Err(failure(
            "CODEX_LAUNCH_INPUT_INVALID",
            ERROR_INVALID_PARAMETER,
        ));
    }
    Ok(result)
}

struct Attributes {
    storage: Vec<usize>,
}
impl Attributes {
    fn new() -> Result<Self, LaunchError> {
        let mut bytes = 0;
        // SAFETY: size probe takes a null list, then we allocate suitably aligned
        // pointer-sized storage and keep it alive until DeleteProcThreadAttributeList.
        unsafe {
            let result = InitializeProcThreadAttributeList(null_mut(), 2, 0, &mut bytes);
            let error = GetLastError();
            if result != 0 || error != ERROR_INSUFFICIENT_BUFFER || bytes == 0 {
                return Err(failure("CODEX_JOB_AT_CREATION_UNSUPPORTED", error));
            }
            let mut storage = vec![0usize; bytes.div_ceil(size_of::<usize>())];
            if InitializeProcThreadAttributeList(storage.as_mut_ptr().cast(), 2, 0, &mut bytes) == 0
            {
                return Err(failure("CODEX_JOB_AT_CREATION_UNSUPPORTED", GetLastError()));
            }
            Ok(Self { storage })
        }
    }
    fn ptr(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.storage.as_mut_ptr().cast()
    }
}
impl Drop for Attributes {
    fn drop(&mut self) {
        // SAFETY: only constructed after successful initialization; storage alive.
        unsafe {
            DeleteProcThreadAttributeList(self.ptr());
        }
    }
}

fn pipe(child_reads: bool) -> Result<(OwnedHandle, OwnedHandle), LaunchError> {
    let (mut read, mut write) = (null_mut(), null_mut());
    // SAFETY: valid output pointers; null security attributes initially make both
    // ends non-inheritable. Adopt successful handles exactly once before fallible work.
    unsafe {
        if CreatePipe(&mut read, &mut write, null(), 0) == 0 {
            return Err(failure("CODEX_PIPE_CREATE_FAILED", GetLastError()));
        }
        let read = OwnedHandle::from_raw_handle(read);
        let write = OwnedHandle::from_raw_handle(write);
        let (child, parent) = if child_reads {
            (read, write)
        } else {
            (write, read)
        };
        if SetHandleInformation(
            child.as_raw_handle(),
            HANDLE_FLAG_INHERIT,
            HANDLE_FLAG_INHERIT,
        ) == 0
            || SetHandleInformation(parent.as_raw_handle(), HANDLE_FLAG_INHERIT, 0) == 0
        {
            return Err(failure("CODEX_HANDLE_POLICY_FAILED", GetLastError()));
        }
        Ok((child, parent))
    }
}

/// Blocking Win32 API: call from the host's blocking worker. No application startup
/// integration is installed by this module. `executable` must be an absolute path.
pub fn launch(request: &LaunchRequest) -> Result<LaunchedChild, LaunchError> {
    launch_inner(
        request,
        #[cfg(test)]
        None,
    )
}

fn launch_inner(
    request: &LaunchRequest,
    #[cfg(test)] fault: Option<Checkpoint>,
) -> Result<LaunchedChild, LaunchError> {
    launch_with_policy(
        request,
        &mut |_| Ok(()),
        #[cfg(test)]
        fault,
    )
}

/// Runtime persists verified policy before CreateProcessW can be called.
pub(super) fn launch_with_policy(
    request: &LaunchRequest,
    policy_verified: &mut impl FnMut(HANDLE) -> Result<(), LaunchError>,
    #[cfg(test)] fault: Option<Checkpoint>,
) -> Result<LaunchedChild, LaunchError> {
    if !request.executable.is_absolute()
        || !request.current_dir.is_absolute()
        || request.runtime_instance_id.is_empty()
        || request.runtime_instance_id.contains(['\\', '/', '\0'])
    {
        return Err(failure(
            "CODEX_LAUNCH_INPUT_INVALID",
            ERROR_INVALID_PARAMETER,
        ));
    }
    let executable = wide(request.executable.as_os_str())?;
    let directory = wide(request.current_dir.as_os_str())?;
    let mut argv = command_line(request.executable.as_os_str(), &request.args)?;
    let job_name = wide(OsStr::new(&format!(
        "Local\\SerenaDesktop.Codex.{}",
        request.runtime_instance_id
    )))?;
    // SAFETY: all UTF-16 inputs are NUL-terminated and live through the calls.
    // Handles are immediately adopted, with GetLastError captured before any API.
    unsafe {
        let handle = CreateJobObjectW(null(), job_name.as_ptr());
        let error = GetLastError();
        if handle.is_null() {
            return Err(failure("CODEX_JOB_CREATE_FAILED", error));
        }
        let job = OwnedHandle::from_raw_handle(handle);
        if error == ERROR_ALREADY_EXISTS {
            return Err(failure("CODEX_JOB_NAME_COLLISION", error));
        }
        #[cfg(test)]
        super::runtime::crash_point("job_created");
        #[cfg(test)]
        fail_at(fault, Checkpoint::JobCreated)?;
        if SetHandleInformation(job.as_raw_handle(), HANDLE_FLAG_INHERIT, 0) == 0 {
            return Err(failure("CODEX_HANDLE_POLICY_FAILED", GetLastError()));
        }
        let mut policy: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
        policy.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if SetInformationJobObject(
            job.as_raw_handle(),
            JobObjectExtendedLimitInformation,
            (&policy as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        ) == 0
        {
            return Err(failure("CODEX_JOB_POLICY_FAILED", GetLastError()));
        }
        #[cfg(test)]
        fail_at(fault, Checkpoint::JobConfigured)?;
        let (stdin_read, stdin_write) = pipe(true)?;
        #[cfg(test)]
        fail_at(fault, Checkpoint::StdinCreated)?;
        let (stdout_write, stdout_read) = pipe(false)?;
        #[cfg(test)]
        fail_at(fault, Checkpoint::StdoutCreated)?;
        let (stderr_write, stderr_read) = pipe(false)?;
        #[cfg(test)]
        fail_at(fault, Checkpoint::StderrCreated)?;
        // Attribute values must outlive the attribute list, including its destructor.
        let jobs = [job.as_raw_handle()];
        let handles = [
            stdin_read.as_raw_handle(),
            stdout_write.as_raw_handle(),
            stderr_write.as_raw_handle(),
        ];
        let mut attributes = Attributes::new()?;
        #[cfg(test)]
        fail_at(fault, Checkpoint::AttributesInitialized)?;
        if UpdateProcThreadAttribute(
            attributes.ptr(),
            0,
            PROC_THREAD_ATTRIBUTE_JOB_LIST as usize,
            jobs.as_ptr().cast(),
            size_of_val(&jobs),
            null_mut(),
            null(),
        ) == 0
        {
            return Err(failure("CODEX_JOB_AT_CREATION_UNSUPPORTED", GetLastError()));
        }
        #[cfg(test)]
        fail_at(fault, Checkpoint::JobAttributeSet)?;
        if UpdateProcThreadAttribute(
            attributes.ptr(),
            0,
            PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
            handles.as_ptr().cast(),
            size_of_val(&handles),
            null_mut(),
            null(),
        ) == 0
        {
            return Err(failure("CODEX_HANDLE_POLICY_FAILED", GetLastError()));
        }
        #[cfg(test)]
        fail_at(fault, Checkpoint::HandleAttributeSet)?;
        policy_verified(job.as_raw_handle())?;
        let mut startup: STARTUPINFOEXW = zeroed();
        startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
        startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        startup.StartupInfo.hStdInput = handles[0];
        startup.StartupInfo.hStdOutput = handles[1];
        startup.StartupInfo.hStdError = handles[2];
        startup.lpAttributeList = attributes.ptr();
        let mut info: PROCESS_INFORMATION = zeroed();
        #[cfg(test)]
        super::runtime::crash_point("before_create_process");
        if CreateProcessW(
            executable.as_ptr(),
            argv.as_mut_ptr(),
            null(),
            null(),
            1,
            EXTENDED_STARTUPINFO_PRESENT | CREATE_NO_WINDOW,
            null(),
            directory.as_ptr(),
            &startup.StartupInfo,
            &mut info,
        ) == 0
        {
            return Err(failure("CODEX_PROCESS_CREATE_FAILED", GetLastError()));
        }
        // First actions after success: close our copies of Child pipe ends.
        drop(stdin_read);
        drop(stdout_write);
        drop(stderr_write);
        #[cfg(test)]
        super::runtime::crash_point("after_create_process");
        let process = OwnedHandle::from_raw_handle(info.hProcess);
        let thread = OwnedHandle::from_raw_handle(info.hThread);
        drop(thread);
        let child = CreatedChild {
            stdin: File::from(stdin_write),
            stdout: File::from(stdout_read),
            stderr: File::from(stderr_read),
            pid: info.dwProcessId,
            process,
            job,
        };
        let mut created: FILETIME = zeroed();
        let mut exited: FILETIME = zeroed();
        let mut kernel: FILETIME = zeroed();
        let mut user: FILETIME = zeroed();
        #[cfg(test)]
        if fault == Some(Checkpoint::ProcessCreated) {
            return Err(LaunchError {
                code: "CODEX_PROCESS_IDENTITY_FAILED",
                win32_error: ERROR_GEN_FAILURE,
                created: Some(Box::new(child)),
            });
        }
        if GetProcessTimes(
            child.process.as_raw_handle(),
            &mut created,
            &mut exited,
            &mut kernel,
            &mut user,
        ) == 0
        {
            return Err(LaunchError {
                code: "CODEX_PROCESS_IDENTITY_FAILED",
                win32_error: GetLastError(),
                created: Some(Box::new(child)),
            });
        }
        Ok(LaunchedChild {
            child,
            creation_filetime: ((created.dwHighDateTime as u64) << 32)
                | created.dwLowDateTime as u64,
        })
    }
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Checkpoint {
    JobCreated,
    JobConfigured,
    StdinCreated,
    StdoutCreated,
    StderrCreated,
    AttributesInitialized,
    JobAttributeSet,
    HandleAttributeSet,
    ProcessCreated,
}
#[cfg(test)]
fn fail_at(fault: Option<Checkpoint>, at: Checkpoint) -> Result<(), LaunchError> {
    if fault == Some(at) {
        Err(failure("TEST_INJECTED_API_FAILURE", ERROR_GEN_FAILURE))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
