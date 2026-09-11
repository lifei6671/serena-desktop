use super::*;
use crate::{
    config::{self, AppPaths, ManagerConfig},
    serena::SupervisorState,
};
use serde_json::{Value, json};

fn paths(root: &std::path::Path) -> AppPaths {
    AppPaths {
        runtime_directory: root.join("runtime"),
        config_file: root.join("config.json"),
        log_directory: root.join("logs"),
        app_log: root.join("logs/app.log"),
        serena_log: root.join("logs/serena.log"),
    }
}
fn configuration() -> ManagerConfig {
    let mut config = ManagerConfig::default();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    config.broker.port = listener.local_addr().unwrap().port();
    config
}
fn broker(paths: AppPaths) -> Arc<Broker> {
    Arc::new(Broker::new(Arc::new(SupervisorState::new(paths).unwrap())))
}
async fn post(
    broker: &Broker,
    method: &str,
    token: Option<&str>,
    host: &str,
    origin: Option<&str>,
) -> reqwest::Response {
    let mut body = json!({"jsonrpc":"2.0","id":1,"method":method});
    if method == "initialize" {
        body["params"] = json!({"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"boundary-test","version":"1"}});
    }
    let mut request = reqwest::Client::new()
        .post(format!(
            "http://127.0.0.1:{}/mcp",
            broker.config().broker.port
        ))
        .header("host", host)
        .header("accept", "application/json, text/event-stream")
        .header("MCP-Protocol-Version", "2025-11-25")
        .header("Mcp-Method", method)
        .json(&body);
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    if let Some(origin) = origin {
        request = request.header("origin", origin);
    }
    request.send().await.unwrap()
}

#[tokio::test]
async fn persisted_self_hosted_recreates_broker_and_preserves_unexpired_authorization() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(directory.path());
    let mut config = configuration();
    config.broker.enabled = true;
    config.remote_access.mode = RemoteAccessMode::SelfHostedOAuth;
    config.remote_access.self_hosted.public_origin = Some("https://mcp.example.com".into());
    config::save(&paths.config_file, &config).unwrap();
    let a = broker(paths.clone());
    assert_eq!(a.remote.policy(), McpAuthPolicy::EmbeddedOAuth);
    a.startup().await.unwrap();
    // Startup creates a fresh runtime before the spawned public probe runs.
    let old_token = a
        .remote
        .inner
        .lock()
        .unwrap()
        .oauth
        .as_mut()
        .unwrap()
        .test_access_token();
    assert_eq!(
        post(&a, "initialize", Some(&old_token), "mcp.example.com", None)
            .await
            .status(),
        200
    );
    a.shutdown().await.unwrap();
    let weak = Arc::downgrade(&a);
    drop(a);
    assert!(
        weak.upgrade().is_none(),
        "Broker A must actually be destroyed"
    );
    let loaded = config::load(&paths.config_file).unwrap();
    assert_eq!(loaded.remote_access, config.remote_access);
    let saved = std::fs::read_to_string(&paths.config_file).unwrap();
    assert!(!saved.contains(&old_token));
    assert!(!saved.contains("instanceId"));
    assert!(!saved.contains("access_token"));
    let b = broker(paths.clone());
    assert_eq!(b.remote.policy(), McpAuthPolicy::EmbeddedOAuth);
    b.startup().await.unwrap();
    // First immediately reachable request, without waiting for OAuth probe/readiness.
    assert_eq!(
        post(&b, "initialize", None, "mcp.example.com", None)
            .await
            .status(),
        401
    );
    assert_eq!(
        post(&b, "initialize", Some(&old_token), "mcp.example.com", None)
            .await
            .status(),
        200
    );
    b.stop().await.unwrap();
    drop(b);
    // Explicit Stop is a revocation and must survive a subsequent application restart.
    let c = broker(paths);
    c.startup().await.unwrap();
    assert_eq!(
        post(&c, "initialize", Some(&old_token), "mcp.example.com", None)
            .await
            .status(),
        401
    );
    c.stop().await.unwrap();
}

#[tokio::test]
async fn corrupt_persisted_oauth_never_becomes_anonymous() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(directory.path());
    let mut config = configuration();
    config.broker.enabled = true;
    config.remote_access.mode = RemoteAccessMode::SelfHostedOAuth;
    config.remote_access.self_hosted.public_origin = Some("https://mcp.example.com".into());
    config::save(&paths.config_file, &config).unwrap();
    std::fs::create_dir_all(&paths.runtime_directory).unwrap();
    let store = paths.runtime_directory.join("oauth-state.json");
    std::fs::write(&store, b"corrupt oauth state").unwrap();
    let broker = broker(paths);
    assert_eq!(broker.remote.policy(), McpAuthPolicy::EmbeddedOAuth);
    assert!(broker.remote.snapshot().status == Status::Error);
    broker.startup().await.unwrap();
    assert_eq!(
        post(&broker, "initialize", None, "127.0.0.1", None)
            .await
            .status(),
        401
    );
    assert_eq!(std::fs::read(&store).unwrap(), b"corrupt oauth state");
    broker.stop().await.unwrap();
    assert!(!store.exists());
}

#[tokio::test]
async fn self_hosted_first_listener_request_has_metadata_and_complete_challenge() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(directory.path());
    let mut config = configuration();
    config.broker.allow_lan = true;
    config::save(&paths.config_file, &config).unwrap();
    let broker = broker(paths);
    // Keep the public probe from making any requests; it must not establish the Runtime.
    let probe = broker.remote.probe_lock.lock().await;
    let context = RemotePublicContext::new("https://mcp.example.com:8443").unwrap();
    broker
        .remote
        .start_mode(broker.clone(), Some(context.clone()))
        .await
        .unwrap();
    // Synchronous assertion before yielding to either the listener or worker task.
    assert_eq!(
        broker
            .remote
            .inner
            .lock()
            .unwrap()
            .oauth
            .as_ref()
            .unwrap()
            .context
            .instance_id,
        context.instance_id
    );
    let response = reqwest::Client::new()
        .get(format!(
            "http://127.0.0.1:{}/.well-known/oauth-authorization-server",
            config.broker.port
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(
        response.json::<Value>().await.unwrap()["issuer"],
        context.public_origin
    );
    let response = post(&broker, "initialize", None, "mcp.example.com:8443", None).await;
    assert_eq!(response.status(), 401);
    assert!(response.headers()["www-authenticate"].to_str().unwrap().contains("resource_metadata=\"https://mcp.example.com:8443/.well-known/oauth-protected-resource/mcp\""));
    broker.stop().await.unwrap();
    drop(probe);
}

#[tokio::test]
async fn self_hosted_bind_failure_restores_previous_runtime_and_config() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(directory.path());
    let occupied = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let mut config = configuration();
    config.broker.port = occupied.local_addr().unwrap().port();
    config.remote_access.mode = RemoteAccessMode::SelfHostedOAuth;
    config.remote_access.self_hosted.public_origin = Some("https://previous.example.com".into());
    config::save(&paths.config_file, &config).unwrap();
    let broker = broker(paths.clone());
    let old_id = broker
        .remote
        .inner
        .lock()
        .unwrap()
        .oauth
        .as_ref()
        .unwrap()
        .context
        .instance_id
        .clone();
    assert!(
        broker
            .remote
            .start_mode(
                broker.clone(),
                Some(RemotePublicContext::new("https://new.example.com").unwrap())
            )
            .await
            .is_err()
    );
    assert_eq!(broker.remote.policy(), McpAuthPolicy::EmbeddedOAuth);
    assert_eq!(
        broker
            .remote
            .inner
            .lock()
            .unwrap()
            .oauth
            .as_ref()
            .unwrap()
            .context
            .instance_id,
        old_id
    );
    assert!(!broker.remote.active());
    assert_eq!(config::load(&paths.config_file).unwrap(), config);
}

#[tokio::test]
async fn self_hosted_persistence_failure_retains_protection_without_listening() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(directory.path());
    let config = configuration();
    config::save(&paths.config_file, &config).unwrap();
    let broker = broker(paths.clone());
    // A directory at the isolated config destination makes replacement fail.
    std::fs::remove_file(&paths.config_file).unwrap();
    std::fs::create_dir(&paths.config_file).unwrap();
    assert!(
        broker
            .remote
            .start_mode(
                broker.clone(),
                Some(RemotePublicContext::new("https://mcp.example.com").unwrap())
            )
            .await
            .is_err()
    );
    assert_eq!(broker.remote.policy(), McpAuthPolicy::EmbeddedOAuth);
    assert!(broker.remote.inner.lock().unwrap().oauth.is_none());
    assert!(broker.remote.snapshot().status == Status::Error);
    assert!(!broker.remote.active());
    assert!(!broker.snapshot().await.running);
    assert_eq!(broker.config(), config);
}

#[tokio::test]
async fn incomplete_self_hosted_restart_stays_fail_closed() {
    for origin in [
        None,
        Some("http://invalid.example"),
        Some("https://mcp.example.com/path"),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(directory.path());
        let mut config = configuration();
        config.broker.enabled = true;
        config.remote_access.mode = RemoteAccessMode::SelfHostedOAuth;
        config.remote_access.self_hosted.public_origin = origin.map(str::to_owned);
        if origin.is_some() {
            // Corrupt historical settings must be rejected, never silently repaired.
            let raw = serde_json::to_vec(&config).unwrap();
            std::fs::write(&paths.config_file, &raw).unwrap();
            assert!(SupervisorState::new(paths.clone()).is_err());
            assert_eq!(std::fs::read(&paths.config_file).unwrap(), raw);
            continue;
        }
        config::save(&paths.config_file, &config).unwrap();
        let broker = broker(paths);
        broker.startup().await.unwrap();
        assert_eq!(broker.remote.policy(), McpAuthPolicy::EmbeddedOAuth);
        assert!(broker.remote.snapshot().status == Status::Error);
        assert_eq!(
            post(&broker, "initialize", None, "127.0.0.1", None)
                .await
                .status(),
            401
        );
        broker.stop().await.unwrap();
    }
}

#[tokio::test]
async fn all_modes_share_json_transport_and_reject_cross_site_origin() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(directory.path());
    config::save(&paths.config_file, &configuration()).unwrap();
    let broker = broker(paths);
    let _upstream = crate::serena::remote_fixture::attach(broker.supervisor.clone()).await;
    broker.start().await.unwrap();
    for mode in [
        RemoteAccessMode::McpOnly,
        RemoteAccessMode::QuickTunnel,
        RemoteAccessMode::SelfHostedOAuth,
    ] {
        let token = {
            let mut inner = broker.remote.inner.lock().unwrap();
            inner.mode = mode;
            if mode == RemoteAccessMode::McpOnly {
                inner.policy = McpAuthPolicy::Passthrough;
                None
            } else {
                inner.policy = McpAuthPolicy::EmbeddedOAuth;
                let mut oauth =
                    Runtime::new(RemotePublicContext::new("https://mcp.example.com").unwrap());
                let token = oauth.test_access_token();
                inner.oauth = Some(oauth);
                Some(token)
            }
        };
        let host = if mode == RemoteAccessMode::SelfHostedOAuth {
            "mcp.example.com"
        } else {
            "127.0.0.1"
        };
        broker.clear_logs();
        let secret = "PRIVATE_TOOL_ARGUMENT_9291";
        let mut call = reqwest::Client::new().post(format!("http://127.0.0.1:{}/mcp", broker.config().broker.port))
            .header("accept", "application/json, text/event-stream")
            .header("MCP-Protocol-Version", "2025-11-25").header("Mcp-Method", "tools/call")
            .json(&json!({"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"source_search_pattern","arguments":{"substring_pattern":secret}}}));
        if let Some(token) = &token {
            call = call.bearer_auth(token);
        }
        assert_eq!(call.send().await.unwrap().status(), 200);
        let logs = broker.log_snapshot().join("\n");
        assert!(logs.contains("source_search_pattern"));
        assert!(!logs.contains(secret));
        if let Some(token) = &token {
            assert!(!logs.contains(token));
        }
        for method in ["initialize", "tools/list"] {
            let response = post(&broker, method, token.as_deref(), host, None).await;
            assert_eq!(response.status(), 200, "{mode:?} {method}");
            assert!(
                response.headers()["content-type"]
                    .to_str()
                    .unwrap()
                    .starts_with("application/json")
            );
            assert!(response.headers().get("mcp-session-id").is_none());
            let value: Value = response.json().await.unwrap();
            assert!(value.get("result").is_some(), "{mode:?} {method}: {value}");
        }
        assert_eq!(
            post(
                &broker,
                "tools/list",
                token.as_deref(),
                host,
                Some("https://evil.example")
            )
            .await
            .status(),
            403
        );
        assert_eq!(
            post(
                &broker,
                "tools/list",
                token.as_deref(),
                "evil.example",
                None
            )
            .await
            .status(),
            403
        );
        if mode == RemoteAccessMode::SelfHostedOAuth {
            assert_eq!(
                post(
                    &broker,
                    "tools/list",
                    token.as_deref(),
                    host,
                    Some("https://mcp.example.com")
                )
                .await
                .status(),
                200
            );
            assert_eq!(
                post(
                    &broker,
                    "tools/list",
                    token.as_deref(),
                    host,
                    Some("https://mcp.example.com:444")
                )
                .await
                .status(),
                403
            );
            assert_eq!(
                post(&broker, "tools/list", token.as_deref(), host, Some("null"))
                    .await
                    .status(),
                403
            );
            let response = reqwest::Client::new()
                .post(format!(
                    "http://127.0.0.1:{}/mcp",
                    broker.config().broker.port
                ))
                .bearer_auth(token.as_deref().unwrap())
                .header("host", host)
                .header("origin", "https://mcp.example.com")
                .header("origin", "https://evil.example")
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), 403);
        }
    }
    broker.stop().await.unwrap();
}

#[tokio::test]
async fn listener_bind_failure_rolls_back_config_and_policy_before_returning() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(directory.path());
    let mut config = configuration();
    let occupied = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    config.broker.port = occupied.local_addr().unwrap().port();
    config::save(&paths.config_file, &config).unwrap();
    let broker = broker(paths.clone());
    assert!(broker.remote.start(broker.clone()).await.is_err());
    assert_eq!(config::load(&paths.config_file).unwrap(), config);
    assert_eq!(broker.remote.policy(), McpAuthPolicy::Passthrough);
    assert!(!broker.snapshot().await.running);
}

#[tokio::test]
async fn quick_first_listener_is_protected_and_runtime_demand_preserves_preferences() {
    for enabled in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(directory.path());
        let mut config = configuration();
        config.broker.enabled = enabled;
        config.broker.allow_lan = true;
        config::save(&paths.config_file, &config).unwrap();
        let broker = broker(paths.clone());
        // Single-thread runtime: start installs protection and listener before worker runs.
        broker.remote.start(broker.clone()).await.unwrap();
        broker.remote.cancel(); // No public tunnel is needed for the listener boundary test.
        assert_eq!(broker.remote.policy(), McpAuthPolicy::EmbeddedOAuth);
        assert_eq!(
            post(&broker, "initialize", None, "127.0.0.1", None)
                .await
                .status(),
            401
        );
        assert_eq!(
            config::load(&paths.config_file).unwrap().broker,
            config.broker
        );
        let before = broker.config();
        let mut collision = before.clone();
        collision.port = collision.broker.port;
        assert!(
            crate::commands::save_config_impl(&broker, collision)
                .await
                .is_err()
        );
        assert_eq!(broker.config(), before);
        broker.remote.stop().await.unwrap();
        assert_eq!(broker.snapshot().await.running, enabled);
        assert_eq!(
            config::load(&paths.config_file).unwrap().broker,
            config.broker
        );
        assert_eq!(broker.remote.snapshot().mode, RemoteAccessMode::QuickTunnel);
        broker.stop().await.unwrap();
    }
}

#[tokio::test]
async fn mcp_only_declaration_requires_explicit_none_acceptance_and_persists() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(directory.path());
    config::save(&paths.config_file, &configuration()).unwrap();
    let broker = broker(paths.clone());
    assert!(
        broker
            .remote
            .apply_mcp_only(&broker, SecurityDeclaration::None, false, None)
            .await
            .is_err()
    );
    for (declaration, accepted) in [
        (SecurityDeclaration::ExternalAuth, false),
        (SecurityDeclaration::None, true),
    ] {
        broker
            .remote
            .apply_mcp_only(&broker, declaration, accepted, None)
            .await
            .unwrap();
        assert_eq!(
            config::load(&paths.config_file)
                .unwrap()
                .remote_access
                .mcp_only
                .security_declaration,
            declaration
        );
        assert_eq!(broker.remote.policy(), McpAuthPolicy::Passthrough);
    }
}

#[test]
fn disconnected_quick_tunnel_keeps_configured_mode_without_context_or_restart() {
    let directory = tempfile::tempdir().unwrap();
    let remote = Remote::from_config(
        &RemoteAccessConfig {
            mode: RemoteAccessMode::QuickTunnel,
            ..Default::default()
        },
        directory.path().join("oauth-state.json"),
    );
    {
        let mut inner = remote.inner.lock().unwrap();
        inner.status = Status::Disconnected;
        inner.cancel = None;
        inner.oauth = None;
    }
    let snapshot = remote.snapshot();
    assert_eq!(snapshot.mode, RemoteAccessMode::QuickTunnel);
    assert!(snapshot.status == Status::Disconnected);
    assert!(!snapshot.active);
    assert!(snapshot.public_context.is_none());
    assert!(remote.task.try_lock().unwrap().is_none());
}

#[tokio::test]
async fn metadata_and_401_without_working_handler_cannot_be_ready() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(directory.path());
    let config = configuration();
    config::save(&paths.config_file, &config).unwrap();
    let broker = broker(paths);
    let origin = format!("http://127.0.0.1:{}", config.broker.port);
    let probe_guard = broker.remote.probe_lock.lock().await;
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
        .context = RemotePublicContext {
        public_origin: origin.clone(),
        mcp_resource: format!("{origin}/mcp"),
        instance_id: "probe-failure".into(),
    };
    drop(probe_guard);
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while broker.remote.snapshot().status != Status::Error {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(broker.remote.snapshot().public_context.is_none());
    assert_eq!(broker.remote.policy(), McpAuthPolicy::EmbeddedOAuth);
    assert!(broker.remote.probe().await.is_err());
    broker.stop().await.unwrap();
}

#[tokio::test]
async fn mcp_only_public_host_survives_reload_without_enabling_oauth() {
    for origin in ["https://mcp.example.com", "https://mcp.example.com:8443"] {
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(directory.path());
        let mut config = configuration();
        config.broker.enabled = true;
        config::save(&paths.config_file, &config).unwrap();
        let a = broker(paths.clone());
        a.remote
            .apply_mcp_only(&a, SecurityDeclaration::ExternalAuth, false, Some(origin))
            .await
            .unwrap();
        a.startup().await.unwrap();
        a.stop().await.unwrap();
        let weak = Arc::downgrade(&a);
        drop(a);
        assert!(weak.upgrade().is_none());
        let loaded = config::load(&paths.config_file).unwrap();
        assert_eq!(loaded.remote_access.mode, RemoteAccessMode::McpOnly);
        assert_eq!(
            loaded.remote_access.mcp_only.security_declaration,
            SecurityDeclaration::ExternalAuth
        );
        assert_eq!(
            loaded.remote_access.mcp_only.public_origin.as_deref(),
            Some(origin)
        );
        let b = broker(paths);
        let _upstream = crate::serena::remote_fixture::attach(b.supervisor.clone()).await;
        b.startup().await.unwrap();
        assert_eq!(b.remote.policy(), McpAuthPolicy::Passthrough);
        assert!(b.remote.inner.lock().unwrap().oauth.is_none());
        assert!(b.remote.snapshot().public_context.is_none());
        for method in ["initialize", "tools/list"] {
            for request_origin in [None, Some(origin)] {
                let response = post(
                    &b,
                    method,
                    None,
                    origin.trim_start_matches("https://"),
                    request_origin,
                )
                .await;
                assert_eq!(response.status(), 200);
                assert!(
                    response.headers()["content-type"]
                        .to_str()
                        .unwrap()
                        .starts_with("application/json")
                );
                let value: Value = response.json().await.unwrap();
                assert!(value.get("result").is_some());
                if method == "tools/list" {
                    assert!(
                        value["result"]["tools"]
                            .as_array()
                            .is_some_and(|tools| !tools.is_empty())
                    );
                }
            }
        }
        assert_eq!(
            post(&b, "initialize", None, "unknown.example.com", None)
                .await
                .status(),
            403
        );
        assert_eq!(
            post(
                &b,
                "initialize",
                None,
                "mcp.example.com",
                Some("https://evil.example")
            )
            .await
            .status(),
            403
        );
        let client = reqwest::Client::new();
        for path in [
            "/.well-known/oauth-authorization-server",
            "/.well-known/oauth-protected-resource/mcp",
        ] {
            assert_eq!(
                client
                    .get(format!("http://127.0.0.1:{}{path}", b.config().broker.port))
                    .send()
                    .await
                    .unwrap()
                    .status(),
                404
            );
        }
        for path in ["/oauth/register", "/oauth/token"] {
            assert_eq!(
                client
                    .post(format!("http://127.0.0.1:{}{path}", b.config().broker.port))
                    .header(
                        "content-type",
                        if path == "/oauth/register" {
                            "application/json"
                        } else {
                            "application/x-www-form-urlencoded"
                        }
                    )
                    .body(if path == "/oauth/register" {
                        "{}"
                    } else {
                        "grant_type=refresh_token"
                    })
                    .send()
                    .await
                    .unwrap()
                    .status(),
                404
            );
        }
        // Removing local configuration immediately removes public allowlist access.
        b.remote
            .apply_mcp_only(&b, SecurityDeclaration::ExternalAuth, false, None)
            .await
            .unwrap();
        assert_eq!(
            post(
                &b,
                "initialize",
                None,
                origin.trim_start_matches("https://"),
                None
            )
            .await
            .status(),
            403
        );
        assert_eq!(
            post(&b, "initialize", None, "127.0.0.1", None)
                .await
                .status(),
            200
        );
        b.stop().await.unwrap();
    }
}

#[tokio::test]
async fn mcp_only_origin_rejects_invalid_input_before_config_or_runtime_change() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(directory.path());
    let config = configuration();
    config::save(&paths.config_file, &config).unwrap();
    let b = broker(paths.clone());
    for origin in [
        "http://example.com",
        "https://example.com/mcp",
        "https://user@example.com",
        "https://example.com?x=1",
        "https://example.com/#x",
    ] {
        assert!(
            b.remote
                .apply_mcp_only(&b, SecurityDeclaration::ExternalAuth, false, Some(origin))
                .await
                .is_err()
        );
        let mut invalid = config.clone();
        invalid.remote_access.mcp_only.public_origin = Some(origin.into());
        assert!(config::save(&paths.config_file, &invalid).is_err());
        assert_eq!(
            config::load(&paths.config_file).unwrap().remote_access,
            config.remote_access
        );
    }
    assert!(
        b.remote
            .apply_mcp_only(
                &b,
                SecurityDeclaration::None,
                true,
                Some("https://mcp.example.com")
            )
            .await
            .is_err()
    );
    assert_eq!(b.remote.policy(), McpAuthPolicy::Passthrough);
}

#[tokio::test]
#[ignore = "requires a user-provided live gateway mapped to REMOTE_GATEWAY_TEST_PORT; exposes only a test upstream"]
async fn real_mcp_only_gateway_initialize_and_tools_list() {
    let origin = std::env::var("REMOTE_GATEWAY_TEST_ORIGIN").expect("gateway Origin required");
    let origin = validate_https_origin(&origin).unwrap();
    let port: u16 = std::env::var("REMOTE_GATEWAY_TEST_PORT")
        .expect("gateway local port required")
        .parse()
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(directory.path());
    let mut config = configuration();
    config.broker.port = port;
    config.broker.enabled = true;
    config::save(&paths.config_file, &config).unwrap();
    let b = broker(paths);
    b.remote
        .apply_mcp_only(&b, SecurityDeclaration::ExternalAuth, false, Some(&origin))
        .await
        .unwrap();
    let _upstream = crate::serena::remote_fixture::attach(b.supervisor.clone()).await;
    b.startup().await.unwrap();
    let result = async {
        let client = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).timeout(std::time::Duration::from_secs(15)).build().unwrap();
        for (stage, body) in [
            ("initialize", json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"gateway-closure-test","version":"1"}}})),
            ("tools_list", json!({"jsonrpc":"2.0","id":2,"method":"tools/list"})),
        ] {
            let url = url::Url::parse(&origin).unwrap();
            let diagnostic = super::super::quick_tunnel::ProbeStage::new(stage, url.host_str().unwrap());
            let response = client.post(format!("{origin}/mcp")).header("Accept", "application/json, text/event-stream").header("MCP-Protocol-Version", "2025-11-25").json(&body).send().await.map_err(|error| diagnostic.network(&error))?;
            if response.status() != 200 { return Err(format!("stage={stage} status={}",response.status().as_u16())); }
            if !response.headers()["content-type"].to_str().unwrap_or("").starts_with("application/json") { return Err(format!("stage={stage} transport failure")); }
            let value: Value = response.json().await.map_err(|_| format!("stage={stage} invalid JSON"))?;
            if value.get("error").is_some() || value.get("result").is_none() { return Err(format!("stage={stage} MCP result failure")); }
            if stage == "tools_list" && !value["result"]["tools"].is_array() { return Err(format!("stage={stage} missing tools")); }
            println!("gateway stage={stage} PASS");
        }
        Ok::<(),String>(())
    }.await;
    b.stop().await.unwrap();
    assert!(b.remote.inner.lock().unwrap().oauth.is_none());
    println!("{}", b.log_snapshot().join("\n"));
    result.unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn oauth_transition_protects_existing_listener_before_slow_persistence() {
    for self_hosted in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(directory.path());
        let mut config = configuration();
        config.broker.enabled = true;
        // Remote-only persistence must not run discovery on this unrelated bad path.
        config.serena_path = Some(directory.path().join("missing-serena.exe"));
        config::save(&paths.config_file, &config).unwrap();
        let broker = broker(paths);
        broker.start().await.unwrap();
        assert_eq!(
            post(&broker, "initialize", None, "127.0.0.1", None)
                .await
                .status(),
            200
        );
        let entered = Arc::new(tokio::sync::Notify::new());
        let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
        let (signal, wait) = (entered.clone(), gate.clone());
        *broker.supervisor.remote_save_hook.lock().unwrap() = Some(Arc::new(move || {
            signal.notify_one();
            let guard = wait.0.lock().unwrap();
            let _guard = wait.1.wait_while(guard, |released| !*released).unwrap();
            if self_hosted {
                Ok(())
            } else {
                Err("injected save failure".into())
            }
        }));
        let probe = broker.remote.probe_lock.lock().await;
        let owner = broker.clone();
        let switching = tokio::spawn(async move {
            let context =
                self_hosted.then(|| RemotePublicContext::new("https://fixture.example").unwrap());
            owner.remote.start_mode(owner.clone(), context).await
        });
        entered.notified().await;
        for _ in 0..20 {
            assert_eq!(
                post(&broker, "initialize", None, "127.0.0.1", None)
                    .await
                    .status(),
                401
            );
        }
        *gate.0.lock().unwrap() = true;
        gate.1.notify_one();
        assert_eq!(switching.await.unwrap().is_ok(), self_hosted);
        assert_eq!(
            post(&broker, "initialize", None, "127.0.0.1", None)
                .await
                .status(),
            401
        );
        assert_eq!(broker.remote.policy(), McpAuthPolicy::EmbeddedOAuth);
        broker.stop().await.unwrap();
        drop(probe);
    }
}

#[tokio::test]
async fn bind_and_rollback_failure_converge_to_protected_error() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(directory.path());
    let occupied = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let mut config = configuration();
    config.broker.port = occupied.local_addr().unwrap().port();
    config::save(&paths.config_file, &config).unwrap();
    let broker = broker(paths);
    let calls = std::sync::atomic::AtomicUsize::new(0);
    *broker.supervisor.remote_save_hook.lock().unwrap() = Some(Arc::new(move || {
        if calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
            Ok(())
        } else {
            Err("injected rollback failure".into())
        }
    }));
    let error = broker
        .remote
        .start_mode(
            broker.clone(),
            Some(RemotePublicContext::new("https://fixture.example").unwrap()),
        )
        .await
        .unwrap_err();
    assert!(error.contains("REMOTE_CONFIG_ROLLBACK_FAILED: injected rollback failure"));
    assert!(
        !error.starts_with("REMOTE_CONFIG_ROLLBACK_FAILED"),
        "primary bind error is retained"
    );
    let snapshot = broker.remote.snapshot();
    assert!(snapshot.status == Status::Error);
    assert_eq!(snapshot.last_error.as_deref(), Some(error.as_str()));
    assert!(!snapshot.active);
    assert!(snapshot.public_context.is_none());
    assert_eq!(broker.remote.policy(), McpAuthPolicy::EmbeddedOAuth);
    assert_eq!(snapshot.config, broker.config().remote_access);
    assert!(!broker.snapshot().await.running);
}

#[tokio::test]
async fn startup_and_disable_share_management_serialization() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(directory.path());
    let mut config = configuration();
    config.broker.enabled = true;
    config::save(&paths.config_file, &config).unwrap();
    let broker = broker(paths);
    let management = broker.management.lock().await;
    let owner = broker.clone();
    let startup = tokio::spawn(async move { owner.startup().await });
    tokio::task::yield_now().await;
    assert!(
        !startup.is_finished(),
        "startup must wait for management before reading preferences"
    );
    let owner = broker.clone();
    let disable = tokio::spawn(async move {
        crate::commands::set_broker_impl(&owner, false, config.broker.port, false).await
    });
    tokio::task::yield_now().await;
    drop(management);
    startup.await.unwrap().unwrap();
    disable.await.unwrap().unwrap();
    assert!(!broker.config().broker.enabled);
    assert!(!broker.snapshot().await.running);
    // A delayed startup after disable must also honor the newly saved preference.
    broker.startup().await.unwrap();
    assert!(!broker.snapshot().await.running);
}

#[tokio::test]
async fn self_hosted_startup_waits_for_delayed_serena_before_probe() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(directory.path());
    let mut config = configuration();
    config.auto_start_server = true;
    config.remote_access.mode = RemoteAccessMode::SelfHostedOAuth;
    config.remote_access.self_hosted.public_origin = Some("https://fixture.example".into());
    config::save(&paths.config_file, &config).unwrap();
    let broker = broker(paths);
    let probe_guard = broker.remote.probe_lock.lock().await;
    let (release, wait) = tokio::sync::oneshot::channel();
    let (fixture_tx, fixture_rx) = tokio::sync::oneshot::channel();
    let owner = broker.clone();
    let preparation = tauri::async_runtime::spawn(async move {
        wait.await.unwrap();
        let fixture = crate::serena::remote_fixture::attach(owner.supervisor.clone()).await;
        assert!(fixture_tx.send(fixture).is_ok());
    });
    let owner = broker.clone();
    let startup = tokio::spawn(async move { owner.startup_after_serena(preparation).await });
    tokio::task::yield_now().await;
    assert!(!startup.is_finished());
    assert!(!broker.snapshot().await.running);
    release.send(()).unwrap();
    let _fixture = fixture_rx.await.unwrap();
    startup.await.unwrap().unwrap();
    // Exercise the real probe against local HTTP, without requiring a public TLS endpoint.
    let origin = format!("http://127.0.0.1:{}", config.broker.port);
    broker
        .remote
        .inner
        .lock()
        .unwrap()
        .oauth
        .as_mut()
        .unwrap()
        .context = RemotePublicContext {
        public_origin: origin.clone(),
        mcp_resource: format!("{origin}/mcp"),
        instance_id: "startup-fixture".into(),
    };
    drop(probe_guard);
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while broker.remote.snapshot().status != Status::Ready {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    broker.shutdown().await.unwrap();
}

#[tokio::test]
async fn quick_manual_probe_failure_hides_url_and_retry_restores_ready() {
    let directory = tempfile::tempdir().unwrap();
    let paths = paths(directory.path());
    let config = configuration();
    config::save(&paths.config_file, &config).unwrap();
    let broker = broker(paths);
    broker.start().await.unwrap();
    let origin = format!("http://127.0.0.1:{}", config.broker.port);
    let cancel = CancellationToken::new();
    {
        let mut inner = broker.remote.inner.lock().unwrap();
        inner.mode = RemoteAccessMode::QuickTunnel;
        inner.policy = McpAuthPolicy::EmbeddedOAuth;
        inner.status = Status::Ready;
        inner.cancel = Some(cancel.clone());
        inner.oauth = Some(Runtime::new(RemotePublicContext {
            public_origin: origin.clone(),
            mcp_resource: format!("{origin}/mcp"),
            instance_id: "quick-fixture".into(),
        }));
    }
    let fixture = crate::serena::remote_fixture::attach(broker.supervisor.clone()).await;
    broker.remote.probe().await.unwrap();
    drop(fixture);
    assert!(broker.remote.probe().await.is_err());
    assert!(broker.remote.snapshot().status == Status::Error);
    assert!(broker.remote.snapshot().public_context.is_none());
    assert!(!cancel.is_cancelled());
    assert_eq!(broker.remote.policy(), McpAuthPolicy::EmbeddedOAuth);
    assert!(broker.remote.inner.lock().unwrap().oauth.is_some());
    let _fixture = crate::serena::remote_fixture::attach(broker.supervisor.clone()).await;
    broker.remote.probe().await.unwrap();
    assert!(broker.remote.snapshot().status == Status::Ready);
    assert!(broker.remote.snapshot().public_context.is_some());
    assert!(
        broker.remote.task.lock().await.is_none(),
        "probe does not launch a tunnel worker"
    );
    broker.stop().await.unwrap();
}
