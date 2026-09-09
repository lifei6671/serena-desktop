//! Runtime process safety only. No Provider initialization or Execution mutation.
use super::windows_launcher::{self, CreatedChild, LaunchError, LaunchRequest};
use crate::agent::store::{RuntimeRecord, StateStore};
use std::{
    ffi::OsStr,
    mem::{size_of, zeroed},
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    ptr::null_mut,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use windows_sys::Win32::{
    Foundation::*,
    System::{
        JobObjects::*,
        RemoteDesktop::ProcessIdToSessionId,
        SystemServices::{JOB_OBJECT_QUERY, JOB_OBJECT_TERMINATE},
        Threading::*,
    },
};

pub struct Runtime {
    id: String,
    store: StateStore,
    child: Option<CreatedChild>,
    recovered_job: Option<OwnedHandle>,
}

/// Failed convergence retains ownership when available. Drop is kernel cleanup,
/// never persisted termination evidence; callers can retry terminate on `runtime`.
pub struct RuntimeFailure {
    pub code: &'static str,
    pub message: String,
    pub runtime: Option<Box<Runtime>>,
}
impl std::fmt::Debug for RuntimeFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeFailure")
            .field("code", &self.code)
            .field("message", &self.message)
            .field("retains_runtime", &self.runtime.is_some())
            .finish()
    }
}
impl std::fmt::Debug for Runtime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Runtime")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}
#[derive(Debug, PartialEq, Eq)]
pub struct RuntimeError {
    pub code: &'static str,
    pub message: String,
}
impl RuntimeError {
    pub(in crate::agent) fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}
impl From<RuntimeError> for RuntimeFailure {
    fn from(error: RuntimeError) -> Self {
        Self {
            code: error.code,
            message: error.message,
            runtime: None,
        }
    }
}

/// Sealed observation: StateStore can consume it but cannot fabricate its fields.
pub(in crate::agent) struct TerminationEvidence {
    id: String,
    kind: &'static str,
    at: i64,
}
impl TerminationEvidence {
    pub(in crate::agent) fn id(&self) -> &str {
        &self.id
    }
    pub(in crate::agent) fn kind(&self) -> &str {
        self.kind
    }
    pub(in crate::agent) fn at(&self) -> i64 {
        self.at
    }
}
fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}
fn win32(code: &'static str, operation: &str) -> RuntimeError {
    RuntimeError::new(
        code,
        format!("{operation} failed: Win32 {}", unsafe { GetLastError() }),
    )
}

pub fn current_session_id() -> Result<u32, RuntimeError> {
    let mut session = 0;
    // SAFETY: valid out pointer and current host process id.
    if unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut session) } == 0 {
        Err(win32(
            "CODEX_SESSION_ID_UNAVAILABLE",
            "ProcessIdToSessionId",
        ))
    } else {
        Ok(session)
    }
}

fn verify_policy(job: HANDLE) -> Result<(), RuntimeError> {
    let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
    let mut flags = 0;
    // SAFETY: borrowed live Job, exact aligned output struct and length.
    if unsafe {
        QueryInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            (&mut info as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            null_mut(),
        )
    } == 0
        || unsafe { GetHandleInformation(job, &mut flags) } == 0
    {
        return Err(win32("CODEX_JOB_POLICY_UNVERIFIED", "Job policy query"));
    }
    let limits = info.BasicLimitInformation.LimitFlags;
    if flags & HANDLE_FLAG_INHERIT != 0
        || limits & JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE == 0
        || limits & (JOB_OBJECT_LIMIT_BREAKAWAY_OK | JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK) != 0
    {
        return Err(RuntimeError::new(
            "CODEX_JOB_POLICY_UNVERIFIED",
            "Job inheritance or limits do not satisfy the required policy",
        ));
    }
    Ok(())
}
fn active_processes(job: HANDLE) -> Result<u32, RuntimeError> {
    let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { zeroed() };
    // SAFETY: borrowed live Job, exact size and initialized output storage.
    if unsafe {
        QueryInformationJobObject(
            job,
            JobObjectBasicAccountingInformation,
            (&mut info as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
            size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
            null_mut(),
        )
    } == 0
    {
        return Err(win32("CODEX_JOB_QUERY_FAILED", "QueryInformationJobObject"));
    }
    Ok(info.ActiveProcesses)
}
fn poll_empty(
    timeout: Duration,
    mut query: impl FnMut() -> Result<u32, RuntimeError>,
) -> Result<(), RuntimeError> {
    let start = Instant::now();
    loop {
        if query()? == 0 {
            return Ok(());
        }
        let Some(remaining) = timeout.checked_sub(start.elapsed()) else {
            return Err(RuntimeError::new(
                "CODEX_RUNTIME_TERMINATION_TIMEOUT",
                "Job did not become empty before the polling deadline",
            ));
        };
        std::thread::sleep(remaining.min(Duration::from_millis(10)));
    }
}

impl Runtime {
    pub(super) fn clone_stdio(&self) -> std::io::Result<(std::fs::File, std::fs::File, std::fs::File)> {
        let child = self.child.as_ref().ok_or_else(|| std::io::Error::other("Recovered Runtime has no stdio"))?;
        Ok((child.stdin.try_clone()?, child.stdout.try_clone()?, child.stderr.try_clone()?))
    }

    pub(super) async fn initialized(&self, identity: &super::protocol::CompatibilityIdentity) -> Result<(), RuntimeError> {
        let store = self.store.clone();
        let id = self.id.clone();
        let identity = identity.clone();
        tokio::task::spawn_blocking(move || store.runtime_initialized(&id, &identity.version, &identity.protocol_schema_sha256, now()))
            .await.map_err(|e|RuntimeError::new("CODEX_RUNTIME_STORE_FAILED",e.to_string()))?
    }
    pub fn id(&self) -> &str {
        &self.id
    }

    pub async fn create(
        store: StateStore,
        owner: String,
        request: LaunchRequest,
        timeout: Duration,
    ) -> Result<Self, RuntimeFailure> {
        tauri::async_runtime::spawn_blocking(move || {
            Self::create_blocking(
                store,
                owner,
                request,
                timeout,
                #[cfg(test)]
                None,
            )
        })
        .await
        .map_err(|e| {
            RuntimeFailure::from(RuntimeError::new(
                "CODEX_RUNTIME_WORKER_FAILED",
                e.to_string(),
            ))
        })?
    }
    fn create_blocking(
        store: StateStore,
        owner: String,
        request: LaunchRequest,
        timeout: Duration,
        #[cfg(test)] fault: Option<StartFault>,
    ) -> Result<Self, RuntimeFailure> {
        let session = current_session_id()?;
        if owner.is_empty()
            || request.runtime_instance_id.is_empty()
            || request.runtime_instance_id.contains(['\\', '/', '\0'])
            || !request.executable.is_absolute()
            || !request.current_dir.is_absolute()
        {
            return Err(RuntimeError::new(
                "CODEX_LAUNCH_INPUT_INVALID",
                "Runtime owner, ID and absolute paths are required",
            )
            .into());
        }
        // Persist paths without silently replacing invalid Unicode in identity data.
        let exe = request.executable.to_str().ok_or_else(|| {
            RuntimeError::new(
                "CODEX_LAUNCH_INPUT_INVALID",
                "Executable path is not valid Unicode",
            )
        })?;
        let id = request.runtime_instance_id.clone();
        store.prepare_runtime(
            &id,
            &owner,
            &format!("Local\\SerenaDesktop.Codex.{id}"),
            session,
            exe,
            now(),
        )?;
        #[cfg(test)]
        crash_point("row_created");
        let mut policy_error = None;
        let launched = windows_launcher::launch_with_policy(
            &request,
            &mut |job| {
                verify_policy(job)
                    .and_then(|()| store.verify_runtime_policy(&id, now()))
                    .map_err(|error| {
                        policy_error = Some(error);
                        LaunchError {
                            code: "CODEX_JOB_POLICY_UNVERIFIED",
                            win32_error: 0,
                            created: None,
                        }
                    })
            },
            #[cfg(test)]
            if fault == Some(StartFault::Identity) {
                Some(windows_launcher::Checkpoint::ProcessCreated)
            } else {
                None
            },
        );
        let launched = match launched {
            Ok(child) => child,
            Err(mut error) => {
                let cause = policy_error.take().unwrap_or_else(|| {
                    RuntimeError::new(
                        error.code,
                        format!("Launcher failed: Win32 {}", error.win32_error),
                    )
                });
                if let Some(child) = error.created.take() {
                    // CreateProcess succeeded: explicitly take ownership, terminate
                    // the entire Job and persist proof even though PID is not stored.
                    let runtime = Self {
                        id,
                        store,
                        child: Some(*child),
                        recovered_job: None,
                    };
                    return match runtime.terminate_blocking(timeout) {
                        Ok(()) => Err(cause.into()),
                        Err(mut failed) => {
                            failed.message = format!(
                                "{}: {}; cleanup: {}",
                                cause.code, cause.message, failed.message
                            );
                            Err(failed)
                        }
                    };
                }
                store.runtime_unknown(&id, &cause, now())?;
                return Err(cause.into());
            }
        };
        #[cfg(test)]
        crash_point("before_pid_persist");
        let token = launched.process_start_token();
        let runtime = Self {
            id,
            store,
            child: Some(launched.child),
            recovered_job: None,
        };
        let persist = runtime.store.start_runtime(
            &runtime.id,
            runtime.child.as_ref().unwrap().pid,
            &token,
            now(),
        );
        if let Err(error) = persist {
            return match runtime.terminate_blocking(timeout) {
                Ok(()) => Err(error.into()),
                Err(mut failed) => {
                    failed.message = format!(
                        "{}: {}; cleanup: {}",
                        error.code, error.message, failed.message
                    );
                    Err(failed)
                }
            };
        }
        // No running state until TASK-005 Provider initialize actually succeeds.
        Ok(runtime)
    }
    fn job(&self) -> HANDLE {
        self.child
            .as_ref()
            .map(|c| c.job.as_raw_handle())
            .unwrap_or_else(|| self.recovered_job.as_ref().unwrap().as_raw_handle())
    }
    fn unknown(self, mut error: RuntimeError) -> RuntimeFailure {
        if let Err(db) = self.store.runtime_unknown(&self.id, &error, now()) {
            error.message = format!("{}; persist unknown failed: {}", error.message, db.message);
        }
        RuntimeFailure {
            code: error.code,
            message: error.message,
            runtime: Some(Box::new(self)),
        }
    }
    pub async fn terminate(self, timeout: Duration) -> Result<(), RuntimeFailure> {
        tauri::async_runtime::spawn_blocking(move || self.terminate_blocking(timeout))
            .await
            .map_err(|e| {
                RuntimeFailure::from(RuntimeError::new(
                    "CODEX_RUNTIME_WORKER_FAILED",
                    e.to_string(),
                ))
            })?
    }
    fn terminate_blocking(self, timeout: Duration) -> Result<(), RuntimeFailure> {
        self.terminate_with_query(timeout, active_processes)
    }
    fn terminate_with_query(
        self,
        timeout: Duration,
        mut query: impl FnMut(HANDLE) -> Result<u32, RuntimeError>,
    ) -> Result<(), RuntimeFailure> {
        // Failure to persist intent must not prevent terminating an already created Job.
        let intent = self.store.runtime_terminating(&self.id, now());
        // SAFETY: Runtime uniquely owns this live Job handle.
        if unsafe { TerminateJobObject(self.job(), 1) } == 0 {
            return Err(self.unknown(win32("CODEX_JOB_TERMINATE_FAILED", "TerminateJobObject")));
        }
        if let Err(error) = poll_empty(timeout, || query(self.job())) {
            return Err(self.unknown(error));
        }
        let evidence = TerminationEvidence {
            id: self.id.clone(),
            kind: "job_active_processes_zero",
            at: now(),
        };
        if let Err(error) = self.store.complete_runtime(&evidence) {
            return Err(self.unknown(RuntimeError::new(
                "CODEX_RUNTIME_EVIDENCE_PERSIST_FAILED",
                format!("{}; intent={intent:?}", error.message),
            )));
        }
        // Drop/close only after the Job-empty evidence is durably committed.
        Ok(())
    }
}

fn safe_policy(row: &RuntimeRecord) -> bool {
    row.job_creation_mode.as_deref() == Some("proc_thread_attribute_job_list")
        && row.job_handle_inheritable == Some(false)
        && row.job_kill_on_close == Some(true)
        && row.job_breakaway_allowed == Some(false)
        && row.job_policy_verified_at.is_some()
}

/// Caller runs this after the previous Host has exited; never shares its live Job.
pub async fn recover(
    store: StateStore,
    id: String,
    timeout: Duration,
) -> Result<(), RuntimeFailure> {
    tauri::async_runtime::spawn_blocking(move || {
        recover_blocking(store, id, timeout, current_session_id(), open_job)
    })
    .await
    .map_err(|e| {
        RuntimeFailure::from(RuntimeError::new(
            "CODEX_RUNTIME_WORKER_FAILED",
            e.to_string(),
        ))
    })?
}
fn open_job(name: &str) -> Result<OwnedHandle, u32> {
    let name: Vec<u16> = OsStr::new(name).encode_wide().chain(Some(0)).collect();
    // SAFETY: validated name, terminated UTF-16, non-inheritable owned result.
    let raw = unsafe { OpenJobObjectW(JOB_OBJECT_QUERY | JOB_OBJECT_TERMINATE, 0, name.as_ptr()) };
    if raw.is_null() {
        Err(unsafe { GetLastError() })
    } else {
        Ok(unsafe { OwnedHandle::from_raw_handle(raw) })
    }
}
fn recover_blocking(
    store: StateStore,
    id: String,
    timeout: Duration,
    session: Result<u32, RuntimeError>,
    open: impl FnOnce(&str) -> Result<OwnedHandle, u32>,
) -> Result<(), RuntimeFailure> {
    let row = tauri::async_runtime::block_on(store.runtime(id.clone()))
        .map_err(|message| RuntimeError::new("CODEX_RUNTIME_STORE_FAILED", message))?
        .ok_or_else(|| {
            RuntimeError::new("CODEX_RUNTIME_NOT_FOUND", "Runtime record does not exist")
        })?;
    if row.state == "terminated" && row.termination_evidence_state == "complete" {
        return Ok(());
    }
    let expected = format!("Local\\SerenaDesktop.Codex.{id}");
    let session = match session {
        Ok(session) => session,
        Err(error) => {
            store.runtime_unknown(&id, &error, now())?;
            return Err(error.into());
        }
    };
    let valid = row.job_session_id == Some(i64::from(session))
        && safe_policy(&row)
        && row.job_name.as_deref() == Some(expected.as_str())
        && !id.contains(['\\', '/', '\0']);
    if !valid {
        let error = RuntimeError::new(
            "CODEX_RUNTIME_EVIDENCE_INCOMPLETE",
            "Original Session, Job name or verified policy is missing or mismatched",
        );
        store.runtime_unknown(&id, &error, now())?;
        return Err(error.into());
    }
    match open(&expected) {
        Ok(job) => {
            let runtime = Runtime {
                id,
                store,
                child: None,
                recovered_job: Some(job),
            };
            match active_processes(runtime.job()) {
                Ok(0) => {
                    let evidence = TerminationEvidence {
                        id: runtime.id.clone(),
                        kind: "job_active_processes_zero",
                        at: now(),
                    };
                    match runtime.store.complete_runtime(&evidence) {
                        Ok(()) => Ok(()),
                        Err(error) => Err(runtime.unknown(RuntimeError::new(
                            "CODEX_RUNTIME_EVIDENCE_PERSIST_FAILED",
                            error.message,
                        ))),
                    }
                }
                Ok(_) => runtime.terminate_blocking(timeout),
                Err(error) => Err(runtime.unknown(error)),
            }
        }
        Err(ERROR_FILE_NOT_FOUND) => {
            store
                .complete_runtime(&TerminationEvidence {
                    id,
                    kind: "managed_job_destroyed",
                    at: now(),
                })
                .map_err(|error| {
                    RuntimeError::new("CODEX_RUNTIME_EVIDENCE_PERSIST_FAILED", error.message)
                })?;
            Ok(())
        }
        Err(error) => {
            let cause = RuntimeError::new(
                "CODEX_JOB_OPEN_FAILED",
                format!("OpenJobObjectW failed: Win32 {error}"),
            );
            store.runtime_unknown(&id, &cause, now())?;
            Err(cause.into())
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ProcessIdentity {
    Same,
    Different,
    Unavailable,
}
/// Diagnostic only. Never returns termination/release evidence or kills a PID.
pub async fn process_identity(pid: u32, token: String) -> ProcessIdentity {
    tauri::async_runtime::spawn_blocking(move || {
        // SAFETY: query-only handle, immediately adopted and closed on every path.
        let raw = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if raw.is_null() {
            return ProcessIdentity::Unavailable;
        }
        let process = unsafe { OwnedHandle::from_raw_handle(raw) };
        let (mut c, mut e, mut k, mut u): (FILETIME, FILETIME, FILETIME, FILETIME) =
            unsafe { zeroed() };
        if unsafe { GetProcessTimes(process.as_raw_handle(), &mut c, &mut e, &mut k, &mut u) } == 0
        {
            return ProcessIdentity::Unavailable;
        }
        if format!("{:08x}{:08x}", c.dwHighDateTime, c.dwLowDateTime) == token {
            ProcessIdentity::Same
        } else {
            ProcessIdentity::Different
        }
    })
    .await
    .unwrap_or(ProcessIdentity::Unavailable)
}

#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum StartFault {
    Identity,
}
#[cfg(test)]
pub(super) fn crash_point(point: &str) {
    if point == "before_create_process"
        && std::env::var("TASK004_CRASH_POINT").as_deref() == Ok("during_create_process")
    {
        let dir = std::env::var_os("TASK004_HOST_DIR").unwrap();
        std::fs::write(
            std::path::Path::new(&dir).join("create.entering"),
            b"entering",
        )
        .unwrap();
    }
    if std::env::var("TASK004_CRASH_POINT").as_deref() == Ok(point) {
        std::process::exit(74);
    }
}

#[cfg(test)]
mod tests;
