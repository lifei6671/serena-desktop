//! 窄 Windows Job recovery，不共享 Codex runtime 的证据或 PID identity。
use super::{TerminationEvidence, valid_identity};
use crate::agent::{coordinator::now, store::RuntimeRecord};
use std::{
    mem::{size_of, zeroed},
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    ptr::null_mut,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::*,
    System::{
        JobObjects::*,
        RemoteDesktop::ProcessIdToSessionId,
        SystemServices::{JOB_OBJECT_QUERY, JOB_OBJECT_TERMINATE},
        Threading::GetCurrentProcessId,
    },
};

/// 获取当前 Local Job namespace，无法确认时不打开任何 Job。
pub(super) fn session() -> Result<u32, String> {
    #[cfg(test)]
    if FAULT.get() == Some(Fault::Session) {
        return Err("CODEBUDDY_SESSION_QUERY_FAILED".into());
    }
    let mut session = 0;
    // SAFETY: 当前 PID 有效，输出指针在调用期间有效。
    if unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut session) } == 0 {
        return Err("CODEBUDDY_SESSION_QUERY_FAILED".into());
    }
    Ok(session)
}

/// 当前 handle 和 live Job policy 必须符合 first-runnable ownership 契约。
fn policy(job: &OwnedHandle) -> Result<(), String> {
    #[cfg(test)]
    if FAULT
        .get()
        .is_some_and(|f| matches!(f, Fault::Policy | Fault::PolicyQuery))
    {
        return Err("CODEBUDDY_JOB_POLICY_FAILED".into());
    }
    let mut flags = 0;
    let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
    // SAFETY: OwnedHandle 保持有效，输出 buffer 与 Win32 class 匹配。
    if unsafe { GetHandleInformation(job.as_raw_handle(), &mut flags) } == 0
        || flags & HANDLE_FLAG_INHERIT != 0
    {
        return Err("CODEBUDDY_JOB_HANDLE_POLICY_FAILED".into());
    }
    if unsafe {
        QueryInformationJobObject(
            job.as_raw_handle(),
            JobObjectExtendedLimitInformation,
            (&mut limits as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            null_mut(),
        )
    } == 0
    {
        return Err("CODEBUDDY_JOB_POLICY_QUERY_FAILED".into());
    }
    let flags = limits.BasicLimitInformation.LimitFlags;
    if flags & JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE == 0
        || flags & (JOB_OBJECT_LIMIT_BREAKAWAY_OK | JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK) != 0
    {
        return Err("CODEBUDDY_JOB_POLICY_INVALID".into());
    }
    Ok(())
}

/// ActiveProcesses 精确为零才构成 Job-level evidence。
fn active(job: &OwnedHandle) -> Result<u32, String> {
    #[cfg(test)]
    match FAULT.get() {
        Some(Fault::Query) => return Err("CODEBUDDY_JOB_QUERY_FAILED".into()),
        Some(Fault::Timeout | Fault::Terminate) => return Ok(1),
        _ => {}
    }
    let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { zeroed() };
    // SAFETY: handle 有效，buffer/class/size 一致。
    if unsafe {
        QueryInformationJobObject(
            job.as_raw_handle(),
            JobObjectBasicAccountingInformation,
            (&mut info as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
            size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
            null_mut(),
        )
    } == 0
    {
        return Err("CODEBUDDY_JOB_QUERY_FAILED".into());
    }
    Ok(info.ActiveProcesses)
}

/// 同步操作只在 blocking worker 执行；名称消失也必须先验证原 identity/policy/session。
pub(super) fn observe(r: RuntimeRecord, timeout: Duration) -> Result<TerminationEvidence, String> {
    valid_identity(&r, session()?)?;
    let name: Vec<u16> = r
        .job_name
        .as_ref()
        .unwrap()
        .encode_utf16()
        .chain(Some(0))
        .collect();
    // SAFETY: NUL terminated name 已验证，FALSE 禁止 handle 继承，仅申请 query/terminate。
    #[cfg(test)]
    let injected_error = match FAULT.get() {
        Some(Fault::AccessDenied) => Some(ERROR_ACCESS_DENIED),
        Some(Fault::Open) => Some(ERROR_INVALID_HANDLE),
        _ => None,
    };
    #[cfg(not(test))]
    let raw = unsafe { OpenJobObjectW(JOB_OBJECT_QUERY | JOB_OBJECT_TERMINATE, 0, name.as_ptr()) };
    #[cfg(test)]
    let raw = if injected_error.is_some() {
        null_mut()
    } else {
        unsafe { OpenJobObjectW(JOB_OBJECT_QUERY | JOB_OBJECT_TERMINATE, 0, name.as_ptr()) }
    };
    let kind = if raw.is_null() {
        #[cfg(not(test))]
        let open_error = unsafe { GetLastError() };
        #[cfg(test)]
        let open_error = injected_error.unwrap_or_else(|| unsafe { GetLastError() });
        // 生产 Win32 错误与测试注入错误共用精确分类，只有 FILE_NOT_FOUND 可构成 destroyed。
        if open_error != ERROR_FILE_NOT_FOUND {
            return Err("CODEBUDDY_JOB_OPEN_FAILED".into());
        }
        "managed_job_destroyed"
    } else {
        // SAFETY: 成功打开的独占 handle 交给 RAII，任何返回路径都关闭。
        let job = unsafe { OwnedHandle::from_raw_handle(raw) };
        policy(&job)?;
        if active(&job)? != 0 {
            #[cfg(test)]
            if FAULT.get() == Some(Fault::Terminate) {
                return Err("CODEBUDDY_JOB_TERMINATE_FAILED".into());
            }
            // SAFETY: 只终止已验证的原 CodeBuddy Job，不使用 PID。
            if unsafe { TerminateJobObject(job.as_raw_handle(), 1) } == 0 {
                return Err("CODEBUDDY_JOB_TERMINATE_FAILED".into());
            }
            let deadline = Instant::now() + timeout;
            while active(&job)? != 0 {
                if Instant::now() >= deadline {
                    return Err("CODEBUDDY_JOB_TERMINATION_TIMEOUT".into());
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        "job_active_processes_zero"
    };
    Ok(TerminationEvidence {
        original: r,
        kind,
        at: now(),
    })
}

/// 只在测试 observer 的 blocking 线程注入 OS 故障，不改变生产 API 或全局并行测试。
#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Fault {
    Session,
    AccessDenied,
    Open,
    Query,
    Policy,
    PolicyQuery,
    Terminate,
    Timeout,
}
#[cfg(test)]
thread_local! {pub(super) static FAULT: std::cell::Cell<Option<Fault>> = const {std::cell::Cell::new(None)};}
