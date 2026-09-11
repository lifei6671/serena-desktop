use super::*;
use crate::{
    config::{AppPaths, ManagerConfig},
    serena::SupervisorState,
};
use std::time::Duration;

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
        state.config.self_hosted.public_origin.as_deref(),
        Some("https://new.example.com")
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
