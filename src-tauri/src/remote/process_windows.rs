//! Job-at-creation, following the existing Codex launcher's Win32 handle policy.
//! Only stderr and NUL are inherited. The unnamed Job is owned solely by the Host.
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
    System::{JobObjects::*, Pipes::CreatePipe, Threading::*},
};

pub(crate) struct ManagedChild {
    // Drop kills the Job before dropping the pipe or process handles.
    _job: OwnedHandle,
    process: OwnedHandle,
    pid: u32,
    pub stderr: Option<tokio::fs::File>,
}
struct Attributes(Vec<usize>);
impl Drop for Attributes {
    fn drop(&mut self) {
        // SAFETY: successfully initialized list; aligned storage remains alive.
        unsafe {
            DeleteProcThreadAttributeList(self.0.as_mut_ptr().cast());
        }
    }
}
fn wide(value: &OsStr) -> io::Result<Vec<u16>> {
    let mut value: Vec<_> = value.encode_wide().collect();
    if value.contains(&0) {
        return Err(io::Error::other("NUL in process input"));
    }
    value.push(0);
    Ok(value)
}
fn argv(command: &std::process::Command) -> io::Result<Vec<u16>> {
    let mut output = Vec::new();
    for arg in std::iter::once(command.get_program()).chain(command.get_args()) {
        if !output.is_empty() {
            output.push(b' ' as u16);
        }
        output.push(b'"' as u16);
        let mut slashes = 0;
        for unit in wide(arg)?.into_iter().take_while(|u| *u != 0) {
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
        return Err(io::Error::other("process command too long"));
    }
    Ok(output)
}
impl ManagedChild {
    pub(crate) fn spawn(command: &mut tokio::process::Command) -> Result<Self, String> {
        Self::create(command.as_std()).map_err(|error| {
            format!(
                "QUICK_TUNNEL_START_FAILED: Win32 {:?}",
                error.raw_os_error()
            )
        })
    }
    fn create(command: &std::process::Command) -> io::Result<Self> {
        let executable = wide(command.get_program())?;
        let mut args = argv(command)?;
        let directory = command
            .get_current_dir()
            .map(|p| wide(p.as_os_str()))
            .transpose()?;
        // Quick Tunnel removes TUNNEL_* variables on the Command; preserve those
        // explicit removals without mutating the host's environment.
        let mut environment = std::env::vars_os().collect::<std::collections::BTreeMap<_, _>>();
        for (key, value) in command.get_envs() {
            environment.retain(|k, _| {
                !k.to_string_lossy()
                    .eq_ignore_ascii_case(&key.to_string_lossy())
            });
            if let Some(value) = value {
                environment.insert(key.to_owned(), value.to_owned());
            }
        }
        let mut env_block = Vec::new();
        for (key, value) in environment {
            let mut pair = key;
            pair.push("=");
            pair.push(value);
            env_block.extend(wide(&pair)?);
        }
        env_block.push(0);
        let nul = File::options().read(true).write(true).open("NUL")?;
        // SAFETY: valid aligned Win32 structures; successful handles are adopted
        // immediately. All attribute values and pipes outlive CreateProcessW.
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
            let (mut read, mut write) = (null_mut(), null_mut());
            if CreatePipe(&mut read, &mut write, null(), 0) == 0 {
                return Err(io::Error::last_os_error());
            }
            let read = OwnedHandle::from_raw_handle(read);
            let write = OwnedHandle::from_raw_handle(write);
            for handle in [write.as_raw_handle(), nul.as_raw_handle()] {
                if SetHandleInformation(handle, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) == 0 {
                    return Err(io::Error::last_os_error());
                }
            }
            let jobs = [job.as_raw_handle()];
            let handles = [write.as_raw_handle(), nul.as_raw_handle()];
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
            startup.StartupInfo.hStdError = write.as_raw_handle();
            startup.lpAttributeList = attributes.0.as_mut_ptr().cast();
            let mut info: PROCESS_INFORMATION = zeroed();
            // No suspended/assign/resume race, no breakaway flags, no fallback.
            if CreateProcessW(
                executable.as_ptr(),
                args.as_mut_ptr(),
                null(),
                null(),
                1,
                EXTENDED_STARTUPINFO_PRESENT | CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT,
                env_block.as_ptr().cast(),
                directory.as_ref().map_or(null(), |v| v.as_ptr()),
                &startup.StartupInfo,
                &mut info,
            ) == 0
            {
                return Err(io::Error::last_os_error());
            }
            let process = OwnedHandle::from_raw_handle(info.hProcess);
            drop(OwnedHandle::from_raw_handle(info.hThread));
            drop(write);
            Ok(Self {
                _job: job,
                process,
                pid: info.dwProcessId,
                stderr: Some(tokio::fs::File::from_std(File::from(read))),
            })
        }
    }
    pub(crate) fn id(&self) -> Option<u32> {
        Some(self.pid)
    }
    pub(crate) fn try_wait(&mut self) -> io::Result<Option<()>> {
        // SAFETY: owned live process handle; zero timeout never blocks executor.
        match unsafe { WaitForSingleObject(self.process.as_raw_handle(), 0) } {
            WAIT_OBJECT_0 => Ok(Some(())),
            WAIT_TIMEOUT => Ok(None),
            _ => Err(io::Error::last_os_error()),
        }
    }
    pub(crate) async fn wait(&mut self) -> io::Result<()> {
        while self.try_wait()?.is_none() {
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        Ok(())
    }
    pub(crate) fn start_kill(&mut self) -> io::Result<()> {
        // SAFETY: owned live Job handle; terminates only this managed process tree.
        if unsafe { TerminateJobObject(self._job.as_raw_handle(), 1) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn child_is_in_noninheritable_kill_job_without_breakaway() {
        let executable = crate::serena::find_executable("ping.exe").unwrap();
        let mut command = crate::mcp::process::command(executable);
        command.args(["-n", "30", "127.0.0.1"]);
        let mut child = ManagedChild::spawn(&mut command).unwrap();
        // SAFETY: handles owned by child; exact output buffer sizes.
        unsafe {
            let mut in_job = 0;
            assert_ne!(
                IsProcessInJob(
                    child.process.as_raw_handle(),
                    child._job.as_raw_handle(),
                    &mut in_job
                ),
                0
            );
            assert_ne!(in_job, 0);
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
            assert_ne!(
                QueryInformationJobObject(
                    child._job.as_raw_handle(),
                    JobObjectExtendedLimitInformation,
                    (&mut limits as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                    size_of_val(&limits) as u32,
                    null_mut()
                ),
                0
            );
            assert_eq!(
                limits.BasicLimitInformation.LimitFlags,
                JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
            );
            let mut flags = 0;
            assert_ne!(
                GetHandleInformation(child._job.as_raw_handle(), &mut flags),
                0
            );
            assert_eq!(flags & HANDLE_FLAG_INHERIT, 0);
        }
        child.start_kill().unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), child.wait())
            .await
            .unwrap()
            .unwrap();
    }
}
