//! Local HTTP upstream fixture. Never evidence of real Serena/ChatGPT integration.
use super::*;
use rmcp::{
    ErrorData, ServerHandler,
    model::*,
    service::{RequestContext, RoleServer},
    transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
    },
};

#[derive(Clone)]
struct Upstream;
impl ServerHandler for Upstream {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::default();
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        info
    }
    async fn list_tools(
        &self,
        _: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let tools = crate::mcp::registry::SOURCES.iter().map(|(_, name, params, _)| (*name, *params))
            .chain([("activate_project", &["project"][..]), ("get_current_config", &[][..])])
            .map(|(name, params)| {
                let properties: serde_json::Map<String, serde_json::Value> = params.iter().copied().chain(["max_answer_chars"])
                    .map(|p| (p.into(), serde_json::json!({}))).collect();
                serde_json::from_value(serde_json::json!({"name":name,"description":"local fixture upstream tool","inputSchema":{"type":"object","properties":properties}})).unwrap()
            }).collect();
        Ok(ListToolsResult {
            tools,
            ..Default::default()
        })
    }
}
pub(crate) struct Fixture {
    supervisor: Arc<SupervisorState>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
        let mut runtime = self.supervisor.runtime.lock().unwrap();
        if let Some(mut process) = runtime.process.take() {
            let _ = process.child.kill();
            let _ = process.child.wait();
        }
        runtime.status = ServerStatus::Stopped;
    }
}
pub(crate) async fn attach(supervisor: Arc<SupervisorState>) -> Fixture {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let service = StreamableHttpService::new(
        || Ok(Upstream),
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default(),
    );
    let task = tokio::spawn(async move {
        axum::serve(listener, axum::Router::new().nest_service("/mcp", service))
            .await
            .unwrap();
    });
    #[cfg(windows)]
    let mut cmd = hidden_command("ping.exe");
    #[cfg(windows)]
    cmd.args(["-n", "600", "127.0.0.1"]);
    #[cfg(not(windows))]
    let mut cmd = Command::new("sleep");
    #[cfg(not(windows))]
    cmd.arg("600");
    let child = cmd
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    #[cfg(windows)]
    let job = contain_process(&child).unwrap();
    {
        let mut runtime = supervisor.runtime.lock().unwrap();
        runtime.process = Some(ManagedProcess {
            child,
            #[cfg(windows)]
            _job: job,
            port,
            dashboard_enabled: false,
            installation: SerenaInstallation {
                state: InstallationState::Standard,
                source: discovery::InstallationSource::External,
                path: PathBuf::new(),
                version: "fixture".into(),
                context: Some("broker".into()),
                error: None,
            },
        });
        runtime.status = ServerStatus::Running;
    }
    Fixture { supervisor, task }
}
