use super::*;
use crate::{
    config::{AppPaths, ManagerConfig},
    remote::{
        ngrok_store::NgrokAuthToken,
        ngrok_tunnel::{NgrokConnector, NgrokConnectorFailure, NgrokTunnelHandle},
    },
    serena::SupervisorState,
};
use std::{
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

#[tokio::test]
async fn self_hosted_probe_retry_stop_and_restart_preserve_auth_boundary() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let paths = AppPaths {
        runtime_directory: root.join("runtime"),
        config_file: root.join("config.json"),
        log_directory: root.join("logs"),
        app_log: root.join("logs/app.log"),
        serena_log: root.join("logs/serena.log"),
    };
    let mut config = ManagerConfig::default();
    let reserved = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    config.broker.port = reserved.local_addr().unwrap().port();
    drop(reserved);
    config.broker.enabled = true;
    let origin = format!("http://127.0.0.1:{}", config.broker.port);
    crate::config::save(&paths.config_file, &config).unwrap();
    let broker = Arc::new(Broker::new(Arc::new(SupervisorState::new(paths).unwrap())));
    let _upstream = crate::serena::remote_fixture::attach(broker.supervisor.clone()).await;
    // HTTP is only used by this local integration fixture; IPC requires HTTPS.
    let context = RemotePublicContext {
        public_origin: origin.clone(),
        mcp_resource: format!("{origin}/mcp"),
        instance_id: "first".into(),
    };
    start_local(&broker, context.clone()).await;
    tokio::time::timeout(Duration::from_secs(5), async {
        while broker.remote.snapshot().status != Status::Ready {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        broker.remote.snapshot().mode,
        RemoteAccessMode::SelfHostedOAuth
    );
    assert!(
        broker
            .remote
            .start_mode(broker.clone(), Some(context.clone()))
            .await
            .is_err()
    );
    assert!(!root.join("runtime/cloudflared").exists());
    broker.remote.probe().await.unwrap();
    broker.remote.stop().await.unwrap();
    broker.remote.stop().await.unwrap();
    assert!(!broker.remote.active());
    assert!(broker.remote.snapshot().public_context.is_none());
    assert_eq!(broker.remote.policy(), McpAuthPolicy::EmbeddedOAuth);
    assert!(
        broker
            .remote
            .start(broker.clone())
            .await
            .unwrap_err()
            .starts_with("REMOTE_ACCESS_MODE_SWITCH_REQUIRED")
    );
    assert_eq!(broker.remote.policy(), McpAuthPolicy::EmbeddedOAuth);
    assert_eq!(
        broker.remote.snapshot().mode,
        RemoteAccessMode::SelfHostedOAuth
    );
    assert!(!root.join("runtime/cloudflared").exists());
    let response = reqwest::Client::new()
        .post(format!("{origin}/mcp"))
        .send()
        .await
        .unwrap();
    assert!(!response.status().is_success());
    let failed = RemotePublicContext {
        public_origin: format!("{origin}/missing"),
        ..context
    };
    start_local(&broker, failed).await;
    tokio::time::timeout(Duration::from_secs(5), async {
        while broker.remote.snapshot().status != Status::Error {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(broker.remote.active());
    assert!(broker.remote.snapshot().public_context.is_none());
    assert!(broker.remote.probe().await.is_err());
    assert_eq!(broker.remote.policy(), McpAuthPolicy::EmbeddedOAuth);
    broker.stop().await.unwrap();
}

#[test]
fn self_hosted_origin_rejects_non_origin_inputs() {
    for origin in [
        "",
        "http://example.com",
        "https://example.com/mcp",
        "https://user:pass@example.com",
        "https://example.com?x=1",
        "https://example.com/#fragment",
    ] {
        assert!(RemotePublicContext::new(origin).is_err(), "{origin}");
    }
    assert_eq!(
        RemotePublicContext::new("https://Example.COM:9443/")
            .unwrap()
            .public_origin,
        "https://example.com:9443"
    );
}

fn remote_with_config_file(config_file: &std::path::Path) -> Remote {
    remote_from_config(&RemoteAccessConfig::default(), config_file)
}

fn remote_from_config(config: &RemoteAccessConfig, config_file: &std::path::Path) -> Remote {
    Remote::from_config(
        config,
        config_file.with_file_name("oauth-state.json"),
        config_file.to_owned(),
    )
}

struct UnusedNgrokConnector {
    calls: Arc<AtomicUsize>,
}

impl NgrokConnector for UnusedNgrokConnector {
    fn connect<'a>(
        &'a self,
        _auth_token: &'a NgrokAuthToken,
        _broker_port: u16,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<Box<dyn NgrokTunnelHandle>, NgrokConnectorFailure>,
                > + Send
                + 'a,
        >,
    > {
        let calls = Arc::clone(&self.calls);
        Box::pin(async move {
            calls.fetch_add(1, Ordering::SeqCst);
            Err(NgrokConnectorFailure::new("TEST_CONNECT_NOT_EXPECTED"))
        })
    }
}

fn broker_with_unused_ngrok_connector(
    directory: &tempfile::TempDir,
    config: &ManagerConfig,
    connector_calls: Arc<AtomicUsize>,
) -> Arc<Broker> {
    broker_with_ngrok_connector(
        directory,
        config,
        Arc::new(UnusedNgrokConnector {
            calls: connector_calls,
        }),
    )
}

fn broker_with_ngrok_connector(
    directory: &tempfile::TempDir,
    config: &ManagerConfig,
    connector: Arc<dyn NgrokConnector>,
) -> Arc<Broker> {
    let root = directory.path();
    let paths = AppPaths {
        runtime_directory: root.join("runtime"),
        config_file: root.join("config.json"),
        log_directory: root.join("logs"),
        app_log: root.join("logs/app.log"),
        serena_log: root.join("logs/serena.log"),
    };
    crate::config::save(&paths.config_file, config).unwrap();
    let supervisor = Arc::new(SupervisorState::new(paths).unwrap());
    let mut broker = Broker::new(Arc::clone(&supervisor));
    broker.remote = Arc::new(Remote::from_config_with_ngrok_connector(
        &supervisor.snapshot().config.remote_access,
        supervisor.paths.runtime_directory.join("oauth-state.json"),
        supervisor.paths.config_file.clone(),
        connector,
    ));
    Arc::new(broker)
}

fn available_loopback_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

struct FakeNgrokTunnel {
    origin: String,
    wait_calls: Arc<AtomicUsize>,
    close_calls: Arc<AtomicUsize>,
    close_mode: LifecycleClose,
}

impl NgrokTunnelHandle for FakeNgrokTunnel {
    fn origin(&self) -> &str {
        &self.origin
    }

    fn wait(
        &mut self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + '_>> {
        let calls = Arc::clone(&self.wait_calls);
        Box::pin(async move {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    }

    fn close(
        &mut self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + '_>> {
        let calls = Arc::clone(&self.close_calls);
        let close_mode = self.close_mode;
        Box::pin(async move {
            let call = calls.fetch_add(1, Ordering::SeqCst);
            match close_mode {
                LifecycleClose::Ok => Ok(()),
                LifecycleClose::FailOnce(error) if call == 0 => Err(error.to_string()),
                LifecycleClose::FailOnce(_) => Ok(()),
                LifecycleClose::PendingOnce if call == 0 => {
                    std::future::pending::<Result<(), String>>().await
                }
                LifecycleClose::PendingOnce => Ok(()),
            }
        })
    }
}

struct FakeNgrokConnector {
    calls: Arc<AtomicUsize>,
    received_port: Arc<std::sync::Mutex<Option<u16>>>,
    outcome: Result<&'static str, &'static str>,
    wait_calls: Arc<AtomicUsize>,
    close_calls: Arc<AtomicUsize>,
    close_mode: LifecycleClose,
}

impl NgrokConnector for FakeNgrokConnector {
    fn connect<'a>(
        &'a self,
        _auth_token: &'a NgrokAuthToken,
        broker_port: u16,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<Box<dyn NgrokTunnelHandle>, NgrokConnectorFailure>,
                > + Send
                + 'a,
        >,
    > {
        self.calls.fetch_add(1, Ordering::SeqCst);
        *self.received_port.lock().unwrap() = Some(broker_port);
        let outcome = self.outcome;
        let wait_calls = Arc::clone(&self.wait_calls);
        let close_calls = Arc::clone(&self.close_calls);
        let close_mode = self.close_mode;
        Box::pin(async move {
            outcome
                .map(|origin| {
                    Box::new(FakeNgrokTunnel {
                        origin: origin.into(),
                        wait_calls,
                        close_calls,
                        close_mode,
                    }) as Box<dyn NgrokTunnelHandle>
                })
                .map_err(NgrokConnectorFailure::new)
        })
    }
}

fn stage_managed_ngrok(remote: &Remote) {
    let mut inner = remote.inner.lock().unwrap();
    inner.mode = RemoteAccessMode::SelfHostedOAuth;
    inner.config.mode = RemoteAccessMode::SelfHostedOAuth;
    inner.config.self_hosted.provider = SelfHostedProvider::Ngrok;
    inner.config.self_hosted.public_origin = None;
    inner.policy = McpAuthPolicy::EmbeddedOAuth;
    inner.status = Status::Starting;
    inner.error = None;
}

#[derive(Clone, Copy)]
enum LifecycleWait {
    Pending,
    Ok,
    Err(&'static str),
}

#[derive(Clone, Copy)]
enum LifecycleClose {
    Ok,
    FailOnce(&'static str),
    PendingOnce,
}

#[derive(Clone)]
enum WorkerConnectOutcome {
    Tunnel {
        origin: &'static str,
        close_mode: LifecycleClose,
        wait_mode: WorkerWait,
    },
    Error(&'static str),
    Pending,
}

#[derive(Clone)]
enum WorkerWait {
    Pending,
    Signal(Arc<std::sync::Mutex<Option<tokio::sync::oneshot::Receiver<()>>>>),
}

struct WorkerNgrokTunnel {
    origin: String,
    wait_calls: Arc<AtomicUsize>,
    close_calls: Arc<AtomicUsize>,
    close_mode: LifecycleClose,
    wait_mode: WorkerWait,
}

impl NgrokTunnelHandle for WorkerNgrokTunnel {
    fn origin(&self) -> &str {
        &self.origin
    }

    fn wait(
        &mut self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + '_>> {
        let calls = Arc::clone(&self.wait_calls);
        let wait_mode = self.wait_mode.clone();
        Box::pin(async move {
            calls.fetch_add(1, Ordering::SeqCst);
            match wait_mode {
                WorkerWait::Pending => std::future::pending::<Result<(), String>>().await,
                WorkerWait::Signal(receiver) => {
                    let receiver = receiver.lock().unwrap().take().unwrap();
                    let _ = receiver.await;
                    Ok(())
                }
            }
        })
    }

    fn close(
        &mut self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + '_>> {
        let calls = Arc::clone(&self.close_calls);
        let close_mode = self.close_mode;
        Box::pin(async move {
            let call = calls.fetch_add(1, Ordering::SeqCst);
            match close_mode {
                LifecycleClose::Ok => Ok(()),
                LifecycleClose::FailOnce(error) if call == 0 => Err(error.to_owned()),
                LifecycleClose::FailOnce(_) => Ok(()),
                LifecycleClose::PendingOnce if call == 0 => {
                    std::future::pending::<Result<(), String>>().await
                }
                LifecycleClose::PendingOnce => Ok(()),
            }
        })
    }
}

struct WorkerNgrokConnector {
    outcome: WorkerConnectOutcome,
    connect_calls: Arc<AtomicUsize>,
    wait_calls: Arc<AtomicUsize>,
    close_calls: Arc<AtomicUsize>,
}

impl NgrokConnector for WorkerNgrokConnector {
    fn connect<'a>(
        &'a self,
        _auth_token: &'a NgrokAuthToken,
        _broker_port: u16,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<Box<dyn NgrokTunnelHandle>, NgrokConnectorFailure>,
                > + Send
                + 'a,
        >,
    > {
        self.connect_calls.fetch_add(1, Ordering::SeqCst);
        let outcome = self.outcome.clone();
        let wait_calls = Arc::clone(&self.wait_calls);
        let close_calls = Arc::clone(&self.close_calls);
        Box::pin(async move {
            match outcome {
                WorkerConnectOutcome::Tunnel {
                    origin,
                    close_mode,
                    wait_mode,
                } => {
                    Ok(Box::new(WorkerNgrokTunnel {
                        origin: origin.to_owned(),
                        wait_calls,
                        close_calls,
                        close_mode,
                        wait_mode,
                    }) as Box<dyn NgrokTunnelHandle>)
                }
                WorkerConnectOutcome::Error(error) => Err(NgrokConnectorFailure::new(error)),
                WorkerConnectOutcome::Pending => std::future::pending().await,
            }
        })
    }
}

struct LifecycleNgrokTunnel {
    wait_mode: LifecycleWait,
    close_mode: LifecycleClose,
    wait_calls: Arc<AtomicUsize>,
    close_calls: Arc<AtomicUsize>,
}

impl NgrokTunnelHandle for LifecycleNgrokTunnel {
    fn origin(&self) -> &str {
        "https://lifecycle.example"
    }

    fn wait(
        &mut self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + '_>> {
        let wait_mode = self.wait_mode;
        let calls = Arc::clone(&self.wait_calls);
        Box::pin(async move {
            calls.fetch_add(1, Ordering::SeqCst);
            match wait_mode {
                LifecycleWait::Pending => {
                    std::future::pending::<Result<(), String>>().await
                }
                LifecycleWait::Ok => Ok(()),
                LifecycleWait::Err(error) => Err(error.to_string()),
            }
        })
    }

    fn close(
        &mut self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + '_>> {
        let close_mode = self.close_mode;
        let calls = Arc::clone(&self.close_calls);
        Box::pin(async move {
            let call = calls.fetch_add(1, Ordering::SeqCst);
            match close_mode {
                LifecycleClose::Ok => Ok(()),
                LifecycleClose::FailOnce(error) if call == 0 => Err(error.to_string()),
                LifecycleClose::FailOnce(_) => Ok(()),
                LifecycleClose::PendingOnce if call == 0 => {
                    std::future::pending::<Result<(), String>>().await
                }
                LifecycleClose::PendingOnce => Ok(()),
            }
        })
    }
}

fn lifecycle_connected(
    wait_mode: LifecycleWait,
    close_mode: LifecycleClose,
    wait_calls: Arc<AtomicUsize>,
    close_calls: Arc<AtomicUsize>,
) -> NgrokConnectedTunnel {
    NgrokConnectedTunnel {
        tunnel: Box::new(LifecycleNgrokTunnel {
            wait_mode,
            close_mode,
            wait_calls,
            close_calls,
        }),
        context: RemotePublicContext::new("https://lifecycle.example").unwrap(),
    }
}

async fn wait_for_remote_status(remote: &Remote, expected: Status) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while remote.snapshot().status != expected {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

async fn wait_for_counter(counter: &AtomicUsize, expected: usize) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while counter.load(Ordering::SeqCst) != expected {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

async fn started_signaled_managed_ngrok_worker(
    close_mode: LifecycleClose,
) -> (
    tempfile::TempDir,
    Arc<Broker>,
    tokio::sync::oneshot::Sender<()>,
    Arc<AtomicUsize>,
    Arc<AtomicUsize>,
) {
    let directory = tempfile::tempdir().unwrap();
    let wait_calls = Arc::new(AtomicUsize::new(0));
    let close_calls = Arc::new(AtomicUsize::new(0));
    let (terminal, receiver) = tokio::sync::oneshot::channel();
    let mut config = ManagerConfig::default();
    config.broker.enabled = false;
    config.broker.port = available_loopback_port();
    config.port = if config.broker.port == 9120 { 9121 } else { 9120 };
    let broker = broker_with_ngrok_connector(
        &directory,
        &config,
        Arc::new(WorkerNgrokConnector {
            outcome: WorkerConnectOutcome::Tunnel {
                origin: "https://managed-terminal.example",
                close_mode,
                wait_mode: WorkerWait::Signal(Arc::new(std::sync::Mutex::new(Some(receiver)))),
            },
            connect_calls: Arc::new(AtomicUsize::new(0)),
            wait_calls: Arc::clone(&wait_calls),
            close_calls: Arc::clone(&close_calls),
        }),
    );
    broker
        .remote
        .set_probe_hook(Arc::new(|| Box::pin(async { Ok(()) })));
    broker
        .remote
        .save_ngrok_auth_token("unit7d4b1-terminal-token".to_owned())
        .unwrap();
    broker
        .remote
        .start_managed_ngrok(Arc::clone(&broker))
        .await
        .unwrap();
    wait_for_remote_status(&broker.remote, Status::Ready).await;
    wait_for_counter(wait_calls.as_ref(), 1).await;
    (directory, broker, terminal, wait_calls, close_calls)
}

async fn start_managed_ngrok_entry(broker: &Arc<Broker>) -> Result<(), String> {
    let _management = broker.management.lock().await;
    broker
        .remote
        .switch_to_managed_ngrok_locked(broker)
        .await
}

#[tokio::test]
async fn connect_ngrok_plan_returns_validated_context_without_remote_side_effects() {
    let directory = tempfile::tempdir().unwrap();
    let connector_calls = Arc::new(AtomicUsize::new(0));
    let received_port = Arc::new(std::sync::Mutex::new(None));
    let wait_calls = Arc::new(AtomicUsize::new(0));
    let close_calls = Arc::new(AtomicUsize::new(0));
    let broker = broker_with_ngrok_connector(
        &directory,
        &ManagerConfig::default(),
        Arc::new(FakeNgrokConnector {
            calls: Arc::clone(&connector_calls),
            received_port: Arc::clone(&received_port),
            outcome: Ok("https://Example.NGROK.app:443/"),
            wait_calls: Arc::clone(&wait_calls),
            close_calls: Arc::clone(&close_calls),
            close_mode: LifecycleClose::Ok,
        }),
    );
    let auth_token = "unit7d1-success-token";
    broker
        .remote
        .save_ngrok_auth_token(auth_token.into())
        .unwrap();
    let plan = match broker.remote.prepare_ngrok_start(&broker) {
        Ok(plan) => plan,
        Err(error) => panic!("ngrok start preparation failed: {error}"),
    };
    let expected_port = plan.broker_port;
    let snapshot_before = serde_json::to_value(broker.remote.snapshot()).unwrap();

    let connected = match broker.remote.connect_ngrok_plan(plan).await {
        Ok(connected) => connected,
        Err(_) => panic!("ngrok plan connection failed"),
    };

    assert_eq!(connector_calls.load(Ordering::SeqCst), 1);
    assert_eq!(*received_port.lock().unwrap(), Some(expected_port));
    assert_eq!(connected.tunnel.origin(), "https://Example.NGROK.app:443/");
    assert_eq!(connected.context.public_origin, "https://example.ngrok.app");
    assert_eq!(
        connected.context.mcp_resource,
        "https://example.ngrok.app/mcp"
    );
    assert_eq!(wait_calls.load(Ordering::SeqCst), 0);
    assert_eq!(close_calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        serde_json::to_value(broker.remote.snapshot()).unwrap(),
        snapshot_before
    );
    assert!(!snapshot_before.to_string().contains(auth_token));
}

#[tokio::test]
async fn connect_ngrok_plan_preserves_connector_failure_without_remote_side_effects() {
    let directory = tempfile::tempdir().unwrap();
    let connector_calls = Arc::new(AtomicUsize::new(0));
    let received_port = Arc::new(std::sync::Mutex::new(None));
    let wait_calls = Arc::new(AtomicUsize::new(0));
    let close_calls = Arc::new(AtomicUsize::new(0));
    let broker = broker_with_ngrok_connector(
        &directory,
        &ManagerConfig::default(),
        Arc::new(FakeNgrokConnector {
            calls: Arc::clone(&connector_calls),
            received_port: Arc::clone(&received_port),
            outcome: Err("NGROK_CONNECT_FAILED"),
            wait_calls: Arc::clone(&wait_calls),
            close_calls: Arc::clone(&close_calls),
            close_mode: LifecycleClose::Ok,
        }),
    );
    let auth_token = "unit7d1-connect-failure-token";
    broker
        .remote
        .save_ngrok_auth_token(auth_token.into())
        .unwrap();
    let plan = match broker.remote.prepare_ngrok_start(&broker) {
        Ok(plan) => plan,
        Err(error) => panic!("ngrok start preparation failed: {error}"),
    };
    let expected_port = plan.broker_port;
    let snapshot_before = serde_json::to_value(broker.remote.snapshot()).unwrap();

    let failure = match broker.remote.connect_ngrok_plan(plan).await {
        Ok(_) => panic!("ngrok plan connection unexpectedly succeeded"),
        Err(failure) => failure,
    };

    assert_eq!(failure.code(), "NGROK_CONNECT_FAILED");
    assert!(!failure.code().contains(auth_token));
    assert!(failure.into_retained_tunnel().is_none());
    assert_eq!(connector_calls.load(Ordering::SeqCst), 1);
    assert_eq!(*received_port.lock().unwrap(), Some(expected_port));
    assert_eq!(wait_calls.load(Ordering::SeqCst), 0);
    assert_eq!(close_calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        serde_json::to_value(broker.remote.snapshot()).unwrap(),
        snapshot_before
    );
}

#[tokio::test]
async fn connect_ngrok_plan_rejects_invalid_origin_without_remote_side_effects() {
    let directory = tempfile::tempdir().unwrap();
    let connector_calls = Arc::new(AtomicUsize::new(0));
    let received_port = Arc::new(std::sync::Mutex::new(None));
    let wait_calls = Arc::new(AtomicUsize::new(0));
    let close_calls = Arc::new(AtomicUsize::new(0));
    let invalid_origin = "http://invalid.example";
    let broker = broker_with_ngrok_connector(
        &directory,
        &ManagerConfig::default(),
        Arc::new(FakeNgrokConnector {
            calls: Arc::clone(&connector_calls),
            received_port: Arc::clone(&received_port),
            outcome: Ok(invalid_origin),
            wait_calls: Arc::clone(&wait_calls),
            close_calls: Arc::clone(&close_calls),
            close_mode: LifecycleClose::Ok,
        }),
    );
    let auth_token = "unit7d1-invalid-origin-token";
    broker
        .remote
        .save_ngrok_auth_token(auth_token.into())
        .unwrap();
    let plan = match broker.remote.prepare_ngrok_start(&broker) {
        Ok(plan) => plan,
        Err(error) => panic!("ngrok start preparation failed: {error}"),
    };
    let expected_port = plan.broker_port;
    let snapshot_before = serde_json::to_value(broker.remote.snapshot()).unwrap();

    let failure = match broker.remote.connect_ngrok_plan(plan).await {
        Ok(_) => panic!("invalid ngrok origin unexpectedly succeeded"),
        Err(failure) => failure,
    };

    assert_eq!(failure.code(), "NGROK_PUBLIC_ORIGIN_INVALID");
    assert!(!failure.code().contains(invalid_origin));
    assert!(!failure.code().contains(auth_token));
    assert!(failure.into_retained_tunnel().is_none());
    assert_eq!(connector_calls.load(Ordering::SeqCst), 1);
    assert_eq!(*received_port.lock().unwrap(), Some(expected_port));
    assert_eq!(wait_calls.load(Ordering::SeqCst), 0);
    assert_eq!(close_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        serde_json::to_value(broker.remote.snapshot()).unwrap(),
        snapshot_before
    );
}

#[tokio::test]
async fn connect_ngrok_plan_retains_tunnel_when_invalid_origin_close_fails() {
    let directory = tempfile::tempdir().unwrap();
    let connector_calls = Arc::new(AtomicUsize::new(0));
    let wait_calls = Arc::new(AtomicUsize::new(0));
    let close_calls = Arc::new(AtomicUsize::new(0));
    let invalid_origin = "http://invalid-retained.example";
    let broker = broker_with_ngrok_connector(
        &directory,
        &ManagerConfig::default(),
        Arc::new(FakeNgrokConnector {
            calls: Arc::clone(&connector_calls),
            received_port: Arc::new(std::sync::Mutex::new(None)),
            outcome: Ok(invalid_origin),
            wait_calls: Arc::clone(&wait_calls),
            close_calls: Arc::clone(&close_calls),
            close_mode: LifecycleClose::FailOnce("FAKE_CLOSE_DETAIL_SHOULD_NOT_ESCAPE"),
        }),
    );
    let auth_token = "unit7d1-retained-origin-token";
    broker
        .remote
        .save_ngrok_auth_token(auth_token.into())
        .unwrap();
    let plan = match broker.remote.prepare_ngrok_start(&broker) {
        Ok(plan) => plan,
        Err(error) => panic!("ngrok start preparation failed: {error}"),
    };
    let snapshot_before = serde_json::to_value(broker.remote.snapshot()).unwrap();

    let failure = match broker.remote.connect_ngrok_plan(plan).await {
        Ok(_) => panic!("invalid ngrok origin unexpectedly succeeded"),
        Err(failure) => failure,
    };

    assert_eq!(failure.code(), "NGROK_TUNNEL_STOP_FAILED");
    assert!(!failure.code().contains(invalid_origin));
    assert!(!failure.code().contains(auth_token));
    assert_eq!(connector_calls.load(Ordering::SeqCst), 1);
    assert_eq!(wait_calls.load(Ordering::SeqCst), 0);
    assert_eq!(close_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        serde_json::to_value(broker.remote.snapshot()).unwrap(),
        snapshot_before
    );
    let mut tunnel = match failure.into_retained_tunnel() {
        Some(tunnel) => tunnel,
        None => panic!("failed close did not retain ngrok tunnel"),
    };
    if tunnel.close().await.is_err() {
        panic!("retained ngrok tunnel did not allow close retry");
    }
    assert_eq!(close_calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn activate_connected_ngrok_reaches_ready_without_durable_oauth_or_close() {
    let directory = tempfile::tempdir().unwrap();
    let connector_calls = Arc::new(AtomicUsize::new(0));
    let received_port = Arc::new(std::sync::Mutex::new(None));
    let wait_calls = Arc::new(AtomicUsize::new(0));
    let close_calls = Arc::new(AtomicUsize::new(0));
    let broker = broker_with_ngrok_connector(
        &directory,
        &ManagerConfig::default(),
        Arc::new(FakeNgrokConnector {
            calls: Arc::clone(&connector_calls),
            received_port,
            outcome: Ok("https://Activation.Example:443/"),
            wait_calls: Arc::clone(&wait_calls),
            close_calls: Arc::clone(&close_calls),
            close_mode: LifecycleClose::Ok,
        }),
    );
    let auth_token = "unit7d2-success-token";
    broker
        .remote
        .save_ngrok_auth_token(auth_token.into())
        .unwrap();
    let plan = match broker.remote.prepare_ngrok_start(&broker) {
        Ok(plan) => plan,
        Err(error) => panic!("ngrok start preparation failed: {error}"),
    };
    let connected = match broker.remote.connect_ngrok_plan(plan).await {
        Ok(connected) => connected,
        Err(_) => panic!("ngrok plan connection failed"),
    };
    stage_managed_ngrok(&broker.remote);
    broker
        .remote
        .set_probe_hook(Arc::new(|| Box::pin(async { Ok(()) })));

    let connected = match broker.remote.activate_connected_ngrok(connected).await {
        Ok(connected) => connected,
        Err(_) => panic!("ngrok activation failed"),
    };

    assert_eq!(connector_calls.load(Ordering::SeqCst), 1);
    assert_eq!(connected.context.public_origin, "https://activation.example");
    assert_eq!(wait_calls.load(Ordering::SeqCst), 0);
    assert_eq!(close_calls.load(Ordering::SeqCst), 0);
    let snapshot = broker.remote.snapshot();
    assert!(snapshot.status == Status::Ready);
    assert_eq!(
        snapshot.public_context.as_ref().unwrap().public_origin,
        "https://activation.example"
    );
    assert_eq!(
        snapshot.public_context.as_ref().unwrap().mcp_resource,
        "https://activation.example/mcp"
    );
    assert!(broker.remote.inner.lock().unwrap().oauth.is_some());
    assert_eq!(broker.remote.policy(), McpAuthPolicy::EmbeddedOAuth);
    assert!(!snapshot.active);
    assert!(!serde_json::to_string(&snapshot).unwrap().contains(auth_token));
    assert!(!directory.path().join("runtime/oauth-state.json").exists());
}

#[tokio::test]
async fn activate_connected_ngrok_probe_failure_closes_and_clears_runtime() {
    let directory = tempfile::tempdir().unwrap();
    let wait_calls = Arc::new(AtomicUsize::new(0));
    let close_calls = Arc::new(AtomicUsize::new(0));
    let broker = broker_with_ngrok_connector(
        &directory,
        &ManagerConfig::default(),
        Arc::new(FakeNgrokConnector {
            calls: Arc::new(AtomicUsize::new(0)),
            received_port: Arc::new(std::sync::Mutex::new(None)),
            outcome: Ok("https://failure.example"),
            wait_calls: Arc::clone(&wait_calls),
            close_calls: Arc::clone(&close_calls),
            close_mode: LifecycleClose::Ok,
        }),
    );
    broker
        .remote
        .save_ngrok_auth_token("unit7d2-probe-failure-token".into())
        .unwrap();
    let plan = match broker.remote.prepare_ngrok_start(&broker) {
        Ok(plan) => plan,
        Err(error) => panic!("ngrok start preparation failed: {error}"),
    };
    let connected = match broker.remote.connect_ngrok_plan(plan).await {
        Ok(connected) => connected,
        Err(_) => panic!("ngrok plan connection failed"),
    };
    stage_managed_ngrok(&broker.remote);
    broker.remote.set_probe_hook(Arc::new(|| {
        Box::pin(async { Err("TEST_PROBE_FAILED".to_string()) })
    }));

    let failure = match broker.remote.activate_connected_ngrok(connected).await {
        Ok(_) => panic!("ngrok activation unexpectedly succeeded"),
        Err(failure) => failure,
    };

    assert_eq!(failure.code(), "TEST_PROBE_FAILED");
    assert!(failure.into_retained().is_none());
    assert_eq!(wait_calls.load(Ordering::SeqCst), 0);
    assert_eq!(close_calls.load(Ordering::SeqCst), 1);
    assert!(broker.remote.inner.lock().unwrap().oauth.is_none());
    let snapshot = broker.remote.snapshot();
    assert!(snapshot.status == Status::Error);
    assert_eq!(snapshot.last_error.as_deref(), Some("TEST_PROBE_FAILED"));
    assert!(snapshot.public_context.is_none());
    assert!(!snapshot.active);
}

#[tokio::test]
async fn activate_connected_ngrok_close_failure_uses_stable_error() {
    let directory = tempfile::tempdir().unwrap();
    let wait_calls = Arc::new(AtomicUsize::new(0));
    let close_calls = Arc::new(AtomicUsize::new(0));
    let broker = broker_with_ngrok_connector(
        &directory,
        &ManagerConfig::default(),
        Arc::new(FakeNgrokConnector {
            calls: Arc::new(AtomicUsize::new(0)),
            received_port: Arc::new(std::sync::Mutex::new(None)),
            outcome: Ok("https://close-failure.example"),
            wait_calls: Arc::clone(&wait_calls),
            close_calls: Arc::clone(&close_calls),
            close_mode: LifecycleClose::FailOnce("FAKE_CLOSE_DETAIL_SHOULD_NOT_ESCAPE"),
        }),
    );
    broker
        .remote
        .save_ngrok_auth_token("unit7d2-close-failure-token".into())
        .unwrap();
    let plan = match broker.remote.prepare_ngrok_start(&broker) {
        Ok(plan) => plan,
        Err(error) => panic!("ngrok start preparation failed: {error}"),
    };
    let connected = match broker.remote.connect_ngrok_plan(plan).await {
        Ok(connected) => connected,
        Err(_) => panic!("ngrok plan connection failed"),
    };
    stage_managed_ngrok(&broker.remote);
    broker.remote.set_probe_hook(Arc::new(|| {
        Box::pin(async { Err("TEST_PROBE_FAILED".to_string()) })
    }));

    let failure = match broker.remote.activate_connected_ngrok(connected).await {
        Ok(_) => panic!("ngrok activation unexpectedly succeeded"),
        Err(failure) => failure,
    };

    assert_eq!(failure.code(), "NGROK_TUNNEL_STOP_FAILED");
    assert!(!failure
        .code()
        .contains("FAKE_CLOSE_DETAIL_SHOULD_NOT_ESCAPE"));
    assert_eq!(wait_calls.load(Ordering::SeqCst), 0);
    assert_eq!(close_calls.load(Ordering::SeqCst), 1);
    assert!(broker.remote.inner.lock().unwrap().oauth.is_none());
    let snapshot = broker.remote.snapshot();
    assert!(snapshot.status == Status::Error);
    assert_eq!(
        snapshot.last_error.as_deref(),
        Some("NGROK_TUNNEL_STOP_FAILED")
    );
    assert!(!snapshot
        .last_error
        .as_deref()
        .unwrap()
        .contains("FAKE_CLOSE_DETAIL_SHOULD_NOT_ESCAPE"));
    assert!(!snapshot.active);
    let mut connected = match failure.into_retained() {
        Some(connected) => connected,
        None => panic!("failed close did not retain connected ngrok tunnel"),
    };
    if connected.tunnel.close().await.is_err() {
        panic!("retained connected ngrok tunnel did not allow close retry");
    }
    assert_eq!(close_calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn activate_connected_ngrok_identity_replacement_preserves_new_runtime() {
    let directory = tempfile::tempdir().unwrap();
    let wait_calls = Arc::new(AtomicUsize::new(0));
    let close_calls = Arc::new(AtomicUsize::new(0));
    let broker = broker_with_ngrok_connector(
        &directory,
        &ManagerConfig::default(),
        Arc::new(FakeNgrokConnector {
            calls: Arc::new(AtomicUsize::new(0)),
            received_port: Arc::new(std::sync::Mutex::new(None)),
            outcome: Ok("https://replaced.example"),
            wait_calls: Arc::clone(&wait_calls),
            close_calls: Arc::clone(&close_calls),
            close_mode: LifecycleClose::Ok,
        }),
    );
    broker
        .remote
        .save_ngrok_auth_token("unit7d2-replacement-token".into())
        .unwrap();
    let plan = match broker.remote.prepare_ngrok_start(&broker) {
        Ok(plan) => plan,
        Err(error) => panic!("ngrok start preparation failed: {error}"),
    };
    let connected = match broker.remote.connect_ngrok_plan(plan).await {
        Ok(connected) => connected,
        Err(_) => panic!("ngrok plan connection failed"),
    };
    stage_managed_ngrok(&broker.remote);
    let replacement_context =
        RemotePublicContext::new("https://replacement.example").unwrap();
    let replacement_instance_id = replacement_context.instance_id.clone();
    let remote = Arc::downgrade(&broker.remote);
    broker.remote.set_probe_hook(Arc::new(move || {
        let remote = remote.clone();
        let replacement_context = replacement_context.clone();
        Box::pin(async move {
            let remote = remote.upgrade().unwrap();
            let mut inner = remote.inner.lock().unwrap();
            inner.oauth = Some(Runtime::new(replacement_context));
            inner.status = Status::Starting;
            inner.error = Some("REPLACEMENT_STATE".to_string());
            Err("TEST_PROBE_FAILED".to_string())
        })
    }));

    let failure = match broker.remote.activate_connected_ngrok(connected).await {
        Ok(_) => panic!("ngrok activation unexpectedly succeeded"),
        Err(failure) => failure,
    };

    assert_eq!(failure.code(), "REMOTE_ACCESS_NOT_RUNNING");
    assert!(failure.into_retained().is_none());
    assert_eq!(wait_calls.load(Ordering::SeqCst), 0);
    assert_eq!(close_calls.load(Ordering::SeqCst), 1);
    let inner = broker.remote.inner.lock().unwrap();
    assert_eq!(
        inner.oauth.as_ref().unwrap().context.instance_id,
        replacement_instance_id
    );
    assert!(inner.status == Status::Starting);
    assert_eq!(inner.error.as_deref(), Some("REPLACEMENT_STATE"));
    assert!(inner.cancel.is_none());
}

#[tokio::test]
async fn wait_connected_ngrok_cancel_closes_once() {
    let wait_calls = Arc::new(AtomicUsize::new(0));
    let close_calls = Arc::new(AtomicUsize::new(0));
    let connected = lifecycle_connected(
        LifecycleWait::Pending,
        LifecycleClose::Ok,
        Arc::clone(&wait_calls),
        Arc::clone(&close_calls),
    );
    let cancel = CancellationToken::new();
    cancel.cancel();

    let result = match tokio::time::timeout(
        Duration::from_secs(1),
        Remote::wait_connected_ngrok(connected, cancel),
    )
    .await
    {
        Ok(result) => result,
        Err(_) => panic!("cancelled ngrok wait did not finish"),
    };
    let outcome = match result {
        Ok(outcome) => outcome,
        Err(_) => panic!("cancelled ngrok wait failed"),
    };

    assert!(outcome == NgrokTunnelEnd::Cancelled);
    assert!(wait_calls.load(Ordering::SeqCst) <= 1);
    assert_eq!(close_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn wait_connected_ngrok_clean_end_is_disconnected_and_closes_once() {
    let wait_calls = Arc::new(AtomicUsize::new(0));
    let close_calls = Arc::new(AtomicUsize::new(0));
    let connected = lifecycle_connected(
        LifecycleWait::Ok,
        LifecycleClose::Ok,
        Arc::clone(&wait_calls),
        Arc::clone(&close_calls),
    );

    let outcome = match Remote::wait_connected_ngrok(connected, CancellationToken::new()).await {
        Ok(outcome) => outcome,
        Err(_) => panic!("clean ngrok end failed"),
    };

    assert!(outcome == NgrokTunnelEnd::Disconnected);
    assert_eq!(wait_calls.load(Ordering::SeqCst), 1);
    assert_eq!(close_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn wait_connected_ngrok_wait_error_is_sanitized_to_disconnected() {
    let wait_calls = Arc::new(AtomicUsize::new(0));
    let close_calls = Arc::new(AtomicUsize::new(0));
    let connected = lifecycle_connected(
        LifecycleWait::Err("FAKE_WAIT_DETAIL_SHOULD_NOT_ESCAPE"),
        LifecycleClose::Ok,
        Arc::clone(&wait_calls),
        Arc::clone(&close_calls),
    );

    let outcome = match Remote::wait_connected_ngrok(connected, CancellationToken::new()).await {
        Ok(outcome) => outcome,
        Err(_) => panic!("ngrok wait error unexpectedly caused close failure"),
    };

    assert!(outcome == NgrokTunnelEnd::Disconnected);
    assert_eq!(wait_calls.load(Ordering::SeqCst), 1);
    assert_eq!(close_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn wait_connected_ngrok_close_failure_uses_stable_error() {
    let wait_calls = Arc::new(AtomicUsize::new(0));
    let close_calls = Arc::new(AtomicUsize::new(0));
    let connected = lifecycle_connected(
        LifecycleWait::Ok,
        LifecycleClose::FailOnce("FAKE_CLOSE_DETAIL_SHOULD_NOT_ESCAPE"),
        Arc::clone(&wait_calls),
        Arc::clone(&close_calls),
    );

    let failure = match Remote::wait_connected_ngrok(connected, CancellationToken::new()).await {
        Ok(_) => panic!("ngrok close failure unexpectedly succeeded"),
        Err(failure) => failure,
    };

    assert_eq!(failure.code(), "NGROK_TUNNEL_STOP_FAILED");
    assert_eq!(wait_calls.load(Ordering::SeqCst), 1);
    assert_eq!(close_calls.load(Ordering::SeqCst), 1);
    let mut connected = failure.into_connected();
    if connected.tunnel.close().await.is_err() {
        panic!("retained ngrok tunnel did not allow close retry");
    }
    assert_eq!(close_calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn stop_retries_retained_ngrok_until_close_confirmed() {
    let remote = Remote::default();
    let wait_calls = Arc::new(AtomicUsize::new(0));
    let close_calls = Arc::new(AtomicUsize::new(0));
    let connected = lifecycle_connected(
        LifecycleWait::Pending,
        LifecycleClose::FailOnce("FAKE_CLOSE_DETAIL_SHOULD_NOT_ESCAPE"),
        Arc::clone(&wait_calls),
        Arc::clone(&close_calls),
    );
    *remote.pending_ngrok.lock().await = Some(connected.tunnel);
    {
        let mut inner = remote.inner.lock().unwrap();
        inner.cancel = Some(CancellationToken::new());
        inner.policy = McpAuthPolicy::EmbeddedOAuth;
        inner.status = Status::Error;
        inner.error = Some("NGROK_TUNNEL_STOP_FAILED".to_owned());
        inner.oauth = None;
    }

    let error = match remote.stop().await {
        Ok(()) => panic!("retained ngrok close failure unexpectedly succeeded"),
        Err(error) => error,
    };

    assert_eq!(error, "NGROK_TUNNEL_STOP_FAILED");
    assert!(!error.contains("FAKE_CLOSE_DETAIL_SHOULD_NOT_ESCAPE"));
    assert_eq!(close_calls.load(Ordering::SeqCst), 1);
    assert!(remote.pending_ngrok.lock().await.is_some());
    let first_snapshot = remote.snapshot();
    assert!(first_snapshot.active);
    assert!(first_snapshot.public_context.is_none());
    assert!(first_snapshot.status == Status::Stopping);

    remote.stop().await.unwrap();

    assert_eq!(close_calls.load(Ordering::SeqCst), 2);
    assert!(remote.pending_ngrok.lock().await.is_none());
    let second_snapshot = remote.snapshot();
    assert!(!second_snapshot.active);
    assert!(second_snapshot.status == Status::Stopped);
    assert!(second_snapshot.last_error.is_none());
    assert_eq!(wait_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test(start_paused = true)]
async fn stop_retains_ngrok_when_close_times_out() {
    let remote = Arc::new(Remote::default());
    let wait_calls = Arc::new(AtomicUsize::new(0));
    let close_calls = Arc::new(AtomicUsize::new(0));
    let connected = lifecycle_connected(
        LifecycleWait::Pending,
        LifecycleClose::PendingOnce,
        Arc::clone(&wait_calls),
        Arc::clone(&close_calls),
    );
    *remote.pending_ngrok.lock().await = Some(connected.tunnel);
    {
        let mut inner = remote.inner.lock().unwrap();
        inner.cancel = Some(CancellationToken::new());
        inner.policy = McpAuthPolicy::EmbeddedOAuth;
        inner.status = Status::Error;
        inner.oauth = None;
    }

    let stopping = Arc::clone(&remote);
    let stop = tokio::spawn(async move { stopping.stop().await });
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(12)).await;

    assert_eq!(stop.await.unwrap().unwrap_err(), "NGROK_TUNNEL_STOP_FAILED");
    assert_eq!(close_calls.load(Ordering::SeqCst), 1);
    assert!(remote.pending_ngrok.lock().await.is_some());
    assert!(remote.active());
    assert!(remote.snapshot().status == Status::Stopping);

    remote.stop().await.unwrap();

    assert_eq!(close_calls.load(Ordering::SeqCst), 2);
    assert!(remote.pending_ngrok.lock().await.is_none());
    assert!(!remote.active());
    assert_eq!(wait_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn stop_closes_retained_ngrok_once_on_success() {
    let remote = Remote::default();
    let wait_calls = Arc::new(AtomicUsize::new(0));
    let close_calls = Arc::new(AtomicUsize::new(0));
    let connected = lifecycle_connected(
        LifecycleWait::Pending,
        LifecycleClose::Ok,
        Arc::clone(&wait_calls),
        Arc::clone(&close_calls),
    );
    *remote.pending_ngrok.lock().await = Some(connected.tunnel);
    {
        let mut inner = remote.inner.lock().unwrap();
        inner.cancel = Some(CancellationToken::new());
        inner.policy = McpAuthPolicy::EmbeddedOAuth;
        inner.status = Status::Error;
        inner.oauth = None;
    }

    remote.stop().await.unwrap();

    assert_eq!(close_calls.load(Ordering::SeqCst), 1);
    assert!(remote.pending_ngrok.lock().await.is_none());
    assert!(!remote.active());
    assert!(remote.snapshot().status == Status::Stopped);

    remote.stop().await.unwrap();

    assert_eq!(close_calls.load(Ordering::SeqCst), 1);
    assert!(remote.pending_ngrok.lock().await.is_none());
    assert!(!remote.active());
    assert!(remote.snapshot().status == Status::Stopped);
    assert_eq!(wait_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn managed_ngrok_worker_reaches_ready_and_stop_closes_once() {
    let directory = tempfile::tempdir().unwrap();
    let connect_calls = Arc::new(AtomicUsize::new(0));
    let wait_calls = Arc::new(AtomicUsize::new(0));
    let close_calls = Arc::new(AtomicUsize::new(0));
    let probe_calls = Arc::new(AtomicUsize::new(0));
    let mut config = ManagerConfig::default();
    config.broker.enabled = false;
    config.broker.port = available_loopback_port();
    config.port = if config.broker.port == 9120 { 9121 } else { 9120 };
    let broker = broker_with_ngrok_connector(
        &directory,
        &config,
        Arc::new(WorkerNgrokConnector {
            outcome: WorkerConnectOutcome::Tunnel {
                origin: "https://managed.example",
                close_mode: LifecycleClose::Ok,
                wait_mode: WorkerWait::Pending,
            },
            connect_calls: Arc::clone(&connect_calls),
            wait_calls: Arc::clone(&wait_calls),
            close_calls: Arc::clone(&close_calls),
        }),
    );
    let hook_calls = Arc::clone(&probe_calls);
    broker.remote.set_probe_hook(Arc::new(move || {
        hook_calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(()) })
    }));
    let auth_token = "unit7d4b1-ready-token";
    broker
        .remote
        .save_ngrok_auth_token(auth_token.to_owned())
        .unwrap();

    broker
        .remote
        .start_managed_ngrok(Arc::clone(&broker))
        .await
        .unwrap();
    wait_for_remote_status(&broker.remote, Status::Ready).await;
    wait_for_counter(wait_calls.as_ref(), 1).await;

    let snapshot = broker.remote.snapshot();
    assert_eq!(snapshot.mode, RemoteAccessMode::SelfHostedOAuth);
    assert_eq!(snapshot.config.self_hosted.provider, SelfHostedProvider::Ngrok);
    assert!(snapshot.config.self_hosted.public_origin.is_none());
    assert_eq!(
        snapshot.public_context.as_ref().unwrap().public_origin,
        "https://managed.example"
    );
    assert!(snapshot.active);
    assert!(broker.remote.policy() == McpAuthPolicy::EmbeddedOAuth);
    assert_eq!(connect_calls.load(Ordering::SeqCst), 1);
    assert_eq!(probe_calls.load(Ordering::SeqCst), 1);
    assert_eq!(close_calls.load(Ordering::SeqCst), 0);
    assert!(!serde_json::to_string(&snapshot).unwrap().contains(auth_token));
    assert!(!directory.path().join("runtime/oauth-state.json").exists());

    broker.remote.stop().await.unwrap();

    assert_eq!(close_calls.load(Ordering::SeqCst), 1);
    assert!(broker.remote.pending_ngrok.lock().await.is_none());
    let stopped = broker.remote.snapshot();
    assert!(!stopped.active);
    assert!(stopped.status == Status::Stopped);
    assert!(stopped.public_context.is_none());
    assert!(broker.remote.inner.lock().unwrap().oauth.is_none());
    assert!(!broker.snapshot().await.running);

    broker.remote.stop().await.unwrap();
    assert_eq!(close_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn persisted_managed_ngrok_reconnects_on_broker_startup() {
    let directory = tempfile::tempdir().unwrap();
    let connect_calls = Arc::new(AtomicUsize::new(0));
    let wait_calls = Arc::new(AtomicUsize::new(0));
    let close_calls = Arc::new(AtomicUsize::new(0));
    let mut config = ManagerConfig::default();
    config.broker.port = available_loopback_port();
    config.port = if config.broker.port == 9120 {
        9121
    } else {
        9120
    };
    let connector = Arc::new(WorkerNgrokConnector {
        outcome: WorkerConnectOutcome::Tunnel {
            origin: "https://managed.example",
            close_mode: LifecycleClose::Ok,
            wait_mode: WorkerWait::Pending,
        },
        connect_calls: Arc::clone(&connect_calls),
        wait_calls: Arc::clone(&wait_calls),
        close_calls: Arc::clone(&close_calls),
    });
    let first = broker_with_ngrok_connector(&directory, &config, connector.clone());
    first
        .remote
        .set_probe_hook(Arc::new(|| Box::pin(async { Ok(()) })));
    let auth_token = "restart-test-ngrok-token";
    first
        .remote
        .save_ngrok_auth_token(auth_token.to_owned())
        .unwrap();
    first
        .remote
        .start_managed_ngrok(Arc::clone(&first))
        .await
        .unwrap();
    wait_for_remote_status(&first.remote, Status::Ready).await;
    first.shutdown().await.unwrap();
    drop(first);

    let saved = crate::config::load(&directory.path().join("config.json")).unwrap();
    assert_eq!(saved.remote_access.mode, RemoteAccessMode::SelfHostedOAuth);
    assert_eq!(
        saved.remote_access.self_hosted.provider,
        SelfHostedProvider::Ngrok
    );
    assert!(
        !std::fs::read_to_string(directory.path().join("config.json"))
            .unwrap()
            .contains(auth_token)
    );
    let restarted = broker_with_ngrok_connector(&directory, &saved, connector);
    assert!(restarted.remote.snapshot().ngrok_auth_configured);
    restarted
        .remote
        .set_probe_hook(Arc::new(|| Box::pin(async { Ok(()) })));

    restarted.startup().await.unwrap();
    wait_for_remote_status(&restarted.remote, Status::Ready).await;
    assert_eq!(connect_calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        restarted
            .remote
            .snapshot()
            .public_context
            .unwrap()
            .public_origin,
        "https://managed.example"
    );
    restarted.shutdown().await.unwrap();
    assert_eq!(close_calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn missing_ngrok_token_does_not_block_enabled_local_broker_startup() {
    let directory = tempfile::tempdir().unwrap();
    let connector_calls = Arc::new(AtomicUsize::new(0));
    let mut config = ManagerConfig::default();
    config.broker.enabled = true;
    config.broker.port = available_loopback_port();
    config.port = if config.broker.port == 9120 {
        9121
    } else {
        9120
    };
    config.remote_access.mode = RemoteAccessMode::SelfHostedOAuth;
    config.remote_access.self_hosted.provider = SelfHostedProvider::Ngrok;
    let broker =
        broker_with_unused_ngrok_connector(&directory, &config, Arc::clone(&connector_calls));
    broker
        .remote
        .save_ngrok_auth_token("test-token".into())
        .unwrap();
    broker.remote.clear_ngrok_auth_token().unwrap();

    assert_eq!(
        broker.startup().await.unwrap_err(),
        "NGROK_AUTH_TOKEN_REQUIRED"
    );
    assert!(broker.snapshot().await.running);
    assert_eq!(broker.remote.policy(), McpAuthPolicy::EmbeddedOAuth);
    assert_eq!(connector_calls.load(Ordering::SeqCst), 0);
    broker.shutdown().await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn managed_ngrok_terminal_hides_context_before_pending_close_and_retains_failure() {
    let (_directory, broker, terminal, wait_calls, close_calls) =
        started_signaled_managed_ngrok_worker(LifecycleClose::PendingOnce).await;

    terminal.send(()).unwrap();
    wait_for_counter(close_calls.as_ref(), 1).await;

    let disconnecting = broker.remote.snapshot();
    assert!(disconnecting.status == Status::Disconnected);
    assert_eq!(
        disconnecting.last_error.as_deref(),
        Some("NGROK_TUNNEL_DISCONNECTED")
    );
    assert!(disconnecting.public_context.is_none());
    assert!(disconnecting.active);
    assert!(broker.remote.inner.lock().unwrap().oauth.is_none());

    tokio::time::advance(Duration::from_secs(12)).await;
    wait_for_remote_status(&broker.remote, Status::Error).await;
    let failed = broker.remote.snapshot();
    assert_eq!(failed.last_error.as_deref(), Some("NGROK_TUNNEL_STOP_FAILED"));
    assert!(failed.active);
    assert!(broker.remote.pending_ngrok.lock().await.is_some());
    assert_eq!(wait_calls.load(Ordering::SeqCst), 1);

    broker.remote.stop().await.unwrap();
    let stopped = broker.remote.snapshot();
    assert!(stopped.status == Status::Stopped);
    assert!(stopped.last_error.is_none());
    assert!(broker.remote.pending_ngrok.lock().await.is_none());
    assert_eq!(close_calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn managed_ngrok_cancelled_terminal_never_reverts_stopping_to_disconnected() {
    let (_directory, broker, terminal, _wait_calls, close_calls) =
        started_signaled_managed_ngrok_worker(LifecycleClose::Ok).await;

    broker.remote.cancel();
    terminal.send(()).unwrap();
    wait_for_counter(close_calls.as_ref(), 1).await;

    let stopping = broker.remote.snapshot();
    assert!(stopping.status == Status::Stopping);
    assert!(stopping.public_context.is_none());
    assert!(stopping.active);

    broker.remote.stop().await.unwrap();
    assert!(broker.remote.snapshot().status == Status::Stopped);
}

#[tokio::test]
async fn managed_ngrok_application_shutdown_closes_once_and_preserves_auth_token() {
    let (directory, broker, _terminal, _wait_calls, close_calls) =
        started_signaled_managed_ngrok_worker(LifecycleClose::Ok).await;

    broker.shutdown().await.unwrap();

    let snapshot = broker.remote.snapshot();
    assert_eq!(close_calls.load(Ordering::SeqCst), 1);
    assert!(snapshot.status == Status::Stopped);
    assert!(!snapshot.active);
    assert!(snapshot.public_context.is_none());
    assert!(snapshot.ngrok_auth_configured);
    assert!(remote_with_config_file(&directory.path().join("config.json"))
        .snapshot()
        .ngrok_auth_configured);
}

#[tokio::test]
async fn managed_ngrok_application_shutdown_retains_failed_close_for_retry_without_clearing_auth() {
    let directory = tempfile::tempdir().unwrap();
    let connector_calls = Arc::new(AtomicUsize::new(0));
    let mut config = ManagerConfig::default();
    config.broker.enabled = false;
    let broker = broker_with_unused_ngrok_connector(&directory, &config, connector_calls);
    let wait_calls = Arc::new(AtomicUsize::new(0));
    let close_calls = Arc::new(AtomicUsize::new(0));
    let connected = lifecycle_connected(
        LifecycleWait::Pending,
        LifecycleClose::FailOnce("FAKE_CLOSE_DETAIL_SHOULD_NOT_ESCAPE"),
        Arc::clone(&wait_calls),
        Arc::clone(&close_calls),
    );
    stage_managed_ngrok(&broker.remote);
    broker
        .remote
        .save_ngrok_auth_token("unit7d4b1-shutdown-token".to_owned())
        .unwrap();
    *broker.remote.pending_ngrok.lock().await = Some(connected.tunnel);
    {
        let mut inner = broker.remote.inner.lock().unwrap();
        inner.cancel = Some(CancellationToken::new());
        inner.status = Status::Error;
    }

    assert_eq!(
        broker.shutdown().await.unwrap_err(),
        "NGROK_TUNNEL_STOP_FAILED"
    );
    let failed = broker.remote.snapshot();
    assert_eq!(close_calls.load(Ordering::SeqCst), 1);
    assert!(failed.status == Status::Stopping);
    assert!(failed.active);
    assert!(failed.public_context.is_none());
    assert!(failed.ngrok_auth_configured);
    assert!(broker.remote.pending_ngrok.lock().await.is_some());
    assert!(remote_with_config_file(&directory.path().join("config.json"))
        .snapshot()
        .ngrok_auth_configured);

    broker.shutdown().await.unwrap();

    let stopped = broker.remote.snapshot();
    assert_eq!(close_calls.load(Ordering::SeqCst), 2);
    assert!(stopped.status == Status::Stopped);
    assert!(!stopped.active);
    assert!(stopped.public_context.is_none());
    assert!(stopped.ngrok_auth_configured);
    assert!(broker.remote.pending_ngrok.lock().await.is_none());
    assert_eq!(wait_calls.load(Ordering::SeqCst), 0);
    assert!(remote_with_config_file(&directory.path().join("config.json"))
        .snapshot()
        .ngrok_auth_configured);
}

#[tokio::test]
async fn managed_ngrok_worker_cancels_pending_connect_without_becoming_ready() {
    let directory = tempfile::tempdir().unwrap();
    let connect_calls = Arc::new(AtomicUsize::new(0));
    let wait_calls = Arc::new(AtomicUsize::new(0));
    let close_calls = Arc::new(AtomicUsize::new(0));
    let mut config = ManagerConfig::default();
    config.broker.enabled = false;
    config.broker.port = available_loopback_port();
    config.port = if config.broker.port == 9120 { 9121 } else { 9120 };
    let broker = broker_with_ngrok_connector(
        &directory,
        &config,
        Arc::new(WorkerNgrokConnector {
            outcome: WorkerConnectOutcome::Pending,
            connect_calls: Arc::clone(&connect_calls),
            wait_calls: Arc::clone(&wait_calls),
            close_calls: Arc::clone(&close_calls),
        }),
    );
    broker
        .remote
        .save_ngrok_auth_token("unit7d4b1-pending-connect-token".to_owned())
        .unwrap();

    broker
        .remote
        .start_managed_ngrok(Arc::clone(&broker))
        .await
        .unwrap();
    wait_for_counter(connect_calls.as_ref(), 1).await;

    tokio::time::timeout(Duration::from_secs(1), broker.remote.stop())
        .await
        .expect("stop did not cancel pending ngrok connect")
        .unwrap();

    let snapshot = broker.remote.snapshot();
    assert!(snapshot.status == Status::Stopped);
    assert!(!snapshot.active);
    assert!(snapshot.public_context.is_none());
    assert!(broker.remote.inner.lock().unwrap().oauth.is_none());
    assert!(broker.remote.pending_ngrok.lock().await.is_none());
    assert_eq!(connect_calls.load(Ordering::SeqCst), 1);
    assert_eq!(wait_calls.load(Ordering::SeqCst), 0);
    assert_eq!(close_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn managed_ngrok_worker_connector_failure_becomes_inactive_error() {
    let directory = tempfile::tempdir().unwrap();
    let connect_calls = Arc::new(AtomicUsize::new(0));
    let wait_calls = Arc::new(AtomicUsize::new(0));
    let close_calls = Arc::new(AtomicUsize::new(0));
    let probe_calls = Arc::new(AtomicUsize::new(0));
    let mut config = ManagerConfig::default();
    config.broker.enabled = false;
    config.broker.port = available_loopback_port();
    config.port = if config.broker.port == 9120 { 9121 } else { 9120 };
    let broker = broker_with_ngrok_connector(
        &directory,
        &config,
        Arc::new(WorkerNgrokConnector {
            outcome: WorkerConnectOutcome::Error("NGROK_CONNECT_FAILED"),
            connect_calls: Arc::clone(&connect_calls),
            wait_calls: Arc::clone(&wait_calls),
            close_calls: Arc::clone(&close_calls),
        }),
    );
    let hook_calls = Arc::clone(&probe_calls);
    broker.remote.set_probe_hook(Arc::new(move || {
        hook_calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(()) })
    }));
    let auth_token = "unit7d4b1-connect-failure-token";
    broker
        .remote
        .save_ngrok_auth_token(auth_token.to_owned())
        .unwrap();

    broker
        .remote
        .start_managed_ngrok(Arc::clone(&broker))
        .await
        .unwrap();
    wait_for_remote_status(&broker.remote, Status::Error).await;

    let snapshot = broker.remote.snapshot();
    assert_eq!(snapshot.last_error.as_deref(), Some("NGROK_CONNECT_FAILED"));
    assert!(!snapshot.active);
    assert!(snapshot.public_context.is_none());
    assert!(broker.remote.inner.lock().unwrap().oauth.is_none());
    assert!(broker.remote.pending_ngrok.lock().await.is_none());
    assert_eq!(connect_calls.load(Ordering::SeqCst), 1);
    assert_eq!(probe_calls.load(Ordering::SeqCst), 0);
    assert_eq!(wait_calls.load(Ordering::SeqCst), 0);
    assert_eq!(close_calls.load(Ordering::SeqCst), 0);
    assert!(!serde_json::to_string(&snapshot).unwrap().contains(auth_token));

    broker.remote.stop().await.unwrap();
    let stopped = broker.remote.snapshot();
    assert!(stopped.status == Status::Stopped);
    assert!(stopped.last_error.is_none());
}

#[tokio::test]
async fn managed_ngrok_worker_retains_context_cleanup_failure_until_stop() {
    let directory = tempfile::tempdir().unwrap();
    let connect_calls = Arc::new(AtomicUsize::new(0));
    let wait_calls = Arc::new(AtomicUsize::new(0));
    let close_calls = Arc::new(AtomicUsize::new(0));
    let probe_calls = Arc::new(AtomicUsize::new(0));
    let mut config = ManagerConfig::default();
    config.broker.enabled = false;
    config.broker.port = available_loopback_port();
    config.port = if config.broker.port == 9120 { 9121 } else { 9120 };
    let invalid_origin = "http://invalid-managed.example";
    let raw_close_error = "FAKE_CLOSE_DETAIL_SHOULD_NOT_ESCAPE";
    let broker = broker_with_ngrok_connector(
        &directory,
        &config,
        Arc::new(WorkerNgrokConnector {
            outcome: WorkerConnectOutcome::Tunnel {
                origin: invalid_origin,
                close_mode: LifecycleClose::FailOnce(raw_close_error),
                wait_mode: WorkerWait::Pending,
            },
            connect_calls: Arc::clone(&connect_calls),
            wait_calls: Arc::clone(&wait_calls),
            close_calls: Arc::clone(&close_calls),
        }),
    );
    let hook_calls = Arc::clone(&probe_calls);
    broker.remote.set_probe_hook(Arc::new(move || {
        hook_calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(()) })
    }));
    let auth_token = "unit7d4b1-context-cleanup-token";
    broker
        .remote
        .save_ngrok_auth_token(auth_token.to_owned())
        .unwrap();

    broker
        .remote
        .start_managed_ngrok(Arc::clone(&broker))
        .await
        .unwrap();
    wait_for_remote_status(&broker.remote, Status::Error).await;

    let snapshot = broker.remote.snapshot();
    assert_eq!(
        snapshot.last_error.as_deref(),
        Some("NGROK_TUNNEL_STOP_FAILED")
    );
    assert!(snapshot.active);
    assert!(snapshot.public_context.is_none());
    assert!(broker.remote.inner.lock().unwrap().oauth.is_none());
    assert!(broker.remote.pending_ngrok.lock().await.is_some());
    assert_eq!(connect_calls.load(Ordering::SeqCst), 1);
    assert_eq!(probe_calls.load(Ordering::SeqCst), 0);
    assert_eq!(wait_calls.load(Ordering::SeqCst), 0);
    assert_eq!(close_calls.load(Ordering::SeqCst), 1);
    let serialized = serde_json::to_string(&snapshot).unwrap();
    assert!(!serialized.contains(auth_token));
    assert!(!serialized.contains(invalid_origin));
    assert!(!serialized.contains(raw_close_error));

    broker.remote.stop().await.unwrap();

    assert_eq!(close_calls.load(Ordering::SeqCst), 2);
    assert!(broker.remote.pending_ngrok.lock().await.is_none());
    assert!(!broker.remote.active());
    assert!(broker.remote.snapshot().status == Status::Stopped);
    assert!(!broker.snapshot().await.running);
}

#[tokio::test]
async fn managed_ngrok_entry_missing_token_is_fail_closed_and_does_not_connect() {
    let directory = tempfile::tempdir().unwrap();
    let connect_calls = Arc::new(AtomicUsize::new(0));
    let wait_calls = Arc::new(AtomicUsize::new(0));
    let close_calls = Arc::new(AtomicUsize::new(0));
    let mut config = ManagerConfig::default();
    config.remote_access.mode = RemoteAccessMode::SelfHostedOAuth;
    config.remote_access.self_hosted.provider = SelfHostedProvider::Ngrok;
    config.remote_access.self_hosted.public_origin = None;
    let broker = broker_with_ngrok_connector(
        &directory,
        &config,
        Arc::new(WorkerNgrokConnector {
            outcome: WorkerConnectOutcome::Error("TEST_CONNECT_NOT_EXPECTED"),
            connect_calls: Arc::clone(&connect_calls),
            wait_calls: Arc::clone(&wait_calls),
            close_calls: Arc::clone(&close_calls),
        }),
    );

    let error = start_managed_ngrok_entry(&broker).await.unwrap_err();

    assert_eq!(error, "NGROK_AUTH_TOKEN_REQUIRED");
    let snapshot = broker.remote.snapshot();
    assert!(!snapshot.active);
    assert!(snapshot.status == Status::Stopped);
    assert!(broker.remote.policy() == McpAuthPolicy::EmbeddedOAuth);
    assert!(broker.remote.inner.lock().unwrap().oauth.is_none());
    assert_eq!(connect_calls.load(Ordering::SeqCst), 0);
    assert_eq!(wait_calls.load(Ordering::SeqCst), 0);
    assert_eq!(close_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn managed_ngrok_entry_switches_from_active_custom_self_hosted_then_starts_ngrok() {
    let directory = tempfile::tempdir().unwrap();
    let connect_calls = Arc::new(AtomicUsize::new(0));
    let wait_calls = Arc::new(AtomicUsize::new(0));
    let close_calls = Arc::new(AtomicUsize::new(0));
    let probe_calls = Arc::new(AtomicUsize::new(0));
    let mut config = ManagerConfig::default();
    config.broker.enabled = false;
    config.broker.port = available_loopback_port();
    config.port = if config.broker.port == 9120 { 9121 } else { 9120 };
    let broker = broker_with_ngrok_connector(
        &directory,
        &config,
        Arc::new(WorkerNgrokConnector {
            outcome: WorkerConnectOutcome::Tunnel {
                origin: "https://managed-entry.example",
                close_mode: LifecycleClose::Ok,
                wait_mode: WorkerWait::Pending,
            },
            connect_calls: Arc::clone(&connect_calls),
            wait_calls: Arc::clone(&wait_calls),
            close_calls: Arc::clone(&close_calls),
        }),
    );
    let hook_calls = Arc::clone(&probe_calls);
    broker.remote.set_probe_hook(Arc::new(move || {
        hook_calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(()) })
    }));
    broker
        .remote
        .save_ngrok_auth_token("unit7e1-switch-token".to_owned())
        .unwrap();
    let custom_context =
        RemotePublicContext::new("https://custom-before-ngrok.example").unwrap();
    let custom_instance_id = custom_context.instance_id.clone();
    broker
        .remote
        .start_mode(Arc::clone(&broker), Some(custom_context))
        .await
        .unwrap();
    wait_for_remote_status(&broker.remote, Status::Ready).await;
    let custom_cancel = broker
        .remote
        .inner
        .lock()
        .unwrap()
        .cancel
        .as_ref()
        .unwrap()
        .clone();

    start_managed_ngrok_entry(&broker).await.unwrap();
    wait_for_remote_status(&broker.remote, Status::Ready).await;
    wait_for_counter(wait_calls.as_ref(), 1).await;

    let snapshot = broker.remote.snapshot();
    let managed_context = snapshot.public_context.as_ref().unwrap();
    assert!(custom_cancel.is_cancelled());
    assert_ne!(managed_context.instance_id, custom_instance_id);
    assert_eq!(managed_context.public_origin, "https://managed-entry.example");
    assert_eq!(snapshot.mode, RemoteAccessMode::SelfHostedOAuth);
    assert_eq!(snapshot.config.self_hosted.provider, SelfHostedProvider::Ngrok);
    assert!(snapshot.config.self_hosted.public_origin.is_none());
    assert!(broker.remote.policy() == McpAuthPolicy::EmbeddedOAuth);
    assert!(snapshot.active);
    assert_eq!(connect_calls.load(Ordering::SeqCst), 1);
    assert_eq!(probe_calls.load(Ordering::SeqCst), 2);
    assert_eq!(close_calls.load(Ordering::SeqCst), 0);

    broker.remote.stop().await.unwrap();

    assert_eq!(close_calls.load(Ordering::SeqCst), 1);
    assert!(!broker.remote.active());
    assert!(broker.remote.snapshot().status == Status::Stopped);
}

#[tokio::test]
async fn managed_ngrok_preflight_missing_token_preserves_active_custom_https() {
    let directory = tempfile::tempdir().unwrap();
    let connect_calls = Arc::new(AtomicUsize::new(0));
    let wait_calls = Arc::new(AtomicUsize::new(0));
    let close_calls = Arc::new(AtomicUsize::new(0));
    let probe_calls = Arc::new(AtomicUsize::new(0));
    let mut config = ManagerConfig::default();
    config.broker.enabled = false;
    config.broker.port = available_loopback_port();
    config.port = if config.broker.port == 9120 { 9121 } else { 9120 };
    let broker = broker_with_ngrok_connector(
        &directory,
        &config,
        Arc::new(WorkerNgrokConnector {
            outcome: WorkerConnectOutcome::Error("TEST_CONNECT_NOT_EXPECTED"),
            connect_calls: Arc::clone(&connect_calls),
            wait_calls: Arc::clone(&wait_calls),
            close_calls: Arc::clone(&close_calls),
        }),
    );
    let hook_calls = Arc::clone(&probe_calls);
    broker.remote.set_probe_hook(Arc::new(move || {
        hook_calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(()) })
    }));
    let custom_context =
        RemotePublicContext::new("https://custom-preflight-missing-token.example").unwrap();
    let custom_origin = custom_context.public_origin.clone();
    let custom_instance_id = custom_context.instance_id.clone();
    broker
        .remote
        .start_mode(Arc::clone(&broker), Some(custom_context))
        .await
        .unwrap();
    wait_for_remote_status(&broker.remote, Status::Ready).await;
    let custom_cancel = broker
        .remote
        .inner
        .lock()
        .unwrap()
        .cancel
        .as_ref()
        .unwrap()
        .clone();
    let config_before = broker.config().remote_access;

    assert_eq!(
        start_managed_ngrok_entry(&broker).await.unwrap_err(),
        "NGROK_AUTH_TOKEN_REQUIRED"
    );

    let snapshot = broker.remote.snapshot();
    let context = snapshot.public_context.as_ref().unwrap();
    assert!(!custom_cancel.is_cancelled());
    assert!(snapshot.active);
    assert!(snapshot.status == Status::Ready);
    assert_eq!(snapshot.mode, RemoteAccessMode::SelfHostedOAuth);
    assert_eq!(snapshot.config.self_hosted.provider, SelfHostedProvider::CustomHttps);
    assert_eq!(context.public_origin, custom_origin);
    assert_eq!(context.instance_id, custom_instance_id);
    assert_eq!(broker.config().remote_access, config_before);
    assert_eq!(connect_calls.load(Ordering::SeqCst), 0);
    assert_eq!(probe_calls.load(Ordering::SeqCst), 1);
    assert_eq!(wait_calls.load(Ordering::SeqCst), 0);
    assert_eq!(close_calls.load(Ordering::SeqCst), 0);

    broker.remote.stop().await.unwrap();
}

#[tokio::test]
async fn managed_ngrok_preflight_database_failure_preserves_active_custom_https() {
    let directory = tempfile::tempdir().unwrap();
    let connect_calls = Arc::new(AtomicUsize::new(0));
    let wait_calls = Arc::new(AtomicUsize::new(0));
    let close_calls = Arc::new(AtomicUsize::new(0));
    let probe_calls = Arc::new(AtomicUsize::new(0));
    let mut config = ManagerConfig::default();
    config.broker.enabled = false;
    config.broker.port = available_loopback_port();
    config.port = if config.broker.port == 9120 { 9121 } else { 9120 };
    let broker = broker_with_ngrok_connector(
        &directory,
        &config,
        Arc::new(WorkerNgrokConnector {
            outcome: WorkerConnectOutcome::Error("TEST_CONNECT_NOT_EXPECTED"),
            connect_calls: Arc::clone(&connect_calls),
            wait_calls: Arc::clone(&wait_calls),
            close_calls: Arc::clone(&close_calls),
        }),
    );
    let hook_calls = Arc::clone(&probe_calls);
    broker.remote.set_probe_hook(Arc::new(move || {
        hook_calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(()) })
    }));
    broker
        .remote
        .save_ngrok_auth_token("unit7e1-port-conflict-token".to_owned())
        .unwrap();
    let custom_context =
        RemotePublicContext::new("https://custom-preflight-port-conflict.example").unwrap();
    let custom_origin = custom_context.public_origin.clone();
    let custom_instance_id = custom_context.instance_id.clone();
    broker
        .remote
        .start_mode(Arc::clone(&broker), Some(custom_context))
        .await
        .unwrap();
    wait_for_remote_status(&broker.remote, Status::Ready).await;
    let custom_cancel = broker
        .remote
        .inner
        .lock()
        .unwrap()
        .cancel
        .as_ref()
        .unwrap()
        .clone();
    let config_before = broker.config().remote_access;
    std::fs::write(
        directory.path().join("remote-access.db"),
        "not a sqlite database",
    )
    .unwrap();

    assert_eq!(
        start_managed_ngrok_entry(&broker).await.unwrap_err(),
        "REMOTE_ACCESS_DATABASE_INIT_FAILED"
    );

    let snapshot = broker.remote.snapshot();
    let context = snapshot.public_context.as_ref().unwrap();
    assert!(!custom_cancel.is_cancelled());
    assert!(snapshot.active);
    assert!(snapshot.status == Status::Ready);
    assert_eq!(snapshot.mode, RemoteAccessMode::SelfHostedOAuth);
    assert_eq!(snapshot.config.self_hosted.provider, SelfHostedProvider::CustomHttps);
    assert_eq!(context.public_origin, custom_origin);
    assert_eq!(context.instance_id, custom_instance_id);
    assert_eq!(broker.config().remote_access, config_before);
    assert_eq!(connect_calls.load(Ordering::SeqCst), 0);
    assert_eq!(probe_calls.load(Ordering::SeqCst), 1);
    assert_eq!(wait_calls.load(Ordering::SeqCst), 0);
    assert_eq!(close_calls.load(Ordering::SeqCst), 0);

    broker.remote.stop().await.unwrap();
}

#[tokio::test]
async fn managed_ngrok_can_switch_to_custom_https() {
    let directory = tempfile::tempdir().unwrap();
    let connect_calls = Arc::new(AtomicUsize::new(0));
    let wait_calls = Arc::new(AtomicUsize::new(0));
    let close_calls = Arc::new(AtomicUsize::new(0));
    let probe_calls = Arc::new(AtomicUsize::new(0));
    let mut config = ManagerConfig::default();
    config.broker.enabled = false;
    config.broker.port = available_loopback_port();
    config.port = if config.broker.port == 9120 { 9121 } else { 9120 };
    let broker = broker_with_ngrok_connector(
        &directory,
        &config,
        Arc::new(WorkerNgrokConnector {
            outcome: WorkerConnectOutcome::Tunnel {
                origin: "https://managed-before-custom.example",
                close_mode: LifecycleClose::Ok,
                wait_mode: WorkerWait::Pending,
            },
            connect_calls: Arc::clone(&connect_calls),
            wait_calls: Arc::clone(&wait_calls),
            close_calls: Arc::clone(&close_calls),
        }),
    );
    let hook_calls = Arc::clone(&probe_calls);
    broker.remote.set_probe_hook(Arc::new(move || {
        hook_calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(()) })
    }));
    broker
        .remote
        .save_ngrok_auth_token("unit7e1-ngrok-to-custom-token".to_owned())
        .unwrap();

    start_managed_ngrok_entry(&broker).await.unwrap();
    wait_for_remote_status(&broker.remote, Status::Ready).await;
    wait_for_counter(wait_calls.as_ref(), 1).await;
    let managed_cancel = broker
        .remote
        .inner
        .lock()
        .unwrap()
        .cancel
        .as_ref()
        .unwrap()
        .clone();

    broker
        .remote
        .switch_mode(
            Arc::clone(&broker),
            Some(RemotePublicContext::new("https://custom-after-ngrok.example").unwrap()),
        )
        .await
        .unwrap();
    wait_for_remote_status(&broker.remote, Status::Ready).await;

    let snapshot = broker.remote.snapshot();
    assert!(managed_cancel.is_cancelled());
    assert_eq!(close_calls.load(Ordering::SeqCst), 1);
    assert_eq!(connect_calls.load(Ordering::SeqCst), 1);
    assert_eq!(probe_calls.load(Ordering::SeqCst), 2);
    assert_eq!(snapshot.mode, RemoteAccessMode::SelfHostedOAuth);
    assert_eq!(
        snapshot.config.self_hosted.provider,
        SelfHostedProvider::CustomHttps
    );
    assert_eq!(
        snapshot.config.self_hosted.public_origin.as_deref(),
        Some("https://custom-after-ngrok.example")
    );
    assert_eq!(
        snapshot.public_context.as_ref().unwrap().public_origin,
        "https://custom-after-ngrok.example"
    );
    assert_eq!(
        broker.config().remote_access.self_hosted.public_origin.as_deref(),
        Some("https://custom-after-ngrok.example")
    );

    broker.remote.stop().await.unwrap();
    assert_eq!(close_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn active_custom_https_still_rejects_duplicate_custom_start() {
    let directory = tempfile::tempdir().unwrap();
    let connector_calls = Arc::new(AtomicUsize::new(0));
    let mut config = ManagerConfig::default();
    config.broker.enabled = false;
    config.broker.port = available_loopback_port();
    config.port = if config.broker.port == 9120 { 9121 } else { 9120 };
    let broker = broker_with_unused_ngrok_connector(
        &directory,
        &config,
        Arc::clone(&connector_calls),
    );
    broker.remote.set_probe_hook(Arc::new(|| Box::pin(async { Ok(()) })));
    broker
        .remote
        .start_mode(
            Arc::clone(&broker),
            Some(RemotePublicContext::new("https://custom-duplicate.example").unwrap()),
        )
        .await
        .unwrap();
    wait_for_remote_status(&broker.remote, Status::Ready).await;
    let custom_cancel = broker
        .remote
        .inner
        .lock()
        .unwrap()
        .cancel
        .as_ref()
        .unwrap()
        .clone();

    let error = broker
        .remote
        .switch_mode(
            Arc::clone(&broker),
            Some(RemotePublicContext::new("https://other-custom.example").unwrap()),
        )
        .await
        .unwrap_err();

    assert_eq!(error, "REMOTE_ACCESS_ALREADY_RUNNING");
    assert!(!custom_cancel.is_cancelled());
    assert_eq!(connector_calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        broker
            .remote
            .snapshot()
            .config
            .self_hosted
            .public_origin
            .as_deref(),
        Some("https://custom-duplicate.example")
    );

    broker.remote.stop().await.unwrap();
}

#[tokio::test]
async fn active_tailscale_funnel_can_switch_to_custom_https() {
    let directory = tempfile::tempdir().unwrap();
    let connector_calls = Arc::new(AtomicUsize::new(0));
    let mut config = ManagerConfig::default();
    config.broker.enabled = false;
    config.broker.port = available_loopback_port();
    config.port = if config.broker.port == 9120 { 9121 } else { 9120 };
    config.remote_access.mode = RemoteAccessMode::SelfHostedOAuth;
    config.remote_access.self_hosted.provider = SelfHostedProvider::TailscaleFunnel;
    config.remote_access.self_hosted.public_origin = Some("https://legacy-tailscale.example".into());
    let broker = broker_with_unused_ngrok_connector(
        &directory,
        &config,
        Arc::clone(&connector_calls),
    );
    broker.remote.set_probe_hook(Arc::new(|| Box::pin(async { Ok(()) })));
    let tailscale_cancel = CancellationToken::new();
    {
        let mut inner = broker.remote.inner.lock().unwrap();
        inner.cancel = Some(tailscale_cancel.clone());
        inner.status = Status::Ready;
    }
    let task_cancel = tailscale_cancel.clone();
    *broker.remote.task.lock().await = Some(tokio::spawn(async move {
        task_cancel.cancelled().await;
    }));

    broker
        .remote
        .switch_mode(
            Arc::clone(&broker),
            Some(RemotePublicContext::new("https://custom-after-tailscale.example").unwrap()),
        )
        .await
        .unwrap();
    wait_for_remote_status(&broker.remote, Status::Ready).await;

    let snapshot = broker.remote.snapshot();
    assert!(tailscale_cancel.is_cancelled());
    assert_eq!(connector_calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        snapshot.config.self_hosted.provider,
        SelfHostedProvider::CustomHttps
    );
    assert_eq!(
        snapshot.config.self_hosted.public_origin.as_deref(),
        Some("https://custom-after-tailscale.example")
    );

    broker.remote.stop().await.unwrap();
}

#[tokio::test]
async fn managed_ngrok_entry_rejects_duplicate_active_ngrok() {
    let directory = tempfile::tempdir().unwrap();
    let connect_calls = Arc::new(AtomicUsize::new(0));
    let wait_calls = Arc::new(AtomicUsize::new(0));
    let close_calls = Arc::new(AtomicUsize::new(0));
    let mut config = ManagerConfig::default();
    config.broker.enabled = false;
    config.broker.port = available_loopback_port();
    config.port = if config.broker.port == 9120 { 9121 } else { 9120 };
    let broker = broker_with_ngrok_connector(
        &directory,
        &config,
        Arc::new(WorkerNgrokConnector {
            outcome: WorkerConnectOutcome::Tunnel {
                origin: "https://managed-duplicate.example",
                close_mode: LifecycleClose::Ok,
                wait_mode: WorkerWait::Pending,
            },
            connect_calls: Arc::clone(&connect_calls),
            wait_calls: Arc::clone(&wait_calls),
            close_calls: Arc::clone(&close_calls),
        }),
    );
    broker.remote.set_probe_hook(Arc::new(|| Box::pin(async { Ok(()) })));
    broker
        .remote
        .save_ngrok_auth_token("unit7e1-duplicate-token".to_owned())
        .unwrap();

    start_managed_ngrok_entry(&broker).await.unwrap();
    wait_for_remote_status(&broker.remote, Status::Ready).await;
    wait_for_counter(wait_calls.as_ref(), 1).await;

    let error = start_managed_ngrok_entry(&broker).await.unwrap_err();

    assert_eq!(error, "REMOTE_ACCESS_ALREADY_RUNNING");
    assert_eq!(connect_calls.load(Ordering::SeqCst), 1);
    assert_eq!(close_calls.load(Ordering::SeqCst), 0);
    assert!(broker.remote.snapshot().status == Status::Ready);
    assert!(broker.remote.active());

    broker.remote.stop().await.unwrap();
    assert_eq!(close_calls.load(Ordering::SeqCst), 1);
}

#[test]
fn prepare_ngrok_start_requires_local_token_without_side_effects() {
    let directory = tempfile::tempdir().unwrap();
    let connector_calls = Arc::new(AtomicUsize::new(0));
    let broker = broker_with_unused_ngrok_connector(
        &directory,
        &ManagerConfig::default(),
        Arc::clone(&connector_calls),
    );
    let snapshot_before = serde_json::to_value(broker.remote.snapshot()).unwrap();
    let config_before = broker.config();

    let error = match broker.remote.prepare_ngrok_start(&broker) {
        Ok(_) => panic!("ngrok start preparation unexpectedly succeeded"),
        Err(error) => error,
    };

    assert_eq!(error, "NGROK_AUTH_TOKEN_REQUIRED");
    assert_eq!(
        serde_json::to_value(broker.remote.snapshot()).unwrap(),
        snapshot_before
    );
    assert_eq!(broker.config(), config_before);
    assert_eq!(connector_calls.load(Ordering::SeqCst), 0);
}

#[test]
fn prepare_ngrok_start_builds_local_plan_without_side_effects() {
    let directory = tempfile::tempdir().unwrap();
    let connector_calls = Arc::new(AtomicUsize::new(0));
    let mut config = ManagerConfig::default();
    config.remote_access.mode = RemoteAccessMode::QuickTunnel;
    config.remote_access.self_hosted.provider = SelfHostedProvider::TailscaleFunnel;
    config.remote_access.self_hosted.public_origin = Some("residual-origin".into());
    let broker = broker_with_unused_ngrok_connector(
        &directory,
        &config,
        Arc::clone(&connector_calls),
    );
    let auth_token = "unit7b-local-test-token";
    broker
        .remote
        .save_ngrok_auth_token(auth_token.into())
        .unwrap();
    let snapshot_before = serde_json::to_value(broker.remote.snapshot()).unwrap();
    let config_before = broker.config();
    let mut expected_config = config_before.remote_access.clone();
    expected_config.mode = RemoteAccessMode::SelfHostedOAuth;
    expected_config.self_hosted.provider = SelfHostedProvider::Ngrok;
    expected_config.self_hosted.public_origin = None;

    let plan = match broker.remote.prepare_ngrok_start(&broker) {
        Ok(plan) => plan,
        Err(error) => panic!("ngrok start preparation failed: {error}"),
    };

    assert_eq!(plan.config, expected_config);
    assert_eq!(plan.broker_port, config_before.broker.port);
    assert_eq!(
        serde_json::to_value(broker.remote.snapshot()).unwrap(),
        snapshot_before
    );
    assert_eq!(broker.config(), config_before);
    assert_eq!(connector_calls.load(Ordering::SeqCst), 0);
    assert!(!snapshot_before.to_string().contains(auth_token));
}

#[test]
fn prepare_ngrok_start_rejects_port_conflict_without_side_effects() {
    let directory = tempfile::tempdir().unwrap();
    let connector_calls = Arc::new(AtomicUsize::new(0));
    let mut config = ManagerConfig::default();
    config.port = config.broker.port;
    let broker = broker_with_unused_ngrok_connector(
        &directory,
        &config,
        Arc::clone(&connector_calls),
    );
    broker
        .remote
        .save_ngrok_auth_token("unit7b-port-conflict-token".into())
        .unwrap();
    let snapshot_before = serde_json::to_value(broker.remote.snapshot()).unwrap();
    let config_before = broker.config();

    let error = match broker.remote.prepare_ngrok_start(&broker) {
        Ok(_) => panic!("ngrok start preparation unexpectedly succeeded"),
        Err(error) => error,
    };

    assert_eq!(error, "Broker 端口必须不同于 Serena 端口。");
    assert_eq!(
        serde_json::to_value(broker.remote.snapshot()).unwrap(),
        snapshot_before
    );
    assert_eq!(broker.config(), config_before);
    assert_eq!(connector_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn apply_ngrok_start_plan_starts_fail_closed_listener_without_connector() {
    let directory = tempfile::tempdir().unwrap();
    let connector_calls = Arc::new(AtomicUsize::new(0));
    let mut config = ManagerConfig::default();
    config.broker.port = available_loopback_port();
    config.port = if config.broker.port == 9121 { 9122 } else { 9121 };
    let broker = broker_with_unused_ngrok_connector(
        &directory,
        &config,
        Arc::clone(&connector_calls),
    );
    let auth_token = "unit7c-success-token";
    broker
        .remote
        .save_ngrok_auth_token(auth_token.into())
        .unwrap();
    let plan = match broker.remote.prepare_ngrok_start(&broker) {
        Ok(plan) => plan,
        Err(error) => panic!("ngrok start preparation failed: {error}"),
    };

    let management = broker.management.lock().await;
    let plan = match broker
        .remote
        .apply_ngrok_start_plan_locked(&broker, plan)
        .await
    {
        Ok(plan) => plan,
        Err(error) => panic!("ngrok start plan application failed: {error}"),
    };
    drop(management);

    let persisted = crate::config::load(&directory.path().join("config.json")).unwrap();
    assert_eq!(persisted.remote_access, plan.config);
    let snapshot = broker.remote.snapshot();
    assert_eq!(snapshot.mode, RemoteAccessMode::SelfHostedOAuth);
    assert_eq!(snapshot.config, plan.config);
    assert_eq!(snapshot.config.self_hosted.provider, SelfHostedProvider::Ngrok);
    assert!(snapshot.config.self_hosted.public_origin.is_none());
    assert!(broker.remote.policy() == McpAuthPolicy::EmbeddedOAuth);
    assert!(broker.remote.inner.lock().unwrap().oauth.is_none());
    assert!(snapshot.status == Status::Starting);
    assert!(!snapshot.active);
    assert!(broker.snapshot().await.running);
    assert_eq!(connector_calls.load(Ordering::SeqCst), 0);
    assert!(!serde_json::to_string(&snapshot).unwrap().contains(auth_token));

    broker.stop_listener().await;
}

#[tokio::test]
async fn apply_ngrok_start_plan_installs_fail_closed_state_before_persistence() {
    let directory = tempfile::tempdir().unwrap();
    let connector_calls = Arc::new(AtomicUsize::new(0));
    let mut config = ManagerConfig::default();
    config.broker.port = available_loopback_port();
    config.port = if config.broker.port == 9121 { 9122 } else { 9121 };
    let broker = broker_with_unused_ngrok_connector(
        &directory,
        &config,
        Arc::clone(&connector_calls),
    );
    broker
        .remote
        .save_ngrok_auth_token("unit7c-order-token".into())
        .unwrap();
    let plan = match broker.remote.prepare_ngrok_start(&broker) {
        Ok(plan) => plan,
        Err(error) => panic!("ngrok start preparation failed: {error}"),
    };
    let remote = Arc::downgrade(&broker.remote);
    *broker.supervisor.remote_save_hook.lock().unwrap() = Some(Arc::new(move || {
        let remote = remote.upgrade().ok_or("TEST_REMOTE_DROPPED")?;
        let inner = remote.inner.lock().unwrap();
        if inner.policy != McpAuthPolicy::EmbeddedOAuth
            || inner.oauth.is_some()
            || inner.status != Status::Starting
        {
            return Err("TEST_FAIL_CLOSED_STATE_NOT_INSTALLED".into());
        }
        Ok(())
    }));

    let management = broker.management.lock().await;
    let result = broker
        .remote
        .apply_ngrok_start_plan_locked(&broker, plan)
        .await;
    drop(management);
    match result {
        Ok(_) => {}
        Err(error) => panic!("ngrok start plan application failed: {error}"),
    }

    assert!(broker.snapshot().await.running);
    assert_eq!(connector_calls.load(Ordering::SeqCst), 0);
    broker.stop_listener().await;
}

#[tokio::test]
async fn apply_ngrok_start_plan_persistence_failure_restores_config_and_oauth() {
    let directory = tempfile::tempdir().unwrap();
    let connector_calls = Arc::new(AtomicUsize::new(0));
    let mut config = ManagerConfig::default();
    config.broker.port = available_loopback_port();
    config.port = if config.broker.port == 9121 { 9122 } else { 9121 };
    let broker = broker_with_unused_ngrok_connector(
        &directory,
        &config,
        Arc::clone(&connector_calls),
    );
    let auth_token = "unit7c-persistence-token";
    broker
        .remote
        .save_ngrok_auth_token(auth_token.into())
        .unwrap();
    let plan = match broker.remote.prepare_ngrok_start(&broker) {
        Ok(plan) => plan,
        Err(error) => panic!("ngrok start preparation failed: {error}"),
    };
    let previous_config = broker.config().remote_access;
    let previous_context = RemotePublicContext::new("https://previous.example").unwrap();
    let previous_instance_id = previous_context.instance_id.clone();
    {
        let mut inner = broker.remote.inner.lock().unwrap();
        inner.oauth = Some(Runtime::new(previous_context));
    }
    let config_file = directory.path().join("config.json");
    std::fs::remove_file(&config_file).unwrap();
    std::fs::create_dir(&config_file).unwrap();

    let management = broker.management.lock().await;
    let error = match broker
        .remote
        .apply_ngrok_start_plan_locked(&broker, plan)
        .await
    {
        Ok(_) => panic!("ngrok start plan application unexpectedly succeeded"),
        Err(error) => error,
    };
    drop(management);

    {
        let inner = broker.remote.inner.lock().unwrap();
        assert_eq!(inner.mode, previous_config.mode);
        assert_eq!(inner.config, previous_config);
        assert_eq!(
            inner.oauth.as_ref().unwrap().context.instance_id,
            previous_instance_id
        );
        assert!(inner.policy == McpAuthPolicy::EmbeddedOAuth);
        assert!(inner.status == Status::Error);
        assert_eq!(inner.error.as_deref(), Some(error.as_str()));
    }
    assert_eq!(broker.config().remote_access, previous_config);
    assert!(!broker.snapshot().await.running);
    assert_eq!(connector_calls.load(Ordering::SeqCst), 0);
    assert!(!error.contains(auth_token));
}

#[tokio::test]
async fn apply_ngrok_start_plan_bind_failure_rolls_back_config_and_runtime() {
    let directory = tempfile::tempdir().unwrap();
    let connector_calls = Arc::new(AtomicUsize::new(0));
    let occupied = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let mut config = ManagerConfig::default();
    config.broker.port = occupied.local_addr().unwrap().port();
    config.port = if config.broker.port == 9121 { 9122 } else { 9121 };
    let broker = broker_with_unused_ngrok_connector(
        &directory,
        &config,
        Arc::clone(&connector_calls),
    );
    let auth_token = "unit7c-bind-token";
    broker
        .remote
        .save_ngrok_auth_token(auth_token.into())
        .unwrap();
    let plan = match broker.remote.prepare_ngrok_start(&broker) {
        Ok(plan) => plan,
        Err(error) => panic!("ngrok start preparation failed: {error}"),
    };
    let previous = broker.config();

    let management = broker.management.lock().await;
    let error = match broker
        .remote
        .apply_ngrok_start_plan_locked(&broker, plan)
        .await
    {
        Ok(_) => panic!("ngrok start plan application unexpectedly succeeded"),
        Err(error) => error,
    };
    drop(management);

    assert!(!error.is_empty());
    assert!(!error.contains("REMOTE_CONFIG_ROLLBACK_FAILED"));
    assert_eq!(
        crate::config::load(&directory.path().join("config.json")).unwrap(),
        previous
    );
    assert_eq!(broker.config(), previous);
    {
        let inner = broker.remote.inner.lock().unwrap();
        assert_eq!(inner.mode, previous.remote_access.mode);
        assert_eq!(inner.config, previous.remote_access);
        assert!(inner.policy == McpAuthPolicy::Passthrough);
        assert!(inner.oauth.is_none());
        assert!(inner.status == Status::Stopped);
        assert!(inner.error.is_none());
    }
    assert!(!broker.snapshot().await.running);
    assert_eq!(connector_calls.load(Ordering::SeqCst), 0);
    assert!(!error.contains(auth_token));
}

#[tokio::test]
async fn injected_probe_hook_completes_without_network() {
    let directory = tempfile::tempdir().unwrap();
    let config_file = directory.path().join("config.json");
    let connector_calls = Arc::new(AtomicUsize::new(0));
    let remote = Remote::from_config_with_ngrok_connector(
        &RemoteAccessConfig::default(),
        directory.path().join("oauth-state.json"),
        config_file,
        Arc::new(UnusedNgrokConnector {
            calls: Arc::clone(&connector_calls),
        }),
    );
    let probe_calls = Arc::new(AtomicUsize::new(0));
    let hook_calls = Arc::clone(&probe_calls);
    remote.set_probe_hook(Arc::new(move || {
        hook_calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(()) })
    }));
    {
        let mut inner = remote.inner.lock().unwrap();
        inner.policy = McpAuthPolicy::EmbeddedOAuth;
        inner.oauth = Some(Runtime::new(
            RemotePublicContext::new("https://fixture.example").unwrap(),
        ));
        inner.cancel = Some(CancellationToken::new());
        inner.status = Status::Verifying;
    }

    remote.probe().await.unwrap();

    let snapshot = remote.snapshot();
    assert_eq!(probe_calls.load(Ordering::SeqCst), 1);
    assert_eq!(connector_calls.load(Ordering::SeqCst), 0);
    assert!(snapshot.status == Status::Ready);
    assert!(snapshot.last_error.is_none());
}

#[test]
fn persisted_custom_https_restores_embedded_oauth_runtime() {
    let directory = tempfile::tempdir().unwrap();
    let config = RemoteAccessConfig {
        mode: RemoteAccessMode::SelfHostedOAuth,
        self_hosted: SelfHostedConfig {
            provider: SelfHostedProvider::CustomHttps,
            public_origin: Some("https://custom.example.com".into()),
        },
        ..Default::default()
    };

    let remote = remote_from_config(&config, &directory.path().join("config.json"));
    let snapshot = remote.snapshot();

    assert_eq!(remote.policy(), McpAuthPolicy::EmbeddedOAuth);
    assert!(snapshot.status == Status::Stopped);
    assert!(snapshot.last_error.is_none());
    assert_eq!(
        remote.public_origin().as_deref(),
        Some("https://custom.example.com")
    );
    assert_eq!(
        snapshot.config.self_hosted.provider,
        SelfHostedProvider::CustomHttps
    );
    assert!(remote.inner.lock().unwrap().oauth.is_some());
}

#[test]
fn persisted_ngrok_without_origin_stays_stopped_without_oauth_runtime() {
    let directory = tempfile::tempdir().unwrap();
    let config = RemoteAccessConfig {
        mode: RemoteAccessMode::SelfHostedOAuth,
        self_hosted: SelfHostedConfig {
            provider: SelfHostedProvider::Ngrok,
            public_origin: None,
        },
        ..Default::default()
    };

    let remote = remote_from_config(&config, &directory.path().join("config.json"));
    let snapshot = remote.snapshot();

    assert_eq!(remote.policy(), McpAuthPolicy::EmbeddedOAuth);
    assert!(snapshot.status == Status::Stopped);
    assert!(snapshot.last_error.is_none());
    assert_eq!(
        snapshot.config.self_hosted.provider,
        SelfHostedProvider::Ngrok
    );
    assert!(snapshot.config.self_hosted.public_origin.is_none());
    assert!(remote.inner.lock().unwrap().oauth.is_none());
}

#[test]
fn persisted_tailscale_ignores_invalid_residual_origin() {
    let directory = tempfile::tempdir().unwrap();
    let config = RemoteAccessConfig {
        mode: RemoteAccessMode::SelfHostedOAuth,
        self_hosted: SelfHostedConfig {
            provider: SelfHostedProvider::TailscaleFunnel,
            public_origin: Some("not-an-origin".into()),
        },
        ..Default::default()
    };

    let remote = remote_from_config(&config, &directory.path().join("config.json"));
    let snapshot = remote.snapshot();

    assert_eq!(remote.policy(), McpAuthPolicy::EmbeddedOAuth);
    assert!(snapshot.status == Status::Stopped);
    assert!(snapshot.last_error.is_none());
    assert_eq!(
        snapshot.config.self_hosted.provider,
        SelfHostedProvider::TailscaleFunnel
    );
    assert_eq!(
        snapshot.config.self_hosted.public_origin.as_deref(),
        Some("not-an-origin")
    );
    assert!(remote.inner.lock().unwrap().oauth.is_none());
}

#[test]
fn ngrok_auth_configuration_updates_snapshot_without_exposing_value() {
    let directory = tempfile::tempdir().unwrap();
    let remote = remote_with_config_file(&directory.path().join("config.json"));
    let auth_token = "test-secret-ngrok-token";

    assert!(!remote.snapshot().ngrok_auth_configured);
    remote
        .save_ngrok_auth_token(format!("  {auth_token}  "))
        .unwrap();
    assert!(remote.snapshot().ngrok_auth_configured);
    let snapshot = serde_json::to_string(&remote.snapshot()).unwrap();
    assert!(snapshot.contains(r#""ngrokAuthConfigured":true"#));
    assert!(!snapshot.contains(auth_token));

    remote.clear_ngrok_auth_token().unwrap();
    assert!(!remote.snapshot().ngrok_auth_configured);
}

#[test]
fn ngrok_auth_configuration_is_restored_after_remote_rebuild() {
    let directory = tempfile::tempdir().unwrap();
    let config_file = directory.path().join("config.json");
    remote_with_config_file(&config_file)
        .save_ngrok_auth_token("persistent-test-token".into())
        .unwrap();

    let rebuilt = remote_with_config_file(&config_file);

    assert!(rebuilt.snapshot().ngrok_auth_configured);
}

#[test]
fn ngrok_blank_auth_token_is_rejected_without_configuring_remote() {
    let directory = tempfile::tempdir().unwrap();
    let remote = remote_with_config_file(&directory.path().join("config.json"));

    assert_eq!(
        remote.save_ngrok_auth_token(" \t\r\n ".into()).unwrap_err(),
        "NGROK_AUTH_TOKEN_REQUIRED"
    );
    assert!(!remote.snapshot().ngrok_auth_configured);
}

#[test]
fn ngrok_database_failure_does_not_block_remote_construction_or_leak_input() {
    let directory = tempfile::tempdir().unwrap();
    let blocked_directory = directory.path().join("not-a-directory");
    std::fs::write(&blocked_directory, "fixture").unwrap();
    let remote = remote_with_config_file(&blocked_directory.join("config.json"));
    let auth_token = "must-not-appear";

    assert!(!remote.snapshot().ngrok_auth_configured);
    let save_error = remote.save_ngrok_auth_token(auth_token.into()).unwrap_err();
    assert_eq!(save_error, "REMOTE_ACCESS_DATABASE_DIRECTORY_CREATE_FAILED");
    assert!(!save_error.contains(auth_token));
    assert_eq!(
        remote.clear_ngrok_auth_token().unwrap_err(),
        "REMOTE_ACCESS_DATABASE_DIRECTORY_CREATE_FAILED"
    );
    assert!(!remote.snapshot().ngrok_auth_configured);
}

#[tokio::test]
async fn lan_configuration_allows_start_without_changing_listener_scope() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let paths = AppPaths {
        runtime_directory: root.join("runtime"),
        config_file: root.join("config.json"),
        log_directory: root.join("logs"),
        app_log: root.join("logs/app.log"),
        serena_log: root.join("logs/serena.log"),
    };
    let mut config = ManagerConfig::default();
    config.broker.allow_lan = true;
    let reserved = std::net::TcpListener::bind("0.0.0.0:0").unwrap();
    config.broker.port = reserved.local_addr().unwrap().port();
    drop(reserved);
    crate::config::save(&paths.config_file, &config).unwrap();
    let broker = Arc::new(Broker::new(Arc::new(SupervisorState::new(paths).unwrap())));
    broker.remote.start(broker.clone()).await.unwrap();
    assert!(broker.config().broker.allow_lan);
    assert_eq!(broker.remote.policy(), McpAuthPolicy::EmbeddedOAuth);
    // Cancel before the spawned tunnel worker runs: this test needs no public tunnel.
    broker.remote.stop().await.unwrap();
    assert!(broker.config().broker.allow_lan);
    assert_eq!(broker.remote.policy(), McpAuthPolicy::EmbeddedOAuth);
    broker.stop().await.unwrap();
}

#[tokio::test]
async fn switching_between_modes_stops_old_runtime_and_applies_each_target() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let paths = AppPaths {
        runtime_directory: root.join("runtime"),
        config_file: root.join("config.json"),
        log_directory: root.join("logs"),
        app_log: root.join("logs/app.log"),
        serena_log: root.join("logs/serena.log"),
    };
    let mut config = ManagerConfig::default();
    let reserved = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    config.broker.port = reserved.local_addr().unwrap().port();
    drop(reserved);
    config.remote_access.mode = RemoteAccessMode::SelfHostedOAuth;
    config.remote_access.self_hosted.provider = SelfHostedProvider::Ngrok;
    config.remote_access.self_hosted.public_origin = Some("https://self.example.com".into());
    crate::config::save(&paths.config_file, &config).unwrap();
    let broker = Arc::new(Broker::new(Arc::new(SupervisorState::new(paths).unwrap())));
    let previous_cancel = CancellationToken::new();
    {
        let mut inner = broker.remote.inner.lock().unwrap();
        inner.cancel = Some(previous_cancel.clone());
        inner.status = Status::Ready;
    }
    let task_cancel = previous_cancel.clone();
    *broker.remote.task.lock().await = Some(tokio::spawn(async move {
        task_cancel.cancelled().await;
    }));

    broker
        .remote
        .switch_mode(broker.clone(), None)
        .await
        .unwrap();

    assert!(previous_cancel.is_cancelled());
    let state = broker.remote.snapshot();
    assert_eq!(state.mode, RemoteAccessMode::QuickTunnel);
    assert_eq!(state.config.mode, RemoteAccessMode::QuickTunnel);
    assert!(state.active);
    assert_eq!(broker.remote.policy(), McpAuthPolicy::EmbeddedOAuth);

    let probe_guard = broker.remote.probe_lock.lock().await;
    broker
        .remote
        .switch_mode(
            broker.clone(),
            Some(RemotePublicContext::new("https://new.example.com").unwrap()),
        )
        .await
        .unwrap();
    let state = broker.remote.snapshot();
    assert_eq!(state.mode, RemoteAccessMode::SelfHostedOAuth);
    assert_eq!(state.config.mode, RemoteAccessMode::SelfHostedOAuth);
    assert_eq!(
        state.config.self_hosted.provider,
        SelfHostedProvider::CustomHttps
    );
    assert_eq!(
        state.config.self_hosted.public_origin.as_deref(),
        Some("https://new.example.com")
    );
    assert_eq!(
        broker.config().remote_access.self_hosted.provider,
        SelfHostedProvider::CustomHttps
    );
    assert!(state.active);
    assert_eq!(broker.remote.policy(), McpAuthPolicy::EmbeddedOAuth);

    broker
        .remote
        .apply_mcp_only(&broker, SecurityDeclaration::ExternalAuth, false, None)
        .await
        .unwrap();
    let state = broker.remote.snapshot();
    assert_eq!(state.mode, RemoteAccessMode::McpOnly);
    assert_eq!(state.config.mode, RemoteAccessMode::McpOnly);
    assert!(!state.active);
    assert_eq!(broker.remote.policy(), McpAuthPolicy::Passthrough);
    drop(probe_guard);
    broker.stop().await.unwrap();
}

#[tokio::test]
#[ignore = "creates a real temporary Quick Tunnel with an empty workspace; run explicitly for network smoke"]
async fn official_quick_tunnel_start_probe_stop() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let paths = AppPaths {
        runtime_directory: root.join("runtime"),
        config_file: root.join("config.json"),
        log_directory: root.join("logs"),
        app_log: root.join("logs/app.log"),
        serena_log: root.join("logs/serena.log"),
    };
    let mut config = ManagerConfig::default();
    let reserved = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    config.broker.port = reserved.local_addr().unwrap().port();
    drop(reserved);
    crate::config::save(&paths.config_file, &config).unwrap();
    let broker = Arc::new(Broker::new(Arc::new(SupervisorState::new(paths).unwrap())));
    let _upstream = crate::serena::remote_fixture::attach(broker.supervisor.clone()).await;
    broker.remote.start(broker.clone()).await.unwrap();
    let result = tokio::time::timeout(Duration::from_secs(190), async {
        loop {
            let state = broker.remote.snapshot();
            match state.status {
                Status::Ready => break,
                Status::Error | Status::Disconnected => {
                    return Err(state.last_error.unwrap_or_default());
                }
                _ => tokio::time::sleep(Duration::from_millis(250)).await,
            }
        }
        assert!(broker.remote.snapshot().public_context.is_some());
        assert_eq!(broker.remote.policy(), McpAuthPolicy::EmbeddedOAuth);
        assert!(broker.remote.start(broker.clone()).await.is_err());
        broker.remote.probe().await?;
        println!("Quick Tunnel public stages PASS: origin, oauth_metadata, unauthorized_mcp, resource_metadata, initialize, tools_list");
        Ok::<(), String>(())
    })
    .await;
    // Cleanup runs before assertions, including network failure/timeout.
    broker.remote.stop().await.unwrap();
    assert!(!broker.remote.active());
    assert!(broker.remote.snapshot().public_context.is_none());
    assert!(broker.remote.inner.lock().unwrap().oauth.is_none());
    assert!(broker.remote.pending_child.lock().await.is_none());
    assert_eq!(broker.remote.policy(), McpAuthPolicy::EmbeddedOAuth);
    broker.stop().await.unwrap();
    println!("Quick Tunnel cleanup PASS: stopped, OAuth revoked, owned child reaped");
    println!("{}", broker.log_snapshot().join("\n"));
    result
        .expect("network startup timed out")
        .expect("public Quick Tunnel smoke failed");
}

#[tokio::test]
async fn stopping_retained_child_revokes_context_and_reaps_before_passthrough() {
    let remote = Remote::default();
    #[cfg(windows)]
    let mut command =
        crate::mcp::process::command(crate::serena::find_executable("ping.exe").unwrap());
    #[cfg(windows)]
    command.args(["-n", "30", "127.0.0.1"]);
    #[cfg(not(windows))]
    let mut command = crate::mcp::process::command("sleep");
    #[cfg(not(windows))]
    command.arg("30");
    *remote.pending_child.lock().await =
        Some(super::super::process::ManagedChild::spawn(&mut command).unwrap());
    {
        let mut inner = remote.inner.lock().unwrap();
        inner.cancel = Some(CancellationToken::new());
        inner.policy = McpAuthPolicy::EmbeddedOAuth;
        inner.status = Status::Error;
        inner.oauth = Some(Runtime::new(
            RemotePublicContext::new("https://old.trycloudflare.com").unwrap(),
        ));
    }
    remote.cancel();
    assert!(remote.inner.lock().unwrap().oauth.is_none());
    assert_eq!(remote.policy(), McpAuthPolicy::EmbeddedOAuth);
    remote.stop().await.unwrap();
    assert!(remote.pending_child.lock().await.is_none());
    assert!(!remote.active());
    assert_eq!(remote.policy(), McpAuthPolicy::EmbeddedOAuth);
    remote.stop().await.unwrap();
}

// The production config remains HTTPS; only the local test wire endpoint is HTTP.
async fn start_local(broker: &Arc<Broker>, context: RemotePublicContext) {
    let guard = broker.remote.probe_lock.lock().await;
    broker
        .remote
        .start_mode(
            broker.clone(),
            Some(RemotePublicContext::new("https://fixture.example").unwrap()),
        )
        .await
        .unwrap();
    broker
        .remote
        .inner
        .lock()
        .unwrap()
        .oauth
        .as_mut()
        .unwrap()
        .context = context;
    drop(guard);
}
