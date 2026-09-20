pub(crate) mod capability_adapters;
mod codegraph;
pub mod git;
mod media;
mod orchestration;
pub mod process;
pub mod projects;
pub mod registry;
pub mod serena;
mod server;
mod source_find;
mod source_list;
mod source_read;
mod source_read_support;
mod source_search;
pub(crate) mod source_write_atomic_replace;
pub(crate) mod source_write_commit;
mod source_write_content;
mod source_write_create;
mod source_write_delete;
mod source_write_domain;
mod source_write_file;
mod source_write_insert;
mod source_write_replace;
mod source_write_support;
pub(crate) mod source_write_text;
use crate::{
    config::{ManagerConfig, Workspace},
    serena::{ServerStatus, SupervisorState},
    workspace_capability::{
        WorkspaceCapabilityError, WorkspaceCapabilityErrorCode, WorkspaceToolCall,
    },
    workspace_registry::{WORKSPACE_NOT_FOUND, WorkspaceRegistry},
};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::{HashMap, VecDeque},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tauri::{AppHandle, Manager};
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

pub struct Active {
    pub workspace: Workspace,
    #[allow(
        dead_code,
        reason = "deprecated compatibility activation retains the client owner"
    )]
    pub client: Arc<serena::Client>,
    #[allow(
        dead_code,
        reason = "deprecated compatibility activation retains the verified process identity"
    )]
    pub pid: u32,
}
pub struct Listener {
    address: std::net::SocketAddr,
    lan_endpoints: Vec<String>,
    started_at: i64,
    cancel: CancellationToken,
    handle: tokio::task::JoinHandle<()>,
}
pub struct Broker {
    pub remote: Arc<crate::remote::Remote>,
    pub product: std::sync::OnceLock<Arc<crate::agent::product::AgentProductService>>,
    pub supervisor: Arc<SupervisorState>,
    pub workspace: RwLock<Option<Active>>,
    pub management: tokio::sync::Mutex<()>,
    pub listener: tokio::sync::Mutex<Option<Listener>>,
    pub operation: Mutex<Option<(String, CancellationToken)>>,
    pub project_sources: Mutex<Vec<PathBuf>>,
    pub sync_warnings: Mutex<Vec<String>>,
    pub error: Mutex<Option<String>>,
    logs: Arc<Mutex<VecDeque<String>>>,
    verified_configs: Mutex<HashMap<String, Vec<u8>>>,
    // Published binding for nonblocking UI reads while indexing owns the query lock.
    published: Mutex<Option<(Workspace, u32, std::sync::Weak<serena::Client>)>>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub running: bool,
    pub started_at: Option<i64>,
    pub port: u16,
    pub listen_address: String,
    pub lan_endpoints: Vec<String>,
    pub active_workspace: Option<Workspace>,
    pub codegraph: Option<Value>,
    pub projects: Vec<Project>,
    pub operation: Option<String>,
    pub last_error: Option<String>,
    pub project_sources: Vec<PathBuf>,
    pub sync_warnings: Vec<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    #[serde(flatten)]
    pub workspace: Workspace,
    pub configured: bool,
}
fn project_config(root: &std::path::Path) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    for name in ["project.yml", "project.local.yml"] {
        let path = root.join(".serena").join(name);
        if name == "project.local.yml" && !path.exists() {
            continue;
        }
        let content = std::fs::read(path).map_err(|e| format!("WORKSPACE_NOT_CONFIGURED: {e}"))?;
        // Preserve boundaries between base and override bytes in the verification cache.
        bytes.extend_from_slice(&(content.len() as u64).to_le_bytes());
        bytes.extend(content);
    }
    Ok(bytes)
}
impl Broker {
    pub fn new(supervisor: Arc<SupervisorState>) -> Self {
        Self {
            remote: Arc::new(crate::remote::Remote::from_config(
                &supervisor.snapshot().config.remote_access,
                supervisor.paths.runtime_directory.join("oauth-state.json"),
                supervisor.paths.config_file.clone(),
            )),
            supervisor,
            product: std::sync::OnceLock::new(),
            workspace: RwLock::new(None),
            management: tokio::sync::Mutex::new(()),
            listener: tokio::sync::Mutex::new(None),
            operation: Mutex::new(None),
            project_sources: Mutex::new(Vec::new()),
            sync_warnings: Mutex::new(Vec::new()),
            error: Mutex::new(None),
            logs: Arc::new(Mutex::new(VecDeque::new())),
            verified_configs: Mutex::new(HashMap::new()),
            published: Mutex::new(None),
        }
    }
    pub fn log(&self, message: &str) {
        append_log(&self.logs, message);
    }
    pub fn log_level(&self, level: &str, message: &str) {
        append_log_level(&self.logs, level, "MCP", message);
    }
    pub fn log_tool(&self, level: &str, message: &str) {
        append_log_level(&self.logs, level, "TOOL", message);
    }
    pub fn log_snapshot(&self) -> Vec<String> {
        self.logs.lock().unwrap().iter().cloned().collect()
    }
    pub fn clear_logs(&self) {
        self.logs.lock().unwrap().clear();
    }
    pub fn config(&self) -> ManagerConfig {
        self.supervisor.snapshot().config
    }
    pub async fn snapshot(&self) -> Snapshot {
        let snapshot = self.supervisor.snapshot();
        let current = self
            .published
            .lock()
            .unwrap()
            .as_ref()
            .filter(|(_, pid, client)| {
                snapshot.server_status == ServerStatus::Running
                    && snapshot.process_id == Some(*pid)
                    && client.upgrade().is_some_and(|client| !client.closed())
            })
            .map(|(workspace, _, _)| workspace.clone());
        let listener = self.listener.lock().await;
        Snapshot {
            running: listener.as_ref().is_some_and(|l| !l.handle.is_finished()),
            started_at: listener
                .as_ref()
                .filter(|l| !l.handle.is_finished())
                .map(|l| l.started_at),
            port: listener
                .as_ref()
                .map(|l| l.address.port())
                .unwrap_or(snapshot.config.broker.port),
            listen_address: listener
                .as_ref()
                .map(|l| l.address)
                .unwrap_or_else(|| snapshot.config.broker.bind_address())
                .ip()
                .to_string(),
            lan_endpoints: listener
                .as_ref()
                .filter(|l| !l.handle.is_finished())
                .map(|l| l.lan_endpoints.clone())
                .unwrap_or_default(),
            active_workspace: current,
            // P2D-009 Remote Gate 前不从 Global ActiveWorkspace 投影 CodeGraph 状态。
            codegraph: None,
            projects: snapshot
                .config
                .workspaces
                .into_iter()
                .map(|w| Project {
                    configured: project_config(&w.root).ok().is_some_and(|bytes| {
                        self.verified_configs.lock().unwrap().get(&w.id) == Some(&bytes)
                    }),
                    workspace: w,
                })
                .collect(),
            operation: self.operation.lock().unwrap().as_ref().map(|v| v.0.clone()),
            last_error: self.error.lock().unwrap().clone(),
            project_sources: self.project_sources.lock().unwrap().clone(),
            sync_warnings: self.sync_warnings.lock().unwrap().clone(),
        }
    }
    pub fn clear_workspace(&self, slot: &mut Option<Active>) {
        *slot = None;
        *self.published.lock().unwrap() = None;
    }
    async fn validate_project(
        &self,
        w: &Workspace,
        cancel: CancellationToken,
    ) -> Result<(), String> {
        let bytes = project_config(&w.root)?;
        if self.verified_configs.lock().unwrap().get(&w.id) == Some(&bytes) {
            return Ok(());
        }
        let snapshot = self.supervisor.snapshot();
        let installation = snapshot
            .installation
            .filter(|i| crate::discovery::supported_version(&i.version))
            .ok_or("BACKEND_UNAVAILABLE: 请先检测官方 Serena")?;
        self.supervisor.paths.verify_serena_config()?;
        // Official Project.load, including legacy language fields and project.local.yml,
        // without activating the project or starting language servers.
        let mut cmd = process::command(installation.path);
        cmd.args(["project", "is_ignored_path", "."])
            .arg(serena::display(&w.root))
            .current_dir(&w.root)
            .env("SERENA_HOME", self.supervisor.paths.serena_home());
        process::run(cmd, 8192, Duration::from_secs(15), cancel)
            .await
            .map_err(|e| format!("WORKSPACE_NOT_CONFIGURED: {e}"))?;
        let bytes = project_config(&w.root)?;
        self.verified_configs
            .lock()
            .unwrap()
            .insert(w.id.clone(), bytes);
        Ok(())
    }
    pub async fn activate(&self, id: &str, cancel: CancellationToken) -> Result<Value, String> {
        let started = Instant::now();
        let mut slot = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err("CANCELLED".into()),
            slot = self.workspace.write() => slot,
        };
        if cancel.is_cancelled() {
            return Err("CANCELLED".into());
        }
        self.log(&format!(
            "项目激活耗时 · 等待工作区锁={}ms",
            started.elapsed().as_millis()
        ));
        let preparation = Instant::now();
        let mut w = self
            .config()
            .workspaces
            .into_iter()
            .find(|w| w.id == id)
            .ok_or("INVALID_WORKSPACE")?;
        let root = git::root(&w.root, cancel.clone()).await?;
        w.root = root.clone();
        self.supervisor.paths.verify_serena_config()?;
        self.validate_project(&w, cancel.clone()).await?;
        self.log(&format!(
            "项目激活耗时 · 项目校验={}ms",
            preparation.elapsed().as_millis()
        ));
        let s = self.supervisor.snapshot();
        if s.server_status != ServerStatus::Running {
            return Err("BACKEND_UNAVAILABLE: 请先启动 Serena".into());
        }
        let handshake = Instant::now();
        let client = Arc::new(serena::Client::connect(s.active_port).await?);
        self.log(&format!(
            "项目激活耗时 · Serena 握手及工具校验={}ms",
            handshake.elapsed().as_millis()
        ));
        if cancel.is_cancelled() {
            return Err("CANCELLED".into());
        }
        // Once Serena activation starts its outcome may be uncertain on failure/cancellation.
        // Keep the old graph alive until commit, but never publish stale source/Git bindings.
        let previous = slot.take();
        *self.published.lock().unwrap() = None;
        let activation = Instant::now();
        tokio::select! {
            result = client.activate(&root) => result?,
            _ = cancel.cancelled() => return Err("CANCELLED".into()),
        };
        self.log(&format!(
            "项目激活耗时 · Serena 激活及确认={}ms",
            activation.elapsed().as_millis()
        ));
        let pid = s.process_id.ok_or("BACKEND_UNAVAILABLE")?;
        if self.supervisor.snapshot().process_id != Some(pid) {
            return Err("BACKEND_UNAVAILABLE".into());
        }
        *self.published.lock().unwrap() = Some((w.clone(), pid, Arc::downgrade(&client)));
        *slot = Some(Active {
            workspace: w.clone(),
            client,
            pid,
        });
        drop(previous);
        self.log(&format!(
            "项目激活耗时 · 总计={}ms",
            started.elapsed().as_millis()
        ));
        Ok(json!({"activeWorkspace":w,"status":"active","codegraph":null,"truncated":false}))
    }
    pub async fn deactivate(&self) -> Result<Value, String> {
        self.clear_workspace(&mut *self.workspace.write().await);
        Ok(json!({"activeWorkspace":null,"status":"inactive","truncated":false}))
    }
    pub async fn agent_operation(&self, args: Value) -> Value {
        let Some(product) = self.product.get() else {
            return crate::agent::product::failure(
                "BACKEND_UNAVAILABLE: Agent service not initialized".into(),
                None,
            );
        };
        if args["action"] == "start" {
            // Local Start 只接受请求 workspaceId，并复用 Remote 的 operation-mutex 原子创建。
            // 这里只做 DTO 错误分类；Resolver 仍只能在 Supervisor 的原子创建路径内调用。
            if let Err(error) = registry::parse_workspace_id(&args) {
                return crate::agent::product::failure(error, None);
            }
            return product
                .operation_resolved_workspace_start(&self.supervisor, args)
                .await;
        }
        product.operation(args, None).await
    }
    pub async fn call_tool(
        &self,
        name: &str,
        args: Value,
        request_cancel: CancellationToken,
    ) -> Result<Value, String> {
        let cancel = request_cancel.child_token();
        // Keep the large dispatch future off the Windows UI/IPC thread's stack.
        // This preserves polling, cancellation and the workspace lock lifetime.
        let mut work = Box::pin(self.dispatch(name, args, cancel.clone()));
        tokio::select! {
            result = &mut work => result,
            _ = tokio::time::sleep(Duration::from_secs(60)) => {
                cancel.cancel();
                let _ = tokio::time::timeout(Duration::from_secs(20), &mut work).await;
                Err("TOOL_TIMEOUT".into())
            },
            _ = request_cancel.cancelled() => {
                cancel.cancel();
                let _ = tokio::time::timeout(Duration::from_secs(20), &mut work).await;
                Err("CANCELLED".into())
            }
        }
    }
    pub async fn dispatch(
        &self,
        name: &str,
        args: Value,
        cancel: CancellationToken,
    ) -> Result<Value, String> {
        if orchestration::contains(name) {
            return Ok(self.orchestration_operation(name, args).await);
        }
        registry::validate(name, &args)?;
        match name {
            "workspace_list" => {
                let snapshot = WorkspaceRegistry::new(&self.supervisor).list();
                return Ok(json!({
                    "registryRevision": snapshot.registry_revision,
                    "workspaces": snapshot.workspaces,
                    "truncated": false
                }));
            }
            "workspace_get" => {
                let snapshot = WorkspaceRegistry::new(&self.supervisor).list();
                let workspace_id: registry::WorkspaceIdArgs =
                    serde_json::from_value(args).expect("validated workspaceId");
                let workspace = snapshot
                    .workspaces
                    .into_iter()
                    .find(|workspace| workspace.id == workspace_id.workspace_id)
                    .ok_or(WORKSPACE_NOT_FOUND)?;
                return Ok(json!({
                    "registryRevision": snapshot.registry_revision,
                    "workspace": workspace,
                    "truncated": false
                }));
            }
            "workspace_current" => {
                let current = self.snapshot().await.active_workspace;
                return Ok(json!({"activeWorkspace":current,"codegraph":null,"truncated":false}));
            }
            "workspace_activate" => {
                let _management = self.management.lock().await;
                if cancel.is_cancelled() {
                    return Err("CANCELLED".into());
                }
                return self.activate(args["id"].as_str().unwrap(), cancel).await;
            }
            "workspace_deactivate" => {
                let _management = self.management.lock().await;
                if cancel.is_cancelled() {
                    return Err("CANCELLED".into());
                }
                return tokio::select! { result = self.deactivate() => result, _ = cancel.cancelled() => Err("CANCELLED".into()) };
            }
            _ => {}
        }
        if registry::GITS.contains(&name) {
            let lease = registry::resolve_workspace_lease(&self.supervisor, &args)?;
            if cancel.is_cancelled() {
                return Err("CANCELLED".into());
            }
            return call_workspace_adapter(
                self.supervisor.workspace_capability_manager(),
                lease,
                name.to_owned(),
                args,
                cancel,
            )
            .await;
        }
        if name == "codegraph_explore" {
            // 公开 CodeGraph 调用只经过 request -> Resolver -> Lease -> Manager -> Adapter。
            // 不触碰 legacy active.graph、DesktopSelectedWorkspace 或 transport session。
            let lease = registry::resolve_workspace_lease(&self.supervisor, &args)?;
            if cancel.is_cancelled() {
                return Err("CANCELLED".into());
            }
            return call_codegraph_adapter(
                self.supervisor.workspace_capability_manager(),
                lease,
                name.to_owned(),
                args,
                cancel,
            )
            .await;
        }
        if registry::is_workspace_scoped_source(name) {
            // 全部 Source Authority 只来自本次请求解析出的 Lease，绝不读取 legacy active Workspace。
            let lease = registry::resolve_workspace_lease(&self.supervisor, &args)?;
            if cancel.is_cancelled() {
                return Err("CANCELLED".into());
            }
            let mut arguments = args
                .as_object()
                .expect("registry validation verified Semantic arguments as an object")
                .clone();
            arguments.remove("workspaceId");
            let arguments = Value::Object(arguments);
            if registry::LOCAL_SOURCES.contains(&name) {
                return call_workspace_adapter(
                    self.supervisor.workspace_capability_manager(),
                    lease,
                    name.to_owned(),
                    arguments,
                    cancel,
                )
                .await;
            }
            // 此处只会到达三个 Semantic Source；四个基础 Source 已在上方本地完成。
            let text = call_workspace_source(
                self.supervisor.workspace_capability_manager(),
                lease.clone(),
                name.to_owned(),
                arguments,
            )
            .await?;
            return Ok(json!({
                "workspace":{"id":lease.workspace_id,"generation":lease.generation},
                "text":text,
                "truncated":false
            }));
        }
        Err("UNKNOWN_TOOL".into())
    }
    pub async fn sync_projects(&self, sources: Vec<PathBuf>) -> Result<usize, String> {
        let _management = self.management.lock().await;
        *self.project_sources.lock().unwrap() = sources.clone();
        let result = projects::read(sources)?;
        let count = WorkspaceRegistry::new(&self.supervisor).import_serena(&result.candidates)?;
        *self.sync_warnings.lock().unwrap() = result.warnings;
        self.log(&format!("已从 Serena 导入 {count} 个项目"));
        Ok(count)
    }
}

/// 将 Manager 的安全错误投影到冻结的 Semantic MCP error surface，不传播 Provider 原始错误。
fn map_semantic_capability_error(error: WorkspaceCapabilityError) -> String {
    match error.code {
        WorkspaceCapabilityErrorCode::Busy => "SEMANTIC_PROVIDER_BUSY".into(),
        WorkspaceCapabilityErrorCode::StartFailed => "SEMANTIC_RUNTIME_START_FAILED".into(),
        WorkspaceCapabilityErrorCode::RuntimeLost => "SEMANTIC_RUNTIME_LOST".into(),
        WorkspaceCapabilityErrorCode::NotFound => "SEMANTIC_PROVIDER_UNAVAILABLE".into(),
        // Runtime/Lease identity mismatch 是契约 fail-closed，不可伪装成 Provider 不可用。
        WorkspaceCapabilityErrorCode::ContractError => "WORKSPACE_CAPABILITY_CONTRACT_ERROR".into(),
        code => serde_json::to_value(code)
            .expect("WorkspaceCapabilityErrorCode must serialize")
            .as_str()
            .expect("WorkspaceCapabilityErrorCode must serialize as a string")
            .into(),
    }
}

/// 在 CodeGraph Adapter compatibility 边界投影统一 Manager 错误，不向公开 MCP 泄露 runtime 细节。
fn map_codegraph_capability_error(error: WorkspaceCapabilityError) -> String {
    match error.code {
        WorkspaceCapabilityErrorCode::Busy => "CODEGRAPH_BUSY".into(),
        WorkspaceCapabilityErrorCode::NotPrepared
        | WorkspaceCapabilityErrorCode::PreparationRequired => "CODEGRAPH_NOT_INITIALIZED".into(),
        WorkspaceCapabilityErrorCode::StartFailed => "CODEGRAPH_RUNTIME_START_FAILED".into(),
        WorkspaceCapabilityErrorCode::RuntimeLost => "CODEGRAPH_RUNTIME_LOST".into(),
        WorkspaceCapabilityErrorCode::NotFound => "CODEGRAPH_RUNTIME_START_FAILED".into(),
        WorkspaceCapabilityErrorCode::ContractError => "WORKSPACE_CAPABILITY_CONTRACT_ERROR".into(),
        code => serde_json::to_value(code)
            .expect("WorkspaceCapabilityErrorCode must serialize")
            .as_str()
            .expect("WorkspaceCapabilityErrorCode must serialize as string")
            .to_owned(),
    }
}

/// 将已解析 Lease 后的 CodeGraph 能力错误固定投影为公开安全对象。
/// workspace 为 null 时仍保留 request-scoped 错误语义，且不会因 Remove race 读取或泄露 root。
fn codegraph_capability_error_value(error: WorkspaceCapabilityError) -> Value {
    let code = map_codegraph_capability_error(error);
    let (message, recoverable) = match code.as_str() {
        "CODEGRAPH_BUSY" => ("CodeGraph is starting for the active workspace.", true),
        "CODEGRAPH_NOT_INITIALIZED" => (
            "The active workspace has no initialized CodeGraph index.",
            false,
        ),
        "CODEGRAPH_RUNTIME_START_FAILED" => {
            ("CodeGraph failed to start for the active workspace.", true)
        }
        "CODEGRAPH_RUNTIME_LOST" => ("The CodeGraph runtime connection was lost.", true),
        "WORKSPACE_CAPABILITY_CONTRACT_ERROR" => {
            ("CodeGraph returned an invalid capability result.", false)
        }
        _ => ("CodeGraph could not complete this request.", false),
    };
    json!({"error":{
        "code":code,
        "message":message,
        "workspace":Value::Null,
        "recoverable":recoverable
    }})
}

/// 将三个 Semantic Source 唯一地交给 request Lease 对应的 Serena Slot，并拒绝非文本 Provider 结果。
async fn call_workspace_source(
    manager: Arc<crate::workspace_capability::WorkspaceCapabilityManager>,
    lease: crate::workspace_resolver::WorkspaceLease,
    tool_name: String,
    arguments: Value,
) -> Result<String, String> {
    let result = manager
        .call(
            "serena",
            lease,
            WorkspaceToolCall {
                tool_name,
                arguments,
                cancellation: CancellationToken::new(),
            },
        )
        .await
        .map_err(map_semantic_capability_error)?;
    result
        .result
        .as_str()
        .map(str::to_owned)
        .ok_or("WORKSPACE_CAPABILITY_CONTRACT_ERROR".into())
}

/// 统一解码无状态 Adapter 的业务结果；Manager 只负责 Provider 选择与 Lease/Runtime 不变量。
async fn call_workspace_adapter(
    manager: Arc<crate::workspace_capability::WorkspaceCapabilityManager>,
    lease: crate::workspace_resolver::WorkspaceLease,
    tool_name: String,
    arguments: Value,
    cancellation: CancellationToken,
) -> Result<Value, String> {
    let result = manager
        .call_tool(
            lease,
            WorkspaceToolCall {
                tool_name,
                arguments,
                cancellation,
            },
        )
        .await
        .map_err(map_semantic_capability_error)?;
    capability_adapters::decode_tool_result(result)
}

/// CodeGraph 专用公开边界：Manager 保持 provider-agnostic，只在 Adapter route 映射兼容错误码。
async fn call_codegraph_adapter(
    manager: Arc<crate::workspace_capability::WorkspaceCapabilityManager>,
    lease: crate::workspace_resolver::WorkspaceLease,
    tool_name: String,
    arguments: Value,
    cancellation: CancellationToken,
) -> Result<Value, String> {
    let result = match manager
        .call_tool(
            lease.clone(),
            WorkspaceToolCall {
                tool_name,
                arguments,
                cancellation,
            },
        )
        .await
    {
        Ok(result) => result,
        Err(error) => return Ok(codegraph_capability_error_value(error)),
    };
    // CodeGraph Provider 保留既有纯文本 Tool family；它不使用 Source/Git 的 AdapterToolResult envelope。
    let Some(text) = result.result.as_str() else {
        return Ok(codegraph_capability_error_value(WorkspaceCapabilityError {
            code: WorkspaceCapabilityErrorCode::ContractError,
        }));
    };
    Ok(
        json!({"workspace":{"id":lease.workspace_id,"generation":lease.generation},
        "text":text,"truncated":false}),
    )
}

fn append_log(logs: &Mutex<VecDeque<String>>, message: &str) {
    append_log_level(logs, "INFO", "MCP", message);
}
fn append_log_level(logs: &Mutex<VecDeque<String>>, level: &str, category: &str, message: &str) {
    let mut logs = logs.lock().unwrap();
    if logs.len() == 500 {
        logs.pop_front();
    }
    logs.push_back(format!(
        "{level:<5} {} [{category}] {message}",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f")
    ));
}
pub fn get(app: &AppHandle) -> Arc<Broker> {
    app.state::<Arc<Broker>>().inner().clone()
}
#[cfg(test)]
mod integration_tests {
    use super::*;
    use crate::{
        config::{self, AppPaths, BrokerConfig, Workspace},
        workspace_capability::{
            CapabilityActionAuthority, CapabilityActionDescriptor, CapabilityActionExecution,
            CapabilityActivitySink, CapabilityFuture, CapabilityPreparationPolicy,
            CapabilityPrepareAction, CapabilityPrepareResult, CapabilityProviderError,
            CapabilityProviderErrorCode, CapabilityReadinessProbe, CapabilityRuntimeHandle,
            CapabilityRuntimeModel, CapabilityRuntimePolicy, CapabilityRuntimeState,
            CapabilityStageDescriptor, CapabilityStageRequirement, CapabilityStopFailure,
            StopEvidence, WorkspaceCapabilityDescriptor, WorkspaceCapabilityManager,
            WorkspaceCapabilityProvider, WorkspaceCapabilityProviderId,
            WorkspaceCapabilityRegistry, WorkspaceToolCall, WorkspaceToolResult,
        },
        workspace_registry::{WorkspaceRegistry, WorkspaceRegistrySnapshot},
        workspace_resolver::WorkspaceLease,
    };
    use rmcp::{
        ServiceExt, model::CallToolRequestParams, transport::StreamableHttpClientTransport,
    };
    fn port() -> u16 {
        std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    }
    fn fixture(dir: &std::path::Path, exe: Option<PathBuf>) -> Arc<Broker> {
        let paths = AppPaths {
            runtime_directory: dir.join("runtime"),
            config_file: dir.join("config.json"),
            log_directory: dir.join("logs"),
            app_log: dir.join("logs/app.log"),
            serena_log: dir.join("logs/serena.log"),
        };
        let config = ManagerConfig {
            serena_path: exe,
            port: port(),
            broker: BrokerConfig {
                enabled: false,
                port: port(),
                allow_lan: false,
            },
            dashboard_enabled: false,
            auto_start_server: false,
            ..Default::default()
        };
        crate::config::save(&paths.config_file, &config).unwrap();
        Arc::new(Broker::new(Arc::new(SupervisorState::new(paths).unwrap())))
    }

    /// 仅用于 Broker Semantic 路由测试的 Provider，记录 server-resolved Lease 和已净化参数。
    struct SemanticRoutingProvider {
        descriptor: WorkspaceCapabilityDescriptor,
        calls: std::sync::Mutex<Vec<(WorkspaceLease, WorkspaceToolCall)>>,
        wrong_runtime: std::sync::atomic::AtomicBool,
        fail_start: std::sync::atomic::AtomicBool,
        probes: std::sync::atomic::AtomicUsize,
        observations: std::sync::atomic::AtomicUsize,
    }

    impl SemanticRoutingProvider {
        /// 创建保留 Serena provider identity 的确定性路由 fixture。
        fn new() -> Self {
            Self {
                descriptor: WorkspaceCapabilityDescriptor {
                    provider_id: WorkspaceCapabilityProviderId::new("serena"),
                    display_name: "Semantic routing fixture".into(),
                    tool_names: registry::SEMANTIC_SOURCES
                        .iter()
                        .map(|source| (*source).into())
                        .collect(),
                    runtime_model: CapabilityRuntimeModel::WorkspaceScopedProcess,
                    readiness_probe: CapabilityReadinessProbe::Required,
                    preparation_policy: CapabilityPreparationPolicy::AutoOnFirstToolCall,
                    stage_descriptors: vec![CapabilityStageDescriptor {
                        id: "project_configuration".into(),
                        display_name: "项目配置".into(),
                        requirement: CapabilityStageRequirement::AutoPreparable,
                    }],
                    action_descriptors: vec![CapabilityActionDescriptor {
                        action_id: "prepare".into(),
                        display_name: "准备".into(),
                        authority: CapabilityActionAuthority::LocalHuman,
                        execution: CapabilityActionExecution::ManagerEnsureRuntime,
                        warm_runtime: true,
                    }],
                    runtime_policy: CapabilityRuntimePolicy {
                        max_instances: 2,
                        idle_timeout_ms: 0,
                        per_slot_concurrency: 1,
                    },
                },
                calls: std::sync::Mutex::new(Vec::new()),
                wrong_runtime: std::sync::atomic::AtomicBool::new(false),
                fail_start: std::sync::atomic::AtomicBool::new(false),
                probes: std::sync::atomic::AtomicUsize::new(0),
                observations: std::sync::atomic::AtomicUsize::new(0),
            }
        }

        /// 创建只用于 Remote cutover Gate 的 CodeGraph Adapter fixture，不触发旧 Global Binding。
        fn codegraph() -> Self {
            let mut provider = Self::new();
            provider.descriptor.provider_id = WorkspaceCapabilityProviderId::new("codegraph");
            provider.descriptor.display_name = "CodeGraph routing fixture".into();
            provider.descriptor.tool_names = vec!["codegraph_explore".into()];
            provider.descriptor.preparation_policy = CapabilityPreparationPolicy::ExplicitOnly;
            provider.descriptor.runtime_policy.idle_timeout_ms = 300_000;
            provider
        }
    }

    impl WorkspaceCapabilityProvider for SemanticRoutingProvider {
        fn descriptor(&self) -> &WorkspaceCapabilityDescriptor {
            &self.descriptor
        }

        fn probe_installation(
            &self,
        ) -> CapabilityFuture<
            '_,
            Result<crate::workspace_capability::CapabilityInstallation, CapabilityProviderError>,
        > {
            self.probes
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Box::pin(async {
                Err(CapabilityProviderError {
                    code: CapabilityProviderErrorCode::OperationFailed,
                })
            })
        }

        fn observe_readiness(
            &self,
            _lease: WorkspaceLease,
        ) -> CapabilityFuture<
            '_,
            Result<crate::workspace_capability::CapabilityObservation, CapabilityProviderError>,
        > {
            self.observations
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Box::pin(async {
                Err(CapabilityProviderError {
                    code: CapabilityProviderErrorCode::OperationFailed,
                })
            })
        }

        fn prepare<'a>(
            &'a self,
            _lease: WorkspaceLease,
            _action: CapabilityPrepareAction,
            _activity: &'a dyn CapabilityActivitySink,
        ) -> CapabilityFuture<'a, Result<CapabilityPrepareResult, CapabilityProviderError>>
        {
            Box::pin(async {
                Err(CapabilityProviderError {
                    code: CapabilityProviderErrorCode::OperationFailed,
                })
            })
        }

        fn start(
            &self,
            lease: WorkspaceLease,
        ) -> CapabilityFuture<'_, Result<CapabilityRuntimeHandle, CapabilityProviderError>>
        {
            let provider_id = self.descriptor.provider_id.clone();
            let wrong_runtime = self.wrong_runtime.load(std::sync::atomic::Ordering::SeqCst);
            let fail_start = self.fail_start.load(std::sync::atomic::Ordering::SeqCst);
            Box::pin(async move {
                if fail_start {
                    return Err(CapabilityProviderError {
                        code: CapabilityProviderErrorCode::OperationFailed,
                    });
                }
                let provider_id = if wrong_runtime {
                    WorkspaceCapabilityProviderId::new("wrong-provider")
                } else {
                    provider_id
                };
                Ok(CapabilityRuntimeHandle::new(provider_id, &lease))
            })
        }

        fn call<'a>(
            &'a self,
            lease: &'a WorkspaceLease,
            _runtime: Option<&'a CapabilityRuntimeHandle>,
            tool: WorkspaceToolCall,
        ) -> CapabilityFuture<'a, Result<WorkspaceToolResult, CapabilityProviderError>> {
            let calls = &self.calls;
            let lease = lease.clone();
            Box::pin(async move {
                calls.lock().unwrap().push((lease.clone(), tool.clone()));
                Ok(WorkspaceToolResult {
                    result: json!(format!("semantic:{}", lease.workspace_id)),
                })
            })
        }

        fn stop(
            &self,
            _runtime: CapabilityRuntimeHandle,
        ) -> CapabilityFuture<'_, Result<StopEvidence, CapabilityStopFailure>> {
            Box::pin(async {
                Ok(StopEvidence {
                    runtime_state: CapabilityRuntimeState::Stopped,
                })
            })
        }
    }

    /// 构造替换为 deterministic Semantic Manager 的 Broker，不触发 Serena Runtime 或全局 active 路径。
    fn semantic_fixture(
        dir: &std::path::Path,
        provider: Arc<SemanticRoutingProvider>,
    ) -> (Arc<Broker>, Arc<WorkspaceCapabilityManager>) {
        semantic_fixture_with_codegraph(
            dir,
            provider,
            Arc::new(SemanticRoutingProvider::codegraph()),
        )
    }

    /// 构造可观测 CodeGraph fixture 的 P2D Gate；生产路由仍只看 Registry descriptor 与 Lease。
    fn semantic_fixture_with_codegraph(
        dir: &std::path::Path,
        provider: Arc<SemanticRoutingProvider>,
        codegraph: Arc<SemanticRoutingProvider>,
    ) -> (Arc<Broker>, Arc<WorkspaceCapabilityManager>) {
        let paths = AppPaths {
            runtime_directory: dir.join("runtime"),
            config_file: dir.join("config.json"),
            log_directory: dir.join("logs"),
            app_log: dir.join("logs/app.log"),
            serena_log: dir.join("logs/serena.log"),
        };
        let config = ManagerConfig {
            port: port(),
            broker: BrokerConfig {
                enabled: false,
                port: port(),
                allow_lan: false,
            },
            auto_start_server: false,
            ..Default::default()
        };
        crate::config::save(&paths.config_file, &config).unwrap();
        let provider_port: Arc<dyn WorkspaceCapabilityProvider> = provider;
        let source_provider: Arc<dyn WorkspaceCapabilityProvider> =
            Arc::new(capability_adapters::SourceCapabilityProvider::new());
        let git_provider: Arc<dyn WorkspaceCapabilityProvider> =
            Arc::new(capability_adapters::GitCapabilityProvider::new());
        let codegraph_provider: Arc<dyn WorkspaceCapabilityProvider> = codegraph;
        let manager = Arc::new(WorkspaceCapabilityManager::new(Arc::new(
            WorkspaceCapabilityRegistry::new(vec![
                provider_port,
                source_provider,
                git_provider,
                codegraph_provider,
            ])
            .unwrap(),
        )));
        let mut supervisor = SupervisorState::new(paths).unwrap();
        supervisor.replace_workspace_capability_manager_for_test(Arc::clone(&manager));
        (Arc::new(Broker::new(Arc::new(supervisor))), manager)
    }

    #[test]
    fn workspace_id_foundation_routes_valid_unknown_ids_to_resolver() {
        let directory = tempfile::tempdir().unwrap();
        let provider = Arc::new(SemanticRoutingProvider::new());
        let (broker, _manager) = semantic_fixture(directory.path(), Arc::clone(&provider));

        assert_eq!(
            registry::resolve_workspace_lease(
                &broker.supervisor,
                &json!({"workspaceId":"unknown"})
            ),
            Err(WORKSPACE_NOT_FOUND.into())
        );
    }

    #[tokio::test]
    async fn semantic_sources_route_each_explicit_lease_without_legacy_active_authority() {
        let directory = tempfile::tempdir().unwrap();
        let provider = Arc::new(SemanticRoutingProvider::new());
        let (broker, _manager) = semantic_fixture(directory.path(), Arc::clone(&provider));
        let root_a = directory.path().join("workspace-a");
        let root_b = directory.path().join("workspace-b");
        std::fs::create_dir(&root_a).unwrap();
        std::fs::create_dir(&root_b).unwrap();
        std::fs::create_dir(root_a.join("src")).unwrap();
        std::fs::create_dir(root_b.join("src")).unwrap();
        std::fs::write(root_a.join("src/lib.rs"), "workspace_a").unwrap();
        std::fs::write(root_b.join("src/lib.rs"), "workspace_b").unwrap();
        let registry = WorkspaceRegistry::new(&broker.supervisor);
        let workspace_a = registry
            .register(root_a, Some("Workspace A".into()))
            .unwrap();
        let workspace_b = registry
            .register(root_b, Some("Workspace B".into()))
            .unwrap();
        // Desktop selection 故意固定到 B；后续请求仍必须严格使用各自 workspaceId。
        broker
            .supervisor
            .select_desktop_workspace(&workspace_b.id)
            .unwrap();

        // 持有 legacy active 写锁仍可完成调用，证明 Semantic route 不读取该全局 Authority。
        let legacy_active_lock = broker.workspace.write().await;
        let result_a = broker
            .dispatch(
                "source_symbols_overview",
                json!({
                    "workspaceId":workspace_a.id,
                    "relative_path":"src/lib.rs",
                    "depth":null,
                    "max_bytes":123
                }),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        let result_b = broker
            .dispatch(
                "source_find_symbol",
                json!({
                    "workspaceId":workspace_b.id,
                    "name_path_pattern":"Widget",
                    "relative_path":null,
                    "include_body":null
                }),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        drop(legacy_active_lock);

        assert_eq!(result_a["workspace"]["id"], workspace_a.id);
        assert_eq!(result_a["workspace"]["generation"], workspace_a.generation);
        assert_eq!(result_b["workspace"]["id"], workspace_b.id);
        assert_eq!(result_b["workspace"]["generation"], workspace_b.generation);
        {
            let calls = provider.calls.lock().unwrap();
            assert_eq!(calls.len(), 2);
            assert_eq!(calls[0].0.workspace_id, workspace_a.id);
            assert_eq!(calls[1].0.workspace_id, workspace_b.id);
            for (_, tool) in calls.iter() {
                let arguments = tool.arguments.as_object().unwrap();
                assert!(arguments.get("workspaceId").is_none());
                for forbidden in ["root", "canonicalRoot", "projectPath"] {
                    assert!(arguments.get(forbidden).is_none(), "{forbidden}");
                }
            }
        }
        for arguments in [
            json!({
                "workspaceId":workspace_a.id,
                "relative_path":"src/lib.rs",
                "root":"caller-supplied"
            }),
            json!({
                "workspaceId":workspace_a.id,
                "relative_path":"src/lib.rs",
                "canonicalRoot":"caller-supplied"
            }),
            json!({
                "workspaceId":workspace_a.id,
                "relative_path":"src/lib.rs",
                "projectPath":"caller-supplied"
            }),
        ] {
            assert!(
                broker
                    .dispatch(
                        "source_symbols_overview",
                        arguments,
                        CancellationToken::new(),
                    )
                    .await
                    .unwrap_err()
                    .starts_with("INVALID_PARAMS")
            );
        }
        assert_eq!(provider.calls.lock().unwrap().len(), 2);
        // 持有 legacy active 写锁仍可完成四个本地基础 Source，证明它们均不读取全局 Authority。
        let legacy_active_lock = broker.workspace.write().await;
        for (name, arguments, workspace) in [
            (
                "source_read_file",
                json!({"relative_path":"src/lib.rs"}),
                &workspace_a,
            ),
            // A/B 均读取同名相对路径，验证 read 的根目录和 Slot 仍完全由请求 Lease 决定。
            (
                "source_read_file",
                json!({"relative_path":"src/lib.rs"}),
                &workspace_b,
            ),
            (
                "source_list_dir",
                json!({"relative_path":"src"}),
                &workspace_b,
            ),
            (
                "source_find_file",
                json!({"file_mask":"*.rs"}),
                &workspace_a,
            ),
            (
                "source_search_pattern",
                json!({"substring_pattern":"Workspace"}),
                &workspace_b,
            ),
        ] {
            let mut arguments = arguments.as_object().unwrap().clone();
            arguments.insert("workspaceId".into(), json!(workspace.id));
            let result = broker
                .dispatch(name, Value::Object(arguments), CancellationToken::new())
                .await
                .unwrap();
            assert_eq!(result["workspace"]["id"], workspace.id, "{name}");
            assert_eq!(
                result["workspace"]["generation"], workspace.generation,
                "{name}"
            );
        }
        drop(legacy_active_lock);
        {
            let calls = provider.calls.lock().unwrap();
            // 四个基础 Source 均只走本地 Rust；本段调用不应触发 Serena Provider。
            assert_eq!(calls.len(), 2);
            assert!(calls.iter().all(|(_, tool)| {
                tool.tool_name != "source_read_file"
                    && tool.tool_name != "source_list_dir"
                    && tool.tool_name != "source_find_file"
                    && tool.tool_name != "source_search_pattern"
            }));
        }
        for relative_path in ["../outside", "C:/outside", "\\\\server\\share"] {
            assert!(
                broker
                    .dispatch(
                        "source_list_dir",
                        json!({"workspaceId":workspace_a.id,"relative_path":relative_path}),
                        CancellationToken::new(),
                    )
                    .await
                    .unwrap_err()
                    .starts_with("INVALID_PATH")
            );
        }
        assert_eq!(provider.calls.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn semantic_sources_preserve_workspace_context_and_contract_errors() {
        let directory = tempfile::tempdir().unwrap();
        let provider = Arc::new(SemanticRoutingProvider::new());
        let (broker, _manager) = semantic_fixture(directory.path(), Arc::clone(&provider));
        let root = directory.path().join("workspace");
        std::fs::create_dir(&root).unwrap();
        let workspace = WorkspaceRegistry::new(&broker.supervisor)
            .register(root, Some("Workspace".into()))
            .unwrap();

        for args in [json!({}), json!({"workspaceId":null})] {
            assert_eq!(
                broker
                    .dispatch("source_find_references", args, CancellationToken::new())
                    .await,
                Err("WORKSPACE_CONTEXT_REQUIRED".into())
            );
        }
        for args in [json!({"workspaceId":3}), json!({"workspaceId":" \t"})] {
            assert!(
                broker
                    .dispatch("source_find_references", args, CancellationToken::new())
                    .await
                    .unwrap_err()
                    .starts_with("INVALID_PARAMS")
            );
        }
        assert_eq!(
            broker
                .dispatch(
                    "source_find_references",
                    json!({"workspaceId":"unknown","relative_path":"src/lib.rs","name_path":"Widget"}),
                    CancellationToken::new(),
                )
                .await,
            Err(WORKSPACE_NOT_FOUND.into())
        );

        provider
            .wrong_runtime
            .store(true, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(
            broker
                .dispatch(
                    "source_find_references",
                    json!({"workspaceId":workspace.id,"relative_path":"src/lib.rs","name_path":"Widget"}),
                    CancellationToken::new(),
                )
                .await,
            Err("WORKSPACE_CAPABILITY_CONTRACT_ERROR".into())
        );
        assert!(provider.calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn serena_missing_keeps_basic_sources_available_without_provider_calls() {
        let directory = tempfile::tempdir().unwrap();
        let provider = Arc::new(SemanticRoutingProvider::new());
        let (broker, _manager) = semantic_fixture(directory.path(), Arc::clone(&provider));
        let root = directory.path().join("workspace");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("file.txt"), "workspace text\n").unwrap();
        std::fs::write(root.join("src/marker.rs"), "// workspace marker\n").unwrap();
        let workspace = WorkspaceRegistry::new(&broker.supervisor)
            .register(root, Some("Workspace".into()))
            .unwrap();

        for (name, valid_arguments) in [
            ("source_read_file", json!({"relative_path":"file.txt"})),
            ("source_list_dir", json!({"relative_path":"src"})),
            ("source_find_file", json!({"file_mask":"*.rs"})),
            (
                "source_search_pattern",
                json!({"substring_pattern":"Workspace"}),
            ),
        ] {
            for arguments in [json!({}), json!({"workspaceId":null})] {
                assert_eq!(
                    broker
                        .dispatch(name, arguments, CancellationToken::new())
                        .await,
                    Err("WORKSPACE_CONTEXT_REQUIRED".into()),
                    "{name}"
                );
            }
            for workspace_id in [json!(3), json!(" \t")] {
                let mut arguments = valid_arguments.as_object().unwrap().clone();
                arguments.insert("workspaceId".into(), workspace_id);
                assert!(
                    broker
                        .dispatch(name, Value::Object(arguments), CancellationToken::new())
                        .await
                        .unwrap_err()
                        .starts_with("INVALID_PARAMS"),
                    "{name}"
                );
            }
            let mut unknown = valid_arguments.as_object().unwrap().clone();
            unknown.insert("workspaceId".into(), json!("unknown"));
            assert_eq!(
                broker
                    .dispatch(name, Value::Object(unknown), CancellationToken::new())
                    .await,
                Err(WORKSPACE_NOT_FOUND.into()),
                "{name}"
            );
        }
        assert_ne!(
            broker.supervisor.snapshot().server_status,
            ServerStatus::Running
        );
        broker.start().await.unwrap();
        let port = broker.snapshot().await.port;
        let client = ()
            .serve(StreamableHttpClientTransport::from_uri(format!(
                "http://127.0.0.1:{port}/mcp"
            )))
            .await
            .unwrap();
        let advertised = client.list_all_tools().await.unwrap();
        for &name in registry::LOCAL_SOURCES
            .iter()
            .chain(registry::SEMANTIC_SOURCES)
        {
            assert!(advertised.iter().any(|tool| tool.name == name), "{name}");
        }
        for (name, arguments) in [
            ("source_read_file", json!({"relative_path":"file.txt"})),
            ("source_list_dir", json!({"relative_path":"src"})),
            ("source_find_file", json!({"file_mask":"*.rs"})),
            (
                "source_search_pattern",
                json!({"substring_pattern":"workspace marker"}),
            ),
        ] {
            let mut arguments = arguments.as_object().unwrap().clone();
            arguments.insert("workspaceId".into(), json!(workspace.id));
            let result = client
                .call_tool(CallToolRequestParams::new(name).with_arguments(arguments))
                .await
                .unwrap();
            assert_ne!(result.is_error, Some(true), "{name}: {result:?}");
            assert_eq!(
                result.structured_content.unwrap()["workspace"]["id"],
                workspace.id,
                "{name}"
            );
        }
        assert!(provider.calls.lock().unwrap().is_empty());
        provider
            .fail_start
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let semantic = client
            .call_tool(
                CallToolRequestParams::new("source_symbols_overview").with_arguments(
                    json!({"workspaceId":workspace.id,"relative_path":"src/marker.rs"})
                        .as_object()
                        .unwrap()
                        .clone(),
                ),
            )
            .await
            .unwrap();
        // capability 失败按既有 MCP error 语义返回，且不影响 listener/list。
        assert_eq!(semantic.is_error, Some(true));
        let text = serde_json::to_string(&semantic).unwrap();
        assert!(text.contains("SEMANTIC_RUNTIME_START_FAILED"), "{text}");
        client.cancel().await.unwrap();
        broker.stop().await.unwrap();
    }

    #[tokio::test]
    async fn workspace_discovery_never_observes_registered_providers() {
        let directory = tempfile::tempdir().unwrap();
        let provider = Arc::new(SemanticRoutingProvider::new());
        let (broker, _) = semantic_fixture(directory.path(), Arc::clone(&provider));
        let root = directory.path().join("workspace");
        std::fs::create_dir(&root).unwrap();
        let workspace = WorkspaceRegistry::new(&broker.supervisor)
            .register(root, Some("Workspace".into()))
            .unwrap();

        broker
            .dispatch("workspace_list", json!({}), CancellationToken::new())
            .await
            .unwrap();
        broker
            .dispatch(
                "workspace_get",
                json!({"workspaceId": workspace.id}),
                CancellationToken::new(),
            )
            .await
            .unwrap();

        assert_eq!(provider.probes.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(
            provider
                .observations
                .load(std::sync::atomic::Ordering::SeqCst),
            0
        );
    }

    #[tokio::test]
    async fn workspace_discovery_reads_registry_without_authority_or_root_validation() {
        let directory = tempfile::tempdir().unwrap();
        let broker = fixture(
            directory.path(),
            Some(directory.path().join("missing-serena.exe")),
        );
        let root_a = directory.path().join("workspace-a");
        let root_b = directory.path().join("workspace-b");
        std::fs::create_dir(&root_a).unwrap();
        std::fs::create_dir(&root_b).unwrap();
        let registry = WorkspaceRegistry::new(&broker.supervisor);
        let initial_revision = registry.list().registry_revision;
        let workspace_a = registry
            .register(root_a.clone(), Some("Workspace A".into()))
            .unwrap();
        let workspace_b = registry
            .register(root_b, Some("Workspace B".into()))
            .unwrap();
        let initial = registry.list();
        assert_eq!(initial.registry_revision, initial_revision + 2);
        assert_eq!(
            initial.workspaces,
            vec![workspace_a.clone(), workspace_b.clone()]
        );
        broker
            .supervisor
            .select_desktop_workspace(&workspace_b.id)
            .unwrap();
        assert!(broker.workspace.read().await.is_none());

        let listed = broker
            .dispatch("workspace_list", json!({}), CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(listed["registryRevision"], initial.registry_revision);
        assert_eq!(
            listed["workspaces"],
            serde_json::to_value(&initial.workspaces).unwrap()
        );
        assert_eq!(listed["truncated"], false);

        let fetched_b = broker
            .dispatch(
                "workspace_get",
                json!({"workspaceId": workspace_b.id}),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(fetched_b["registryRevision"], initial.registry_revision);
        assert_eq!(
            fetched_b["workspace"],
            serde_json::to_value(&workspace_b).unwrap()
        );
        assert_eq!(fetched_b["truncated"], false);

        let legacy_server = super::orchestration_tests::active(&broker, directory.path()).await;
        assert_eq!(
            broker.workspace.read().await.as_ref().unwrap().workspace.id,
            "W"
        );
        std::fs::remove_dir(root_a).unwrap();
        let fetched_a = broker
            .dispatch(
                "workspace_get",
                json!({"workspaceId": workspace_a.id}),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(fetched_a["registryRevision"], initial.registry_revision);
        assert_eq!(
            fetched_a["workspace"],
            serde_json::to_value(&workspace_a).unwrap()
        );
        assert_eq!(
            broker.supervisor.desktop_selected_workspace().unwrap(),
            workspace_b
        );
        assert_eq!(
            broker.workspace.read().await.as_ref().unwrap().workspace.id,
            "W"
        );
        assert_eq!(
            broker
                .dispatch("git_status", json!({}), CancellationToken::new())
                .await,
            Err("WORKSPACE_CONTEXT_REQUIRED".into())
        );
        assert_eq!(
            broker
                .dispatch(
                    "workspace_get",
                    json!({"workspaceId": "unknown"}),
                    CancellationToken::new(),
                )
                .await,
            Err("WORKSPACE_NOT_FOUND".into())
        );
        assert_eq!(registry.list(), initial);

        let root_c = directory.path().join("workspace-c");
        std::fs::create_dir(root_c.clone()).unwrap();
        let workspace_c = registry
            .register(root_c, Some("Workspace C".into()))
            .unwrap();
        let updated = registry.list();
        assert_eq!(updated.registry_revision, initial.registry_revision + 1);
        assert_eq!(
            broker
                .dispatch("workspace_list", json!({}), CancellationToken::new())
                .await
                .unwrap()["registryRevision"],
            updated.registry_revision
        );
        assert_eq!(
            broker
                .dispatch(
                    "workspace_get",
                    json!({"workspaceId": workspace_c.id}),
                    CancellationToken::new(),
                )
                .await
                .unwrap()["registryRevision"],
            updated.registry_revision
        );
        assert_eq!(registry.list(), updated);
        legacy_server.abort();
    }

    #[tokio::test]
    async fn codegraph_requires_explicit_workspace_before_any_active_workspace_read() {
        let directory = tempfile::tempdir().unwrap();
        let broker = fixture(directory.path(), None);
        let root = directory.path().join("desktop-selected-workspace");
        std::fs::create_dir(&root).unwrap();
        let workspace = WorkspaceRegistry::new(&broker.supervisor)
            .register(root.clone(), Some("Desktop selected".into()))
            .unwrap();

        // Desktop selection remains a UI default only and cannot supply CodeGraph authority.
        broker
            .supervisor
            .select_desktop_workspace(&workspace.id)
            .unwrap();
        let active_guard = broker.workspace.write().await;
        for result in [
            tokio::time::timeout(
                Duration::from_millis(100),
                broker.dispatch(
                    "codegraph_explore",
                    json!({"query":"symbol"}),
                    CancellationToken::new(),
                ),
            )
            .await,
            tokio::time::timeout(
                Duration::from_millis(100),
                broker.call_tool(
                    "codegraph_explore",
                    json!({"query":"symbol"}),
                    CancellationToken::new(),
                ),
            )
            .await,
        ] {
            assert_eq!(result.unwrap(), Err("WORKSPACE_CONTEXT_REQUIRED".into()));
        }
        drop(active_guard);
        // 即使旧 ActiveWorkspace 已存在，缺少 workspaceId 也不能重新连到 active.graph 或启动子进程。
        let active_server = super::orchestration_tests::active(&broker, &root).await;
        assert!(broker.workspace.read().await.is_some());
        let logs_before_direct_call = broker.log_snapshot();
        assert_eq!(
            broker
                .dispatch(
                    "codegraph_explore",
                    json!({"query":"symbol"}),
                    CancellationToken::new(),
                )
                .await,
            Err("WORKSPACE_CONTEXT_REQUIRED".into())
        );
        active_server.abort();
        assert!(!root.join(".codegraph").exists());
        assert_eq!(broker.log_snapshot(), logs_before_direct_call);
    }

    #[tokio::test]
    /// P2D-009：Remote CodeGraph A/B 只把 Resolver 建立的 Lease 交给 Adapter，不读取 Desktop selection。
    async fn p2d_009_remote_codegraph_routes_ab_through_explicit_leases() {
        let directory = tempfile::tempdir().unwrap();
        let semantic = Arc::new(SemanticRoutingProvider::new());
        let codegraph = Arc::new(SemanticRoutingProvider::codegraph());
        let (broker, _manager) =
            semantic_fixture_with_codegraph(directory.path(), semantic, Arc::clone(&codegraph));
        let root_a = directory.path().join("codegraph-a");
        let root_b = directory.path().join("codegraph-b");
        std::fs::create_dir(&root_a).unwrap();
        std::fs::create_dir(&root_b).unwrap();
        let registry = WorkspaceRegistry::new(&broker.supervisor);
        let workspace_a = registry
            .register(root_a, Some("CodeGraph A".into()))
            .unwrap();
        let workspace_b = registry
            .register(root_b, Some("CodeGraph B".into()))
            .unwrap();
        broker
            .supervisor
            .select_desktop_workspace(&workspace_b.id)
            .unwrap();

        for workspace in [&workspace_a, &workspace_b] {
            let response = broker
                .dispatch(
                    "codegraph_explore",
                    json!({"workspaceId":workspace.id,"query":"lease route"}),
                    CancellationToken::new(),
                )
                .await
                .unwrap();
            assert_eq!(
                response,
                json!({
                    "workspace":{"id":workspace.id,"generation":workspace.generation},
                    "text":format!("semantic:{}", workspace.id),
                    "truncated":false
                })
            );
            let root = workspace.root.to_string_lossy();
            assert!(
                !serde_json::to_string(&response)
                    .unwrap()
                    .contains(root.as_ref())
            );
        }
        let calls = codegraph.calls.lock().unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].0.workspace_id, workspace_a.id);
        assert_eq!(calls[1].0.workspace_id, workspace_b.id);
        assert_eq!(
            calls[0].0.canonical_root,
            workspace_a.root.canonicalize().unwrap()
        );
        assert_eq!(
            calls[1].0.canonical_root,
            workspace_b.root.canonicalize().unwrap()
        );
        assert_eq!(calls[0].1.arguments["workspaceId"], workspace_a.id);
        assert_eq!(calls[1].1.arguments["workspaceId"], workspace_b.id);
    }

    #[test]
    /// CodeGraph public compatibility 映射固定在 Broker Adapter 边界，Manager Core 不识别 providerId。
    fn p2d_009_codegraph_compatibility_mapper_is_stable() {
        for (input, expected) in [
            (WorkspaceCapabilityErrorCode::Busy, "CODEGRAPH_BUSY"),
            (
                WorkspaceCapabilityErrorCode::NotPrepared,
                "CODEGRAPH_NOT_INITIALIZED",
            ),
            (
                WorkspaceCapabilityErrorCode::PreparationRequired,
                "CODEGRAPH_NOT_INITIALIZED",
            ),
            (
                WorkspaceCapabilityErrorCode::StartFailed,
                "CODEGRAPH_RUNTIME_START_FAILED",
            ),
            (
                WorkspaceCapabilityErrorCode::RuntimeLost,
                "CODEGRAPH_RUNTIME_LOST",
            ),
        ] {
            assert_eq!(
                map_codegraph_capability_error(WorkspaceCapabilityError { code: input }),
                expected
            );
        }
    }

    #[test]
    /// P2D-009：已进入 CodeGraph compatibility 边界的能力错误必须保持结构化且不泄露 Workspace root。
    fn p2d_009_codegraph_capability_errors_have_safe_structured_shape() {
        for (input, code, message, recoverable) in [
            (
                WorkspaceCapabilityErrorCode::Busy,
                "CODEGRAPH_BUSY",
                "CodeGraph is starting for the active workspace.",
                true,
            ),
            (
                WorkspaceCapabilityErrorCode::NotPrepared,
                "CODEGRAPH_NOT_INITIALIZED",
                "The active workspace has no initialized CodeGraph index.",
                false,
            ),
            (
                WorkspaceCapabilityErrorCode::PreparationRequired,
                "CODEGRAPH_NOT_INITIALIZED",
                "The active workspace has no initialized CodeGraph index.",
                false,
            ),
            (
                WorkspaceCapabilityErrorCode::ContractError,
                "WORKSPACE_CAPABILITY_CONTRACT_ERROR",
                "CodeGraph returned an invalid capability result.",
                false,
            ),
        ] {
            let value = codegraph_capability_error_value(WorkspaceCapabilityError { code: input });
            assert_eq!(
                value,
                json!({"error":{
                    "code":code,
                    "message":message,
                    "workspace":Value::Null,
                    "recoverable":recoverable
                }})
            );
            assert!(!serde_json::to_string(&value).unwrap().contains("root"));
        }
    }

    #[tokio::test]
    async fn git_dispatch_uses_only_each_explicit_workspace_lease() {
        let directory = tempfile::tempdir().unwrap();
        let broker = fixture(
            directory.path(),
            Some(directory.path().join("missing-serena.exe")),
        );
        let root_a = directory.path().join("repository-a");
        let root_b = directory.path().join("repository-b");
        for root in [&root_a, &root_b] {
            let mut command = process::command("git");
            command.arg("init").arg(root);
            process::run(
                command,
                8192,
                Duration::from_secs(10),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        }
        std::fs::write(root_a.join("only-a.txt"), "A\n").unwrap();
        std::fs::write(root_b.join("only-b.txt"), "B\n").unwrap();
        let registry = WorkspaceRegistry::new(&broker.supervisor);
        let workspace_a = registry
            .register(root_a.clone(), Some("Workspace A".into()))
            .unwrap();
        let workspace_b = registry
            .register(root_b.clone(), Some("Workspace B".into()))
            .unwrap();
        broker
            .supervisor
            .select_desktop_workspace(&workspace_b.id)
            .unwrap();
        // Discovery 只返回 Registry catalog，不能建立后续 Tool 的 Workspace binding。
        assert_eq!(
            broker
                .dispatch("workspace_list", json!({}), CancellationToken::new())
                .await
                .unwrap()["workspaces"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            broker
                .dispatch(
                    "workspace_get",
                    json!({"workspaceId":workspace_a.id}),
                    CancellationToken::new(),
                )
                .await
                .unwrap()["workspace"]["id"],
            workspace_a.id
        );
        assert!(broker.workspace.read().await.is_none());

        let (status_a, status_b) = tokio::join!(
            broker.dispatch(
                "git_status",
                json!({"workspaceId":workspace_a.id}),
                CancellationToken::new(),
            ),
            broker.dispatch(
                "git_status",
                json!({"workspaceId":workspace_b.id}),
                CancellationToken::new(),
            )
        );
        let status_a = status_a.unwrap();
        let status_b = status_b.unwrap();
        assert!(status_a["text"].as_str().unwrap().contains("only-a.txt"));
        assert!(!status_a["text"].as_str().unwrap().contains("only-b.txt"));
        assert!(status_b["text"].as_str().unwrap().contains("only-b.txt"));
        assert!(!status_b["text"].as_str().unwrap().contains("only-a.txt"));
        assert_eq!(
            status_a["workspace"],
            json!({"id":workspace_a.id,"generation":workspace_a.generation})
        );
        assert_eq!(
            status_b["workspace"],
            json!({"id":workspace_b.id,"generation":workspace_b.generation})
        );
        for status in [&status_a, &status_b] {
            assert!(status["workspace"].get("name").is_none());
            assert!(status["workspace"].get("root").is_none());
        }
        // 取消仍在 Git 的 stateless command 边界返回，且不会依赖 Serena。
        let cancelled = CancellationToken::new();
        cancelled.cancel();
        assert_eq!(
            broker
                .dispatch(
                    "git_status",
                    json!({"workspaceId":workspace_a.id}),
                    cancelled,
                )
                .await,
            Err("CANCELLED".into())
        );
        // 将遗留 Global ActiveWorkspace 刻意设为 B；后续请求 A 仍只使用 A 的 Lease。
        let active_b = super::orchestration_tests::active(&broker, &root_b).await;
        {
            let mut active = broker.workspace.write().await;
            let active = active.as_mut().unwrap();
            active.workspace.id = workspace_b.id.clone();
            active.workspace.generation = workspace_b.generation;
        }
        assert_eq!(
            broker.workspace.read().await.as_ref().unwrap().workspace.id,
            workspace_b.id
        );
        let active_b_request_a = broker
            .dispatch(
                "git_status",
                json!({"workspaceId":workspace_a.id}),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert!(
            active_b_request_a["text"]
                .as_str()
                .unwrap()
                .contains("only-a.txt")
        );
        // 旧 direct handler 即使仍可调用，也不得为后续 Tool 建立 Workspace Authority。
        assert_eq!(
            broker
                .dispatch("workspace_current", json!({}), CancellationToken::new())
                .await
                .unwrap()["activeWorkspace"],
            Value::Null
        );
        let activate_error = broker
            .dispatch("workspace_activate", json!({}), CancellationToken::new())
            .await
            .unwrap_err();
        assert!(activate_error.starts_with("INVALID_PARAMS"));
        assert_eq!(
            broker
                .dispatch("workspace_deactivate", json!({}), CancellationToken::new())
                .await
                .unwrap()["status"],
            "inactive"
        );
        for (name, args) in [
            ("source_symbols_overview", json!({})),
            ("git_status", json!({})),
        ] {
            assert_eq!(
                broker.dispatch(name, args, CancellationToken::new()).await,
                Err("WORKSPACE_CONTEXT_REQUIRED".into()),
                "{name} must not inherit a compatibility or Discovery context"
            );
        }
        assert!(
            !active_b_request_a["text"]
                .as_str()
                .unwrap()
                .contains("only-b.txt")
        );
        assert_eq!(
            broker.supervisor.desktop_selected_workspace().unwrap().id,
            workspace_b.id
        );

        for args in [json!({}), json!({"workspaceId":null})] {
            assert_eq!(
                broker
                    .dispatch("git_status", args, CancellationToken::new())
                    .await,
                Err("WORKSPACE_CONTEXT_REQUIRED".into())
            );
        }
        for args in [
            json!({"workspaceId":7}),
            json!({"workspaceId":""}),
            json!({"workspaceId":" \n"}),
        ] {
            assert!(
                broker
                    .dispatch("git_status", args, CancellationToken::new())
                    .await
                    .unwrap_err()
                    .starts_with("INVALID_PARAMS")
            );
        }
        assert_eq!(
            broker
                .dispatch(
                    "git_status",
                    json!({"workspaceId":"unknown"}),
                    CancellationToken::new(),
                )
                .await,
            Err("WORKSPACE_NOT_FOUND".into())
        );

        let ordinary_root = directory.path().join("ordinary-directory");
        std::fs::create_dir(&ordinary_root).unwrap();
        let ordinary = registry
            .register(ordinary_root.clone(), Some("Ordinary".into()))
            .unwrap();
        // 普通目录同样先由 Registry 解析为 Lease；是否为 Git 仓库只能由后续 Git 命令决定。
        let ordinary_lease = registry::resolve_workspace_lease(
            &broker.supervisor,
            &json!({"workspaceId": ordinary.id}),
        )
        .unwrap();
        assert_eq!(ordinary_lease.workspace_id, ordinary.id);
        assert_eq!(
            ordinary_lease.canonical_root,
            ordinary_root.canonicalize().unwrap()
        );
        let error = broker
            .dispatch(
                "git_status",
                json!({"workspaceId":ordinary.id}),
                CancellationToken::new(),
            )
            .await
            .unwrap_err();
        assert!(error.starts_with("BACKEND_ERROR:"), "{error}");
        // 非 Git discovery 路由保持独立，不以 Git 的 Lease 作为隐式状态。
        assert_eq!(
            broker
                .dispatch(
                    "workspace_get",
                    json!({"workspaceId":workspace_b.id}),
                    CancellationToken::new()
                )
                .await
                .unwrap()["workspace"]["id"],
            workspace_b.id
        );
        active_b.abort();
    }

    #[tokio::test]
    async fn startup_and_restart_preserve_registry_despite_conflicting_serena_registry() {
        let directory = tempfile::tempdir().unwrap();
        let paths = AppPaths {
            runtime_directory: directory.path().join("runtime"),
            config_file: directory.path().join("config.json"),
            log_directory: directory.path().join("logs"),
            app_log: directory.path().join("logs/app.log"),
            serena_log: directory.path().join("logs/serena.log"),
        };
        let config = ManagerConfig {
            broker: BrokerConfig {
                enabled: false,
                port: port(),
                allow_lan: false,
            },
            workspace_registry_revision: 42,
            workspaces: vec![
                Workspace {
                    id: "first".into(),
                    name: "First".into(),
                    root: "C:/manager/first".into(),
                    generation: 3,
                },
                Workspace {
                    id: "second".into(),
                    name: "Second".into(),
                    root: "C:/manager/second".into(),
                    generation: 9,
                },
            ],
            ..ManagerConfig::default()
        };
        config::save(&paths.config_file, &config).unwrap();
        let expected = WorkspaceRegistrySnapshot {
            registry_revision: config.workspace_registry_revision,
            workspaces: config.workspaces.clone(),
        };
        let bytes = std::fs::read(&paths.config_file).unwrap();

        let conflicting_root = directory.path().join("serena-project");
        std::fs::create_dir_all(conflicting_root.join(".serena")).unwrap();
        std::fs::write(
            conflicting_root.join(".serena/project.yml"),
            "project_name: Replaces Manager Registry\n",
        )
        .unwrap();
        std::fs::create_dir_all(paths.serena_home()).unwrap();
        std::fs::write(
            paths.serena_home().join("serena_config.yml"),
            format!(
                "projects:\n  - '{}'\n",
                conflicting_root.display().to_string().replace('\\', "/")
            ),
        )
        .unwrap();

        let broker = Arc::new(Broker::new(Arc::new(
            SupervisorState::new(paths.clone()).unwrap(),
        )));
        crate::finish_broker_startup(broker.clone(), tauri::async_runtime::spawn(async {}))
            .await
            .unwrap();
        assert_eq!(WorkspaceRegistry::new(&broker.supervisor).list(), expected);
        assert_eq!(broker.config(), config);
        assert_eq!(std::fs::read(&paths.config_file).unwrap(), bytes);

        drop(broker);
        let restarted = Arc::new(Broker::new(Arc::new(
            SupervisorState::new(paths.clone()).unwrap(),
        )));
        crate::finish_broker_startup(restarted.clone(), tauri::async_runtime::spawn(async {}))
            .await
            .unwrap();
        assert_eq!(
            WorkspaceRegistry::new(&restarted.supervisor).list(),
            expected
        );
        assert_eq!(restarted.config(), config);
        assert_eq!(std::fs::read(paths.config_file).unwrap(), bytes);
    }
    #[test]
    fn ipc_tool_future_fits_the_windows_ui_stack() {
        let dir = tempfile::tempdir().unwrap();
        let broker = fixture(dir.path(), None);
        let work = broker.call_tool(
            "workspace_activate",
            json!({"id":"x"}),
            CancellationToken::new(),
        );
        let size = std::mem::size_of_val(&work);
        // Tauri constructs and moves this future on the UI thread before spawning it.
        // The previous inline dispatch occupied 40 KiB and overflowed that stack.
        assert!(size < 4096, "IPC tool future occupies {size} bytes");
    }

    #[test]
    fn managed_trust_configuration_is_explicit() {
        let dir = tempfile::tempdir().unwrap();
        let b = fixture(dir.path(), None);
        b.supervisor.paths.prepare_serena(false).unwrap();
        assert!(b.supervisor.paths.verify_serena_config().is_ok());
        let path = b.supervisor.paths.serena_home().join("serena_config.yml");
        std::fs::write(&path, "{}").unwrap();
        assert!(b.supervisor.paths.verify_serena_config().is_err());
        b.supervisor.paths.prepare_serena(false).unwrap();
        assert!(b.supervisor.paths.verify_serena_config().is_ok());
    }

    #[tokio::test]
    async fn failed_restart_keeps_workspace_unbound() {
        let dir = tempfile::tempdir().unwrap();
        let broker = fixture(dir.path(), Some(dir.path().join("missing-serena.exe")));
        assert!(crate::commands::restart_serena_impl(&broker).await.is_err());
        assert!(broker.workspace.read().await.is_none());
        assert!(broker.published.lock().unwrap().is_none());
        assert!(broker.snapshot().await.codegraph.is_none());
    }

    #[tokio::test]
    async fn ui_snapshot_does_not_wait_for_workspace_transition() {
        let dir = tempfile::tempdir().unwrap();
        let broker = fixture(dir.path(), None);
        let _transition = broker.workspace.write().await;
        let snapshot = tokio::time::timeout(Duration::from_millis(100), broker.snapshot())
            .await
            .unwrap();
        assert!(snapshot.active_workspace.is_none());
        assert!(snapshot.codegraph.is_none());
    }
    #[test]
    fn mcp_logs_keep_recent_entries_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let broker = fixture(dir.path(), None);
        assert!(broker.log_snapshot().is_empty());
        for i in 0..510 {
            broker.log(&format!("entry-{i}"));
        }
        let logs = broker.log_snapshot();
        assert_eq!(logs.len(), 500);
        assert!(logs[0].starts_with("INFO  "));
        assert!(logs[0].contains("[MCP] entry-10"));
        assert!(logs[0].ends_with("entry-10"));
        assert!(logs[499].ends_with("entry-509"));
    }

    #[test]
    fn clearing_mcp_logs_is_idempotent_and_keeps_new_entries() {
        let dir = tempfile::tempdir().unwrap();
        let broker = fixture(dir.path(), None);
        broker.log("old entry");
        broker.clear_logs();
        assert!(broker.log_snapshot().is_empty());
        broker.clear_logs();
        broker.log("new entry");
        let logs = broker.log_snapshot();
        assert_eq!(logs.len(), 1);
        assert!(logs[0].ends_with("new entry"));
    }
    #[tokio::test]
    async fn sync_compatibility_helper_is_additive_idempotent_and_does_not_activate_workspace() {
        let dir = tempfile::tempdir().unwrap();
        // A nonexistent executable proves Import does not require a working installation.
        let b = fixture(dir.path(), Some(dir.path().join("missing-serena.exe")));
        let local_a_root = dir.path().join("local-a");
        let local_b_root = dir.path().join("local-b");
        let imported_root = dir.path().join("imported");
        let invalid_root = dir.path().join("invalid");
        for root in [&local_a_root, &local_b_root, &imported_root, &invalid_root] {
            std::fs::create_dir_all(root.join(".serena")).unwrap();
        }
        let registry = WorkspaceRegistry::new(&b.supervisor);
        let local_a = registry
            .register(local_a_root, Some("Local A".into()))
            .unwrap();
        let local_b = registry
            .register(local_b_root.clone(), Some("Local B".into()))
            .unwrap();
        b.supervisor.select_desktop_workspace(&local_b.id).unwrap();
        std::fs::write(
            local_b_root.join(".serena/project.yml"),
            "project_name: Serena must not rename Local B\n",
        )
        .unwrap();
        std::fs::write(
            imported_root.join(".serena/project.yml"),
            "project_name: Imported C\n",
        )
        .unwrap();
        std::fs::write(invalid_root.join(".serena/project.yml"), "[invalid").unwrap();
        let source = dir.path().join("registry.yml");
        std::fs::write(
            &source,
            json!({"projects": [local_b_root, imported_root, invalid_root]}).to_string(),
        )
        .unwrap();
        let before = b.config();
        *b.sync_warnings.lock().unwrap() = vec!["同步失败：此前的配置错误".into()];
        *b.error.lock().unwrap() = Some("listener error".into());
        assert_eq!(b.sync_projects(vec![source.clone()]).await.unwrap(), 1);
        assert_eq!(b.sync_warnings.lock().unwrap().len(), 1);
        assert_eq!(b.error.lock().unwrap().as_deref(), Some("listener error"));
        assert_eq!(b.config().serena_path, before.serena_path);
        assert_eq!(b.config().broker, before.broker);
        assert_eq!(
            b.config().workspace_registry_revision,
            before.workspace_registry_revision + 1
        );
        assert_eq!(b.config().workspaces[..2], [local_a, local_b]);
        assert_eq!(b.config().workspaces[2].name, "Imported C");
        assert_eq!(
            b.supervisor
                .desktop_selected_workspace()
                .map(|workspace| workspace.id),
            before.desktop_selected_workspace_id
        );
        assert!(b.workspace.read().await.is_none());
        assert!(b.published.lock().unwrap().is_none());
        let saved = b.config();
        let bytes = std::fs::read(&b.supervisor.paths.config_file).unwrap();
        assert_eq!(b.sync_projects(vec![source.clone()]).await.unwrap(), 0);
        assert_eq!(b.config(), saved);
        assert_eq!(
            std::fs::read(&b.supervisor.paths.config_file).unwrap(),
            bytes
        );
        assert_eq!(
            b.sync_projects(vec![dir.path().join("missing-registry.yml")])
                .await
                .unwrap(),
            0
        );
        assert_eq!(b.config(), saved);
        assert_eq!(
            std::fs::read(&b.supervisor.paths.config_file).unwrap(),
            bytes
        );
        std::fs::write(&source, "[broken yaml").unwrap();
        assert!(b.sync_projects(vec![source]).await.is_err());
        assert_eq!(b.config(), saved);
        assert_eq!(
            std::fs::read(&b.supervisor.paths.config_file).unwrap(),
            bytes
        );
    }

    #[test]
    fn preparing_backend_preserves_registered_projects() {
        let dir = tempfile::tempdir().unwrap();
        let b = fixture(dir.path(), None);
        b.supervisor.paths.prepare_serena(false).unwrap();
        let path = b.supervisor.paths.serena_home().join("serena_config.yml");
        std::fs::write(
            &path,
            json!({"projects": ["C:/projects/example"]}).to_string(),
        )
        .unwrap();
        b.supervisor.paths.prepare_serena(true).unwrap();
        let config: serde_yaml_ng::Value =
            serde_yaml_ng::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(config["projects"][0].as_str(), Some("C:/projects/example"));
        b.supervisor.paths.verify_serena_config().unwrap();
    }
    #[tokio::test]
    async fn request_logs_include_missing_path_and_distinguish_peer_from_forwarded_headers() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let dir = tempfile::tempdir().unwrap();
        let broker = fixture(dir.path(), None);
        broker.start().await.unwrap();
        let port = broker.config().broker.port;
        let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .unwrap();
        let peer = stream.local_addr().unwrap();
        let request = format!(
            "GET /missing/resource?token=query-do-not-log HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nCF-Connecting-IP: 203.0.113.8\r\nX-Forwarded-For: 203.0.113.8, 192.0.2.1\r\nX-Forwarded-Host: serena.example.com\r\nAuthorization: Bearer do-not-log\r\nConnection: close\r\n\r\n"
        );
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut response = Vec::new();
        tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut response))
            .await
            .unwrap()
            .unwrap();
        let logs = broker.log_snapshot().join("\n");
        assert!(String::from_utf8_lossy(&response).starts_with("HTTP/1.1 404"));
        assert!(logs.contains("HTTP GET · 404 Not Found · path=\"/missing/resource\""));
        assert!(!logs.contains("token="));
        assert!(logs.contains(&format!("peer={peer}")));
        assert!(logs.contains(&format!("host=\"127.0.0.1:{port}\"")));
        assert!(logs.contains("cf-connecting-ip=\"203.0.113.8\""));
        assert!(logs.contains("x-forwarded-for=\"203.0.113.8, 192.0.2.1\""));
        assert!(logs.contains("x-forwarded-host=\"serena.example.com\""));
        assert!(!logs.contains("do-not-log"));
        broker.stop().await.unwrap();
    }
    #[tokio::test]
    async fn http_negotiates_supported_versions_with_json_without_sessions() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        async fn post(port: u16, session: Option<&str>, body: Value) -> String {
            let protocol = body["params"]["_meta"]["io.modelcontextprotocol/protocolVersion"]
                .as_str()
                .unwrap_or("2025-03-26")
                .to_owned();
            let method = body["method"].as_str().unwrap().to_owned();
            let body = body.to_string();
            let session = session
                .map(|id| format!("Mcp-Session-Id: {id}\r\n"))
                .unwrap_or_default();
            let request = format!(
                "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nMCP-Protocol-Version: {protocol}\r\nMcp-Method: {method}\r\n{session}Connection: close\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            );
            tokio::time::timeout(Duration::from_secs(5), async {
                let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
                    .await
                    .unwrap();
                stream.write_all(request.as_bytes()).await.unwrap();
                let mut response = Vec::new();
                stream.read_to_end(&mut response).await.unwrap();
                String::from_utf8(response).unwrap()
            })
            .await
            .expect("HTTP request timed out")
        }

        fn messages(response: &str) -> Vec<Value> {
            assert!(response.starts_with("HTTP/1.1 200"), "{response}");
            let (headers, body) = response.split_once("\r\n\r\n").unwrap();
            assert!(
                headers
                    .to_ascii_lowercase()
                    .contains("content-type: application/json")
            );
            assert!(!headers.to_ascii_lowercase().contains("mcp-session-id"));
            assert!(!headers.to_ascii_lowercase().contains("text/event-stream"));
            vec![serde_json::from_str(body).unwrap()]
        }

        let dir = tempfile::tempdir().unwrap();
        let broker = fixture(dir.path(), None);
        broker.start().await.unwrap();
        let port = broker.config().broker.port;
        let init = post(port, None, json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"legacy-client","version":"1"}}})).await;
        assert_eq!(
            messages(&init)[0]["result"]["protocolVersion"],
            "2025-03-26"
        );
        let notified = post(
            port,
            None,
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        )
        .await;
        assert!(notified.starts_with("HTTP/1.1 202"));
        let tools = post(
            port,
            None,
            json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}),
        )
        .await;
        // Discovery is local: stopped Serena does not remove Source Tool descriptors.
        let tool_messages = messages(&tools);
        let names = tool_messages[0]["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|tool| tool["name"].as_str())
            .collect::<std::collections::HashSet<_>>();
        for &name in registry::LOCAL_SOURCES
            .iter()
            .chain(registry::SEMANTIC_SOURCES)
        {
            assert!(names.contains(name), "{name}");
        }
        for method in ["server/discover", "tools/list"] {
            let response = post(
                port,
                None,
                json!({
                    "jsonrpc": "2.0", "id": 3, "method": method,
                    "params": {"_meta": {
                        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
                        "io.modelcontextprotocol/clientCapabilities": {}
                    }}
                }),
            )
            .await;
            assert!(response.starts_with("HTTP/1.1 400"), "{response}");
            let (headers, body) = response.split_once("\r\n\r\n").unwrap();
            assert!(
                headers
                    .to_ascii_lowercase()
                    .contains("content-type: application/json")
            );
            let response: Value = serde_json::from_str(body).unwrap();
            assert_eq!(response["error"]["code"], -32022);
            assert_eq!(
                response["error"]["data"]["supported"],
                json!(["2024-11-05", "2025-03-26", "2025-06-18", "2025-11-25"])
            );
        }
        broker.stop().await.unwrap();
    }

    #[tokio::test]
    async fn http_surface_and_no_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let broker = fixture(dir.path(), None);
        broker.start().await.unwrap();
        let transport = StreamableHttpClientTransport::from_uri(format!(
            "http://127.0.0.1:{}/mcp",
            broker.config().broker.port
        ));
        let client = ().serve(transport).await.unwrap();
        let image = client
            .call_tool(
                CallToolRequestParams::new("media_read_image")
                    .with_arguments(json!({"path":"test.png"}).as_object().unwrap().clone()),
            )
            .await
            .unwrap_err();
        assert!(image.to_string().contains("WORKSPACE_CONTEXT_REQUIRED"));
        let tools = client.list_all_tools().await.unwrap();
        assert!(
            tools.iter().any(|tool| tool.name == "codegraph_explore"),
            "CodeGraph discovery is local and must not require an active workspace"
        );
        assert!(
            client
                .call_tool(CallToolRequestParams::new("git_status"))
                .await
                .unwrap_err()
                .to_string()
                .contains("WORKSPACE_CONTEXT_REQUIRED")
        );
        let graph = client
            .call_tool(
                CallToolRequestParams::new("codegraph_explore")
                    .with_arguments(json!({"query":"x"}).as_object().unwrap().clone()),
            )
            .await
            .unwrap_err();
        assert!(graph.to_string().contains("WORKSPACE_CONTEXT_REQUIRED"));
        let guard = broker.workspace.write().await;
        let (r, ()) = tokio::join!(
            client.call_tool(CallToolRequestParams::new("workspace_deactivate")),
            async {
                tokio::time::timeout(Duration::from_secs(5), async {
                    while !broker
                        .log_snapshot()
                        .iter()
                        .any(|line| line.contains("tool=\"workspace_deactivate\""))
                    {
                        tokio::task::yield_now().await;
                    }
                })
                .await
                .unwrap();
                tokio::time::sleep(Duration::from_millis(40)).await;
                drop(guard);
            }
        );
        let r = r.unwrap();
        assert!(
            serde_json::to_value(r)
                .unwrap()
                .to_string()
                .contains("inactive")
        );
        assert!(
            client
                .call_tool(CallToolRequestParams::new("workspace_activate"))
                .await
                .is_err()
        );
        let logs = broker.log_snapshot();
        assert!(logs.iter().any(|line| line.starts_with("INFO ")
            && line.contains("request=")
            && line.contains("tool=")));
        assert!(
            !logs
                .iter()
                .any(|line| line.contains("入参=") || line.contains("\"query\":\"x\""))
        );
        for line in &logs {
            if line.contains("tools/call ") {
                assert!(line.contains("[TOOL] tools/call "), "{line}");
                assert!(!line.contains("[MCP]"), "{line}");
            } else {
                assert!(line.contains("[MCP]"), "{line}");
            }
        }
        assert!(logs.iter().any(|line| line.starts_with("WARN ") && line.contains("error_code=INVALID_PARAMS")));
        assert!(logs.iter().any(|line| line.contains("MCP 已监听")));
        assert!(logs.iter().any(|line| line.contains("HTTP POST")));
        assert!(logs.iter().any(|line| line.contains("tools/list")));
        for (tool, outcome) in [
            ("media_read_image", "error_code=INVALID_PARAMS"),
            ("git_status", "error_code=INVALID_PARAMS"),
            ("workspace_deactivate", "success=true"),
            ("workspace_activate", "error_code=INVALID_PARAMS"),
        ] {
            let line = logs
                .iter()
                .find(|line| line.contains(&format!("tool={tool:?}")) && line.contains(outcome))
                .unwrap();
            let elapsed: f64 = line.split("duration_ms=").nth(1).unwrap().parse().unwrap();
            assert!(elapsed >= 0.0);
            if tool == "workspace_deactivate" {
                // Includes time waiting for the workspace lock, not just HTTP headers.
                assert!(elapsed >= 40.0, "{line}");
            }
        }
        assert!(!logs.iter().any(|line| line.contains("响应头")));
        drop(client);
        broker.stop().await.unwrap();
        assert!(!broker.snapshot().await.running);
        assert!(broker.log_snapshot().last().unwrap().contains("MCP 已停止"));
    }

    #[tokio::test]
    async fn media_read_image_uses_only_the_request_workspace_lease() {
        let dir = tempfile::tempdir().unwrap();
        let broker = fixture(dir.path(), None);
        let root_a = dir.path().join("workspace-a");
        let root_b = dir.path().join("workspace-b");
        std::fs::create_dir(&root_a).unwrap();
        std::fs::create_dir(&root_b).unwrap();
        image::RgbImage::from_pixel(4, 4, image::Rgb([255, 0, 0]))
            .save(root_a.join("marker.png"))
            .unwrap();
        image::RgbImage::from_pixel(4, 4, image::Rgb([0, 0, 255]))
            .save(root_b.join("marker.png"))
            .unwrap();
        let registry = WorkspaceRegistry::new(&broker.supervisor);
        let workspace_a = registry
            .register(root_a.clone(), Some("Workspace A".into()))
            .unwrap();
        let workspace_b = registry
            .register(root_b.clone(), Some("Workspace B".into()))
            .unwrap();
        // Desktop selection 与 legacy ActiveWorkspace 都刻意指向 B；请求 A 仍必须只读取 A。
        broker
            .supervisor
            .select_desktop_workspace(&workspace_b.id)
            .unwrap();
        let legacy = super::orchestration_tests::active(&broker, &root_b).await;
        broker.start().await.unwrap();
        let client = ()
            .serve(StreamableHttpClientTransport::from_uri(format!(
                "http://127.0.0.1:{}/mcp",
                broker.config().broker.port
            )))
            .await
            .unwrap();
        for (workspace_id, expected) in [
            (&workspace_a.id, [255, 0, 0]),
            (&workspace_b.id, [0, 0, 255]),
        ] {
            let result = client
                .call_tool(
                    CallToolRequestParams::new("media_read_image").with_arguments(
                        json!({"workspaceId":workspace_id,"path":"marker.png"})
                            .as_object()
                            .unwrap()
                            .clone(),
                    ),
                )
                .await
                .unwrap();
            let image = media::tests::assert_image(&result, "image/png");
            assert_eq!(image.to_rgb8().get_pixel(0, 0).0, expected);
        }
        let unknown = client
            .call_tool(
                CallToolRequestParams::new("media_read_image").with_arguments(
                    json!({"workspaceId":"unknown","path":"marker.png"})
                        .as_object()
                        .unwrap()
                        .clone(),
                ),
            )
            .await
            .unwrap();
        assert_eq!(unknown.is_error, Some(true));
        assert!(
            serde_json::to_string(&unknown)
                .unwrap()
                .contains("WORKSPACE_NOT_FOUND")
        );
        client.cancel().await.unwrap();
        broker.stop().await.unwrap();
        legacy.abort();
    }

    #[tokio::test]
    async fn broker_listener_scope_changes_after_restart() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let dir = tempfile::tempdir().unwrap();
        let broker = fixture(dir.path(), None);
        let mut previous_started_at = None;
        for allow_lan in [false, true, false] {
            let mut config = broker.config();
            config.broker.allow_lan = allow_lan;
            broker.supervisor.replace_config(config).unwrap();
            let requested_at = chrono::Utc::now().timestamp_millis();
            broker.start().await.unwrap();
            let state = broker.snapshot().await;
            assert!(state.running);
            let started_at = state.started_at.expect("running listener has a start time");
            assert!(started_at >= requested_at);
            assert!(previous_started_at.is_none_or(|previous| started_at >= previous));
            assert_eq!(
                serde_json::to_value(&state).unwrap()["startedAt"],
                started_at
            );
            previous_started_at = Some(started_at);
            assert_eq!(
                state.listen_address,
                if allow_lan { "0.0.0.0" } else { "127.0.0.1" }
            );
            if !allow_lan {
                assert!(state.lan_endpoints.is_empty());
            }
            let mut socket = tokio::net::TcpStream::connect(("127.0.0.1", state.port))
                .await
                .unwrap();
            socket
                .write_all(
                    b"GET /mcp HTTP/1.1\r\nHost: untrusted.example\r\nConnection: close\r\n\r\n",
                )
                .await
                .unwrap();
            let mut response = Vec::new();
            tokio::time::timeout(Duration::from_secs(5), socket.read_to_end(&mut response))
                .await
                .unwrap()
                .unwrap();
            assert!(String::from_utf8_lossy(&response).starts_with("HTTP/1.1 403"));
            let client = ()
                .serve(StreamableHttpClientTransport::from_uri(format!(
                    "http://127.0.0.1:{}/mcp",
                    state.port
                )))
                .await
                .unwrap();
            let result = client
                .call_tool(CallToolRequestParams::new("workspace_current"))
                .await
                .unwrap();
            assert_ne!(result.is_error, Some(true));
            assert!(result.structured_content.unwrap()["activeWorkspace"].is_null());
            drop(client);
            broker.stop().await.unwrap();
            let stopped = broker.snapshot().await;
            assert!(!stopped.running);
            assert!(stopped.started_at.is_none());
        }
    }

    #[tokio::test]
    #[ignore = "requires BROKER_TEST_LAN_IP set to a local non-loopback IPv4 address"]
    async fn broker_accepts_lan_address_only_when_enabled() {
        let ip: std::net::Ipv4Addr = std::env::var("BROKER_TEST_LAN_IP")
            .unwrap()
            .parse()
            .unwrap();
        assert!(!ip.is_loopback() && !ip.is_unspecified());
        let dir = tempfile::tempdir().unwrap();
        let broker = fixture(dir.path(), None);
        for allow_lan in [false, true, false] {
            let mut config = broker.config();
            config.broker.allow_lan = allow_lan;
            broker.supervisor.replace_config(config).unwrap();
            broker.start().await.unwrap();
            let port = broker.snapshot().await.port;
            if allow_lan {
                assert!(
                    broker
                        .snapshot()
                        .await
                        .lan_endpoints
                        .contains(&format!("http://{ip}:{port}/mcp"))
                );
                let client = tokio::time::timeout(
                    Duration::from_secs(10),
                    ().serve(StreamableHttpClientTransport::from_uri(format!(
                        "http://{ip}:{port}/mcp"
                    ))),
                )
                .await
                .unwrap()
                .unwrap();
                let result = client
                    .call_tool(CallToolRequestParams::new("workspace_current"))
                    .await
                    .unwrap();
                assert_ne!(result.is_error, Some(true));
                drop(client);
            } else {
                let connection = tokio::time::timeout(
                    Duration::from_secs(2),
                    tokio::net::TcpStream::connect((ip, port)),
                )
                .await;
                assert!(!matches!(connection, Ok(Ok(_))));
            }
            broker.stop().await.unwrap();
        }
    }
    #[tokio::test]
    #[ignore = "requires SERENA_TEST_EXE pointing at the official 1.7.0 test installation"]
    async fn official_serena_agent_schema_round_trip() {
        let exe = PathBuf::from(std::env::var_os("SERENA_TEST_EXE").expect("set SERENA_TEST_EXE"));
        let dir = tempfile::tempdir().unwrap();
        let broker = fixture(dir.path(), Some(exe));
        let store = crate::agent::store::StateStore::open(dir.path().join("agent"))
            .await
            .unwrap();
        assert!(
            broker
                .product
                .set(Arc::new(crate::agent::product::AgentProductService::new(
                    store.clone()
                )))
                .is_ok()
        );
        store
            .create_work_run(
                "contract-work".into(),
                "W".into(),
                dir.path().to_string_lossy().into(),
                1,
                "title".into(),
                None,
                1,
            )
            .await
            .unwrap();
        let supervisor = broker.supervisor.clone();
        tauri::async_runtime::spawn_blocking(move || supervisor.start())
            .await
            .unwrap()
            .unwrap();
        broker.start().await.unwrap();
        let result = tokio::time::timeout(Duration::from_secs(60), async {
            let mut disabled_tools = Vec::new();
            for enabled in [false, true] {
                let mut config = broker.config();
                config.agent_enabled = enabled;
                broker.supervisor.replace_config(config).unwrap();
                // Each case negotiates initialize through the real HTTP transport.
                let client = ()
                    .serve(StreamableHttpClientTransport::from_uri(format!(
                        "http://127.0.0.1:{}/mcp",
                        broker.config().broker.port
                    )))
                    .await
                    .unwrap();
                let page = client.list_tools(None).await.unwrap();
                assert!(page.next_cursor.is_none());
                if let Some(path) = std::env::var_os("SERENA_AGENT_CONTRACT_EVIDENCE") {
                    let path = PathBuf::from(path);
                    std::fs::create_dir_all(&path).unwrap();
                    std::fs::write(path.join(format!("tools-list-agent-{enabled}.json")), serde_json::to_vec_pretty(&page).unwrap()).unwrap();
                }
                let tools = page.tools;
                if enabled {
                    assert_eq!(tools.len(), disabled_tools.len() + 4);
                    assert_eq!(tools.iter().filter(|t| orchestration::contains(&t.name)).count(), 4);
                    assert_eq!(
                        tools
                            .iter()
                            .map(|t| &t.name)
                            .collect::<std::collections::HashSet<_>>()
                            .len(),
                        tools.len()
                    );
                    let agent = tools.iter().find(|t| t.name == "agent_execute").unwrap();
                    assert_eq!(agent, &orchestration::descriptors().pop().unwrap());
                    assert_eq!(json!(agent.annotations), json!({"readOnlyHint":false,"destructiveHint":true,"idempotentHint":false,"openWorldHint":true}));
                    let hash = registry::tool_contract_hash(agent);
                    let logs = broker.log_snapshot().join("\n");
                    assert!(logs.contains(&format!("agent_execute={hash}")));
                    assert!(logs.contains("orchestration contracts sha256"));
                    assert!(logs.contains("agentEnabled=true orchestration"));
                    if let Some(path) = std::env::var_os("SERENA_AGENT_CONTRACT_EVIDENCE") {
                        let path = PathBuf::from(path);
                        std::fs::write(path.join("agent-contract-sha256.txt"), &hash).unwrap();
                        std::fs::write(path.join("broker-contract.log"), logs).unwrap();
                    }
                    assert_eq!(agent.input_schema["type"], "object");
                    assert!(agent.output_schema.is_some());
                    assert!(
                        serde_json::to_value(agent)
                            .unwrap()
                            .get("outputSchema")
                            .is_some()
                    );
                    assert_eq!(
                        tools
                            .into_iter()
                            .filter(|t| !orchestration::contains(&t.name))
                            .collect::<Vec<_>>(),
                        disabled_tools
                    );
                } else {
                    assert!(!tools.iter().any(|t| t.name == "agent_execute"));
                    disabled_tools = tools;
                }
                for (args, error) in [
                    (json!({"action":"list","workRunId":"contract-work"}), None),
                    (json!({"action":"start"}), Some("WORK_INVALID_ARGUMENT")),
                    (
                        json!({"action":"list","limit":101}),
                        Some("WORK_INVALID_ARGUMENT"),
                    ),
                    (
                        json!({"action":"list","unexpected":true}),
                        Some("WORK_INVALID_ARGUMENT"),
                    ),
                ] {
                    let result = client
                        .call_tool(
                            CallToolRequestParams::new("agent_query")
                                .with_arguments(args.as_object().unwrap().clone()),
                        )
                        .await
                        .unwrap();
                    let envelope = result.structured_content.as_ref().unwrap();
                    let rmcp::model::ContentBlock::Text(text) = &result.content[0] else {
                        panic!("expected text JSON")
                    };
                    assert_eq!(
                        serde_json::from_str::<Value>(&text.text).unwrap(),
                        *envelope
                    );
                    let expected_error = if enabled {
                        error
                    } else {
                        Some("AGENT_DISABLED")
                    };
                    assert_eq!(envelope.as_object().unwrap().len(), 2);
                    assert!(envelope.get("control").is_none());
                    if let Some(code) = expected_error {
                        assert_eq!(result.is_error, Some(true));
                        assert_eq!(envelope["ok"], false);
                        assert_eq!(envelope["error"]["code"], code);
                    } else {
                        assert_ne!(result.is_error, Some(true));
                        assert_eq!(envelope, &json!({"ok":true,"data":{"executions":[]}}));
                    }
                }
                if enabled {
                    store.product_create_fresh("contract-e".into(), "contract-a".into(), "key".into(), "contract-fixture".into(), (Some(crate::agent::store::transactions::product::WorkspaceSnapshot { id:"W".into(), root:dir.path().to_string_lossy().into(), generation:1 })).as_ref().unwrap().id.clone(), Some(crate::agent::store::transactions::product::WorkspaceSnapshot { id:"W".into(), root:dir.path().to_string_lossy().into(), generation:1 }), 1).await.unwrap();
                    let observed = client.call_tool(CallToolRequestParams::new("agent_query").with_arguments(json!({"action":"observe","executionId":"contract-e","waitMs":0}).as_object().unwrap().clone())).await.unwrap();
                    let envelope = observed.structured_content.as_ref().unwrap();
                    assert_eq!(envelope["ok"], true);
                    assert_eq!(envelope.as_object().unwrap().len(), 2);
                    assert!(envelope.get("control").is_none());
                    assert_eq!(envelope["data"]["status"], "dispatch_pending");
                    assert_eq!(envelope["data"]["unchanged"], false);
                    assert_eq!(envelope["data"]["nextAction"]["action"], "resume_pending");
                    if let Some(path) = std::env::var_os("SERENA_AGENT_CONTRACT_EVIDENCE") {
                        std::fs::write(PathBuf::from(path).join("observe-control.json"), serde_json::to_vec_pretty(&observed).unwrap()).unwrap();
                    }
                }
                client.cancel().await.unwrap();
            }
        })
        .await;
        broker.stop().await.unwrap();
        let supervisor = broker.supervisor.clone();
        tauri::async_runtime::spawn_blocking(move || supervisor.stop())
            .await
            .unwrap()
            .unwrap();
        result.expect("Agent MCP round-trip timed out");
    }

    #[tokio::test]
    async fn agent_observe_http_disconnect_reconnect_and_repeat_result() {
        let dir = tempfile::tempdir().unwrap();
        let broker = fixture(dir.path(), None);
        let store = crate::agent::store::StateStore::open(dir.path().join("agent"))
            .await
            .unwrap();
        let root = dir.path().to_string_lossy().into_owned();
        store
            .product_create_fresh(
                "observe-e".into(),
                "observe-a".into(),
                "key".into(),
                "prompt".into(),
                (Some(
                    crate::agent::store::transactions::product::WorkspaceSnapshot {
                        id: "W".into(),
                        root: root.clone(),
                        generation: 1,
                    },
                ))
                .as_ref()
                .unwrap()
                .id
                .clone(),
                Some(
                    crate::agent::store::transactions::product::WorkspaceSnapshot {
                        id: "W".into(),
                        root: root.clone(),
                        generation: 1,
                    },
                ),
                1,
            )
            .await
            .unwrap();
        assert!(
            broker
                .product
                .set(Arc::new(crate::agent::product::AgentProductService::new(
                    store.clone()
                )))
                .is_ok()
        );
        let mut config = broker.config();
        config.agent_enabled = true;
        broker.supervisor.replace_config(config).unwrap();
        broker.start().await.unwrap();
        let test = tokio::time::timeout(Duration::from_secs(15), async {
            let uri = format!("http://127.0.0.1:{}/mcp", broker.config().broker.port);
            let client = ().serve(StreamableHttpClientTransport::from_uri(uri.clone())).await.unwrap();
            let secret = "AGENT_HTTP_PROMPT_SECRET_59317";
            let invalid = client.call_tool(CallToolRequestParams::new("agent_query").with_arguments(json!({"action":"start","prompt":secret}).as_object().unwrap().clone())).await.unwrap();
            assert_eq!(invalid.structured_content.unwrap()["error"]["code"], "WORK_INVALID_ARGUMENT");
            let log = broker.log_snapshot().join("\n");
            assert!(!log.contains(secret));
            assert!(log.contains("tool=\"agent_query\""));
            assert!(!log.contains("promptBytes"));
            let before = store.execution("observe-e".into()).await.unwrap().unwrap();
            let args = json!({"action":"observe","executionId":"observe-e","waitMs":500});
            assert!(tokio::time::timeout(Duration::from_millis(60), client.call_tool(CallToolRequestParams::new("agent_query").with_arguments(args.as_object().unwrap().clone()))).await.is_err());
            client.cancel().await.unwrap();
            assert_eq!(store.execution("observe-e".into()).await.unwrap().unwrap(), before);
            assert!(store.workspace_claim(root.clone()).await.unwrap().is_some());

            store.request_cancel("observe-e".into(), 2).await.unwrap();
            let result = json!({"executionId":"observe-e","finalResult":[{"type":"agentMessage","phase":"final_answer","text":"Reconnect 原文\n <literal>"}]});
            let db = rusqlite::Connection::open(dir.path().join("agent/agent-state.db")).unwrap();
            db.execute("UPDATE executions SET final_result_json=?1 WHERE id='observe-e'", [result.to_string()]).unwrap();
            let finished = store.execution("observe-e".into()).await.unwrap().unwrap();
            let mut revision = Value::Null;
            // Both sessions perform initialize and read the same persisted terminal result.
            for _ in 0..2 {
                let client = ().serve(StreamableHttpClientTransport::from_uri(uri.clone())).await.unwrap();
                for include in [false, true, true] {
                    let args = json!({"action":"observe","executionId":"observe-e","knownRevision":revision,"includeResult":include,"waitMs":20000});
                    let response = client.call_tool(CallToolRequestParams::new("agent_query").with_arguments(args.as_object().unwrap().clone())).await.unwrap();
                    assert_ne!(response.is_error, Some(true));
                    let envelope = response.structured_content.unwrap();
                    let rmcp::model::ContentBlock::Text(text) = &response.content[0] else { panic!("text JSON required") };
                    assert_eq!(serde_json::from_str::<Value>(&text.text).unwrap(), envelope);
                    assert_eq!(envelope["ok"], true);
                    assert_eq!(envelope.as_object().unwrap().len(), 2);
                    assert!(envelope.get("control").is_none());
                    assert_eq!(envelope["data"]["resultAvailable"], true);
                    if revision.is_string() { assert_eq!(envelope["data"]["revision"], revision); assert_eq!(envelope["data"]["unchanged"], true); }
                    revision = envelope["data"]["revision"].clone();
                    if include { assert_eq!(envelope["data"]["finalResult"], result); }
                    else { assert!(envelope["data"].get("finalResult").is_none()); }
                    if include {
                        assert!(envelope["data"].get("nextAction").is_none());
                    } else {
                        assert_eq!(envelope["data"]["nextAction"]["action"], "review_result");
                    }
                }
                let invalid = client.call_tool(CallToolRequestParams::new("agent_query").with_arguments(json!({"action":"observe","executionId":"observe-e","waitMs":20001}).as_object().unwrap().clone())).await.unwrap();
                assert_eq!(
                    invalid.structured_content.unwrap()["error"]["code"],
                    "AGENT_OBSERVE_INVALID_ARGUMENT"
                );
                client.cancel().await.unwrap();
            }
            assert_eq!(store.execution("observe-e".into()).await.unwrap().unwrap(), finished);
            assert!(finished.runtime_instance_id.is_none());
            assert!(finished.turn_id.is_none());
            assert!(store.workspace_claim(root).await.unwrap().is_none());
            assert_eq!(store.product_read(None, None, None, 20).await.unwrap().len(), 1);
        }).await;
        broker.stop().await.unwrap();
        test.expect("Observe HTTP round-trip timed out");
    }

    #[tokio::test]
    #[ignore = "requires SERENA_TEST_EXE pointing at the official 1.7.0 test installation"]
    async fn official_serena_lifecycle() {
        let exe = PathBuf::from(std::env::var_os("SERENA_TEST_EXE").expect("set SERENA_TEST_EXE"));
        let dir = tempfile::tempdir().unwrap();
        let b = fixture(dir.path(), Some(exe));
        for name in ["one", "two"] {
            let root = dir.path().join(name);
            std::fs::create_dir(&root).unwrap();
            let mut cmd = process::command("git");
            cmd.arg("init").arg(&root);
            process::run(cmd, 8192, Duration::from_secs(10), CancellationToken::new())
                .await
                .unwrap();
            std::fs::write(
                root.join("example.py"),
                format!("def {name}():\n    return 1\n"),
            )
            .unwrap();
            b.supervisor.paths.prepare_serena(false).unwrap();
            let mut cmd = process::command(b.config().serena_path.unwrap());
            cmd.args(["project", "create"])
                .arg(&root)
                .args(["--language", "python"])
                .env("SERENA_HOME", b.supervisor.paths.serena_home());
            process::run(cmd, 8192, Duration::from_secs(60), CancellationToken::new())
                .await
                .unwrap();
        }
        b.supervisor.detect_serena();
        b.sync_projects(vec![
            b.supervisor.paths.serena_home().join("serena_config.yml"),
        ])
        .await
        .unwrap();
        assert_eq!(
            b.config().workspaces.len(),
            2,
            "{:?}",
            b.sync_warnings.lock().unwrap()
        );
        let project1 = dir.path().join("one/.serena/project.yml");
        let mut cfg: serde_yaml_ng::Value =
            serde_yaml_ng::from_str(&std::fs::read_to_string(&project1).unwrap()).unwrap();
        cfg["activation_command"] =
            serde_yaml_ng::Value::String("cmd /c echo unsafe>activation-marker.txt".into());
        std::fs::write(&project1, serde_yaml_ng::to_string(&cfg).unwrap()).unwrap();
        let supervisor = b.supervisor.clone();
        tauri::async_runtime::spawn_blocking(move || supervisor.start())
            .await
            .unwrap()
            .unwrap();
        let result = async {
            b.start().await?;
            let discovery = ()
                .serve(StreamableHttpClientTransport::from_uri(format!(
                    "http://127.0.0.1:{}/mcp",
                    b.config().broker.port
                )))
                .await
                .map_err(|e| e.to_string())?;
            let tools = discovery
                .list_all_tools()
                .await
                .map_err(|e| e.to_string())?;
            assert_eq!(tools.len(), if b.config().agent_enabled { 21 } else { 17 });
            assert!(!tools.iter().any(|tool| tool.name == "agent"));
            assert_eq!(
                tools.iter().filter(|tool| orchestration::contains(&tool.name)).count(),
                if b.config().agent_enabled { 4 } else { 0 }
            );
            assert!(b.snapshot().await.active_workspace.is_none());
            let activated = b.activate("project-1", CancellationToken::new()).await?;
            // Real local HTTP Broker -> rmcp -> native ImageContent POC.
            // The downstream Serena only establishes the normal active binding;
            // no downstream or test double manufactures an image Tool Result.
            for (file, format, mime) in [
                ("image-poc.png", image::ImageFormat::Png, "image/png"),
                ("image-poc.jpg", image::ImageFormat::Jpeg, "image/jpeg"),
                ("image-poc.webp", image::ImageFormat::WebP, "image/png"),
            ] {
                media::tests::poc_image().save_with_format(dir.path().join("one").join(file), format).unwrap();
                let result = discovery.call_tool(CallToolRequestParams::new("media_read_image").with_arguments(json!({"workspaceId":"project-1","path":file}).as_object().unwrap().clone())).await.map_err(|e| e.to_string())?;
                let decoded = media::tests::assert_image(&result, mime);
                assert_eq!((decoded.width(), decoded.height()), (640, 320));
                let pixel = decoded.to_rgb8().get_pixel(320, 230).0;
                assert!(pixel[0] > 240 && pixel[1] < 15 && pixel[2] < 15);
                println!("MCP ImageContent POC: {file}, {mime}, 640x320; SERENA IMAGE TEST / 9274 / red circle");
            }
            for (path, code) in [("../outside.png", "INVALID_PATH"), ("missing.png", "INVALID_PATH"), ("example.py", "UNSUPPORTED_MEDIA_TYPE")] {
                let result = discovery.call_tool(CallToolRequestParams::new("media_read_image").with_arguments(json!({"workspaceId":"project-1","path":path}).as_object().unwrap().clone())).await.map_err(|e| e.to_string())?;
                assert_eq!(result.is_error, Some(true));
                assert!(serde_json::to_string(&result).unwrap().contains(code));
            }
            assert!(activated["codegraph"].is_null());
            assert!(b.snapshot().await.codegraph.is_none());
            assert_eq!(
                b.dispatch(
                    "codegraph_explore",
                    json!({"query":"one"}),
                    CancellationToken::new()
                )
                .await,
                Err("WORKSPACE_CONTEXT_REQUIRED".into())
            );
            assert!(
                b.activate("missing", CancellationToken::new())
                    .await
                    .is_err()
            );
            assert_eq!(b.snapshot().await.active_workspace.unwrap().id, "project-1");
            assert_eq!(
                b.dispatch(
                    "codegraph_explore",
                    json!({"query":"one"}),
                    CancellationToken::new(),
                )
                .await,
                Err("WORKSPACE_CONTEXT_REQUIRED".into())
            );
            assert_eq!(
                b.dispatch(
                    "git_status",
                    json!({"workspaceId":"project-1"}),
                    CancellationToken::new()
                )
                    .await?["workspace"]["id"],
                "project-1"
            );
            let ui = b.snapshot().await;
            assert!(ui.codegraph.is_none());
            assert_eq!(ui.active_workspace.unwrap().id, "project-1");
            // A UI refresh during a write must return promptly without exposing another binding.
            {
                let _transition = b.workspace.write().await;
                let ui = tokio::time::timeout(Duration::from_millis(100), b.snapshot())
                    .await
                    .unwrap();
                assert!(ui.codegraph.is_none());
            }
            let crashed = discovery
                .call_tool(
                    CallToolRequestParams::new("codegraph_explore")
                        .with_arguments(json!({"query":"crash"}).as_object().unwrap().clone()),
                )
                .await
                .map_err(|e| e.to_string())?;
            assert_eq!(crashed.is_error, Some(true));
            assert!(
                serde_json::to_string(&crashed)
                    .unwrap()
                    .contains("WORKSPACE_CONTEXT_REQUIRED")
            );
            let current = discovery
                .call_tool(CallToolRequestParams::new("workspace_current"))
                .await
                .map_err(|e| e.to_string())?;
            let current = current.structured_content.unwrap();
            assert_eq!(current["activeWorkspace"]["id"], "project-1");
            assert!(current["codegraph"].is_null());
            assert!(b.snapshot().await.codegraph.is_none());
            assert!(b.snapshot().await.running);
            assert_eq!(
                b.dispatch("git_status", json!({}), CancellationToken::new())
                    .await?["workspace"]["id"],
                "project-1"
            );
            drop(discovery);
            let output = b
                .dispatch(
                    "source_read_file",
                    json!({"workspaceId":"project-1","relative_path":"example.py"}),
                    CancellationToken::new(),
                )
                .await?;
            assert!(output["text"].as_str().unwrap().contains("def one"));
            for (name, args) in [
                (
                    "source_list_dir",
                    json!({"workspaceId":"project-1","relative_path":""}),
                ),
                (
                    "source_find_file",
                    json!({"workspaceId":"project-1","file_mask":"*.py"}),
                ),
                (
                    "source_search_pattern",
                    json!({"workspaceId":"project-1","substring_pattern":"def one"}),
                ),
            ] {
                let result = b
                    .dispatch(name, args, CancellationToken::new())
                    .await
                    .map_err(|e| format!("{name}: {e}"))?;
                assert_eq!(result["workspace"]["id"], "project-1");
                assert_eq!(result["truncated"], false);
            }
            for (name, args) in [
                (
                    "source_read_file",
                    json!({"workspaceId":"project-1","relative_path":"../outside"}),
                ),
                (
                    "source_read_file",
                    json!({"workspaceId":"project-1","relative_path":"missing.py"}),
                ),
            ] {
                assert!(
                    b.dispatch(name, args, CancellationToken::new())
                        .await
                        .is_err()
                );
            }
            let truncated = b
                .dispatch(
                    "source_read_file",
                    json!({"workspaceId":"project-1","relative_path":"example.py", "max_bytes":1}),
                    CancellationToken::new(),
                )
                .await?;
            assert_eq!(truncated["truncated"], true);
            assert!(truncated["text"].as_str().unwrap().len() <= 1);
            // Invalid configuration is rejected without changing A during validation.
            let config2 = dir.path().join("two/.serena/project.yml");
            let original = std::fs::read(&config2).unwrap();
            std::fs::write(&config2, "[bad yaml").unwrap();
            assert!(
                b.validate_project(&b.config().workspaces[1], CancellationToken::new())
                    .await
                    .is_err()
            );
            assert_eq!(b.snapshot().await.active_workspace.unwrap().id, "project-1");
            std::fs::write(
                &config2,
                "project_name: two\nlanguage_servers: [invalid-language]\n",
            )
            .unwrap();
            assert!(
                b.validate_project(&b.config().workspaces[1], CancellationToken::new())
                    .await
                    .is_err()
            );
            assert!(
                !b.snapshot()
                    .await
                    .projects
                    .iter()
                    .find(|p| p.workspace.id == "project-2")
                    .unwrap()
                    .configured
            );
            // Official loader supports the legacy singular language field and default project name.
            std::fs::write(&config2, "language: python\n").unwrap();
            b.validate_project(&b.config().workspaces[1], CancellationToken::new())
                .await?;
            std::fs::write(&config2, original).unwrap();
            // The exclusive transition waits until an in-flight read completes.
            let reading = b.workspace.read().await;
            assert!(
                tokio::time::timeout(
                    Duration::from_millis(20),
                    b.activate("project-2", CancellationToken::new())
                )
                .await
                .is_err()
            );
            assert_eq!(reading.as_ref().unwrap().workspace.id, "project-1");
            drop(reading);
            let reading = b.workspace.read().await;
            b.sync_projects(vec![
                b.supervisor.paths.serena_home().join("serena_config.yml"),
            ])
            .await?;
            assert_eq!(b.snapshot().await.active_workspace.unwrap().id, "project-1");
            drop(reading);
            b.start().await?;
            let uri = format!("http://127.0.0.1:{}/mcp", b.config().broker.port);
            let client1 =
                ().serve(StreamableHttpClientTransport::from_uri(uri.clone()))
                    .await
                    .map_err(|e| e.to_string())?;
            let client2 =
                ().serve(StreamableHttpClientTransport::from_uri(uri))
                    .await
                    .map_err(|e| e.to_string())?;
            let advertised = client1.list_all_tools().await.map_err(|e| e.to_string())?;
            assert_eq!(advertised.len(), 19); // Agent is opt-in.
            for &(public_name, _, _) in registry::SOURCES {
                let description = advertised
                    .iter()
                    .find(|tool| tool.name == public_name)
                    .and_then(|tool| tool.description.as_deref())
                    .unwrap();
                assert!(description.contains("【做什么】"), "{public_name}");
                assert!(description.contains("【关键约束】"), "{public_name}");
            }
            assert_eq!(b.snapshot().await.active_workspace.unwrap().id, "project-1");
            let read_guard = b.workspace.read().await;
            let request = client1
                .send_cancellable_request(
                    rmcp::model::ClientRequest::CallToolRequest(rmcp::model::CallToolRequest::new(
                        CallToolRequestParams::new("workspace_activate")
                            .with_arguments(json!({"id":"project-2"}).as_object().unwrap().clone()),
                    )),
                    rmcp::service::PeerRequestOptions::no_options(),
                )
                .await
                .map_err(|e| e.to_string())?;
            request
                .cancel(Some("test queued cancellation".into()))
                .await
                .map_err(|e| e.to_string())?;
            // Wait for server-side handling of the cancellation while the read still owns A.
            tokio::time::sleep(Duration::from_millis(100)).await;
            drop(read_guard);
            let current = client2
                .call_tool(CallToolRequestParams::new("workspace_current"))
                .await
                .map_err(|e| e.to_string())?;
            assert_eq!(
                current.structured_content.unwrap()["activeWorkspace"]["id"],
                "project-1"
            );
            let switched = client1
                .call_tool(
                    CallToolRequestParams::new("workspace_activate")
                        .with_arguments(json!({"id":"project-2"}).as_object().unwrap().clone()),
                )
                .await
                .map_err(|e| e.to_string())?;
            assert_ne!(switched.is_error, Some(true));
            let output = client2
                .call_tool(
                    CallToolRequestParams::new("source_read_file").with_arguments(
                        json!({"workspaceId":"project-2","relative_path":"example.py"})
                            .as_object()
                            .unwrap()
                            .clone(),
                    ),
                )
                .await
                .map_err(|e| e.to_string())?;
            assert!(
                output.structured_content.unwrap()["text"]
                    .as_str()
                    .unwrap()
                    .contains("def two")
            );
            drop(client1);
            drop(client2);
            b.stop().await?;
            b.activate("project-2", CancellationToken::new()).await?;
            let output = b
                .dispatch(
                    "source_read_file",
                    json!({"workspaceId":"project-2","relative_path":"example.py"}),
                    CancellationToken::new(),
                )
                .await?;
            assert!(output["text"].as_str().unwrap().contains("def two"));
            // Preferences save without stopping the running PID.
            // Restart then reactivates the same workspace using the new process.
            for dashboard in [true, false] {
                let (before_workspace, before_pid) = {
                    let slot = b.workspace.read().await;
                    let active = slot.as_ref().unwrap();
                    (active.workspace.clone(), active.pid)
                };
                let mut next = b.config();
                next.dashboard_enabled = dashboard;
                next.open_dashboard_on_launch = false;
                crate::commands::save_config_impl(&b, next).await?;
                assert_eq!(b.supervisor.snapshot().process_id, Some(before_pid));
                assert_eq!(b.supervisor.snapshot().server_status, ServerStatus::Running);
                assert_eq!(b.supervisor.snapshot().active_dashboard_enabled, !dashboard);
                crate::commands::restart_serena_impl(&b).await?;
                let slot = b.workspace.read().await;
                let restored = slot.as_ref().unwrap();
                assert_eq!(restored.workspace, before_workspace);
                assert_ne!(restored.pid, before_pid);
                assert_eq!(b.supervisor.snapshot().active_dashboard_enabled, dashboard);
                drop(slot);
                let output = b
                    .dispatch(
                        "source_read_file",
                        json!({"workspaceId":"project-2","relative_path":"example.py"}),
                        CancellationToken::new(),
                    )
                    .await?;
                assert!(output["text"].as_str().unwrap().contains("def two"));
                assert_eq!(
                    b.dispatch("git_status", json!({}), CancellationToken::new())
                        .await?["workspace"]["id"],
                    "project-2"
                );
            }
            let supervisor = b.supervisor.clone();
            tauri::async_runtime::spawn_blocking(move || supervisor.stop())
                .await
                .unwrap()?;
            assert_eq!(
                b.dispatch(
                    "codegraph_explore",
                    json!({"query":"x"}),
                    CancellationToken::new()
                )
                .await,
                Err("WORKSPACE_CONTEXT_REQUIRED".into())
            );
            assert_eq!(
                b.dispatch(
                    "source_read_file",
                    json!({"relative_path":"example.py"}),
                    CancellationToken::new()
                )
                .await
                .unwrap_err(),
                "WORKSPACE_CONTEXT_REQUIRED"
            );
            assert!(b.workspace.read().await.is_none());
            assert!(b.published.lock().unwrap().is_none());
            assert!(b.snapshot().await.codegraph.is_none());
            b.deactivate().await?;
            assert_eq!(
                b.dispatch(
                    "codegraph_explore",
                    json!({"query":"one"}),
                    CancellationToken::new()
                )
                .await,
                Err("WORKSPACE_CONTEXT_REQUIRED".into())
            );
            assert!(
                b.dispatch("git_status", json!({}), CancellationToken::new())
                    .await
                    .unwrap_err()
                    .contains("WORKSPACE_CONTEXT_REQUIRED")
            );
            assert!(
                b.activate("missing", CancellationToken::new())
                    .await
                    .is_err()
            );
            assert!(b.snapshot().await.active_workspace.is_none());
            // A restart without an active workspace must not resurrect a deactivated project.
            crate::commands::restart_serena_impl(&b).await?;
            assert!(b.snapshot().await.active_workspace.is_none());
            b.activate("project-2", CancellationToken::new()).await?;
            std::fs::remove_file(dir.path().join("two/.serena/project.yml")).unwrap();
            let error = crate::commands::restart_serena_impl(&b).await.unwrap_err();
            assert!(error.contains("Serena 已重启，但恢复项目"), "{error}");
            assert_eq!(b.supervisor.snapshot().server_status, ServerStatus::Running);
            assert!(b.workspace.read().await.is_none());
            assert!(b.snapshot().await.codegraph.is_none());
            Ok::<_, String>(())
        }
        .await;
        b.deactivate().await.unwrap();
        let supervisor = b.supervisor.clone();
        tauri::async_runtime::spawn_blocking(move || supervisor.stop())
            .await
            .unwrap()
            .unwrap();
        if result.is_err() {
            eprintln!(
                "{}",
                std::fs::read_to_string(&b.supervisor.paths.serena_log).unwrap_or_default()
            );
        }
        assert!(!dir.path().join("one/activation-marker.txt").exists());
        result.unwrap();
    }

    #[tokio::test]
    /// P2A2-013：请求级 WorkspaceLease 必须在 A/B 并发、兼容后端和无会话重连下保持唯一 Authority。
    async fn p2a2_013_request_ab_isolation_backend_continuity_and_sessionless_gate() {
        let directory = tempfile::tempdir().unwrap();
        let provider = Arc::new(SemanticRoutingProvider::new());
        let (broker, _manager) = semantic_fixture(directory.path(), Arc::clone(&provider));
        let root_a = directory.path().join("workspace-a");
        let root_b = directory.path().join("workspace-b");

        // 两个根目录保留同名 Source 文件和不同 Marker，并各自初始化为独立 Git 仓库。
        for (root, marker) in [(&root_a, "A"), (&root_b, "B")] {
            std::fs::create_dir_all(root.join("src")).unwrap();
            std::fs::write(root.join("src/marker.rs"), format!("// SOURCE-{marker}\n")).unwrap();
            std::fs::write(root.join("git-marker.txt"), format!("GIT-{marker}\n")).unwrap();
            image::RgbImage::from_pixel(
                4,
                4,
                if marker == "A" {
                    image::Rgb([255, 0, 0])
                } else {
                    image::Rgb([0, 0, 255])
                },
            )
            .save(root.join("marker.png"))
            .unwrap();
            let mut init = process::command("git");
            init.arg("init").arg(root);
            process::run(
                init,
                8192,
                Duration::from_secs(10),
                CancellationToken::new(),
            )
            .await
            .unwrap();
            for args in [
                vec!["add", "."],
                vec![
                    "-c",
                    "user.name=P2A2-013",
                    "-c",
                    "user.email=p2a2-013@example.invalid",
                    "commit",
                    "-m",
                    "fixture",
                    "--no-gpg-sign",
                ],
            ] {
                let mut commit = process::command("git");
                commit.arg("-C").arg(root).args(args);
                process::run(
                    commit,
                    8192,
                    Duration::from_secs(10),
                    CancellationToken::new(),
                )
                .await
                .unwrap();
            }
            // 让 status/diff 的内容也能识别各自 canonical root。
            std::fs::write(
                root.join("git-marker.txt"),
                format!("GIT-{marker}-changed\n"),
            )
            .unwrap();
            std::fs::write(root.join(format!("status-marker-{marker}.txt")), marker).unwrap();
        }

        let registry = WorkspaceRegistry::new(&broker.supervisor);
        let workspace_a = registry
            .register(root_a.clone(), Some("Workspace A".into()))
            .unwrap();
        let workspace_b = registry
            .register(root_b.clone(), Some("Workspace B".into()))
            .unwrap();
        broker
            .supervisor
            .select_desktop_workspace(&workspace_b.id)
            .unwrap();

        // legacy ActiveWorkspace 与 Desktop selection 都指向 B，不能改变请求 A 的 Lease。
        let legacy = super::orchestration_tests::active(&broker, &root_b).await;
        {
            let mut active = broker.workspace.write().await;
            let active = active.as_mut().unwrap();
            active.workspace.id = workspace_b.id.clone();
            active.workspace.generation = workspace_b.generation;
        }

        // Work begin 同样必须冻结请求 A 的 Lease，而非从 legacy active 或 Desktop selection 推导。
        let store = crate::agent::store::StateStore::open(directory.path().join("state"))
            .await
            .unwrap();
        assert!(
            broker
                .product
                .set(Arc::new(crate::agent::product::AgentProductService::new(
                    store.clone(),
                )))
                .is_ok()
        );
        let mut config = broker.config();
        config.agent_enabled = true;
        broker.supervisor.replace_config(config).unwrap();
        let work = broker
            .dispatch(
                "work_update",
                json!({"action":"begin","workspaceId":workspace_a.id,"title":"A only"}),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        let work_run_id = work["data"]["workRun"]["workRunId"]
            .as_str()
            .unwrap()
            .to_owned();
        let work = store.work_run(work_run_id).await.unwrap().unwrap();
        assert_eq!(work.workspace_id, workspace_a.id);
        assert_eq!(
            work.canonical_workspace_root,
            workspace_a.root.to_string_lossy()
        );
        assert_eq!(work.workspace_generation, workspace_a.generation);

        // tools/list 的公开表不依赖 Serena，CodeGraph 只公开新的 WorkspaceLease Adapter route。
        let advertised = registry::list(false);
        for name in registry::LOCAL_SOURCES
            .iter()
            .chain(registry::SEMANTIC_SOURCES)
            .chain(registry::GITS)
            .chain(["codegraph_explore"].iter())
            .chain(["media_read_image"].iter())
        {
            assert!(advertised.iter().any(|tool| tool.name == *name), "{name}");
        }

        // Discovery 是 catalog-only；已知 ID 仍可直接复用，但遗漏 ID 不得绑定到 A/B 任一方。
        assert_eq!(
            broker
                .dispatch("workspace_list", json!({}), CancellationToken::new())
                .await
                .unwrap()["workspaces"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            broker
                .dispatch(
                    "workspace_get",
                    json!({"workspaceId":workspace_a.id}),
                    CancellationToken::new(),
                )
                .await
                .unwrap()["workspace"]["id"],
            workspace_a.id
        );
        for (name, args) in [
            ("source_read_file", json!({"relative_path":"src/marker.rs"})),
            ("git_status", json!({})),
        ] {
            assert_eq!(
                broker.dispatch(name, args, CancellationToken::new()).await,
                Err("WORKSPACE_CONTEXT_REQUIRED".into()),
                "{name}"
            );
        }
        assert_eq!(
            broker
                .read_image(json!({"path":"marker.png"}), CancellationToken::new(),)
                .await
                .unwrap_err(),
            "WORKSPACE_CONTEXT_REQUIRED"
        );

        // 缺失、类型/空白错误及有效未知 ID 必须在三类公开后端保持稳定分类。
        for (name, valid) in [
            ("source_read_file", json!({"relative_path":"src/marker.rs"})),
            ("git_status", json!({})),
        ] {
            for workspace_id in [json!(7), json!(" \t")] {
                let mut invalid = valid.as_object().unwrap().clone();
                invalid.insert("workspaceId".into(), workspace_id);
                assert!(
                    broker
                        .dispatch(name, Value::Object(invalid), CancellationToken::new())
                        .await
                        .unwrap_err()
                        .starts_with("INVALID_PARAMS"),
                    "{name}"
                );
            }
            let mut unknown = valid.as_object().unwrap().clone();
            unknown.insert("workspaceId".into(), json!("unknown"));
            assert_eq!(
                broker
                    .dispatch(name, Value::Object(unknown), CancellationToken::new())
                    .await,
                Err("WORKSPACE_NOT_FOUND".into()),
                "{name}"
            );
        }
        for workspace_id in [json!(7), json!(" \t"), json!("unknown")] {
            let error = broker
                .read_image(
                    json!({"workspaceId":workspace_id,"path":"marker.png"}),
                    CancellationToken::new(),
                )
                .await
                .unwrap_err();
            if workspace_id == json!("unknown") {
                assert_eq!(error, "WORKSPACE_NOT_FOUND");
            } else {
                assert!(error.starts_with("INVALID_PARAMS"));
            }
        }

        // 相同相对路径的并发本地 Source 请求由各自 Lease 提供 provenance 与文件正文。
        let (source_a, source_b) = tokio::join!(
            broker.dispatch(
                "source_read_file",
                json!({"workspaceId":workspace_a.id,"relative_path":"src/marker.rs"}),
                CancellationToken::new(),
            ),
            broker.dispatch(
                "source_read_file",
                json!({"workspaceId":workspace_b.id,"relative_path":"src/marker.rs"}),
                CancellationToken::new(),
            )
        );
        let source_a = source_a.unwrap();
        let source_b = source_b.unwrap();
        assert_eq!(source_a["workspace"]["id"], workspace_a.id);
        assert!(source_a["text"].as_str().unwrap().contains("SOURCE-A"));
        assert_eq!(source_b["workspace"]["id"], workspace_b.id);
        assert!(source_b["text"].as_str().unwrap().contains("SOURCE-B"));

        // 四个本地基础 Source 与三个 Semantic Source 都必须继续以请求 A 的 Lease 工作。
        for (name, args) in [
            ("source_list_dir", json!({"relative_path":"src"})),
            ("source_find_file", json!({"file_mask":"*.rs"})),
            (
                "source_search_pattern",
                json!({"substring_pattern":"SOURCE-A"}),
            ),
            (
                "source_symbols_overview",
                json!({"relative_path":"src/marker.rs"}),
            ),
            ("source_find_symbol", json!({"name_path_pattern":"Marker"})),
            (
                "source_find_references",
                json!({"relative_path":"src/marker.rs","name_path":"Marker"}),
            ),
        ] {
            let mut args = args.as_object().unwrap().clone();
            args.insert("workspaceId".into(), json!(workspace_a.id));
            let result = broker
                .dispatch(name, Value::Object(args), CancellationToken::new())
                .await
                .unwrap();
            assert_eq!(result["workspace"]["id"], workspace_a.id, "{name}");
            if registry::LOCAL_SOURCES.contains(&name) {
                assert_ne!(
                    result["text"],
                    json!(format!("semantic:{}", workspace_a.id))
                );
            } else {
                assert_eq!(
                    result["text"],
                    json!(format!("semantic:{}", workspace_a.id)),
                    "{name}"
                );
            }
        }

        // Git A/B 并发分别从各自 canonical root 读取，六个公开 Tool 均保持可用。
        let (git_a, git_b) = tokio::join!(
            broker.dispatch(
                "git_status",
                json!({"workspaceId":workspace_a.id}),
                CancellationToken::new(),
            ),
            broker.dispatch(
                "git_status",
                json!({"workspaceId":workspace_b.id}),
                CancellationToken::new(),
            )
        );
        let git_a = git_a.unwrap();
        let git_b = git_b.unwrap();
        assert_eq!(git_a["workspace"]["id"], workspace_a.id);
        assert!(
            git_a["text"]
                .as_str()
                .unwrap()
                .contains("status-marker-A.txt")
        );
        assert_eq!(git_b["workspace"]["id"], workspace_b.id);
        assert!(
            git_b["text"]
                .as_str()
                .unwrap()
                .contains("status-marker-B.txt")
        );
        for name in registry::GITS {
            let result = broker
                .dispatch(
                    name,
                    json!({"workspaceId":workspace_a.id}),
                    CancellationToken::new(),
                )
                .await
                .unwrap();
            assert_eq!(result["workspace"]["id"], workspace_a.id, "{name}");
            assert!(!result["text"].as_str().unwrap().is_empty(), "{name}");
        }
        println!(
            "P2A2-013 A/B trace: source=A:{} B:{}; git=A:{} B:{}",
            source_a["workspace"]["generation"],
            source_b["workspace"]["generation"],
            git_a["workspace"]["generation"],
            git_b["workspace"]["generation"],
        );

        // 兼容 ActiveWorkspace 操作后，显式 A 仍不能退回 Desktop B 或旧 active B。
        assert_eq!(
            broker.workspace.read().await.as_ref().unwrap().workspace.id,
            workspace_b.id
        );
        // Serena 未运行时 workspace_current 不投影 legacy slot，不能让其成为请求上下文。
        assert!(
            broker
                .dispatch("workspace_current", json!({}), CancellationToken::new())
                .await
                .unwrap()["activeWorkspace"]
                .is_null()
        );
        assert_eq!(
            broker
                .dispatch("workspace_deactivate", json!({}), CancellationToken::new())
                .await
                .unwrap()["status"],
            "inactive"
        );
        assert_eq!(
            broker
                .dispatch(
                    "git_status",
                    json!({"workspaceId":workspace_a.id}),
                    CancellationToken::new(),
                )
                .await
                .unwrap()["workspace"]["id"],
            workspace_a.id
        );

        // 无状态 Streamable HTTP 重连不能恢复或建立 Workspace binding。
        broker.start().await.unwrap();
        let uri = format!("http://127.0.0.1:{}/mcp", broker.config().broker.port);
        let client = ().serve(StreamableHttpClientTransport::from_uri(uri.clone())).await.unwrap();
        let missing = client
            .call_tool(
                CallToolRequestParams::new("git_status")
                    .with_arguments(serde_json::Map::<String, Value>::new()),
            )
            .await
            .unwrap_err();
        assert!(missing.to_string().contains("WORKSPACE_CONTEXT_REQUIRED"));
        client.cancel().await.unwrap();
        let reconnected = ().serve(StreamableHttpClientTransport::from_uri(uri)).await.unwrap();
        let missing = reconnected
            .call_tool(
                CallToolRequestParams::new("source_read_file").with_arguments(
                    json!({"relative_path":"src/marker.rs"})
                        .as_object()
                        .unwrap()
                        .clone(),
                ),
            )
            .await
            .unwrap_err();
        assert!(missing.to_string().contains("WORKSPACE_CONTEXT_REQUIRED"));
        let codegraph = reconnected
            .call_tool(
                CallToolRequestParams::new("codegraph_explore").with_arguments(
                    json!({"query":"legacy active graph"})
                        .as_object()
                        .unwrap()
                        .clone(),
                ),
            )
            .await
            .unwrap_err();
        assert!(codegraph.to_string().contains("WORKSPACE_CONTEXT_REQUIRED"));
        reconnected.cancel().await.unwrap();
        broker.stop().await.unwrap();
        legacy.abort();
    }
}

#[cfg(test)]
pub(crate) mod orchestration_tests;
