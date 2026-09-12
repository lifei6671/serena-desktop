use super::*;
use crate::{
    mcp::Broker,
    oauth::{PendingView, Runtime},
};
use serde::Serialize;
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

const REMOTE_ACCESS_TASK_FAILED: &str = "REMOTE_ACCESS_TASK_FAILED";
const REMOTE_ACCESS_STOP_TIMEOUT: &str = "REMOTE_ACCESS_STOP_TIMEOUT";

#[cfg(test)]
#[path = "boundary_tests.rs"]
mod boundary_tests;
#[cfg(test)]
#[path = "manager_tests.rs"]
mod tests;

#[cfg(test)]
pub(crate) type ProbeHook = Arc<
    dyn Fn() -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send>>
        + Send
        + Sync,
>;

#[allow(dead_code, reason = "used by managed ngrok startup in next unit")]
struct NgrokStartPlan {
    auth_token: super::ngrok_store::NgrokAuthToken,
    config: RemoteAccessConfig,
    broker_port: u16,
}

#[allow(dead_code, reason = "owned by managed ngrok worker in next unit")]
struct NgrokConnectedTunnel {
    tunnel: Box<dyn super::ngrok_tunnel::NgrokTunnelHandle>,
    context: RemotePublicContext,
}

struct NgrokConnectFailure {
    code: String,
    retained_tunnel: Option<Box<dyn super::ngrok_tunnel::NgrokTunnelHandle>>,
}

impl NgrokConnectFailure {
    #[allow(dead_code, reason = "used by managed ngrok worker in next unit")]
    fn code(&self) -> &str {
        &self.code
    }

    #[allow(dead_code, reason = "used by managed ngrok worker in next unit")]
    fn into_retained_tunnel(
        self,
    ) -> Option<Box<dyn super::ngrok_tunnel::NgrokTunnelHandle>> {
        self.retained_tunnel
    }
}

struct NgrokActivationFailure {
    code: String,
    retained: Option<NgrokConnectedTunnel>,
}

impl NgrokActivationFailure {
    #[allow(dead_code, reason = "used by managed ngrok worker in next unit")]
    fn code(&self) -> &str {
        &self.code
    }

    #[allow(dead_code, reason = "used by managed ngrok worker in next unit")]
    fn into_retained(self) -> Option<NgrokConnectedTunnel> {
        self.retained
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum NgrokTunnelEnd {
    Cancelled,
    Disconnected,
}

struct NgrokTunnelCloseFailure {
    connected: NgrokConnectedTunnel,
}

impl NgrokTunnelCloseFailure {
    #[allow(dead_code, reason = "used by managed ngrok worker in next unit")]
    fn code(&self) -> &'static str {
        "NGROK_TUNNEL_STOP_FAILED"
    }

    #[allow(dead_code, reason = "used by managed ngrok worker in next unit")]
    fn into_connected(self) -> NgrokConnectedTunnel {
        self.connected
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Stopped,
    Starting,
    Installing,
    DiscoveringUrl,
    Verifying,
    Ready,
    Stopping,
    Error,
    Disconnected,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub mode: RemoteAccessMode,
    pub config: RemoteAccessConfig,
    pub ngrok_auth_configured: bool,
    pub status: Status,
    pub public_context: Option<RemotePublicContext>,
    pub last_error: Option<String>,
    pub authorized_clients: usize,
    pub pending: Vec<PendingView>,
    pub active: bool,
}
pub(crate) struct Inner {
    pub mode: RemoteAccessMode,
    pub config: RemoteAccessConfig,
    pub status: Status,
    pub policy: McpAuthPolicy,
    pub oauth: Option<Runtime>,
    pub error: Option<String>,
    pub cancel: Option<CancellationToken>,
    pub ngrok_auth_configured: bool,
}
pub struct Remote {
    pub(crate) inner: Mutex<Inner>,
    oauth_store: Option<std::path::PathBuf>,
    config_file: Option<std::path::PathBuf>,
    #[allow(dead_code, reason = "used by managed ngrok lifecycle in next unit")]
    ngrok_connector: Arc<dyn super::ngrok_tunnel::NgrokConnector>,
    #[cfg(test)]
    probe_hook: Mutex<Option<ProbeHook>>,
    // Owns the entire install/start/wait/exit lifecycle. Stop waits for this task.
    probe_lock: tokio::sync::Mutex<()>,
    task: tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
    pub(super) pending_child: tokio::sync::Mutex<Option<super::process::ManagedChild>>,
    pending_ngrok:
        tokio::sync::Mutex<Option<Box<dyn super::ngrok_tunnel::NgrokTunnelHandle>>>,
    broker: Mutex<std::sync::Weak<Broker>>,
    pub app: std::sync::OnceLock<tauri::AppHandle>,
}
impl Default for Remote {
    fn default() -> Self {
        Self::with_ngrok_connector(Arc::new(super::ngrok_tunnel::SdkNgrokConnector))
    }
}
impl Remote {
    fn with_ngrok_connector(
        ngrok_connector: Arc<dyn super::ngrok_tunnel::NgrokConnector>,
    ) -> Self {
        Self {
            oauth_store: None,
            config_file: None,
            ngrok_connector,
            #[cfg(test)]
            probe_hook: Mutex::new(None),
            inner: Mutex::new(Inner {
                mode: RemoteAccessMode::default(),
                config: RemoteAccessConfig::default(),
                status: Status::Stopped,
                policy: McpAuthPolicy::Passthrough,
                oauth: None,
                error: None,
                cancel: None,
                ngrok_auth_configured: false,
            }),
            task: tokio::sync::Mutex::new(None),
            probe_lock: tokio::sync::Mutex::new(()),
            pending_child: tokio::sync::Mutex::new(None),
            pending_ngrok: tokio::sync::Mutex::new(None),
            broker: Mutex::new(std::sync::Weak::new()),
            app: std::sync::OnceLock::new(),
        }
    }

    pub fn public_origin(&self) -> Option<String> {
        let inner = self.inner.lock().unwrap();
        if inner.mode == RemoteAccessMode::McpOnly {
            return (inner.config.mcp_only.security_declaration
                == SecurityDeclaration::ExternalAuth)
                .then_some(inner.config.mcp_only.public_origin.as_deref())
                .flatten()
                .and_then(|origin| validate_https_origin(origin).ok());
        }
        inner
            .oauth
            .as_ref()
            .map(|o| o.context.public_origin.clone())
    }
    pub fn from_config(
        config: &RemoteAccessConfig,
        oauth_store: std::path::PathBuf,
        config_file: std::path::PathBuf,
    ) -> Self {
        Self::from_config_core(
            config,
            oauth_store,
            config_file,
            Arc::new(super::ngrok_tunnel::SdkNgrokConnector),
        )
    }
    fn from_config_core(
        config: &RemoteAccessConfig,
        oauth_store: std::path::PathBuf,
        config_file: std::path::PathBuf,
        ngrok_connector: Arc<dyn super::ngrok_tunnel::NgrokConnector>,
    ) -> Self {
        let ngrok_auth_configured = super::ngrok_store::NgrokStore::open(&config_file)
            .and_then(|store| store.read())
            .is_ok_and(|auth_token| auth_token.is_some());
        let remote = Self {
            oauth_store: Some(oauth_store),
            config_file: Some(config_file),
            ..Self::with_ngrok_connector(ngrok_connector)
        };
        {
            let mut inner = remote.inner.lock().unwrap();
            inner.mode = config.mode;
            inner.config = config.clone();
            inner.ngrok_auth_configured = ngrok_auth_configured;
            if config.mode != RemoteAccessMode::McpOnly {
                inner.policy = McpAuthPolicy::EmbeddedOAuth;
            }
            if config.mode == RemoteAccessMode::SelfHostedOAuth
                && config.self_hosted.provider == SelfHostedProvider::CustomHttps
            {
                match RemotePublicContext::new(
                    config.self_hosted.public_origin.as_deref().unwrap_or(""),
                ) {
                    Ok(context) => {
                        match Runtime::open(context, remote.oauth_store.as_ref().unwrap().clone()) {
                            Ok(runtime) => inner.oauth = Some(runtime),
                            Err(error) => {
                                inner.status = Status::Error;
                                inner.error = Some(error.1.into());
                            }
                        }
                    }
                    Err(error) => {
                        inner.status = Status::Error;
                        inner.error = Some(error);
                    }
                }
            }
        }
        remote
    }
    #[cfg(test)]
    pub(crate) fn from_config_with_ngrok_connector(
        config: &RemoteAccessConfig,
        oauth_store: std::path::PathBuf,
        config_file: std::path::PathBuf,
        ngrok_connector: Arc<dyn super::ngrok_tunnel::NgrokConnector>,
    ) -> Self {
        Self::from_config_core(config, oauth_store, config_file, ngrok_connector)
    }
    #[cfg(test)]
    pub(crate) fn set_probe_hook(&self, hook: ProbeHook) {
        *self.probe_hook.lock().unwrap() = Some(hook);
    }
    pub async fn apply_mcp_only(
        &self,
        broker: &Arc<Broker>,
        declaration: SecurityDeclaration,
        risk_accepted: bool,
        public_origin: Option<&str>,
    ) -> Result<(), String> {
        if declaration == SecurityDeclaration::None && !risk_accepted {
            return Err("MCP_ONLY_RISK_NOT_ACCEPTED".into());
        }
        let public_origin = public_origin.map(validate_https_origin).transpose()?;
        if declaration != SecurityDeclaration::ExternalAuth && public_origin.is_some() {
            return Err("MCP_ONLY_ORIGIN_REQUIRES_EXTERNAL_AUTH".into());
        }
        let _management = broker.management.lock().await;
        self.stop().await?;
        let mut config = broker.config();
        config.remote_access.mode = RemoteAccessMode::McpOnly;
        config.remote_access.mcp_only.security_declaration = declaration;
        config.remote_access.mcp_only.public_origin = public_origin;
        Self::persist_config(broker, config.remote_access.clone()).await?;
        let mut inner = self.inner.lock().unwrap();
        inner.mode = RemoteAccessMode::McpOnly;
        inner.config = config.remote_access;
        inner.policy = McpAuthPolicy::Passthrough;
        inner.error = None;
        Ok(())
    }
    #[cfg(test)]
    pub fn policy(&self) -> McpAuthPolicy {
        self.inner.lock().unwrap().policy
    }
    pub fn active(&self) -> bool {
        self.inner.lock().unwrap().cancel.is_some()
    }
    pub fn save_ngrok_auth_token(&self, auth_token: String) -> Result<(), String> {
        let auth_token = auth_token.trim();
        if auth_token.is_empty() {
            return Err("NGROK_AUTH_TOKEN_REQUIRED".into());
        }
        let mut inner = self.inner.lock().unwrap();
        let config_file = self
            .config_file
            .as_deref()
            .ok_or_else(|| "REMOTE_ACCESS_DATABASE_OPEN_FAILED".to_string())?;
        let store = super::ngrok_store::NgrokStore::open(config_file)?;
        store.save(&super::ngrok_store::NgrokAuthToken::new(auth_token.into()))?;
        inner.ngrok_auth_configured = true;
        Ok(())
    }
    pub fn clear_ngrok_auth_token(&self) -> Result<(), String> {
        let mut inner = self.inner.lock().unwrap();
        let config_file = self
            .config_file
            .as_deref()
            .ok_or_else(|| "REMOTE_ACCESS_DATABASE_OPEN_FAILED".to_string())?;
        let store = super::ngrok_store::NgrokStore::open(config_file)?;
        store.delete()?;
        inner.ngrok_auth_configured = false;
        Ok(())
    }
    #[allow(dead_code, reason = "used by managed ngrok startup in next unit")]
    fn prepare_ngrok_start(&self, broker: &Arc<Broker>) -> Result<NgrokStartPlan, String> {
        let config_file = self
            .config_file
            .as_deref()
            .ok_or_else(|| "REMOTE_ACCESS_DATABASE_OPEN_FAILED".to_string())?;
        let auth_token = super::ngrok_store::NgrokStore::open(config_file)?
            .read()?
            .ok_or_else(|| "NGROK_AUTH_TOKEN_REQUIRED".to_string())?;
        let config = broker.config();
        if config.broker.port == config.port {
            return Err("Broker 端口必须不同于 Serena 端口。".into());
        }

        let broker_port = config.broker.port;
        let mut config = config.remote_access;
        config.mode = RemoteAccessMode::SelfHostedOAuth;
        config.self_hosted.provider = SelfHostedProvider::Ngrok;
        config.self_hosted.public_origin = None;
        Ok(NgrokStartPlan {
            auth_token,
            config,
            broker_port,
        })
    }
    #[allow(
        dead_code,
        reason = "used by managed ngrok worker startup in next unit"
    )]
    async fn apply_ngrok_start_plan_locked(
        &self,
        broker: &Arc<Broker>,
        plan: NgrokStartPlan,
    ) -> Result<NgrokStartPlan, String> {
        let previous = broker.config();
        let previous_runtime = {
            let mut inner = self.inner.lock().unwrap();
            let previous_runtime = (
                inner.policy,
                inner.oauth.take(),
                inner.status,
                inner.error.take(),
            );
            inner.policy = McpAuthPolicy::EmbeddedOAuth;
            inner.mode = plan.config.mode;
            inner.config = plan.config.clone();
            inner.oauth = None;
            inner.status = Status::Starting;
            inner.error = None;
            previous_runtime
        };

        if let Err(error) = Self::persist_config(broker, plan.config.clone()).await {
            let mut inner = self.inner.lock().unwrap();
            inner.mode = previous.remote_access.mode;
            inner.config = previous.remote_access;
            inner.oauth = previous_runtime.1;
            inner.status = Status::Error;
            inner.error = Some(error.clone());
            return Err(error);
        }

        if let Err(error) = broker.start().await {
            let rollback = Self::persist_config(broker, previous.remote_access.clone()).await;
            let mut inner = self.inner.lock().unwrap();
            if let Err(rollback) = rollback {
                let diagnostic = format!("{error}; REMOTE_CONFIG_ROLLBACK_FAILED: {rollback}");
                inner.policy = McpAuthPolicy::EmbeddedOAuth;
                inner.status = Status::Error;
                inner.error = Some(diagnostic.clone());
                return Err(diagnostic);
            }
            (inner.policy, inner.oauth, inner.status, inner.error) = previous_runtime;
            inner.mode = previous.remote_access.mode;
            inner.config = previous.remote_access;
            return Err(error);
        }

        *self.broker.lock().unwrap() = Arc::downgrade(broker);
        Ok(plan)
    }
    #[allow(dead_code, reason = "used by managed ngrok worker in next unit")]
    async fn connect_ngrok_plan(
        &self,
        plan: NgrokStartPlan,
    ) -> Result<NgrokConnectedTunnel, NgrokConnectFailure> {
        let NgrokStartPlan {
            auth_token,
            config,
            broker_port,
        } = plan;
        drop(config);
        let mut tunnel = match self.ngrok_connector.connect(&auth_token, broker_port).await {
            Ok(tunnel) => tunnel,
            Err(failure) => {
                let (code, retained_tunnel) = failure.into_parts();
                return Err(NgrokConnectFailure {
                    code,
                    retained_tunnel,
                });
            }
        };
        drop(auth_token);
        let origin = match validate_https_origin(tunnel.origin()) {
            Ok(origin) => origin,
            Err(_) => {
                return if Self::close_ngrok_tunnel(tunnel.as_mut()).await.is_ok() {
                    Err(NgrokConnectFailure {
                        code: "NGROK_PUBLIC_ORIGIN_INVALID".to_string(),
                        retained_tunnel: None,
                    })
                } else {
                    Err(NgrokConnectFailure {
                        code: "NGROK_TUNNEL_STOP_FAILED".to_string(),
                        retained_tunnel: Some(tunnel),
                    })
                };
            }
        };
        let context = match RemotePublicContext::new(&origin) {
            Ok(context) => context,
            Err(code) => {
                return if Self::close_ngrok_tunnel(tunnel.as_mut()).await.is_ok() {
                    Err(NgrokConnectFailure {
                        code,
                        retained_tunnel: None,
                    })
                } else {
                    Err(NgrokConnectFailure {
                        code: "NGROK_TUNNEL_STOP_FAILED".to_string(),
                        retained_tunnel: Some(tunnel),
                    })
                };
            }
        };
        Ok(NgrokConnectedTunnel { tunnel, context })
    }
    #[allow(dead_code, reason = "used by managed ngrok worker in next unit")]
    async fn activate_connected_ngrok(
        &self,
        mut connected: NgrokConnectedTunnel,
    ) -> Result<NgrokConnectedTunnel, NgrokActivationFailure> {
        let instance_id = connected.context.instance_id.clone();
        {
            let mut inner = self.inner.lock().unwrap();
            inner.policy = McpAuthPolicy::EmbeddedOAuth;
            inner.oauth = Some(Runtime::new(connected.context.clone()));
            inner.status = Status::Verifying;
            inner.error = None;
        }

        let probe_error = match self.probe().await {
            Ok(()) => {
                let current = self
                    .inner
                    .lock()
                    .unwrap()
                    .oauth
                    .as_ref()
                    .is_some_and(|oauth| oauth.context.instance_id == instance_id);
                if current {
                    return Ok(connected);
                }
                "REMOTE_ACCESS_NOT_RUNNING".to_string()
            }
            Err(error) => error,
        };

        let close_failed = Self::close_ngrok_tunnel(connected.tunnel.as_mut())
            .await
            .is_err();
        let code = if close_failed {
            "NGROK_TUNNEL_STOP_FAILED".to_string()
        } else {
            probe_error
        };
        let mut inner = self.inner.lock().unwrap();
        if inner
            .oauth
            .as_ref()
            .is_some_and(|oauth| oauth.context.instance_id == instance_id)
        {
            inner.oauth = None;
            inner.status = Status::Error;
            inner.error = Some(code.clone());
        }
        Err(NgrokActivationFailure {
            code,
            retained: close_failed.then_some(connected),
        })
    }
    #[allow(dead_code, reason = "used by managed ngrok worker in next unit")]
    async fn wait_connected_ngrok(
        mut connected: NgrokConnectedTunnel,
        cancel: CancellationToken,
    ) -> Result<NgrokTunnelEnd, NgrokTunnelCloseFailure> {
        let outcome = Self::wait_for_connected_ngrok_terminal(&mut connected, &cancel).await;
        if Self::close_ngrok_tunnel(connected.tunnel.as_mut())
            .await
            .is_err()
        {
            return Err(NgrokTunnelCloseFailure { connected });
        }
        Ok(outcome)
    }
    async fn wait_for_connected_ngrok_terminal(
        connected: &mut NgrokConnectedTunnel,
        cancel: &CancellationToken,
    ) -> NgrokTunnelEnd {
        let outcome = tokio::select! {
            biased;
            _ = cancel.cancelled() => NgrokTunnelEnd::Cancelled,
            _ = connected.tunnel.wait() => NgrokTunnelEnd::Disconnected,
        };
        outcome
    }
    async fn close_ngrok_tunnel(
        tunnel: &mut dyn super::ngrok_tunnel::NgrokTunnelHandle,
    ) -> Result<(), ()> {
        match tokio::time::timeout(
            super::ngrok_tunnel::NGROK_TUNNEL_CLOSE_DEADLINE,
            tunnel.close(),
        )
        .await
        {
            Ok(Ok(())) => Ok(()),
            Ok(Err(_)) | Err(_) => Err(()),
        }
    }
    async fn retain_pending_ngrok(
        &self,
        tunnel: Box<dyn super::ngrok_tunnel::NgrokTunnelHandle>,
    ) {
        let mut pending = self.pending_ngrok.lock().await;
        debug_assert!(pending.is_none());
        *pending = Some(tunnel);
    }
    async fn run_managed_ngrok_worker(
        self: Arc<Self>,
        broker: Arc<Broker>,
        plan: NgrokStartPlan,
        cancel: CancellationToken,
    ) {
        let connected = tokio::select! {
            biased;
            _ = cancel.cancelled() => {
                let mut inner = self.inner.lock().unwrap();
                inner.oauth = None;
                inner.status = Status::Stopping;
                return;
            }
            result = self.connect_ngrok_plan(plan) => result,
        };
        let connected = match connected {
            Ok(connected) => connected,
            Err(failure) => {
                let code = failure.code().to_owned();
                if let Some(tunnel) = failure.into_retained_tunnel() {
                    self.retain_pending_ngrok(tunnel).await;
                    let mut inner = self.inner.lock().unwrap();
                    inner.oauth = None;
                    if !cancel.is_cancelled() {
                        inner.status = Status::Error;
                        inner.error = Some("NGROK_TUNNEL_STOP_FAILED".to_owned());
                    }
                    return;
                }
                let cancelled = {
                    let mut inner = self.inner.lock().unwrap();
                    let cancelled = cancel.is_cancelled();
                    inner.oauth = None;
                    if !cancelled {
                        inner.cancel = None;
                        inner.status = Status::Error;
                        inner.error = Some(code);
                    }
                    cancelled
                };
                if !cancelled && !broker.config().broker.enabled {
                    broker.stop_listener().await;
                }
                return;
            }
        };

        if cancel.is_cancelled() {
            match Self::wait_connected_ngrok(connected, cancel.clone()).await {
                Ok(_) => {
                    let mut inner = self.inner.lock().unwrap();
                    inner.oauth = None;
                    inner.status = Status::Stopping;
                }
                Err(failure) => {
                    let connected = failure.into_connected();
                    self.retain_pending_ngrok(connected.tunnel).await;
                    let mut inner = self.inner.lock().unwrap();
                    inner.oauth = None;
                    inner.status = Status::Stopping;
                }
            }
            return;
        }

        let mut connected = match self.activate_connected_ngrok(connected).await {
            Ok(connected) => connected,
            Err(failure) => {
                let code = failure.code().to_owned();
                if let Some(connected) = failure.into_retained() {
                    self.retain_pending_ngrok(connected.tunnel).await;
                    let mut inner = self.inner.lock().unwrap();
                    inner.oauth = None;
                    if cancel.is_cancelled() {
                        inner.status = Status::Stopping;
                    } else {
                        inner.status = Status::Error;
                        inner.error = Some("NGROK_TUNNEL_STOP_FAILED".to_owned());
                    }
                    return;
                }
                let cancelled = {
                    let mut inner = self.inner.lock().unwrap();
                    let cancelled = cancel.is_cancelled();
                    inner.oauth = None;
                    if cancelled {
                        inner.status = Status::Stopping;
                    } else {
                        inner.cancel = None;
                        inner.status = Status::Error;
                        inner.error = Some(code);
                    }
                    cancelled
                };
                if !cancelled && !broker.config().broker.enabled {
                    broker.stop_listener().await;
                }
                return;
            }
        };

        let outcome = Self::wait_for_connected_ngrok_terminal(&mut connected, &cancel).await;
        let cancelled = {
            let mut inner = self.inner.lock().unwrap();
            let cancelled = outcome == NgrokTunnelEnd::Cancelled
                || cancel.is_cancelled()
                || inner.status == Status::Stopping;
            inner.oauth = None;
            if cancelled {
                inner.status = Status::Stopping;
            } else {
                inner.status = Status::Disconnected;
                inner.error = Some("NGROK_TUNNEL_DISCONNECTED".to_owned());
            }
            cancelled
        };

        if Self::close_ngrok_tunnel(connected.tunnel.as_mut())
            .await
            .is_err()
        {
            self.retain_pending_ngrok(connected.tunnel).await;
            let mut inner = self.inner.lock().unwrap();
            if cancelled || cancel.is_cancelled() || inner.status == Status::Stopping {
                inner.status = Status::Stopping;
            } else {
                inner.status = Status::Error;
                inner.error = Some("NGROK_TUNNEL_STOP_FAILED".to_owned());
            }
            return;
        }

        let cancelled = {
            let mut inner = self.inner.lock().unwrap();
            let cancelled = cancelled || cancel.is_cancelled() || inner.status == Status::Stopping;
            if cancelled {
                inner.status = Status::Stopping;
            } else {
                inner.cancel = None;
            }
            cancelled
        };
        if !cancelled && !broker.config().broker.enabled {
            broker.stop_listener().await;
        }
    }
    pub(crate) async fn start_managed_ngrok_locked(
        self: &Arc<Self>,
        broker: &Arc<Broker>,
    ) -> Result<(), String> {
        let plan = self.prepare_ngrok_start(broker)?;
        self.start_managed_ngrok_plan_locked(broker, plan).await
    }
    async fn start_managed_ngrok_plan_locked(
        self: &Arc<Self>,
        broker: &Arc<Broker>,
        plan: NgrokStartPlan,
    ) -> Result<(), String> {
        let mut task = self.task.lock().await;
        if self.active() {
            return Err("REMOTE_ACCESS_ALREADY_RUNNING".to_owned());
        }
        if self.pending_ngrok.lock().await.is_some() {
            return Err("NGROK_TUNNEL_STOP_FAILED".to_owned());
        }
        if let Some(previous) = task.take() {
            previous.await.map_err(|_| REMOTE_ACCESS_TASK_FAILED)?;
        }
        let plan = self.apply_ngrok_start_plan_locked(broker, plan).await?;
        let cancel = CancellationToken::new();
        {
            let mut inner = self.inner.lock().unwrap();
            inner.cancel = Some(cancel.clone());
        }
        let remote = Arc::clone(self);
        let owner = Arc::clone(broker);
        *task = Some(tokio::spawn(async move {
            remote
                .run_managed_ngrok_worker(owner, plan, cancel)
                .await;
        }));
        Ok(())
    }
    async fn switch_to_managed_ngrok_locked(
        self: &Arc<Self>,
        broker: &Arc<Broker>,
    ) -> Result<(), String> {
        let current = self.snapshot();
        if current.active
            && current.mode == RemoteAccessMode::SelfHostedOAuth
            && current.config.self_hosted.provider == SelfHostedProvider::Ngrok
        {
            return Err("REMOTE_ACCESS_ALREADY_RUNNING".to_owned());
        }
        let plan = self.prepare_ngrok_start(broker)?;
        self.stop().await?;
        self.start_managed_ngrok_plan_locked(broker, plan).await
    }
    #[cfg(test)]
    async fn start_managed_ngrok(
        self: &Arc<Self>,
        broker: Arc<Broker>,
    ) -> Result<(), String> {
        let _management = broker.management.lock().await;
        self.start_managed_ngrok_locked(&broker).await
    }
    pub fn snapshot(&self) -> Snapshot {
        let mut inner = self.inner.lock().unwrap();
        let status = inner.status;
        let (public_context, authorized_clients, pending) = inner
            .oauth
            .as_mut()
            .map(|o| {
                (
                    if status == Status::Ready {
                        Some(o.context.clone())
                    } else {
                        None
                    },
                    o.client_count(),
                    o.pending(),
                )
            })
            .unwrap_or_default();
        Snapshot {
            mode: inner.mode,
            config: inner.config.clone(),
            ngrok_auth_configured: inner.ngrok_auth_configured,
            status,
            public_context,
            authorized_clients,
            pending,
            last_error: inner.error.clone(),
            active: inner.cancel.is_some(),
        }
    }
    pub fn status(&self, status: Status) {
        let mut inner = self.inner.lock().unwrap();
        if inner.cancel.as_ref().is_some_and(|c| !c.is_cancelled()) {
            inner.status = status;
        }
    }
    #[cfg(test)]
    pub async fn start(self: &Arc<Self>, broker: Arc<Broker>) -> Result<(), String> {
        let _management = broker.management.lock().await;
        if self.inner.lock().unwrap().mode == RemoteAccessMode::SelfHostedOAuth {
            return Err(
                "REMOTE_ACCESS_MODE_SWITCH_REQUIRED: 请使用连接方式切换入口开启快捷隧道".into(),
            );
        }
        self.start_mode_locked(&broker, None).await
    }
    #[cfg(test)]
    pub async fn start_mode(
        self: &Arc<Self>,
        broker: Arc<Broker>,
        context: Option<RemotePublicContext>,
    ) -> Result<(), String> {
        let _management = broker.management.lock().await;
        self.start_mode_locked(&broker, context).await
    }
    async fn switch_mode(
        self: &Arc<Self>,
        broker: Arc<Broker>,
        context: Option<RemotePublicContext>,
    ) -> Result<(), String> {
        let target = if context.is_some() {
            RemoteAccessMode::SelfHostedOAuth
        } else {
            RemoteAccessMode::QuickTunnel
        };
        let _management = broker.management.lock().await;
        let current = self.snapshot();
        let same_active_provider = match target {
            RemoteAccessMode::QuickTunnel => current.mode == RemoteAccessMode::QuickTunnel,
            RemoteAccessMode::SelfHostedOAuth => {
                current.mode == RemoteAccessMode::SelfHostedOAuth
                    && current.config.self_hosted.provider == SelfHostedProvider::CustomHttps
            }
            RemoteAccessMode::McpOnly => false,
        };
        if current.active && same_active_provider {
            return Err("REMOTE_ACCESS_ALREADY_RUNNING".into());
        }
        self.stop().await?;
        self.start_mode_locked(&broker, context).await
    }
    // Caller owns Broker.management; startup must not recursively acquire it.
    pub(crate) async fn start_mode_locked(
        self: &Arc<Self>,
        broker: &Arc<Broker>,
        context: Option<RemotePublicContext>,
    ) -> Result<(), String> {
        let mut task = self.task.lock().await;
        if self.active() {
            return Err("REMOTE_ACCESS_ALREADY_RUNNING".into());
        }
        if let Some(previous) = task.take() {
            previous.await.map_err(|_| REMOTE_ACCESS_TASK_FAILED)?;
        }
        let previous = broker.config();
        let mut config = previous.clone();
        // Runtime demand must not change the user's persistent Broker preference.
        if config.broker.port == config.port {
            return Err("Broker 端口必须不同于 Serena 端口。".into());
        }
        config.remote_access.mode = if context.is_some() {
            RemoteAccessMode::SelfHostedOAuth
        } else {
            RemoteAccessMode::QuickTunnel
        };
        if let Some(context) = &context {
            config.remote_access.self_hosted.provider = SelfHostedProvider::CustomHttps;
            config.remote_access.self_hosted.public_origin = Some(context.public_origin.clone());
        }
        let self_hosted = context.is_some();
        let oauth = context
            .map(|context| {
                Runtime::open(
                    context,
                    broker
                        .supervisor
                        .paths
                        .runtime_directory
                        .join("oauth-state.json"),
                )
                .map_err(|e| e.1.to_owned())
            })
            .transpose()?;
        let previous_runtime = {
            let mut inner = self.inner.lock().unwrap();
            let previous_runtime = (
                inner.policy,
                inner.oauth.take(),
                inner.status,
                inner.error.take(),
            );
            inner.policy = McpAuthPolicy::EmbeddedOAuth;
            inner.mode = config.remote_access.mode;
            inner.config = config.remote_access.clone();
            inner.oauth = oauth;
            inner.status = if self_hosted {
                Status::Verifying
            } else {
                Status::Starting
            };
            previous_runtime
        };
        if let Err(error) = Self::persist_config(broker, config.remote_access).await {
            let mut inner = self.inner.lock().unwrap();
            // Failed mode changes stay protected until an explicit user action.
            inner.mode = previous.remote_access.mode;
            inner.config = previous.remote_access;
            inner.oauth = previous_runtime.1;
            inner.status = Status::Error;
            inner.error = Some(error.clone());
            return Err(error);
        }
        // Self-hosted discovery and the full challenge are installed before listening.
        // Quick Tunnel has protection only until its public origin is discovered.
        if let Err(error) = broker.start().await {
            let rollback = Self::persist_config(broker, previous.remote_access.clone()).await;
            let mut inner = self.inner.lock().unwrap();
            if let Err(rollback) = rollback {
                let diagnostic = format!("{error}; REMOTE_CONFIG_ROLLBACK_FAILED: {rollback}");
                inner.policy = McpAuthPolicy::EmbeddedOAuth;
                inner.status = Status::Error;
                inner.error = Some(diagnostic.clone());
                return Err(diagnostic);
            }
            (inner.policy, inner.oauth, inner.status, inner.error) = previous_runtime;
            inner.mode = previous.remote_access.mode;
            inner.config = previous.remote_access;
            return Err(error);
        }
        *self.broker.lock().unwrap() = Arc::downgrade(broker);
        let cancel = CancellationToken::new();
        {
            let mut inner = self.inner.lock().unwrap();
            inner.cancel = Some(cancel.clone());
        }
        let remote = self.clone();
        let owner = broker.clone();
        *task = Some(tokio::spawn(async move {
            if self_hosted {
                let result = tokio::select! {
                    biased;
                    _ = cancel.cancelled() => Ok(()),
                    result = remote.probe() => result,
                };
                if let Err(error) = result {
                    let mut inner = remote.inner.lock().unwrap();
                    if !cancel.is_cancelled() {
                        inner.status = Status::Error;
                        inner.error = Some(error);
                    }
                }
                // The external proxy remains reachable after failure or Stop.
                // Keep EmbeddedOAuth until an explicit switch to MCP-only.
                cancel.cancelled().await;
                let mut inner = remote.inner.lock().unwrap();
                inner.oauth = None;
                inner.cancel = None;
                inner.status = Status::Stopped;
                inner.error = None;
                return;
            }
            owner.log("Remote quick_tunnel starting · Embedded OAuth");
            let result = super::quick_tunnel::run(&remote, &owner, cancel.clone()).await;
            {
                let mut inner = remote.inner.lock().unwrap();
                inner.oauth = None;
                // A failed wait is not proof of exit. Keep OAuth fail-closed until
                // the retained process has been reaped by an explicit Stop.
                let cleanup_failed = result
                    .as_ref()
                    .is_err_and(|e| e == "QUICK_TUNNEL_STOP_FAILED");
                if !cleanup_failed {
                    inner.cancel = None;
                    // Configured OAuth modes remain fail-closed until explicit MCP-only.
                }
                match result {
                    Ok(()) => {
                        inner.status = Status::Stopped;
                        inner.error = None;
                    }
                    Err(error) => {
                        inner.status = if error == "QUICK_TUNNEL_DISCONNECTED" {
                            Status::Disconnected
                        } else {
                            Status::Error
                        };
                        owner.log_level("ERROR", &format!("Remote quick_tunnel · {error}"));
                        inner.error = Some(error);
                    }
                }
            }
            if !owner.config().broker.enabled {
                owner.stop_listener().await;
            }
            owner.log("Remote quick_tunnel ended · OAuth context revoked");
        }));
        Ok(())
    }
    async fn persist_config(
        broker: &Arc<Broker>,
        config: RemoteAccessConfig,
    ) -> Result<(), String> {
        let supervisor = broker.supervisor.clone();
        tokio::task::spawn_blocking(move || supervisor.replace_remote_access(config))
            .await
            .map_err(|e| format!("REMOTE_CONFIG_SAVE_FAILED: {e}"))?
    }
    pub fn cancel(&self) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(cancel) = &inner.cancel {
            cancel.cancel();
            inner.oauth = None;
            inner.status = Status::Stopping;
        }
    }
    pub async fn stop(&self) -> Result<(), String> {
        // Explicit Stop revokes the durable grant, unlike application shutdown.
        {
            let mut inner = self.inner.lock().unwrap();
            if let Some(runtime) = &mut inner.oauth {
                runtime.revoke_persisted().map_err(|e| e.1.to_owned())?;
            } else if let Some(path) = &self.oauth_store {
                Runtime::clear_store(path).map_err(|e| e.1.to_owned())?;
            }
            // Block token issuance between durable revocation and worker shutdown.
            inner.oauth = None;
        }
        self.shutdown().await
    }
    async fn close_pending_ngrok(&self) -> Result<(), String> {
        let mut pending = self.pending_ngrok.lock().await;
        let Some(tunnel) = pending.as_mut() else {
            return Ok(());
        };
        // Intentionally hold the async mutex guard across close: cancellation must leave
        // the tunnel owner in Remote instead of dropping a temporarily taken handle.
        if Self::close_ngrok_tunnel(tunnel.as_mut()).await.is_err() {
            return Err("NGROK_TUNNEL_STOP_FAILED".to_owned());
        }
        *pending = None;
        Ok(())
    }
    pub async fn shutdown(&self) -> Result<(), String> {
        let mut task = self.task.lock().await;
        self.cancel();
        if let Some(handle) = task.as_mut() {
            // Do not detach a live child or claim stopped when exit isn't confirmed.
            tokio::time::timeout(std::time::Duration::from_secs(20), handle)
                .await
                .map_err(|_| REMOTE_ACCESS_STOP_TIMEOUT)?
                .map_err(|_| REMOTE_ACCESS_TASK_FAILED)?;
        }
        *task = None;
        {
            let mut retained = self.pending_child.lock().await;
            if let Some(child) = retained.as_mut() {
                super::quick_tunnel::stop_child(child).await?;
                *retained = None;
            }
        }
        self.close_pending_ngrok().await?;
        {
            let mut inner = self.inner.lock().unwrap();
            inner.oauth = None;
            inner.cancel = None;
            inner.status = Status::Stopped;
            inner.error = None;
        }
        let broker = self.broker.lock().unwrap().upgrade();
        if let Some(broker) = broker
            && !broker.config().broker.enabled
        {
            broker.stop_listener().await;
        }
        Ok(())
    }
    pub async fn probe(&self) -> Result<(), String> {
        let _probe_lock = self.probe_lock.lock().await;
        let (context, credential) = {
            let mut inner = self.inner.lock().unwrap();
            let prepared = inner
                .oauth
                .as_mut()
                .ok_or_else(|| "REMOTE_ACCESS_NOT_RUNNING".to_owned())
                .and_then(|oauth| {
                    Ok((
                        oauth.context.clone(),
                        oauth.probe_credential().map_err(|e| e.1.to_owned())?,
                    ))
                });
            match prepared {
                Ok(value) => value,
                Err(error) => {
                    inner.status = Status::Error;
                    inner.error = Some(error.clone());
                    return Err(error);
                }
            }
        };
        // Cancellation and every error revoke the internal credential too.
        struct Revoke<'a>(&'a Remote, &'a str);
        impl Drop for Revoke<'_> {
            fn drop(&mut self) {
                let mut inner = self.0.inner.lock().unwrap();
                if let Some(oauth) = &mut inner.oauth
                    && oauth.context.instance_id == self.1
                {
                    oauth.revoke_probe();
                }
            }
        }
        let revoke = Revoke(self, &context.instance_id);
        #[cfg(test)]
        let probe_hook = { self.probe_hook.lock().unwrap().clone() };
        #[cfg(test)]
        let result = if let Some(probe_hook) = probe_hook {
            probe_hook().await
        } else {
            super::quick_tunnel::probe(&context, &credential).await
        };
        #[cfg(not(test))]
        let result = super::quick_tunnel::probe(&context, &credential).await;
        drop(revoke);
        let mut inner = self.inner.lock().unwrap();
        if !inner
            .oauth
            .as_ref()
            .is_some_and(|o| o.context.instance_id == context.instance_id)
        {
            return Err("REMOTE_ACCESS_NOT_RUNNING".into());
        }
        inner.status = if result.is_ok() {
            Status::Ready
        } else {
            Status::Error
        };
        inner.error = result.as_ref().err().cloned();
        result
    }
}

#[tauri::command]
pub fn remote_state(app: tauri::AppHandle) -> Snapshot {
    crate::mcp::get(&app).remote.snapshot()
}
#[tauri::command]
pub fn remote_save_ngrok_auth(app: tauri::AppHandle, auth_token: String) -> Result<(), String> {
    crate::mcp::get(&app)
        .remote
        .save_ngrok_auth_token(auth_token)
}
#[tauri::command]
pub fn remote_clear_ngrok_auth(app: tauri::AppHandle) -> Result<(), String> {
    crate::mcp::get(&app).remote.clear_ngrok_auth_token()
}
#[tauri::command]
pub async fn remote_start_ngrok(app: tauri::AppHandle) -> Result<(), String> {
    let broker = crate::mcp::get(&app);
    let _management = broker.management.lock().await;
    broker.remote.switch_to_managed_ngrok_locked(&broker).await
}
#[tauri::command]
pub async fn remote_start(
    app: tauri::AppHandle,
    mode: RemoteAccessMode,
    public_origin: Option<String>,
    security_declaration: Option<SecurityDeclaration>,
    risk_accepted: Option<bool>,
) -> Result<(), String> {
    let broker = crate::mcp::get(&app);
    match mode {
        RemoteAccessMode::QuickTunnel => broker.remote.switch_mode(broker.clone(), None).await,
        RemoteAccessMode::SelfHostedOAuth => {
            let context = RemotePublicContext::new(public_origin.as_deref().unwrap_or("").trim())?;
            broker
                .remote
                .switch_mode(broker.clone(), Some(context))
                .await
        }
        RemoteAccessMode::McpOnly => {
            broker
                .remote
                .apply_mcp_only(
                    &broker,
                    security_declaration.ok_or("MCP_ONLY_SECURITY_DECLARATION_REQUIRED")?,
                    risk_accepted.unwrap_or(false),
                    public_origin
                        .as_deref()
                        .map(str::trim)
                        .filter(|s| !s.is_empty()),
                )
                .await
        }
    }
}
#[tauri::command]
pub async fn remote_stop(app: tauri::AppHandle) -> Result<(), String> {
    let broker = crate::mcp::get(&app);
    let _management = broker.management.lock().await;
    broker.remote.stop().await
}
#[tauri::command]
pub async fn remote_probe(app: tauri::AppHandle) -> Result<(), String> {
    crate::mcp::get(&app).remote.probe().await
}
#[tauri::command]
pub fn remote_approve(app: tauri::AppHandle, id: String, allow: bool) -> Result<(), String> {
    let broker = crate::mcp::get(&app);
    let mut inner = broker.remote.inner.lock().unwrap();
    inner
        .oauth
        .as_mut()
        .ok_or("OAUTH_AUTHORIZATION_EXPIRED")?
        .decide(&id, allow)
        .map_err(|e| e.1.into())
}
