//! 单次 index 的 Job-at-creation。沿用现有 remote/process_windows 的 Win32 containment 语义。
use std::{
    ffi::OsStr,
    fs::File,
    io,
    mem::{size_of, zeroed},
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Foundation::*,
    System::{JobObjects::*, Threading::*},
};

/// Job 在 process handle 前释放，终止所有 index/Language Server 子孙。
pub(super) struct IndexProcess {
    _job: OwnedHandle,
    process: OwnedHandle,
}

/// Attribute list 在 CreateProcess 返回前始终保有对齐存储。
struct Attributes(Vec<usize>);
impl Drop for Attributes {
    fn drop(&mut self) {
        // SAFETY: 构造完成后才创建本 guard，底层存储仍存活。
        unsafe {
            DeleteProcThreadAttributeList(self.0.as_mut_ptr().cast());
        }
    }
}

/// Windows 字符串只允许一个结尾 NUL。
fn wide(value: &OsStr) -> io::Result<Vec<u16>> {
    let mut value = value.encode_wide().collect::<Vec<_>>();
    if value.contains(&0) {
        return Err(io::Error::other("invalid process input"));
    }
    value.push(0);
    Ok(value)
}

/// 编码直接 argv 所需的 CRT quoting，不经过 cmd/PowerShell 或 shell 解析。
fn argv(command: &std::process::Command) -> io::Result<Vec<u16>> {
    let mut output = Vec::new();
    for arg in std::iter::once(command.get_program()).chain(command.get_args()) {
        if !output.is_empty() {
            output.push(b' ' as u16);
        }
        output.push(b'"' as u16);
        let mut slashes = 0;
        for unit in wide(arg)?.into_iter().take_while(|unit| *unit != 0) {
            if unit == b'\\' as u16 {
                slashes += 1;
                continue;
            }
            output.extend(std::iter::repeat_n(
                b'\\' as u16,
                if unit == b'"' as u16 {
                    slashes * 2 + 1
                } else {
                    slashes
                },
            ));
            slashes = 0;
            output.push(unit);
        }
        output.extend(std::iter::repeat_n(b'\\' as u16, slashes * 2));
        output.push(b'"' as u16);
    }
    output.push(0);
    if output.len() > 32767 {
        return Err(io::Error::other("process input too long"));
    }
    Ok(output)
}

impl IndexProcess {
    /// 创建进程时原子加入私有 Job；所有标准流接 NUL，只有 NUL handle 被继承。
    pub(super) fn spawn(command: std::process::Command) -> io::Result<Self> {
        let executable = wide(command.get_program())?;
        let mut args = argv(&command)?;
        let mut environment = std::env::vars_os().collect::<std::collections::BTreeMap<_, _>>();
        for (key, value) in command.get_envs() {
            environment.retain(|existing, _| {
                !existing
                    .to_string_lossy()
                    .eq_ignore_ascii_case(&key.to_string_lossy())
            });
            if let Some(value) = value {
                environment.insert(key.to_owned(), value.to_owned());
            }
        }
        let mut env_block = Vec::new();
        for (mut key, value) in environment {
            key.push("=");
            key.push(value);
            env_block.extend(wide(&key)?);
        }
        env_block.push(0);
        let nul = File::options().read(true).write(true).open("NUL")?;
        // SAFETY: 所有结构正确对齐，指针对象活到 CreateProcessW 返回，成功句柄立即交给 RAII。
        unsafe {
            let raw = CreateJobObjectW(null(), null());
            if raw.is_null() {
                return Err(io::Error::last_os_error());
            }
            let job = OwnedHandle::from_raw_handle(raw);
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                job.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            ) == 0
            {
                return Err(io::Error::last_os_error());
            }
            if SetHandleInformation(
                nul.as_raw_handle(),
                HANDLE_FLAG_INHERIT,
                HANDLE_FLAG_INHERIT,
            ) == 0
            {
                return Err(io::Error::last_os_error());
            }
            let jobs = [job.as_raw_handle()];
            let handles = [nul.as_raw_handle()];
            let mut bytes = 0;
            InitializeProcThreadAttributeList(null_mut(), 2, 0, &mut bytes);
            if bytes == 0 {
                return Err(io::Error::last_os_error());
            }
            let mut storage = vec![0usize; bytes.div_ceil(size_of::<usize>())];
            if InitializeProcThreadAttributeList(storage.as_mut_ptr().cast(), 2, 0, &mut bytes) == 0
            {
                return Err(io::Error::last_os_error());
            }
            let mut attributes = Attributes(storage);
            for (attribute, pointer, bytes) in [
                (
                    PROC_THREAD_ATTRIBUTE_JOB_LIST,
                    jobs.as_ptr().cast(),
                    size_of_val(&jobs),
                ),
                (
                    PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
                    handles.as_ptr().cast(),
                    size_of_val(&handles),
                ),
            ] {
                if UpdateProcThreadAttribute(
                    attributes.0.as_mut_ptr().cast(),
                    0,
                    attribute as usize,
                    pointer,
                    bytes,
                    null_mut(),
                    null(),
                ) == 0
                {
                    return Err(io::Error::last_os_error());
                }
            }
            let mut startup: STARTUPINFOEXW = zeroed();
            startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
            startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
            startup.StartupInfo.hStdInput = nul.as_raw_handle();
            startup.StartupInfo.hStdOutput = nul.as_raw_handle();
            startup.StartupInfo.hStdError = nul.as_raw_handle();
            startup.lpAttributeList = attributes.0.as_mut_ptr().cast();
            let mut info: PROCESS_INFORMATION = zeroed();
            if CreateProcessW(
                executable.as_ptr(),
                args.as_mut_ptr(),
                null(),
                null(),
                1,
                EXTENDED_STARTUPINFO_PRESENT | CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT,
                env_block.as_ptr().cast(),
                null(),
                &startup.StartupInfo,
                &mut info,
            ) == 0
            {
                return Err(io::Error::last_os_error());
            }
            let process = OwnedHandle::from_raw_handle(info.hProcess);
            drop(OwnedHandle::from_raw_handle(info.hThread));
            Ok(Self { _job: job, process })
        }
    }

    /// 仅暴露退出成功与否；公共 Activity 不含 exit code、PID 或 Win32 error。
    pub(super) fn try_wait(&mut self) -> io::Result<Option<bool>> {
        // SAFETY: process 由本对象唯一持有，零超时不阻塞 runtime。
        unsafe {
            match WaitForSingleObject(self.process.as_raw_handle(), 0) {
                WAIT_TIMEOUT => Ok(None),
                WAIT_OBJECT_0 => {
                    let mut code = 0;
                    if GetExitCodeProcess(self.process.as_raw_handle(), &mut code) == 0 {
                        return Err(io::Error::last_os_error());
                    }
                    Ok(Some(code == 0))
                }
                _ => Err(io::Error::last_os_error()),
            }
        }
    }
}
