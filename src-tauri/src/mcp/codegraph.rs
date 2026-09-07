use crate::config::Workspace;
use rmcp::{
    RoleClient, ServiceExt,
    model::CallToolRequestParams,
    service::RunningService,
    transport::{TokioChildProcess, which_command},
};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    io::Read,
    process::Stdio,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::io::AsyncReadExt;
use tokio_util::sync::CancellationToken;

#[cfg(test)]
use std::path::PathBuf;

const COOLDOWN: Duration = Duration::from_secs(30);
const START_TIMEOUT: Duration = Duration::from_secs(25);
const QUERY_TIMEOUT: Duration = Duration::from_secs(20);
type Logs = Arc<Mutex<VecDeque<String>>>;

#[cfg(test)]
#[path = "codegraph_tests.rs"]
pub(super) mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Error {
    WorkspaceNotActive,
    NotInitialized,
    Starting,
    StartFailed,
    RuntimeLost,
    Unavailable,
    Cancelled,
    Timeout,
    Upstream,
    OutputLimit,
}
impl Error {
    fn code(self) -> &'static str {
        match self {
            Self::WorkspaceNotActive => "WORKSPACE_NOT_ACTIVE",
            Self::NotInitialized => "CODEGRAPH_NOT_INITIALIZED",
            Self::Starting => "CODEGRAPH_STARTING",
            Self::StartFailed => "CODEGRAPH_START_FAILED",
            Self::RuntimeLost => "CODEGRAPH_RUNTIME_LOST",
            Self::Unavailable => "CODEGRAPH_UNAVAILABLE",
            Self::Cancelled => "CODEGRAPH_CANCELLED",
            Self::Timeout => "CODEGRAPH_TIMEOUT",
            Self::Upstream => "CODEGRAPH_UPSTREAM_ERROR",
            Self::OutputLimit => "CODEGRAPH_OUTPUT_LIMIT",
        }
    }
    fn message(self) -> &'static str {
        match self {
            Self::WorkspaceNotActive => "Activate a workspace before querying CodeGraph.",
            Self::NotInitialized => "The active workspace has no initialized CodeGraph index.",
            Self::Starting => "CodeGraph is starting for the active workspace.",
            Self::StartFailed => "CodeGraph failed to start for the active workspace.",
            Self::RuntimeLost => "The CodeGraph runtime connection was lost.",
            Self::Unavailable => "CodeGraph is unavailable for the active workspace.",
            Self::Cancelled => "The CodeGraph request was cancelled.",
            Self::Timeout => "The CodeGraph request exceeded its time limit.",
            Self::Upstream => "CodeGraph could not complete this query. See local diagnostics.",
            Self::OutputLimit => "Narrow the query or reduce maxFiles.",
        }
    }
    pub(super) fn value(self, workspace: Option<&Workspace>) -> Value {
        json!({"error":{"code":self.code(),"message":self.message(),
            "workspace":workspace.map(|w| json!({"id":w.id,"name":w.name})),
            "recoverable":matches!(self, Self::Starting | Self::StartFailed | Self::RuntimeLost | Self::Timeout)}})
    }
}

struct Client {
    service: RunningService<RoleClient, ()>,
    stderr_task: tokio::task::JoinHandle<()>,
}
impl Drop for Client {
    fn drop(&mut self) {
        self.service.cancellation_token().cancel();
        self.stderr_task.abort();
    }
}
enum State {
    Starting,
    Ready(Arc<Client>),
    Failed(Error),
}
struct RuntimeState {
    state: State,
    last_failure_at: Option<String>,
    last_recovery: Option<Instant>,
}
struct Runtime {
    workspace: Workspace,
    generation: u64,
    state: Mutex<RuntimeState>,
    query: tokio::sync::Mutex<()>,
    logs: Logs,
    #[cfg(test)]
    mock: Option<PathBuf>,
}
pub struct Binding {
    runtime: Arc<Runtime>,
    cancel: CancellationToken,
    startup: Option<tokio::task::JoinHandle<()>>,
}
impl Drop for Binding {
    fn drop(&mut self) {
        self.cancel.cancel();
        if let Some(task) = self.startup.take() {
            task.abort();
        }
        let starting = matches!(self.runtime.state.lock().unwrap().state, State::Starting);
        self.runtime.log(
            if starting {
                "stale generation discarded"
            } else {
                "binding release"
            },
            "",
        );
    }
}

// Dropped requests must invalidate the session, including the outer Broker deadline.
struct Flight(Option<Arc<Client>>);
impl Drop for Flight {
    fn drop(&mut self) {
        if let Some(client) = self.0.take() {
            client.service.cancellation_token().cancel();
        }
    }
}
struct Starting<'a> {
    runtime: &'a Runtime,
    completed: bool,
}
impl Drop for Starting<'_> {
    fn drop(&mut self) {
        if !self.completed {
            self.runtime
                .fail(Error::StartFailed, "initialization cancelled");
        }
    }
}

impl Binding {
    pub(super) fn begin(workspace: &Workspace, generation: u64, logs: Logs) -> Self {
        let mut binding = Self::new(workspace, generation, logs);
        binding.start();
        binding
    }
    fn new(workspace: &Workspace, generation: u64, logs: Logs) -> Self {
        Self {
            runtime: Arc::new(Runtime {
                workspace: workspace.clone(),
                generation,
                logs,
                state: Mutex::new(RuntimeState {
                    state: State::Starting,
                    last_failure_at: None,
                    last_recovery: None,
                }),
                query: tokio::sync::Mutex::new(()),
                #[cfg(test)]
                mock: None,
            }),
            cancel: CancellationToken::new(),
            startup: None,
        }
    }
    fn start(&mut self) {
        self.runtime.log("binding prepare", "");
        if let Err(error) = self.runtime.check_index() {
            self.runtime.fail(error, "index absent or invalid");
            return;
        }
        let runtime = self.runtime.clone();
        let cancel = self.cancel.clone();
        self.startup = Some(tokio::spawn(async move {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => runtime.log("stale generation discarded", ""),
                _ = runtime.initialize() => {},
            }
        }));
    }
    fn verify(&self, workspace: &Workspace, generation: u64) -> Result<(), Error> {
        if self.runtime.workspace.id != workspace.id
            || self.runtime.workspace.root != workspace.root
            || self.runtime.generation != generation
        {
            self.runtime.log(
                "binding identity mismatch",
                "request rejected before dispatch",
            );
            return Err(Error::Unavailable);
        }
        Ok(())
    }
    pub(super) fn status(&self, workspace: &Workspace, generation: u64) -> Value {
        let error = self
            .verify(workspace, generation)
            .err()
            .or_else(|| self.runtime.current_error());
        let state = self.runtime.state.lock().unwrap();
        let status = match error {
            None => "ready",
            Some(Error::Starting) => "starting",
            Some(Error::NotInitialized) => "not_initialized",
            Some(Error::Unavailable) => "unavailable",
            Some(Error::StartFailed) => "start_failed",
            _ => "runtime_lost",
        };
        json!({"status":status,"workspaceId":self.runtime.workspace.id,"root":self.runtime.workspace.root,
            "generation":self.runtime.generation,"lastError":error.map(|e| e.value(Some(workspace))["error"].clone()),
            "lastFailureAt":state.last_failure_at})
    }
    pub(super) async fn explore(
        &self,
        workspace: &Workspace,
        generation: u64,
        args: Value,
        cancel: CancellationToken,
    ) -> Result<String, Error> {
        self.verify(workspace, generation)?;
        // Preserve Starting as an observable capability rather than queue behind startup.
        if self.runtime.current_error() == Some(Error::Starting) {
            return Err(Error::Starting);
        }
        let _query = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(Error::Cancelled),
            _ = self.cancel.cancelled() => return Err(Error::Unavailable),
            guard = self.runtime.query.lock() => guard,
        };
        let mut recovered = false;
        loop {
            if let Some(error) = self.runtime.current_error() {
                if !matches!(error, Error::StartFailed | Error::RuntimeLost) || recovered {
                    return Err(error);
                }
                {
                    let mut state = self.runtime.state.lock().unwrap();
                    if state.last_recovery.is_some_and(|t| t.elapsed() < COOLDOWN) {
                        return Err(error);
                    }
                    state.last_recovery = Some(Instant::now());
                    state.state = State::Starting;
                }
                recovered = true;
                self.runtime.log("runtime recovery", "one attempt");
                tokio::select! {
                    biased;
                    _ = cancel.cancelled() => { self.runtime.fail(Error::StartFailed, "recovery cancelled"); return Err(Error::Cancelled); },
                    _ = self.cancel.cancelled() => return Err(Error::Unavailable),
                    result = self.runtime.initialize() => result?,
                }
            }
            let client = {
                let state = self.runtime.state.lock().unwrap();
                match &state.state {
                    State::Ready(client) => client.clone(),
                    State::Failed(error) => return Err(*error),
                    State::Starting => return Err(Error::Starting),
                }
            };
            let result = tokio::select! {
                biased;
                _ = cancel.cancelled() => { client.service.cancellation_token().cancel(); return Err(Error::Cancelled); },
                _ = self.cancel.cancelled() => return Err(Error::Unavailable),
                result = self.runtime.call(&client, args.clone()) => result,
            };
            match result {
                Err(Error::RuntimeLost) => {
                    self.runtime
                        .fail(Error::RuntimeLost, "transport failure / runtime exit");
                    if recovered {
                        return result;
                    }
                }
                Err(Error::Timeout) => {
                    self.runtime.fail(Error::RuntimeLost, "query timeout");
                    return result;
                }
                Err(Error::NotInitialized) => {
                    self.runtime
                        .fail(Error::NotInitialized, "upstream index no longer loaded");
                    return result;
                }
                _ => return result,
            }
        }
    }
}

impl Runtime {
    fn log(&self, event: &str, detail: &str) {
        super::append_log(
            &self.logs,
            &format!(
                "CodeGraph · workspace={} generation={} · {event} · {:?}",
                self.workspace.id,
                self.generation,
                detail.chars().take(2048).collect::<String>()
            ),
        );
    }
    fn fail(&self, error: Error, detail: &str) {
        let mut state = self.state.lock().unwrap();
        state.state = State::Failed(error);
        state.last_failure_at = Some(chrono::Utc::now().to_rfc3339());
        drop(state);
        self.log(error.code(), detail);
    }
    fn current_error(&self) -> Option<Error> {
        let mut state = self.state.lock().unwrap();
        match &state.state {
            State::Ready(client)
                if client.service.is_closed() || client.service.is_transport_closed() =>
            {
                state.state = State::Failed(Error::RuntimeLost);
                state.last_failure_at = Some(chrono::Utc::now().to_rfc3339());
                drop(state);
                self.log("runtime exit", "closed MCP transport");
                Some(Error::RuntimeLost)
            }
            State::Ready(_) => None,
            State::Starting => Some(Error::Starting),
            State::Failed(e) => Some(*e),
        }
    }
    fn check_index(&self) -> Result<(), Error> {
        let path = self.workspace.root.join(".codegraph/codegraph.db");
        let mut file = std::fs::File::open(path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Error::NotInitialized
            } else {
                Error::StartFailed
            }
        })?;
        let mut header = [0; 16];
        file.read_exact(&mut header)
            .map_err(|_| Error::StartFailed)?;
        if &header != b"SQLite format 3\0" {
            return Err(Error::StartFailed);
        }
        Ok(())
    }
    async fn initialize(&self) -> Result<(), Error> {
        let mut attempt = Starting {
            runtime: self,
            completed: false,
        };
        self.log(
            "runtime start",
            &super::serena::display(&self.workspace.root),
        );
        let result = match self.check_index() {
            Err(error) => Err(error),
            Ok(()) => match tokio::time::timeout(START_TIMEOUT, self.connect()).await {
                Ok(result) => result,
                Err(_) => {
                    self.log("MCP initialization failure", "timeout");
                    Err(Error::StartFailed)
                }
            },
        };
        attempt.completed = true;
        match result {
            Ok(client) => {
                self.state.lock().unwrap().state = State::Ready(client);
                self.log(
                    "runtime ready",
                    "initialize and tools/list contract check succeeded",
                );
                Ok(())
            }
            Err(error) => {
                self.fail(error, "MCP initialization failure");
                Err(error)
            }
        }
    }
    async fn connect(&self) -> Result<Arc<Client>, Error> {
        #[cfg(test)]
        let mock_command = self.mock.as_ref().map(|script| {
            let mut command = which_command("node").unwrap();
            command.arg(super::serena::display(script));
            command
        });
        #[cfg(not(test))]
        let mock_command: Option<tokio::process::Command> = None;
        let mut command = match mock_command {
            Some(command) => command,
            None => {
                let mut command = which_command("codegraph").map_err(|e| {
                    self.log("runtime unavailable", &e.to_string());
                    Error::Unavailable
                })?;
                command
                    .args(["serve", "--mcp", "--path"])
                    .arg(super::serena::display(&self.workspace.root));
                command
            }
        };
        command
            .current_dir(&self.workspace.root)
            .env("CODEGRAPH_DIR", ".codegraph")
            .kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
        let (transport, stderr) = TokioChildProcess::builder(command)
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                self.log("runtime start failure", &e.to_string());
                Error::StartFailed
            })?;
        // Drain stderr without blocking the child; retain at most 8 KiB locally per runtime.
        let logs = self.logs.clone();
        let label = format!(
            "CodeGraph stderr · workspace={} generation={}",
            self.workspace.id, self.generation
        );
        let stderr_task = tokio::spawn(async move {
            if let Some(mut stderr) = stderr {
                let mut remaining = 8192;
                let mut bytes = [0; 1024];
                while let Ok(n) = stderr.read(&mut bytes).await {
                    if n == 0 {
                        break;
                    }
                    let take = n.min(remaining);
                    if take > 0 {
                        super::append_log(
                            &logs,
                            &format!("{label} · {:?}", String::from_utf8_lossy(&bytes[..take])),
                        );
                        remaining -= take;
                    }
                }
            }
        });
        // This guard also closes stderr if initialize is cancelled before a Client exists.
        struct StderrGuard(Option<tokio::task::JoinHandle<()>>);
        impl Drop for StderrGuard {
            fn drop(&mut self) {
                if let Some(task) = self.0.take() {
                    task.abort();
                }
            }
        }
        let mut stderr_guard = StderrGuard(Some(stderr_task));
        let service = ().serve(transport).await.map_err(|e| {
            self.log("MCP initialization failure", &e.to_string());
            Error::StartFailed
        })?;
        let client = Arc::new(Client {
            service,
            stderr_task: stderr_guard.0.take().unwrap(),
        });
        let tools = client.service.list_all_tools().await.map_err(|e| {
            self.log("tools/list failure", &e.to_string());
            Error::StartFailed
        })?;
        let tool = tools
            .iter()
            .find(|t| t.name == "codegraph_explore")
            .ok_or_else(|| {
                self.log("tool contract failure", "codegraph_explore missing");
                Error::StartFailed
            })?;
        let props = tool.input_schema.get("properties");
        if !["query", "maxFiles"]
            .iter()
            .all(|name| props.and_then(|p| p.get(name)).is_some())
            || tool
                .input_schema
                .get("required")
                .and_then(Value::as_array)
                .is_none_or(|required| required.is_empty() || required.iter().any(|v| v != "query"))
        {
            self.log(
                "tool contract failure",
                "unsupported codegraph_explore schema",
            );
            return Err(Error::StartFailed);
        }
        Ok(client)
    }
    async fn call(&self, client: &Arc<Client>, args: Value) -> Result<String, Error> {
        let args: super::registry::GraphArgs =
            serde_json::from_value(args).map_err(|_| Error::Upstream)?;
        let args = json!({"query":args.query,"maxFiles":args.max_files.unwrap_or(12)});
        let mut flight = Flight(Some(client.clone()));
        let result = tokio::time::timeout(
            QUERY_TIMEOUT,
            client.service.call_tool(
                CallToolRequestParams::new("codegraph_explore")
                    .with_arguments(args.as_object().cloned().unwrap_or_default()),
            ),
        )
        .await
        .map_err(|_| Error::Timeout)?
        .map_err(|e| {
            self.log("transport failure", &e.to_string());
            Error::RuntimeLost
        })?;
        flight.0 = None;
        if result.is_error == Some(true) {
            self.log("upstream query failure", "isError=true (response omitted)");
            return Err(Error::Upstream);
        }
        let value = serde_json::to_value(result).map_err(|_| Error::Upstream)?;
        let mut texts = Vec::new();
        for item in value["content"].as_array().ok_or(Error::Upstream)? {
            if item["type"] != "text" {
                return Err(Error::Upstream);
            }
            texts.push(item["text"].as_str().ok_or(Error::Upstream)?);
        }
        let text = texts.join("\n");
        if text.starts_with("No CodeGraph project is loaded")
            || text.starts_with("The project at ") && text.contains("isn't indexed")
        {
            return Err(Error::NotInitialized);
        }
        if text.len() > 262144 {
            return Err(Error::OutputLimit);
        }
        Ok(text)
    }
}
