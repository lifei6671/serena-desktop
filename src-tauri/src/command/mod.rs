#![cfg_attr(not(windows), allow(dead_code, unused_imports))]

use crate::{
    agent::store::{CommandRunReceipt, CommandRunRecord, CreateCommandRunInput, StateStore},
    serena::SupervisorState,
    workspace_path::WorkspacePathResolver,
};
use rmcp::schemars::{self, JsonSchema};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap},
    ffi::OsString,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicI64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

#[cfg(windows)]
mod windows_launcher;

const DEFAULT_TIMEOUT_MS: u64 = 30_000;
const MAX_TIMEOUT_MS: u64 = 86_400_000;
const DEFAULT_YIELD_MS: u64 = 5_000;
const MAX_YIELD_MS: u64 = 30_000;
const DEFAULT_OBSERVE_MS: u64 = 15_000;
const MAX_OBSERVE_MS: u64 = 20_000;
const DEFAULT_OUTPUT_BYTES: usize = 64 * 1024;
const MAX_OUTPUT_BYTES: usize = 1024 * 1024;
const LIVE_OUTPUT_TAIL_BYTES: usize = 1024 * 1024;
const MAX_CONCURRENT_RUNS: usize = 32;
const COMPLETED_LIVE_RETENTION_MS: i64 = 60 * 60 * 1000;
const MAX_RETAINED_COMPLETED_LIVE: usize = 32;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "mode",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum CommandSpec {
    Process {
        executable: String,
        #[serde(default)]
        args: Vec<String>,
    },
    Shell {
        command: String,
    },
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionMode {
    #[default]
    Auto,
    Sync,
    Async,
}

impl ExecutionMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Sync => "sync",
            Self::Async => "async",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "action",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ExecuteRequest {
    Start {
        workspace_id: String,
        request_key: String,
        work_run_id: Option<String>,
        spec: CommandSpec,
        relative_cwd: Option<String>,
        #[serde(default)]
        env: BTreeMap<String, String>,
        timeout_ms: Option<u64>,
        execution_mode: Option<ExecutionMode>,
        yield_time_ms: Option<u64>,
    },
    Cancel {
        command_run_id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "action",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum QueryRequest {
    Get {
        command_run_id: String,
    },
    List {
        workspace_id: Option<String>,
        work_run_id: Option<String>,
        #[schemars(range(min = 1, max = 100))]
        limit: Option<u32>,
    },
    Observe {
        command_run_id: String,
        known_revision: Option<String>,
        #[schemars(range(min = 0, max = 20000))]
        wait_ms: Option<u64>,
    },
    Output {
        command_run_id: String,
        stdout_cursor: Option<u64>,
        stderr_cursor: Option<u64>,
        #[schemars(range(min = 1, max = 1048576))]
        max_output_bytes: Option<usize>,
    },
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommandRunView {
    pub command_run_id: String,
    pub request_key: String,
    pub workspace_id: String,
    pub workspace_generation: u64,
    pub mode: String,
    pub relative_cwd: String,
    pub execution_mode: String,
    pub status: String,
    pub revision: String,
    pub runtime_platform: String,
    pub started_at: Option<i64>,
    pub completed_at: Option<i64>,
    pub exit_code: Option<i32>,
    pub command_ok: Option<bool>,
    pub timed_out: bool,
    pub termination_reason: Option<String>,
    pub stdout_total_bytes: u64,
    pub stderr_total_bytes: u64,
    pub stdout_sha256: Option<String>,
    pub stderr_sha256: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub output_available: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OutputSegment {
    pub text: String,
    pub cursor: u64,
    pub next_cursor: u64,
    pub total_bytes: u64,
    pub dropped_bytes: u64,
    pub truncated: bool,
    pub lossy_utf8: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommandOutputView {
    pub command_run_id: String,
    pub retained: bool,
    pub stdout: OutputSegment,
    pub stderr: OutputSegment,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommandObservation {
    pub command_run: CommandRunView,
    pub unchanged: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum CommandData {
    Run { command_run: CommandRunView },
    List { command_runs: Vec<CommandRunView> },
    Observation { observation: CommandObservation },
    Output { output: CommandOutputView },
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommandError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum CommandEnvelope {
    Success { ok: bool, data: Box<CommandData> },
    Failure { ok: bool, error: CommandError },
}

#[cfg(windows)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TerminationIntent {
    Cancel,
    Timeout,
    Shutdown,
}

struct OutputBuffer {
    tail: Vec<u8>,
    total_bytes: u64,
    dropped_bytes: u64,
    hasher: Sha256,
}

impl Default for OutputBuffer {
    fn default() -> Self {
        Self {
            tail: Vec::new(),
            total_bytes: 0,
            dropped_bytes: 0,
            hasher: Sha256::new(),
        }
    }
}

impl OutputBuffer {
    fn append(&mut self, bytes: &[u8]) {
        self.hasher.update(bytes);
        self.total_bytes = self.total_bytes.saturating_add(bytes.len() as u64);
        self.tail.extend_from_slice(bytes);
        if self.tail.len() > LIVE_OUTPUT_TAIL_BYTES {
            let drop = self.tail.len() - LIVE_OUTPUT_TAIL_BYTES;
            self.tail.drain(..drop);
            self.dropped_bytes = self.dropped_bytes.saturating_add(drop as u64);
        }
    }

    fn digest(&self) -> String {
        hex_bytes(self.hasher.clone().finalize().as_slice())
    }

    fn segment(&self, cursor: u64, max_bytes: usize) -> OutputSegment {
        let base = self.total_bytes.saturating_sub(self.tail.len() as u64);
        let requested = cursor.min(self.total_bytes);
        let start = requested.max(base);
        let offset = usize::try_from(start.saturating_sub(base))
            .unwrap_or(self.tail.len())
            .min(self.tail.len());
        let available = &self.tail[offset..];
        let take = available.len().min(max_bytes);
        let bytes = &available[..take];
        let text = String::from_utf8_lossy(bytes);
        let lossy_utf8 = matches!(&text, std::borrow::Cow::Owned(_));
        OutputSegment {
            text: text.into_owned(),
            cursor: requested,
            next_cursor: start.saturating_add(take as u64),
            total_bytes: self.total_bytes,
            dropped_bytes: base.saturating_sub(requested),
            truncated: take < available.len(),
            lossy_utf8,
        }
    }
}

struct LiveCommand {
    stdout: Arc<Mutex<OutputBuffer>>,
    stderr: Arc<Mutex<OutputBuffer>>,
    #[cfg(windows)]
    control: Arc<windows_launcher::ProcessControl>,
    #[cfg(windows)]
    intent: Mutex<Option<TerminationIntent>>,
    terminal_at: AtomicI64,
}

impl LiveCommand {
    fn observation_revision(&self, record: &CommandRunRecord) -> String {
        let stdout = self.stdout.lock().unwrap().total_bytes;
        let stderr = self.stderr.lock().unwrap().total_bytes;
        observation_revision(record, stdout, stderr)
    }

    #[cfg(windows)]
    fn set_intent(&self, intent: TerminationIntent) -> bool {
        let mut current = self.intent.lock().unwrap();
        if current.is_some() {
            false
        } else {
            *current = Some(intent);
            true
        }
    }

    #[cfg(windows)]
    fn intent(&self) -> Option<TerminationIntent> {
        *self.intent.lock().unwrap()
    }
}

pub struct CommandService {
    store: StateStore,
    supervisor: Arc<SupervisorState>,
    live: Arc<Mutex<HashMap<String, Arc<LiveCommand>>>>,
    permits: Arc<Semaphore>,
    closing: AtomicBool,
}

impl CommandService {
    pub async fn new(store: StateStore, supervisor: Arc<SupervisorState>) -> Result<Self, String> {
        store
            .recover_command_runs(cfg!(windows), now_millis())
            .await?;
        Ok(Self {
            store,
            supervisor,
            live: Arc::new(Mutex::new(HashMap::new())),
            permits: Arc::new(Semaphore::new(MAX_CONCURRENT_RUNS)),
            closing: AtomicBool::new(false),
        })
    }

    pub async fn execute(&self, request: ExecuteRequest) -> CommandEnvelope {
        match self.execute_inner(request).await {
            Ok(data) => CommandEnvelope::Success {
                ok: true,
                data: Box::new(data),
            },
            Err(error) => CommandEnvelope::Failure {
                ok: false,
                error: command_error(&error),
            },
        }
    }

    pub async fn query(&self, request: QueryRequest) -> CommandEnvelope {
        match self.query_inner(request).await {
            Ok(data) => CommandEnvelope::Success {
                ok: true,
                data: Box::new(data),
            },
            Err(error) => CommandEnvelope::Failure {
                ok: false,
                error: command_error(&error),
            },
        }
    }

    async fn execute_inner(&self, request: ExecuteRequest) -> Result<CommandData, String> {
        self.prune_live();
        match request {
            ExecuteRequest::Start {
                workspace_id,
                request_key,
                work_run_id,
                spec,
                relative_cwd,
                env,
                timeout_ms,
                execution_mode,
                yield_time_ms,
            } => {
                self.start(
                    workspace_id,
                    request_key,
                    work_run_id,
                    spec,
                    relative_cwd,
                    env,
                    timeout_ms,
                    execution_mode.unwrap_or_default(),
                    yield_time_ms,
                )
                .await
            }
            ExecuteRequest::Cancel { command_run_id } => self.cancel(command_run_id).await,
        }
    }

    async fn query_inner(&self, request: QueryRequest) -> Result<CommandData, String> {
        self.prune_live();
        match request {
            QueryRequest::Get { command_run_id } => {
                validate_id(&command_run_id)?;
                Ok(CommandData::Run {
                    command_run: self.view(&command_run_id).await?,
                })
            }
            QueryRequest::List {
                workspace_id,
                work_run_id,
                limit,
            } => {
                if let Some(id) = workspace_id.as_deref() {
                    validate_id(id)?;
                }
                if let Some(id) = work_run_id.as_deref() {
                    validate_id(id)?;
                }
                let rows = self
                    .store
                    .list_command_runs(workspace_id, work_run_id, limit.unwrap_or(20) as usize)
                    .await?;
                let mut views = Vec::with_capacity(rows.len());
                for row in rows {
                    views.push(self.view_from_record(row));
                }
                Ok(CommandData::List {
                    command_runs: views,
                })
            }
            QueryRequest::Observe {
                command_run_id,
                known_revision,
                wait_ms,
            } => {
                validate_id(&command_run_id)?;
                if known_revision
                    .as_ref()
                    .is_some_and(|value| value.trim().is_empty())
                {
                    return Err("COMMAND_INVALID_ARGUMENT".into());
                }
                let wait_ms = wait_ms.unwrap_or(DEFAULT_OBSERVE_MS);
                if wait_ms > MAX_OBSERVE_MS {
                    return Err("COMMAND_INVALID_ARGUMENT".into());
                }
                let deadline = Instant::now() + Duration::from_millis(wait_ms);
                loop {
                    let view = self.view(&command_run_id).await?;
                    let terminal = is_terminal(&view.status);
                    let unchanged = known_revision
                        .as_ref()
                        .is_some_and(|known| known == &view.revision);
                    if known_revision.is_none()
                        || !unchanged
                        || terminal
                        || Instant::now() >= deadline
                    {
                        return Ok(CommandData::Observation {
                            observation: CommandObservation {
                                command_run: view,
                                unchanged,
                            },
                        });
                    }
                    tokio::time::sleep(Duration::from_millis(250)).await;
                }
            }
            QueryRequest::Output {
                command_run_id,
                stdout_cursor,
                stderr_cursor,
                max_output_bytes,
            } => {
                validate_id(&command_run_id)?;
                let max_bytes = max_output_bytes.unwrap_or(DEFAULT_OUTPUT_BYTES);
                if !(1..=MAX_OUTPUT_BYTES).contains(&max_bytes) {
                    return Err("COMMAND_INVALID_ARGUMENT".into());
                }
                let record = self
                    .store
                    .command_run(command_run_id.clone())
                    .await?
                    .ok_or("COMMAND_RUN_NOT_FOUND")?;
                let live = self.live.lock().unwrap().get(&command_run_id).cloned();
                let output = if let Some(live) = live {
                    CommandOutputView {
                        command_run_id,
                        retained: true,
                        stdout: live
                            .stdout
                            .lock()
                            .unwrap()
                            .segment(stdout_cursor.unwrap_or(0), max_bytes),
                        stderr: live
                            .stderr
                            .lock()
                            .unwrap()
                            .segment(stderr_cursor.unwrap_or(0), max_bytes),
                    }
                } else {
                    CommandOutputView {
                        command_run_id,
                        retained: false,
                        stdout: unavailable_segment(
                            stdout_cursor.unwrap_or(record.stdout_total_bytes),
                            record.stdout_total_bytes,
                        ),
                        stderr: unavailable_segment(
                            stderr_cursor.unwrap_or(record.stderr_total_bytes),
                            record.stderr_total_bytes,
                        ),
                    }
                };
                Ok(CommandData::Output { output })
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn start(
        &self,
        workspace_id: String,
        request_key: String,
        work_run_id: Option<String>,
        spec: CommandSpec,
        relative_cwd: Option<String>,
        env: BTreeMap<String, String>,
        timeout_ms: Option<u64>,
        execution_mode: ExecutionMode,
        yield_time_ms: Option<u64>,
    ) -> Result<CommandData, String> {
        if self.closing.load(Ordering::Acquire) {
            return Err("COMMAND_RUNTIME_CLOSING".into());
        }
        validate_id(&workspace_id)?;
        validate_request_key(&request_key)?;
        if let Some(id) = work_run_id.as_deref() {
            validate_id(id)?;
        }

        let timeout_ms = timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS);
        if timeout_ms == 0 || timeout_ms > MAX_TIMEOUT_MS {
            return Err("COMMAND_INVALID_ARGUMENT".into());
        }
        let yield_ms = yield_time_ms.unwrap_or(DEFAULT_YIELD_MS);
        if yield_ms > MAX_YIELD_MS {
            return Err("COMMAND_INVALID_ARGUMENT".into());
        }
        validate_spec(&spec)?;
        validate_env(&env)?;

        #[cfg(not(windows))]
        {
            let _ = (
                workspace_id,
                request_key,
                work_run_id,
                spec,
                relative_cwd,
                env,
                timeout_ms,
                execution_mode,
                yield_ms,
            );
            Err("COMMAND_RUNTIME_UNAVAILABLE_ON_PLATFORM".into())
        }

        #[cfg(windows)]
        {
            // CommandRun is a real Workspace owner for its whole live lifetime.
            // Reuse the existing Supervisor guard so Remove and Command start share
            // one admission/linearization boundary.
            let (lease, workspace_guard) =
                self.supervisor.resolve_workspace_write_guard(&workspace_id)?;
            let path_resolver = WorkspacePathResolver::new(&lease);
            let (relative_cwd, cwd) = match relative_cwd {
                None => (".".to_string(), path_resolver.root()?),
                Some(value) if value.trim() == "." => (".".to_string(), path_resolver.root()?),
                Some(value) => {
                    let path = path_resolver.resolve(&value)?;
                    if !path.is_dir() {
                        return Err("COMMAND_WORKDIR_NOT_DIRECTORY".into());
                    }
                    (value, path)
                }
            };

            let normalized = serde_json::json!({
                "workspaceId": &workspace_id,
                "workspaceGeneration": lease.generation,
                "canonicalWorkspaceRoot": lease.canonical_root.to_string_lossy(),
                "workRunId": &work_run_id,
                "requestKey": &request_key,
                "spec": &spec,
                "relativeCwd": &relative_cwd,
                "env": &env,
                "timeoutMs": timeout_ms,
            });
            let request_hash = hex_digest(
                &serde_json::to_vec(&normalized).map_err(|_| "COMMAND_INVALID_ARGUMENT")?,
            );
            if let Some(existing) = self
                .store
                .command_run_by_request_key(workspace_id.clone(), request_key.clone())
                .await?
            {
                if existing.request_hash != request_hash {
                    return Err("COMMAND_REQUEST_KEY_CONFLICT".into());
                }
                return Ok(CommandData::Run {
                    command_run: self.view_from_record(existing),
                });
            }

            let (executable, args) = windows_invocation(&spec)?;
            let environment = command_environment(&env, &lease.workspace_id);
            let permit = self
                .permits
                .clone()
                .try_acquire_owned()
                .map_err(|_| "COMMAND_SESSION_LIMIT_REACHED".to_string())?;
            let command_run_id = crate::agent::task_manager::AgentTaskManager::id("command");
            let now = now_millis();
            let mode = match &spec {
                CommandSpec::Process { .. } => "process",
                CommandSpec::Shell { .. } => "shell",
            };
            let create = self
                .store
                .create_command_run(
                    command_run_id.clone(),
                    CreateCommandRunInput {
                        request_key,
                        request_hash,
                        workspace_id: lease.workspace_id.clone(),
                        canonical_workspace_root: lease
                            .canonical_root
                            .to_string_lossy()
                            .into_owned(),
                        workspace_generation: lease.generation,
                        work_run_id,
                        mode: mode.into(),
                        relative_cwd: relative_cwd.clone(),
                        execution_mode: execution_mode.as_str().into(),
                        timeout_ms,
                        runtime_platform: "windows".into(),
                        containment_type: "job_at_creation".into(),
                    },
                    now,
                )
                .await?;
            if !create.created {
                drop(permit);
                return Ok(CommandData::Run {
                    command_run: self.view_from_record(create.record),
                });
            }

            let launch_request = windows_launcher::LaunchRequest {
                executable,
                args,
                current_dir: cwd,
                environment,
                command_run_id: command_run_id.clone(),
            };
            let launch_result =
                tokio::task::spawn_blocking(move || windows_launcher::launch(&launch_request))
                    .await;
            let launched = match launch_result {
                Ok(Ok(value)) => value,
                Ok(Err(error)) => {
                    let code = error.code.to_string();
                    let _ = self
                        .store
                        .command_mark_terminal(
                            command_run_id.clone(),
                            "failed".into(),
                            CommandRunReceipt {
                                error_code: Some(code.clone()),
                                error_message: Some(error.to_string()),
                                termination_reason: Some("launch_failed".into()),
                                ..CommandRunReceipt::default()
                            },
                            now_millis(),
                        )
                        .await;
                    drop(permit);
                    return Err(code);
                }
                Err(error) => {
                    let code = "COMMAND_PROCESS_CREATE_FAILED".to_string();
                    let _ = self
                        .store
                        .command_mark_terminal(
                            command_run_id.clone(),
                            "failed".into(),
                            CommandRunReceipt {
                                error_code: Some(code.clone()),
                                error_message: Some(error.to_string()),
                                termination_reason: Some("launch_task_failed".into()),
                                ..CommandRunReceipt::default()
                            },
                            now_millis(),
                        )
                        .await;
                    drop(permit);
                    return Err(code);
                }
            };

            let pid = launched.pid;
            let stdout = Arc::new(Mutex::new(OutputBuffer::default()));
            let stderr = Arc::new(Mutex::new(OutputBuffer::default()));
            let live = Arc::new(LiveCommand {
                stdout: stdout.clone(),
                stderr: stderr.clone(),
                control: launched.control.clone(),
                intent: Mutex::new(None),
                terminal_at: AtomicI64::new(0),
            });
            if let Err(error) = self
                .store
                .command_mark_running(command_run_id.clone(), pid, now_millis())
                .await
            {
                let control = launched.control.clone();
                let termination =
                    tokio::task::spawn_blocking(move || control.terminate(Duration::from_secs(3)))
                        .await;
                let (status, error_code, error_message) = match termination {
                    Ok(Ok(())) => (
                        "failed",
                        "COMMAND_STATE_PERSIST_FAILED".to_string(),
                        error.clone(),
                    ),
                    Ok(Err(termination_error)) => (
                        "unknown",
                        termination_error.code.to_string(),
                        format!("{error}; {}", termination_error),
                    ),
                    Err(join_error) => (
                        "unknown",
                        "COMMAND_PROCESS_TERMINATE_FAILED".to_string(),
                        format!("{error}; {join_error}"),
                    ),
                };
                let _ = self
                    .store
                    .command_mark_terminal(
                        command_run_id.clone(),
                        status.into(),
                        CommandRunReceipt {
                            termination_reason: Some("state_persist_failed".into()),
                            error_code: Some(error_code),
                            error_message: Some(error_message),
                            ..CommandRunReceipt::default()
                        },
                        now_millis(),
                    )
                    .await;
                drop(permit);
                return Err(error);
            }
            self.live
                .lock()
                .unwrap()
                .insert(command_run_id.clone(), live.clone());

            drop(launched.stdin);
            let stdout_task = spawn_reader(launched.stdout, stdout);
            let stderr_task = spawn_reader(launched.stderr, stderr);
            self.spawn_waiter(
                command_run_id.clone(),
                launched.control,
                live,
                stdout_task,
                stderr_task,
                Duration::from_millis(timeout_ms),
                permit,
                workspace_guard,
            );

            let wait_ms = match execution_mode {
                ExecutionMode::Async => 0,
                ExecutionMode::Auto => yield_ms,
                // Keep every MCP control-plane call bounded even when the process timeout is hours.
                ExecutionMode::Sync => timeout_ms.min(MAX_YIELD_MS),
            };
            if wait_ms > 0 {
                let deadline = Instant::now() + Duration::from_millis(wait_ms);
                loop {
                    let view = self.view(&command_run_id).await?;
                    if is_terminal(&view.status) || Instant::now() >= deadline {
                        return Ok(CommandData::Run { command_run: view });
                    }
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            }
            Ok(CommandData::Run {
                command_run: self.view(&command_run_id).await?,
            })
        }
    }

    #[cfg(windows)]
    fn spawn_waiter(
        &self,
        command_run_id: String,
        control: Arc<windows_launcher::ProcessControl>,
        live: Arc<LiveCommand>,
        stdout_task: tokio::task::JoinHandle<()>,
        stderr_task: tokio::task::JoinHandle<()>,
        timeout: Duration,
        permit: OwnedSemaphorePermit,
        _workspace_guard: crate::serena::WorkspaceWriteGuard,
    ) {
        let store = self.store.clone();
        tokio::spawn(async move {
            let wait_control = control.clone();
            let mut wait_parent = tokio::task::spawn_blocking(move || wait_control.wait_parent());
            let exit_result = tokio::select! {
                result = &mut wait_parent => result
                    .map_err(|_| "COMMAND_PROCESS_WAIT_FAILED".to_string())
                    .and_then(|result| result.map_err(|error| error.code.to_string())),
                _ = tokio::time::sleep(timeout) => {
                    if live.set_intent(TerminationIntent::Timeout) {
                        let _ = store.command_mark_cancelling(command_run_id.clone(), now_millis()).await;
                    }
                    let timeout_control = control.clone();
                    let terminate = tokio::task::spawn_blocking(move || {
                        timeout_control.terminate(Duration::from_secs(3))
                    }).await;
                    match terminate {
                        Ok(Ok(())) => {}
                        Ok(Err(error)) => {
                            let _ = store.command_mark_terminal(
                                command_run_id.clone(),
                                "unknown".into(),
                                CommandRunReceipt {
                                    timed_out: true,
                                    termination_reason: Some("timeout_termination_failed".into()),
                                    error_code: Some(error.code.into()),
                                    error_message: Some(error.to_string()),
                                    ..CommandRunReceipt::default()
                                },
                                now_millis(),
                            ).await;
                            live.terminal_at.store(now_millis(), Ordering::Release);
                            drop(permit);
                            return;
                        }
                        Err(_) => {
                            let _ = store.command_mark_terminal(
                                command_run_id.clone(),
                                "unknown".into(),
                                CommandRunReceipt {
                                    timed_out: true,
                                    termination_reason: Some("timeout_termination_failed".into()),
                                    error_code: Some("COMMAND_PROCESS_TERMINATE_FAILED".into()),
                                    ..CommandRunReceipt::default()
                                },
                                now_millis(),
                            ).await;
                            live.terminal_at.store(now_millis(), Ordering::Release);
                            drop(permit);
                            return;
                        }
                    }
                    wait_parent.await
                        .map_err(|_| "COMMAND_PROCESS_WAIT_FAILED".to_string())
                        .and_then(|result| result.map_err(|error| error.code.to_string()))
                }
            };

            let seal_control = control.clone();
            let seal = tokio::task::spawn_blocking(move || {
                seal_control.seal_after_parent_exit(Duration::from_secs(3))
            })
            .await;
            let _ = stdout_task.await;
            let _ = stderr_task.await;

            let intent = live.intent();
            let stdout = live.stdout.lock().unwrap();
            let stderr = live.stderr.lock().unwrap();
            let mut receipt = CommandRunReceipt {
                exit_code: exit_result.as_ref().ok().copied(),
                timed_out: intent == Some(TerminationIntent::Timeout),
                termination_reason: Some(match intent {
                    Some(TerminationIntent::Cancel) => "user_cancelled".into(),
                    Some(TerminationIntent::Timeout) => "timeout".into(),
                    Some(TerminationIntent::Shutdown) => "host_shutdown".into(),
                    None => "exited".into(),
                }),
                stdout_total_bytes: stdout.total_bytes,
                stderr_total_bytes: stderr.total_bytes,
                stdout_sha256: Some(stdout.digest()),
                stderr_sha256: Some(stderr.digest()),
                ..CommandRunReceipt::default()
            };
            drop(stdout);
            drop(stderr);

            let status = match (&exit_result, seal, intent) {
                (_, Ok(Err(error)), _) => {
                    receipt.error_code = Some(error.code.into());
                    receipt.error_message = Some(error.to_string());
                    receipt.termination_reason = Some("job_cleanup_failed".into());
                    "unknown"
                }
                (_, Err(_), _) => {
                    receipt.error_code = Some("COMMAND_PROCESS_TERMINATION_FAILED".into());
                    receipt.termination_reason = Some("job_cleanup_failed".into());
                    "unknown"
                }
                (Err(code), _, _) => {
                    receipt.error_code = Some(code.clone());
                    receipt.termination_reason = Some("wait_failed".into());
                    "unknown"
                }
                (_, _, Some(TerminationIntent::Cancel)) => "cancelled",
                (_, _, Some(TerminationIntent::Timeout)) => {
                    receipt.error_code = Some("COMMAND_TIMEOUT".into());
                    "failed"
                }
                (_, _, Some(TerminationIntent::Shutdown)) => "interrupted",
                (Ok(0), _, None) => "completed",
                (Ok(_), _, None) => "failed",
            };

            let _ = store
                .command_mark_terminal(command_run_id, status.into(), receipt, now_millis())
                .await;
            live.terminal_at.store(now_millis(), Ordering::Release);
            drop(permit);
        });
    }

    async fn cancel(&self, command_run_id: String) -> Result<CommandData, String> {
        validate_id(&command_run_id)?;
        let record = self
            .store
            .command_run(command_run_id.clone())
            .await?
            .ok_or("COMMAND_RUN_NOT_FOUND")?;
        if is_terminal(&record.status) {
            return Ok(CommandData::Run {
                command_run: self.view_from_record(record),
            });
        }

        #[cfg(not(windows))]
        {
            Err("COMMAND_RUNTIME_UNAVAILABLE_ON_PLATFORM".into())
        }

        #[cfg(windows)]
        {
            let live = self
                .live
                .lock()
                .unwrap()
                .get(&command_run_id)
                .cloned()
                .ok_or("COMMAND_RUNTIME_STATE_INCONSISTENT")?;
            live.set_intent(TerminationIntent::Cancel);
            self.store
                .command_mark_cancelling(command_run_id.clone(), now_millis())
                .await?;
            let control = live.control.clone();
            tokio::task::spawn_blocking(move || control.terminate(Duration::from_secs(3)))
                .await
                .map_err(|_| "COMMAND_PROCESS_TERMINATE_FAILED".to_string())?
                .map_err(|error| error.code.to_string())?;
            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                let view = self.view(&command_run_id).await?;
                if is_terminal(&view.status) || Instant::now() >= deadline {
                    return Ok(CommandData::Run { command_run: view });
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
    }

    pub async fn shutdown(&self) -> Result<(), String> {
        self.closing.store(true, Ordering::Release);
        #[cfg(windows)]
        {
            let running = self
                .live
                .lock()
                .unwrap()
                .values()
                .filter(|live| live.terminal_at.load(Ordering::Acquire) == 0)
                .cloned()
                .collect::<Vec<_>>();
            let mut errors = Vec::new();
            for live in running {
                live.set_intent(TerminationIntent::Shutdown);
                let control = live.control.clone();
                match tokio::task::spawn_blocking(move || control.terminate(Duration::from_secs(3)))
                    .await
                {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => errors.push(error.code.to_string()),
                    Err(_) => errors.push("COMMAND_PROCESS_TERMINATE_FAILED".into()),
                }
            }
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if self
                    .live
                    .lock()
                    .unwrap()
                    .values()
                    .all(|live| live.terminal_at.load(Ordering::Acquire) != 0)
                {
                    break;
                }
                if Instant::now() >= deadline {
                    errors.push("COMMAND_SHUTDOWN_TIMEOUT".into());
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            if !errors.is_empty() {
                return Err(errors.join(","));
            }
        }
        Ok(())
    }

    async fn view(&self, command_run_id: &str) -> Result<CommandRunView, String> {
        let record = self
            .store
            .command_run(command_run_id.to_owned())
            .await?
            .ok_or("COMMAND_RUN_NOT_FOUND")?;
        Ok(self.view_from_record(record))
    }

    fn view_from_record(&self, record: CommandRunRecord) -> CommandRunView {
        let live = self.live.lock().unwrap().get(&record.id).cloned();
        let (stdout_total, stderr_total, output_available, revision) = if let Some(live) = live {
            let stdout_total = live.stdout.lock().unwrap().total_bytes;
            let stderr_total = live.stderr.lock().unwrap().total_bytes;
            let revision = live.observation_revision(&record);
            (stdout_total, stderr_total, true, revision)
        } else {
            (
                record.stdout_total_bytes,
                record.stderr_total_bytes,
                false,
                observation_revision(
                    &record,
                    record.stdout_total_bytes,
                    record.stderr_total_bytes,
                ),
            )
        };
        let command_ok = match record.status.as_str() {
            "completed" => Some(record.exit_code == Some(0) && !record.timed_out),
            "failed" => Some(false),
            _ => None,
        };
        CommandRunView {
            command_run_id: record.id,
            request_key: record.request_key,
            workspace_id: record.workspace_id,
            workspace_generation: record.workspace_generation,
            mode: record.mode,
            relative_cwd: record.relative_cwd,
            execution_mode: record.execution_mode,
            status: record.status,
            revision,
            runtime_platform: record.runtime_platform,
            started_at: record.started_at,
            completed_at: record.completed_at,
            exit_code: record.exit_code,
            command_ok,
            timed_out: record.timed_out,
            termination_reason: record.termination_reason,
            stdout_total_bytes: stdout_total,
            stderr_total_bytes: stderr_total,
            stdout_sha256: record.stdout_sha256,
            stderr_sha256: record.stderr_sha256,
            error_code: record.error_code,
            error_message: record.error_message,
            output_available,
            created_at: record.created_at,
            updated_at: record.updated_at,
        }
    }

    fn prune_live(&self) {
        let now = now_millis();
        let mut live = self.live.lock().unwrap();
        live.retain(|_, command| {
            let terminal_at = command.terminal_at.load(Ordering::Acquire);
            terminal_at == 0 || now.saturating_sub(terminal_at) <= COMPLETED_LIVE_RETENTION_MS
        });
        let mut completed = live
            .iter()
            .filter_map(|(id, command)| {
                let at = command.terminal_at.load(Ordering::Acquire);
                (at != 0).then(|| (id.clone(), at))
            })
            .collect::<Vec<_>>();
        if completed.len() > MAX_RETAINED_COMPLETED_LIVE {
            completed.sort_by_key(|(_, at)| *at);
            let excess = completed.len() - MAX_RETAINED_COMPLETED_LIVE;
            for (id, _) in completed.into_iter().take(excess) {
                live.remove(&id);
            }
        }
    }
}

#[cfg(windows)]
fn spawn_reader(file: File, output: Arc<Mutex<OutputBuffer>>) -> tokio::task::JoinHandle<()> {
    tokio::task::spawn_blocking(move || {
        let mut file = file;
        let mut buffer = [0_u8; 8192];
        loop {
            match file.read(&mut buffer) {
                Ok(0) => return,
                Ok(read) => output.lock().unwrap().append(&buffer[..read]),
                Err(_) => return,
            }
        }
    })
}

fn validate_id(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
    {
        Err("COMMAND_INVALID_ARGUMENT".into())
    } else {
        Ok(())
    }
}

fn validate_request_key(value: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > 200 || value.contains(['\r', '\n', '\0']) {
        Err("COMMAND_INVALID_ARGUMENT".into())
    } else {
        Ok(())
    }
}

fn validate_spec(spec: &CommandSpec) -> Result<(), String> {
    match spec {
        CommandSpec::Process { executable, args } => {
            if executable.trim().is_empty()
                || executable.len() > 1024
                || executable.contains(['/', '\\', ':', '\0', '\r', '\n'])
                || args.len() > 256
                || args
                    .iter()
                    .any(|value| value.contains('\0') || value.len() > 32 * 1024)
            {
                return Err("COMMAND_INVALID_ARGUMENT".into());
            }
        }
        CommandSpec::Shell { command } => {
            if command.trim().is_empty() || command.len() > 64 * 1024 || command.contains('\0') {
                return Err("COMMAND_INVALID_ARGUMENT".into());
            }
        }
    }
    Ok(())
}

fn validate_env(env: &BTreeMap<String, String>) -> Result<(), String> {
    if env.len() > 128 {
        return Err("COMMAND_ENV_INVALID".into());
    }
    let mut total = 0usize;
    for (key, value) in env {
        total = total.saturating_add(key.len()).saturating_add(value.len());
        if key.is_empty()
            || key.contains(['=', '\0', '\r', '\n'])
            || value.contains('\0')
            || key.to_ascii_uppercase().starts_with("SERENA_DESKTOP_")
        {
            return Err("COMMAND_ENV_INVALID".into());
        }
    }
    if total > 64 * 1024 {
        return Err("COMMAND_ENV_INVALID".into());
    }
    Ok(())
}

#[cfg(windows)]
fn windows_invocation(spec: &CommandSpec) -> Result<(PathBuf, Vec<OsString>), String> {
    match spec {
        CommandSpec::Process { executable, args } => {
            let path = resolve_native_executable(executable)
                .ok_or_else(|| "COMMAND_EXECUTABLE_NOT_FOUND".to_string())?;
            Ok((path, args.iter().map(OsString::from).collect()))
        }
        CommandSpec::Shell { command } => {
            const UTF8_PREFIX: &str = "try { [Console]::InputEncoding=[System.Text.UTF8Encoding]::new($false); [Console]::OutputEncoding=[System.Text.UTF8Encoding]::new($false); $OutputEncoding=[System.Text.UTF8Encoding]::new($false) } catch {}\n";
            if let Some(path) = resolve_native_executable("pwsh.exe")
                .or_else(|| resolve_native_executable("powershell.exe"))
            {
                return Ok((
                    path,
                    vec![
                        "-NoLogo".into(),
                        "-NoProfile".into(),
                        "-NonInteractive".into(),
                        "-Command".into(),
                        format!("{UTF8_PREFIX}{command}").into(),
                    ],
                ));
            }
            let cmd = resolve_native_executable("cmd.exe")
                .ok_or_else(|| "COMMAND_SHELL_NOT_FOUND".to_string())?;
            Ok((
                cmd,
                vec!["/D".into(), "/S".into(), "/C".into(), command.into()],
            ))
        }
    }
}

#[cfg(windows)]
fn resolve_native_executable(name: &str) -> Option<PathBuf> {
    let name_path = Path::new(name);
    if name_path.is_absolute() || name.contains(['/', '\\', ':']) {
        return None;
    }
    let has_extension = name_path.extension().is_some();
    let path = std::env::var_os("PATH")?;
    for directory in std::env::split_paths(&path) {
        let direct = directory.join(name);
        if direct.is_file() && has_extension {
            return std::fs::canonicalize(direct).ok();
        }
        if !has_extension {
            for extension in ["exe", "com"] {
                let candidate = directory.join(format!("{name}.{extension}"));
                if candidate.is_file()
                    && let Ok(canonical) = std::fs::canonicalize(candidate)
                {
                    return Some(canonical);
                }
            }
        }
    }
    None
}

#[cfg(windows)]
fn command_environment(
    explicit: &BTreeMap<String, String>,
    workspace_id: &str,
) -> Vec<(OsString, OsString)> {
    let mut values = BTreeMap::<String, String>::new();
    for key in [
        "PATH",
        "PATHEXT",
        "SystemRoot",
        "WINDIR",
        "ComSpec",
        "TEMP",
        "TMP",
        "USERPROFILE",
        "HOMEDRIVE",
        "HOMEPATH",
        "HOME",
        "APPDATA",
        "LOCALAPPDATA",
        "PROGRAMDATA",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "NUMBER_OF_PROCESSORS",
        "PROCESSOR_ARCHITECTURE",
        "LANG",
        "LC_ALL",
        "SSL_CERT_FILE",
        "SSL_CERT_DIR",
    ] {
        if let Some(value) = std::env::var_os(key) {
            values.insert(key.to_string(), value.to_string_lossy().into_owned());
        }
    }
    for (key, value) in explicit {
        let existing = values
            .keys()
            .find(|candidate| candidate.eq_ignore_ascii_case(key))
            .cloned();
        if let Some(existing) = existing {
            values.remove(&existing);
        }
        values.insert(key.clone(), value.clone());
    }
    values.insert("SERENA_DESKTOP_COMMAND".into(), "1".into());
    values.insert("SERENA_DESKTOP_WORKSPACE_ID".into(), workspace_id.into());
    values
        .into_iter()
        .map(|(key, value)| (OsString::from(key), OsString::from(value)))
        .collect()
}

fn unavailable_segment(cursor: u64, total_bytes: u64) -> OutputSegment {
    OutputSegment {
        text: String::new(),
        cursor: cursor.min(total_bytes),
        next_cursor: total_bytes,
        total_bytes,
        dropped_bytes: total_bytes.saturating_sub(cursor),
        truncated: total_bytes > cursor,
        lossy_utf8: false,
    }
}

fn observation_revision(record: &CommandRunRecord, stdout: u64, stderr: u64) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"command-observation-v1\0");
    hasher.update(record.id.as_bytes());
    hasher.update(b"\0");
    hasher.update(record.status.as_bytes());
    hasher.update(b"\0");
    hasher.update(record.revision.to_le_bytes());
    hasher.update(stdout.to_le_bytes());
    hasher.update(stderr.to_le_bytes());
    hex_bytes(hasher.finalize().as_slice())
}

fn hex_digest(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex_bytes(hasher.finalize().as_slice())
}

fn hex_bytes(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn is_terminal(status: &str) -> bool {
    matches!(
        status,
        "completed" | "failed" | "cancelled" | "interrupted" | "unknown"
    )
}

fn command_error(raw: &str) -> CommandError {
    let code = raw
        .split_once(':')
        .map_or(raw, |(code, _)| code)
        .trim()
        .to_string();
    CommandError {
        message: raw.to_string(),
        code,
    }
}

fn now_millis() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_cursor_is_client_owned_and_reports_dropped_prefix() {
        let mut output = OutputBuffer::default();
        output.append(b"abcdef");
        let first = output.segment(0, 3);
        let second = output.segment(first.next_cursor, 3);
        assert_eq!(first.text, "abc");
        assert_eq!(second.text, "def");
        assert_eq!(second.next_cursor, 6);
    }

    #[test]
    fn command_environment_does_not_accept_reserved_values() {
        let mut env = BTreeMap::new();
        env.insert("SERENA_DESKTOP_WORKSPACE_ID".into(), "spoofed".into());
        assert_eq!(validate_env(&env), Err("COMMAND_ENV_INVALID".into()));
    }

    #[test]
    fn process_spec_accepts_only_native_path_search_names() {
        assert!(
            validate_spec(&CommandSpec::Process {
                executable: "cargo".into(),
                args: vec!["test".into()],
            })
            .is_ok()
        );
        assert!(
            validate_spec(&CommandSpec::Process {
                executable: r"C:\\Tools\\cargo.exe".into(),
                args: vec![],
            })
            .is_err()
        );
    }

    #[cfg(windows)]
    async fn windows_fixture() -> (tempfile::TempDir, CommandService) {
        use crate::config::{self, AppPaths, ManagerConfig, Workspace};

        let directory = tempfile::tempdir().unwrap();
        let workspace_root = directory.path().join("workspace");
        std::fs::create_dir(&workspace_root).unwrap();
        let paths = AppPaths {
            runtime_directory: directory.path().join("runtime"),
            config_file: directory.path().join("config.json"),
            log_directory: directory.path().join("logs"),
            app_log: directory.path().join("logs/app.log"),
            serena_log: directory.path().join("logs/serena.log"),
        };
        let config = ManagerConfig {
            workspaces: vec![Workspace {
                id: "command-test".into(),
                name: "Command Test".into(),
                root: workspace_root,
                generation: 1,
            }],
            ..ManagerConfig::default()
        };
        config::save(&paths.config_file, &config).unwrap();
        let supervisor = Arc::new(SupervisorState::new(paths).unwrap());
        let store = StateStore::open(directory.path().join("state"))
            .await
            .unwrap();
        let service = CommandService::new(store, supervisor).await.unwrap();
        (directory, service)
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn windows_process_command_completes_and_preserves_output_until_retention() {
        let (_directory, service) = windows_fixture().await;
        let started = service
            .execute(ExecuteRequest::Start {
                workspace_id: "command-test".into(),
                request_key: "short-process".into(),
                work_run_id: None,
                spec: CommandSpec::Process {
                    executable: "cmd.exe".into(),
                    args: vec![
                        "/D".into(),
                        "/S".into(),
                        "/C".into(),
                        "echo command-runtime-ok".into(),
                    ],
                },
                relative_cwd: None,
                env: BTreeMap::new(),
                timeout_ms: Some(10_000),
                execution_mode: Some(ExecutionMode::Auto),
                yield_time_ms: Some(5_000),
            })
            .await;
        let run = match started {
            CommandEnvelope::Success { data, .. } => match *data {
                CommandData::Run { command_run } => command_run,
                other => panic!("unexpected command data: {other:?}"),
            },
            other => panic!("unexpected command result: {other:?}"),
        };
        assert_eq!(run.status, "completed");
        assert_eq!(run.exit_code, Some(0));
        assert_eq!(run.command_ok, Some(true));

        let output = service
            .query(QueryRequest::Output {
                command_run_id: run.command_run_id,
                stdout_cursor: Some(0),
                stderr_cursor: Some(0),
                max_output_bytes: None,
            })
            .await;
        let output = match output {
            CommandEnvelope::Success { data, .. } => match *data {
                CommandData::Output { output } => output,
                other => panic!("unexpected command data: {other:?}"),
            },
            other => panic!("unexpected output result: {other:?}"),
        };
        assert!(output.retained);
        assert!(output.stdout.text.contains("command-runtime-ok"));
        assert_eq!(output.stdout.dropped_bytes, 0);
        assert_eq!(output.stdout.next_cursor, output.stdout.total_bytes);
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn windows_shell_command_can_be_cancelled_with_job_empty_evidence() {
        let (_directory, service) = windows_fixture().await;
        let started = service
            .execute(ExecuteRequest::Start {
                workspace_id: "command-test".into(),
                request_key: "cancel-shell".into(),
                work_run_id: None,
                spec: CommandSpec::Shell {
                    command: "Write-Output 'started'; Start-Sleep -Seconds 30".into(),
                },
                relative_cwd: None,
                env: BTreeMap::new(),
                timeout_ms: Some(60_000),
                execution_mode: Some(ExecutionMode::Async),
                yield_time_ms: None,
            })
            .await;
        let run = match started {
            CommandEnvelope::Success { data, .. } => match *data {
                CommandData::Run { command_run } => command_run,
                other => panic!("unexpected command data: {other:?}"),
            },
            other => panic!("unexpected command result: {other:?}"),
        };
        assert_eq!(run.status, "running");

        let cancelled = service
            .execute(ExecuteRequest::Cancel {
                command_run_id: run.command_run_id.clone(),
            })
            .await;
        let mut cancelled = match cancelled {
            CommandEnvelope::Success { data, .. } => match *data {
                CommandData::Run { command_run } => command_run,
                other => panic!("unexpected command data: {other:?}"),
            },
            other => panic!("unexpected cancel result: {other:?}"),
        };
        if !is_terminal(&cancelled.status) {
            let observed = service
                .query(QueryRequest::Observe {
                    command_run_id: run.command_run_id,
                    known_revision: Some(cancelled.revision.clone()),
                    wait_ms: Some(5_000),
                })
                .await;
            cancelled = match observed {
                CommandEnvelope::Success { data, .. } => match *data {
                    CommandData::Observation { observation } => observation.command_run,
                    other => panic!("unexpected command data: {other:?}"),
                },
                other => panic!("unexpected observe result: {other:?}"),
            };
        }
        assert_eq!(cancelled.status, "cancelled");
        assert_eq!(
            cancelled.termination_reason.as_deref(),
            Some("user_cancelled")
        );
    }
}
