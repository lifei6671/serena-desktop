#![cfg(windows)]

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
    sync::Arc,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::*,
    System::{JobObjects::*, Pipes::CreatePipe, Threading::*},
};

#[derive(Debug)]
pub(crate) struct LaunchRequest {
    pub executable: PathBuf,
    pub args: Vec<OsString>,
    pub current_dir: PathBuf,
    pub environment: Vec<(OsString, OsString)>,
    pub command_run_id: String,
}

#[derive(Debug)]
pub(crate) struct LaunchedProcess {
    pub stdin: File,
    pub stdout: File,
    pub stderr: File,
    pub pid: u32,
    pub control: Arc<ProcessControl>,
}

#[derive(Debug)]
pub(crate) struct ProcessControl {
    process: OwnedHandle,
    job: OwnedHandle,
}

#[derive(Debug)]
pub(crate) struct LaunchError {
    pub code: &'static str,
    pub win32_error: u32,
}

impl std::fmt::Display for LaunchError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{} (Win32 {})", self.code, self.win32_error)
    }
}

impl std::error::Error for LaunchError {}

fn failure(code: &'static str, win32_error: u32) -> LaunchError {
    LaunchError { code, win32_error }
}

fn wide(value: &OsStr) -> Result<Vec<u16>, LaunchError> {
    let mut result = value.encode_wide().collect::<Vec<_>>();
    if result.contains(&0) {
        return Err(failure(
            "COMMAND_LAUNCH_INPUT_INVALID",
            ERROR_INVALID_PARAMETER,
        ));
    }
    result.push(0);
    Ok(result)
}

/// Microsoft CRT argv quoting. Keep this identical in strength to Codex launcher:
/// every argument is quoted and backslashes before quotes/trailing quotes are doubled.
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
                    "COMMAND_LAUNCH_INPUT_INVALID",
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
            "COMMAND_LAUNCH_INPUT_INVALID",
            ERROR_INVALID_PARAMETER,
        ));
    }
    Ok(result)
}

fn environment_block(values: &[(OsString, OsString)]) -> Result<Vec<u16>, LaunchError> {
    let mut entries = values
        .iter()
        .map(|(key, value)| {
            let key_units = key.encode_wide().collect::<Vec<_>>();
            let value_units = value.encode_wide().collect::<Vec<_>>();
            if key_units.is_empty()
                || key_units.contains(&0)
                || value_units.contains(&0)
                || key_units.contains(&(b'=' as u16))
            {
                return Err(failure("COMMAND_ENV_INVALID", ERROR_INVALID_PARAMETER));
            }
            Ok((key.to_string_lossy().to_uppercase(), key_units, value_units))
        })
        .collect::<Result<Vec<_>, LaunchError>>()?;
    entries.sort_by(|left, right| left.0.cmp(&right.0));

    let mut block = Vec::new();
    for (_, key, value) in entries {
        block.extend(key);
        block.push(b'=' as u16);
        block.extend(value);
        block.push(0);
    }
    block.push(0);
    Ok(block)
}

struct Attributes {
    storage: Vec<usize>,
}

impl Attributes {
    fn new() -> Result<Self, LaunchError> {
        let mut bytes = 0;
        unsafe {
            let result = InitializeProcThreadAttributeList(null_mut(), 2, 0, &mut bytes);
            let error = GetLastError();
            if result != 0 || error != ERROR_INSUFFICIENT_BUFFER || bytes == 0 {
                return Err(failure("COMMAND_JOB_AT_CREATION_UNSUPPORTED", error));
            }
            let mut storage = vec![0usize; bytes.div_ceil(size_of::<usize>())];
            if InitializeProcThreadAttributeList(storage.as_mut_ptr().cast(), 2, 0, &mut bytes) == 0
            {
                return Err(failure(
                    "COMMAND_JOB_AT_CREATION_UNSUPPORTED",
                    GetLastError(),
                ));
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
        unsafe {
            DeleteProcThreadAttributeList(self.ptr());
        }
    }
}

fn pipe(child_reads: bool) -> Result<(OwnedHandle, OwnedHandle), LaunchError> {
    let (mut read, mut write) = (null_mut(), null_mut());
    unsafe {
        if CreatePipe(&mut read, &mut write, null(), 0) == 0 {
            return Err(failure("COMMAND_PIPE_CREATE_FAILED", GetLastError()));
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
            return Err(failure("COMMAND_HANDLE_POLICY_FAILED", GetLastError()));
        }
        Ok((child, parent))
    }
}

pub(crate) fn launch(request: &LaunchRequest) -> Result<LaunchedProcess, LaunchError> {
    if !request.executable.is_absolute()
        || !request.current_dir.is_absolute()
        || request.command_run_id.is_empty()
        || request.command_run_id.contains(['\\', '/', '\0'])
    {
        return Err(failure("COMMAND_LAUNCH_INPUT_INVALID", ERROR_INVALID_PARAMETER));
    }

    let executable = wide(request.executable.as_os_str())?;
    let directory = wide(request.current_dir.as_os_str())?;
    let mut argv = command_line(request.executable.as_os_str(), &request.args)?;
    let mut environment = environment_block(&request.environment)?;
    let job_name = wide(OsStr::new(&format!(
        "Local\\SerenaDesktop.Command.{}",
        request.command_run_id
    )))?;

    unsafe {
        let handle = CreateJobObjectW(null(), job_name.as_ptr());
        let error = GetLastError();
        if handle.is_null() {
            return Err(failure("COMMAND_JOB_CREATE_FAILED", error));
        }
        let job = OwnedHandle::from_raw_handle(handle);
        if error == ERROR_ALREADY_EXISTS {
            return Err(failure("COMMAND_JOB_NAME_COLLISION", error));
        }
        if SetHandleInformation(job.as_raw_handle(), HANDLE_FLAG_INHERIT, 0) == 0 {
            return Err(failure("COMMAND_HANDLE_POLICY_FAILED", GetLastError()));
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
            return Err(failure("COMMAND_JOB_POLICY_FAILED", GetLastError()));
        }

        let (stdin_read, stdin_write) = pipe(true)?;
        let (stdout_write, stdout_read) = pipe(false)?;
        let (stderr_write, stderr_read) = pipe(false)?;

        let jobs = [job.as_raw_handle()];
        let handles = [
            stdin_read.as_raw_handle(),
            stdout_write.as_raw_handle(),
            stderr_write.as_raw_handle(),
        ];
        let mut attributes = Attributes::new()?;
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
            return Err(failure(
                "COMMAND_JOB_AT_CREATION_UNSUPPORTED",
                GetLastError(),
            ));
        }
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
            return Err(failure("COMMAND_HANDLE_POLICY_FAILED", GetLastError()));
        }

        let mut startup: STARTUPINFOEXW = zeroed();
        startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
        startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        startup.StartupInfo.hStdInput = handles[0];
        startup.StartupInfo.hStdOutput = handles[1];
        startup.StartupInfo.hStdError = handles[2];
        startup.lpAttributeList = attributes.ptr();

        let mut info: PROCESS_INFORMATION = zeroed();
        if CreateProcessW(
            executable.as_ptr(),
            argv.as_mut_ptr(),
            null(),
            null(),
            1,
            EXTENDED_STARTUPINFO_PRESENT | CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT,
            environment.as_mut_ptr().cast(),
            directory.as_ptr(),
            &startup.StartupInfo,
            &mut info,
        ) == 0
        {
            return Err(failure("COMMAND_PROCESS_CREATE_FAILED", GetLastError()));
        }

        drop(stdin_read);
        drop(stdout_write);
        drop(stderr_write);
        let process = OwnedHandle::from_raw_handle(info.hProcess);
        let thread = OwnedHandle::from_raw_handle(info.hThread);
        drop(thread);

        Ok(LaunchedProcess {
            stdin: File::from(stdin_write),
            stdout: File::from(stdout_read),
            stderr: File::from(stderr_read),
            pid: info.dwProcessId,
            control: Arc::new(ProcessControl { process, job }),
        })
    }
}

impl ProcessControl {
    pub(crate) fn wait_parent(&self) -> Result<i32, LaunchError> {
        unsafe {
            let state = WaitForSingleObject(self.process.as_raw_handle(), INFINITE);
            if state != WAIT_OBJECT_0 {
                return Err(failure("COMMAND_PROCESS_WAIT_FAILED", GetLastError()));
            }
            let mut code = 0;
            if GetExitCodeProcess(self.process.as_raw_handle(), &mut code) == 0 {
                return Err(failure("COMMAND_PROCESS_WAIT_FAILED", GetLastError()));
            }
            Ok(code as i32)
        }
    }

    /// Terminate the whole Job and require Job-level empty evidence before returning.
    pub(crate) fn terminate(&self, timeout: Duration) -> Result<(), LaunchError> {
        unsafe {
            if TerminateJobObject(self.job.as_raw_handle(), 1) == 0 {
                let error = GetLastError();
                if error != ERROR_ACCESS_DENIED {
                    return Err(failure("COMMAND_PROCESS_TERMINATE_FAILED", error));
                }
            }
        }
        self.wait_job_empty(timeout)
    }

    /// Natural wrapper exit may leave background descendants. CommandRun terminal means
    /// the owned Job no longer contains writers, so close that gap before persisting receipt.
    pub(crate) fn seal_after_parent_exit(&self, timeout: Duration) -> Result<(), LaunchError> {
        self.terminate(timeout)
    }

    fn wait_job_empty(&self, timeout: Duration) -> Result<(), LaunchError> {
        let deadline = Instant::now() + timeout;
        loop {
            let mut accounting: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { zeroed() };
            let ok = unsafe {
                QueryInformationJobObject(
                    self.job.as_raw_handle(),
                    JobObjectBasicAccountingInformation,
                    (&mut accounting as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                    size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                    null_mut(),
                )
            };
            if ok == 0 {
                return Err(failure("COMMAND_JOB_QUERY_FAILED", unsafe {
                    GetLastError()
                }));
            }
            if accounting.ActiveProcesses == 0 {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(failure("COMMAND_PROCESS_TERMINATION_TIMEOUT", WAIT_TIMEOUT));
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argv_quoting_handles_spaces_quotes_backslashes_and_empty_values() {
        let line = command_line(
            OsStr::new(r"C:\\Program Files\\tool.exe"),
            &[
                OsString::from("plain"),
                OsString::from("with space"),
                OsString::from(r#"quote"value"#),
                OsString::from(r"trail\\"),
                OsString::from(""),
            ],
        )
        .unwrap();
        let decoded = String::from_utf16_lossy(&line[..line.len() - 1]);
        assert!(decoded.contains(r#""with space""#));
        assert!(decoded.contains(r#""quote\"value""#));
        assert!(decoded.ends_with(r#""""#));
    }

    #[test]
    fn environment_block_is_double_nul_terminated() {
        let block = environment_block(&[
            (OsString::from("PATH"), OsString::from(r"C:\\Tools")),
            (OsString::from("TEMP"), OsString::from(r"C:\\Temp")),
        ])
        .unwrap();
        assert_eq!(block.last(), Some(&0));
        assert_eq!(block[block.len() - 2], 0);
    }
}
