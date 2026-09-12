mod codegraph;
pub mod git;
mod media;
mod orchestration;
pub mod process;
pub mod projects;
pub mod registry;
pub mod serena;
mod server;
mod source_read;
use crate::{
    config::{ManagerConfig, Workspace},
    serena::{ServerStatus, SupervisorState},
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
    pub client: Arc<serena::Client>,
    pub pid: u32,
    pub graph: codegraph::Binding,
    pub generation: u64,
}
pub struct Listener {
    address: std::net::SocketAddr,
    lan_endpoints: Vec<String>,
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
    graph_generation: std::sync::atomic::AtomicU64,
    verified_configs: Mutex<HashMap<String, Vec<u8>>>,
    // Published binding for nonblocking UI reads while indexing owns the query lock.
    published: Mutex<Option<(Workspace, u32, std::sync::Weak<serena::Client>)>>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub running: bool,
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
            graph_generation: std::sync::atomic::AtomicU64::new(0),
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
        // UI polling must not queue behind activation or a pending workspace writer.
        let codegraph = self.workspace.try_read().ok().and_then(|slot| {
            slot.as_ref()
                .filter(|active| {
                    current.as_ref().is_some_and(|w| {
                        w.id == active.workspace.id && w.root == active.workspace.root
                    })
                })
                .map(|active| active.graph.status(&active.workspace, active.generation))
        });
        Snapshot {
            running: listener.as_ref().is_some_and(|l| !l.handle.is_finished()),
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
            codegraph,
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
        // The task owns only this generation's runtime, never the Active slot.
        // Slow graph startup cannot consume Serena's activation timeout budget.
        let generation = self
            .graph_generation
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            + 1;
        let graph = codegraph::Binding::begin(&w, generation, self.logs.clone());
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
        let graph_status = graph.status(&w, generation);
        *slot = Some(Active {
            workspace: w.clone(),
            client,
            pid,
            graph,
            generation,
        });
        self.log(&format!(
            "CodeGraph · binding switch · workspace={} generation={generation}",
            w.id
        ));
        drop(previous);
        self.log(&format!(
            "项目激活耗时 · 总计={}ms",
            started.elapsed().as_millis()
        ));
        Ok(
            json!({"activeWorkspace":w,"status":"active","codegraph":graph_status,"truncated":false}),
        )
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
        let workspace = if args["action"] == "start" {
            self.workspace.read().await.as_ref().map(|active| {
                crate::agent::store::transactions::product::WorkspaceSnapshot {
                    id: active.workspace.id.clone(),
                    root: active.workspace.root.to_string_lossy().into_owned(),
                }
            })
        } else {
            None
        };
        product.operation(args, workspace).await
    }
    pub async fn call_tool(
        &self,
        name: &str,
        args: Value,
        request_cancel: CancellationToken,
    ) -> Result<Value, String> {
        if name == "codegraph_explore" {
            // Graph owns a single 50-second budget including queueing and recovery.
            return Box::pin(self.dispatch(name, args, request_cancel)).await;
        }
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
        if name == "codegraph_explore" {
            return Ok(self.explore_graph(args, cancel).await);
        }
        match name {
            "workspace_list" => {
                return Ok(json!({"workspaces":self.config().workspaces,"truncated":false}));
            }
            "workspace_current" => {
                let slot = self.workspace.read().await;
                let current = self.snapshot().await.active_workspace;
                let graph = current
                    .as_ref()
                    .and_then(|_| slot.as_ref())
                    .map(|a| a.graph.status(&a.workspace, a.generation));
                return Ok(json!({"activeWorkspace":current,"codegraph":graph,"truncated":false}));
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
        let slot = self.workspace.read().await;
        if cancel.is_cancelled() {
            return Err("CANCELLED".into());
        }
        let active = slot.as_ref().ok_or("NO_ACTIVE_WORKSPACE")?;
        let s = self.supervisor.snapshot();
        if s.server_status != ServerStatus::Running
            || s.process_id != Some(active.pid)
            || active.client.closed()
        {
            drop(slot);
            let mut current = self.workspace.write().await;
            let now = self.supervisor.snapshot();
            if current.as_ref().is_some_and(|a| {
                now.server_status != ServerStatus::Running
                    || now.process_id != Some(a.pid)
                    || a.client.closed()
            }) {
                self.clear_workspace(&mut current);
            }
            return Err("NO_ACTIVE_WORKSPACE".into());
        }
        let (text, truncated) = if name.starts_with("git_") {
            let result = git::call(
                name,
                &active.workspace.root,
                serde_json::from_value(args).map_err(|e| format!("INVALID_PARAMS: {e}"))?,
                cancel,
            )
            .await?;
            (result.text, result.truncated)
        } else {
            let a: registry::SourceArgs =
                serde_json::from_value(args.clone()).map_err(|e| e.to_string())?;
            let limit = a.max_bytes.unwrap_or(if name == "source_read_file" {
                32768
            } else {
                65536
            });
            let max = if name == "source_read_file" {
                131072
            } else {
                262144
            };
            if limit == 0 || limit > max {
                return Err("INVALID_PARAMS: max_bytes 超出范围".into());
            }
            let rel = a.relative_path.as_deref().unwrap_or("");
            if name != "source_read_file" {
                let checked = process::safe_relative(&active.workspace.root, rel)?;
                let checked = if name == "source_find_references" {
                    active.workspace.root.clone()
                } else {
                    checked
                };
                let tree_root = active.workspace.root.clone();
                tokio::task::spawn_blocking(move || process::check_subtree(&tree_root, &checked))
                    .await
                    .map_err(|e| e.to_string())??;
            }
            let mut remote = args.as_object().unwrap().clone();
            remote.remove("max_bytes");
            remote.retain(|_, v| !v.is_null());
            remote.insert("relative_path".into(), json!(rel));
            let remote_name = registry::SOURCES.iter().find(|t| t.0 == name).unwrap().1;
            if name != "source_find_file" {
                remote.insert("max_answer_chars".into(), json!(limit));
            }
            if name == "source_list_dir" {
                remote.entry("recursive").or_insert(json!(false));
            }
            if name == "source_read_file" {
                return source_read::read(
                    &active.workspace,
                    &active.client,
                    Value::Object(remote),
                    limit,
                    cancel,
                )
                .await;
            }
            let text = tokio::select! {
                result = active.client.call(remote_name, Value::Object(remote)) => result?,
                _ = cancel.cancelled() => return Err("CANCELLED".into()),
            };
            if text.len() > limit {
                return Err("OUTPUT_LIMIT_EXCEEDED: 缩小路径、行范围或匹配条件".into());
            }
            (text, false)
        };
        let mut result = json!({"workspace":active.workspace,"text":text,"truncated":truncated});
        if truncated {
            result["hint"] = json!("请缩小路径、行范围或日志数量");
        }
        Ok(result)
    }
    async fn clear_invalid_graph_workspace(&self, observed: &Workspace, generation: u64, pid: u32) {
        let mut slot = self.workspace.write().await;
        if let Some(active) = slot.as_ref() {
            if active.workspace.id != observed.id
                || active.workspace.root != observed.root
                || active.generation != generation
                || active.pid != pid
            {
                return;
            }
            let s = self.supervisor.snapshot();
            if s.server_status != ServerStatus::Running
                || s.process_id != Some(active.pid)
                || active.client.closed()
            {
                self.clear_workspace(&mut slot);
            }
        }
    }
    async fn explore_graph(&self, args: Value, cancel: CancellationToken) -> Value {
        let mut queried_workspace = None;
        let query = async {
            let slot = self.workspace.read().await;
            let active = slot.as_ref().ok_or(codegraph::Error::WorkspaceNotActive)?;
            let s = self.supervisor.snapshot();
            if s.server_status != ServerStatus::Running
                || s.process_id != Some(active.pid)
                || active.client.closed()
            {
                let observed = (active.workspace.clone(), active.generation, active.pid);
                drop(slot);
                self.clear_invalid_graph_workspace(&observed.0, observed.1, observed.2)
                    .await;
                return Err(codegraph::Error::WorkspaceNotActive);
            }
            queried_workspace = Some(active.workspace.clone());
            match active
                .graph
                .explore(&active.workspace, active.generation, args, cancel.clone())
                .await
            {
                Ok(text) => Ok(json!({"workspace":active.workspace,"text":text,"truncated":false})),
                Err(error) => Ok(error.value(Some(&active.workspace))),
            }
        };
        let result = tokio::select! {
            biased;
            _ = cancel.cancelled() => Err(codegraph::Error::Cancelled),
            result = tokio::time::timeout(Duration::from_secs(50), query) => result.unwrap_or(Err(codegraph::Error::Timeout)),
        };
        result.unwrap_or_else(|error| error.value(queried_workspace.as_ref()))
    }
    pub async fn sync_projects(&self, sources: Vec<PathBuf>) -> Result<usize, String> {
        let _management = self.management.lock().await;
        *self.project_sources.lock().unwrap() = sources.clone();
        let mut previous = self.config().workspaces;
        if let Some((active, _, _)) = self.published.lock().unwrap().as_ref()
            && !previous.iter().any(|w| w.id == active.id)
        {
            previous.push(active.clone());
        }
        let result = projects::read(sources, &previous)?;
        let count = result.projects.len();
        self.supervisor.replace_workspaces(result.projects)?;
        *self.sync_warnings.lock().unwrap() = result.warnings;
        self.log(&format!("已同步 {count} 个 Serena 项目"));
        Ok(count)
    }
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
    use crate::config::{AppPaths, BrokerConfig};
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
    async fn sync_updates_only_catalog_without_waiting_for_workspace_or_detecting_serena() {
        let dir = tempfile::tempdir().unwrap();
        // A nonexistent executable proves synchronization does not require a working installation.
        let b = fixture(dir.path(), Some(dir.path().join("missing-serena.exe")));
        let root = dir.path().join("one");
        std::fs::create_dir_all(root.join(".serena")).unwrap();
        std::fs::write(root.join(".serena/project.yml"), "language: python\n").unwrap();
        let source = dir.path().join("registry.yml");
        std::fs::write(&source, json!({"projects": [root]}).to_string()).unwrap();
        let before = b.config();
        *b.sync_warnings.lock().unwrap() = vec!["同步失败：此前的配置错误".into()];
        *b.error.lock().unwrap() = Some("listener error".into());
        let guard = b.workspace.write().await;
        assert_eq!(b.sync_projects(vec![source.clone()]).await.unwrap(), 1);
        assert!(b.sync_warnings.lock().unwrap().is_empty());
        assert_eq!(b.error.lock().unwrap().as_deref(), Some("listener error"));
        assert_eq!(b.config().serena_path, before.serena_path);
        assert_eq!(b.config().broker, before.broker);
        let saved = b.config().workspaces;
        std::fs::write(&source, "[broken yaml").unwrap();
        assert!(b.sync_projects(vec![source]).await.is_err());
        assert_eq!(b.config().workspaces, saved);
        drop(guard);
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
        // Do not register placeholder descriptions when the upstream is stopped.
        assert!(
            messages(&tools)[0]["error"]["message"]
                .as_str()
                .unwrap()
                .contains("BACKEND_UNAVAILABLE")
        );
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
            .unwrap();
        assert_eq!(image.is_error, Some(true));
        assert!(
            serde_json::to_string(&image)
                .unwrap()
                .contains("NO_ACTIVE_WORKSPACE")
        );
        assert!(
            client
                .list_all_tools()
                .await
                .unwrap_err()
                .to_string()
                .contains("BACKEND_UNAVAILABLE")
        );
        let r = client
            .call_tool(CallToolRequestParams::new("git_status"))
            .await
            .unwrap();
        assert!(
            serde_json::to_value(r)
                .unwrap()
                .to_string()
                .contains("NO_ACTIVE_WORKSPACE")
        );
        let graph = client
            .call_tool(
                CallToolRequestParams::new("codegraph_explore")
                    .with_arguments(json!({"query":"x"}).as_object().unwrap().clone()),
            )
            .await
            .unwrap();
        assert_eq!(graph.is_error, Some(true));
        assert_eq!(
            graph.structured_content.unwrap()["error"]["code"],
            "WORKSPACE_NOT_ACTIVE"
        );
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
        assert!(
            logs.iter()
                .any(|line| line.starts_with("ERROR ") && line.contains("success=false"))
        );
        assert!(logs.iter().any(|line| line.starts_with("WARN ") && line.contains("error_code=INVALID_PARAMS")));
        assert!(logs.iter().any(|line| line.contains("MCP 已监听")));
        assert!(logs.iter().any(|line| line.contains("HTTP POST")));
        assert!(logs.iter().any(|line| line.contains("tools/list")));
        for (tool, outcome) in [
            ("git_status", "success=false"),
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
    async fn broker_listener_scope_changes_after_restart() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let dir = tempfile::tempdir().unwrap();
        let broker = fixture(dir.path(), None);
        for allow_lan in [false, true, false] {
            let mut config = broker.config();
            config.broker.allow_lan = allow_lan;
            broker.supervisor.replace_config(config).unwrap();
            broker.start().await.unwrap();
            let state = broker.snapshot().await;
            assert!(state.running);
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
            assert!(!broker.snapshot().await.running);
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
                    if let Some(code) = expected_error {
                        assert_eq!(result.is_error, Some(true));
                        assert_eq!(envelope["ok"], false);
                        assert_eq!(envelope["error"]["code"], code);
                        if enabled {
                            assert_eq!(envelope["control"], json!({"requestAccepted":false,"providerInvoked":false,"dispatchCertainty":"not_dispatched","nextAction":{"action":"correct_input"}}));
                        }
                    } else {
                        assert_ne!(result.is_error, Some(true));
                        assert_eq!(envelope, &json!({"ok":true,"data":{"executions":[]},"control":null}));
                    }
                }
                if enabled {
                    store.product_create_fresh("contract-e".into(), "contract-a".into(), "key".into(), "contract-fixture".into(), (Some(crate::agent::store::transactions::product::WorkspaceSnapshot { id:"W".into(), root:dir.path().to_string_lossy().into() })).as_ref().unwrap().id.clone(), Some(crate::agent::store::transactions::product::WorkspaceSnapshot { id:"W".into(), root:dir.path().to_string_lossy().into() }), 1).await.unwrap();
                    let observed = client.call_tool(CallToolRequestParams::new("agent_query").with_arguments(json!({"action":"observe","executionId":"contract-e","waitMs":0}).as_object().unwrap().clone())).await.unwrap();
                    let envelope = observed.structured_content.as_ref().unwrap();
                    assert_eq!(envelope["ok"], true);
                    assert_eq!(envelope["control"]["requestAccepted"], true);
                    assert_eq!(envelope["control"]["providerInvoked"], false);
                    assert_eq!(envelope["control"]["dispatchCertainty"], "not_dispatched");
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
                    assert_eq!(envelope["data"]["resultAvailable"], true);
                    if revision.is_string() { assert_eq!(envelope["data"]["revision"], revision); assert_eq!(envelope["data"]["unchanged"], true); }
                    revision = envelope["data"]["revision"].clone();
                    if include { assert_eq!(envelope["data"]["finalResult"], result); }
                    else { assert!(envelope["data"].get("finalResult").is_none()); }
                    if include {
                        assert!(envelope["data"]["nextAction"].is_null());
                        assert!(envelope["control"]["nextAction"].is_null());
                    } else {
                        assert_eq!(envelope["data"]["nextAction"]["action"], "review_result");
                        assert_eq!(envelope["control"]["nextAction"]["action"], "review_result");
                    }
                }
                let invalid = client.call_tool(CallToolRequestParams::new("agent_query").with_arguments(json!({"action":"observe","executionId":"observe-e","waitMs":20001}).as_object().unwrap().clone())).await.unwrap();
                assert_eq!(invalid.structured_content.unwrap()["error"]["code"], "WORK_INVALID_ARGUMENT");
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
            assert_eq!(tools.len(), if b.config().agent_enabled { 23 } else { 19 });
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
                let result = discovery.call_tool(CallToolRequestParams::new("media_read_image").with_arguments(json!({"path":file}).as_object().unwrap().clone())).await.map_err(|e| e.to_string())?;
                let decoded = media::tests::assert_image(&result, mime);
                assert_eq!((decoded.width(), decoded.height()), (640, 320));
                let pixel = decoded.to_rgb8().get_pixel(320, 230).0;
                assert!(pixel[0] > 240 && pixel[1] < 15 && pixel[2] < 15);
                println!("MCP ImageContent POC: {file}, {mime}, 640x320; SERENA IMAGE TEST / 9274 / red circle");
            }
            for (path, code) in [("../outside.png", "INVALID_PATH"), ("missing.png", "INVALID_PATH"), ("example.py", "UNSUPPORTED_MEDIA_TYPE")] {
                let result = discovery.call_tool(CallToolRequestParams::new("media_read_image").with_arguments(json!({"path":path}).as_object().unwrap().clone())).await.map_err(|e| e.to_string())?;
                assert_eq!(result.is_error, Some(true));
                assert!(serde_json::to_string(&result).unwrap().contains(code));
            }
            let observed_a = {
                let slot = b.workspace.read().await;
                let active = slot.as_ref().unwrap();
                (active.workspace.clone(), active.generation, active.pid)
            };
            assert_eq!(activated["codegraph"]["status"], "not_initialized");
            assert_eq!(
                b.snapshot().await.codegraph.unwrap()["status"],
                "not_initialized"
            );
            assert_eq!(
                b.dispatch(
                    "codegraph_explore",
                    json!({"query":"one"}),
                    CancellationToken::new()
                )
                .await
                .unwrap()["error"]["code"],
                "CODEGRAPH_NOT_INITIALIZED"
            );
            assert!(
                b.activate("missing", CancellationToken::new())
                    .await
                    .is_err()
            );
            assert_eq!(b.snapshot().await.active_workspace.unwrap().id, "project-1");
            {
                let mut slot = b.workspace.write().await;
                let active = slot.as_mut().unwrap();
                active.graph = codegraph::tests::fixture(
                    &active.workspace,
                    active.generation,
                    "slow",
                    b.logs.clone(),
                );
                assert_eq!(
                    active.graph.status(&active.workspace, active.generation)["status"],
                    "starting"
                );
            }
            let starting = b
                .dispatch(
                    "codegraph_explore",
                    json!({"query":"one"}),
                    CancellationToken::new(),
                )
                .await?;
            assert_eq!(starting["error"]["code"], "CODEGRAPH_STARTING");
            assert_eq!(
                b.dispatch("git_status", json!({}), CancellationToken::new())
                    .await?["workspace"]["id"],
                "project-1"
            );
            {
                let mut slot = b.workspace.write().await;
                let active = slot.as_mut().unwrap();
                active.graph =
                    codegraph::tests::mock(&active.workspace, active.generation, b.logs.clone())
                        .await;
                assert_eq!(
                    active.graph.status(&active.workspace, active.generation)["status"],
                    "ready"
                );
            }
            let ui = b.snapshot().await;
            assert_eq!(ui.codegraph.as_ref().unwrap()["status"], "ready");
            assert_eq!(
                ui.codegraph.unwrap()["workspaceId"],
                ui.active_workspace.unwrap().id
            );
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
                    .contains("CODEGRAPH_RUNTIME_LOST")
            );
            let error = crashed.structured_content.unwrap();
            assert_eq!(error["error"]["code"], "CODEGRAPH_RUNTIME_LOST");
            assert_eq!(error["error"]["workspace"]["id"], "project-1");
            assert!(error["error"].get("root").is_none());
            let current = discovery
                .call_tool(CallToolRequestParams::new("workspace_current"))
                .await
                .map_err(|e| e.to_string())?;
            let current = current.structured_content.unwrap();
            assert_eq!(current["activeWorkspace"]["id"], "project-1");
            assert_eq!(current["codegraph"]["workspaceId"], "project-1");
            assert_eq!(current["codegraph"]["status"], "runtime_lost");
            assert_eq!(
                b.snapshot().await.codegraph.unwrap()["status"],
                "runtime_lost"
            );
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
                    json!({"relative_path":"example.py"}),
                    CancellationToken::new(),
                )
                .await?;
            assert!(output["text"].as_str().unwrap().contains("def one"));
            for (name, args) in [
                ("source_list_dir", json!({"relative_path":""})),
                ("source_find_file", json!({"file_mask":"*.py"})),
                (
                    "source_search_pattern",
                    json!({"substring_pattern":"def one"}),
                ),
                (
                    "source_symbols_overview",
                    json!({"relative_path":"example.py"}),
                ),
                (
                    "source_find_symbol",
                    json!({"name_path_pattern":"one", "relative_path":"example.py"}),
                ),
                (
                    "source_find_references",
                    json!({"name_path":"one", "relative_path":"example.py"}),
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
                ("source_read_file", json!({"relative_path":"../outside"})),
                ("source_read_file", json!({"relative_path":"missing.py"})),
                (
                    "source_read_file",
                    json!({"relative_path":"example.py", "max_bytes":1}),
                ),
            ] {
                assert!(
                    b.dispatch(name, args, CancellationToken::new())
                        .await
                        .is_err()
                );
            }
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
            let upstream = serena::Client::connect(b.supervisor.snapshot().active_port).await?;
            let advertised = client1.list_all_tools().await.map_err(|e| e.to_string())?;
            assert_eq!(advertised.len(), 19); // Agent is opt-in.
            for (public_name, remote_name, _, _) in registry::SOURCES {
                let original = upstream
                    .tools
                    .iter()
                    .find(|t| t.name == *remote_name)
                    .unwrap();
                let exposed = advertised.iter().find(|t| t.name == *public_name).unwrap();
                assert_eq!(exposed.description, original.description, "{public_name}");
                assert!(!exposed.description.as_deref().unwrap().is_empty());
            }
            drop(upstream);
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
                        json!({"relative_path":"example.py"})
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
                    json!({"relative_path":"example.py"}),
                    CancellationToken::new(),
                )
                .await?;
            assert!(output["text"].as_str().unwrap().contains("def two"));
            // Preferences save without stopping the running PID or releasing its binding.
            // Restart then reactivates the same workspace using the new process/generation.
            for dashboard in [true, false] {
                let (before_workspace, before_pid, before_generation, released) = {
                    let slot = b.workspace.read().await;
                    let active = slot.as_ref().unwrap();
                    (
                        active.workspace.clone(),
                        active.pid,
                        active.generation,
                        codegraph::tests::release_signal(&active.graph),
                    )
                };
                let mut next = b.config();
                next.dashboard_enabled = dashboard;
                next.open_dashboard_on_launch = false;
                crate::commands::save_config_impl(&b, next).await?;
                assert_eq!(b.supervisor.snapshot().process_id, Some(before_pid));
                assert_eq!(b.supervisor.snapshot().server_status, ServerStatus::Running);
                assert_eq!(b.supervisor.snapshot().active_dashboard_enabled, !dashboard);
                assert!(!released.is_cancelled());
                assert_eq!(
                    b.workspace.read().await.as_ref().unwrap().generation,
                    before_generation
                );
                crate::commands::restart_serena_impl(&b).await?;
                let slot = b.workspace.read().await;
                let restored = slot.as_ref().unwrap();
                assert_eq!(restored.workspace, before_workspace);
                assert_ne!(restored.pid, before_pid);
                assert!(restored.generation > before_generation);
                assert!(released.is_cancelled());
                assert_eq!(b.supervisor.snapshot().active_dashboard_enabled, dashboard);
                drop(slot);
                let output = b
                    .dispatch(
                        "source_read_file",
                        json!({"relative_path":"example.py"}),
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
            let (workspace_b, generation_b, pid_b, released) = {
                let mut slot = b.workspace.write().await;
                let active = slot.as_mut().unwrap();
                active.graph =
                    codegraph::tests::mock(&active.workspace, active.generation, b.logs.clone())
                        .await;
                (
                    active.workspace.clone(),
                    active.generation,
                    active.pid,
                    codegraph::tests::release_signal(&active.graph),
                )
            };
            // B has committed before the stale A observer obtains the write lock.
            // Make B invalid too, so only identity rechecking can protect it.
            let supervisor = b.supervisor.clone();
            tauri::async_runtime::spawn_blocking(move || supervisor.stop())
                .await
                .unwrap()?;
            b.clear_invalid_graph_workspace(&observed_a.0, observed_a.1, observed_a.2)
                .await;
            b.clear_invalid_graph_workspace(&workspace_b, generation_b - 1, pid_b)
                .await;
            {
                let slot = b.workspace.read().await;
                let active = slot.as_ref().unwrap();
                assert_eq!(active.workspace.id, workspace_b.id);
                assert_eq!(
                    active.graph.status(&workspace_b, generation_b)["status"],
                    "ready"
                );
                assert!(!released.is_cancelled());
            }
            assert_eq!(
                b.dispatch(
                    "codegraph_explore",
                    json!({"query":"x"}),
                    CancellationToken::new()
                )
                .await?["error"]["code"],
                "WORKSPACE_NOT_ACTIVE"
            );
            assert!(b.workspace.read().await.is_none());
            assert!(b.published.lock().unwrap().is_none());
            assert!(released.is_cancelled());
            assert!(b.snapshot().await.codegraph.is_none());
            assert!(
                b.dispatch(
                    "source_read_file",
                    json!({"relative_path":"example.py"}),
                    CancellationToken::new()
                )
                .await
                .unwrap_err()
                .contains("NO_ACTIVE_WORKSPACE")
            );
            b.deactivate().await?;
            assert_eq!(
                b.dispatch(
                    "codegraph_explore",
                    json!({"query":"one"}),
                    CancellationToken::new()
                )
                .await?["error"]["code"],
                "WORKSPACE_NOT_ACTIVE"
            );
            assert!(
                b.dispatch("git_status", json!({}), CancellationToken::new())
                    .await
                    .unwrap_err()
                    .contains("NO_ACTIVE_WORKSPACE")
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
}

#[cfg(test)]
pub(crate) mod orchestration_tests;
