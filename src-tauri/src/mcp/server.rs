use super::*;
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use rmcp::{
    ErrorData, ServerHandler,
    model::*,
    service::{RequestContext, RoleServer},
};
#[derive(Clone)]
struct Handler(Arc<Broker>);
fn origin_allowed(headers: &axum::http::HeaderMap, allowed: &[String]) -> bool {
    let values: Vec<_> = headers.get_all("origin").iter().collect();
    if values.is_empty() {
        return true;
    }
    if values.len() != 1 {
        return false;
    }
    let Some(url) = values[0]
        .to_str()
        .ok()
        .and_then(|v| url::Url::parse(v).ok())
    else {
        return false;
    };
    url.path() == "/"
        && url.query().is_none()
        && url.fragment().is_none()
        && url.username().is_empty()
        && url.password().is_none()
        && allowed.contains(&url.origin().ascii_serialization())
}
impl ServerHandler for Handler {
    fn supported_protocol_versions(&self) -> std::borrow::Cow<'static, [ProtocolVersion]> {
        // Keep the external Broker on the negotiated protocol versions verified here.
        // Cloudflare discovery succeeds through 2025-11-25; its 2026-07-28
        // discovery path currently fails after tools/list.
        std::borrow::Cow::Borrowed(&[
            ProtocolVersion::V_2024_11_05,
            ProtocolVersion::V_2025_03_26,
            ProtocolVersion::V_2025_06_18,
            ProtocolVersion::V_2025_11_25,
        ])
    }

    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::default();
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        info.instructions = Some("所有会话共享一个活动项目；先查询或激活项目。".into());
        info
    }

    async fn list_tools(
        &self,
        _: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        self.0.log("tools/list · 读取 Serena 原始工具描述");
        let snapshot = self.0.supervisor.snapshot();
        if snapshot.server_status != ServerStatus::Running {
            return Err(ErrorData::internal_error(
                "BACKEND_UNAVAILABLE: 请先启动 Serena 以读取原始工具描述",
                None,
            ));
        }
        let client = tokio::select! {
            result = serena::Client::connect(snapshot.active_port) => result,
            _ = context.ct.cancelled() => return Err(ErrorData::internal_error("CANCELLED", None)),
        }
        .map_err(|e| ErrorData::internal_error(e, None))?;
        let current = self.0.supervisor.snapshot();
        if current.server_status != ServerStatus::Running
            || current.process_id != snapshot.process_id
        {
            return Err(ErrorData::internal_error(
                "BACKEND_UNAVAILABLE: Serena 已重启，请重新读取工具列表",
                None,
            ));
        }
        let tools = registry::list(&client.tools, self.0.config().agent_enabled)
            .map_err(|e| ErrorData::internal_error(e, None))?;
        if let Some(agent) = tools.iter().find(|t| t.name == "agent") {
            self.0.log(&format!(
                "agent contract published {}",
                registry::agent_contract_diagnostic(true, agent)
            ));
        } else {
            self.0
                .log("agent contract published agentEnabled=false (agent absent)");
        }
        self.0
            .log("tools/list · 返回工具列表（Serena 描述原样传递）");
        Ok(ListToolsResult {
            tools,
            ..Default::default()
        })
    }
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let started = std::time::Instant::now();
        let args = Value::Object(request.arguments.unwrap_or_default());
        let request_id = &context.id;
        self.0.log_tool(
            "INFO",
            &format!("tools/call request={request_id:?} tool={:?}", request.name),
        );
        registry::validate(&request.name, &args).map_err(|e| {
            self.0.log_tool("WARN", &format!("tools/call request={request_id:?} tool={:?} error_code=INVALID_PARAMS duration_ms={:.3}", request.name, started.elapsed().as_secs_f64() * 1000.0));
            ErrorData::invalid_params(e, None)
        })?;
        let result = if request.name == "media_read_image" {
            self.0.read_image(args, context.ct).await
        } else {
            self.0
                .call_tool(&request.name, args, context.ct)
                .await
                .map(|v| {
                    let content = vec![ContentBlock::text(v.to_string())];
                    let mut result = if (request.name == "codegraph_explore"
                        && v.get("error").is_some())
                        || (request.name == "agent" && v["ok"] == false)
                    {
                        CallToolResult::error(content)
                    } else {
                        CallToolResult::success(content)
                    };
                    result.structured_content = Some(v);
                    result
                })
        };
        let failed = result
            .as_ref()
            .map_or(true, |value| value.is_error == Some(true));
        self.0.log_tool(
            if failed { "ERROR" } else { "INFO" },
            &format!(
                "tools/call request={request_id:?} tool={:?} success={} duration_ms={:.3}",
                request.name,
                !failed,
                started.elapsed().as_secs_f64() * 1000.0
            ),
        );
        Ok(match result {
            Ok(v) => v,
            Err(e) => {
                CallToolResult::error(vec![ContentBlock::text(json!({"error":e}).to_string())])
            }
        }
        .into())
    }
}
impl Broker {
    /// Application startup waits for detection/optional auto-start before public probing.
    pub(crate) async fn startup_after_serena(
        self: &Arc<Self>,
        preparation: tauri::async_runtime::JoinHandle<()>,
    ) -> Result<(), String> {
        preparation
            .await
            .map_err(|e| format!("SERENA_STARTUP_FAILED: {e}"))?;
        self.startup().await
    }
    pub async fn startup(self: &Arc<Self>) -> Result<(), String> {
        let _management = self.management.lock().await;
        // Broker::new already installed the persisted policy before any listener.
        let config = self.config();
        if config.remote_access.mode == crate::remote::RemoteAccessMode::SelfHostedOAuth {
            let context = self
                .remote
                .inner
                .lock()
                .unwrap()
                .oauth
                .as_ref()
                .map(|o| o.context.clone());
            if let Some(context) = context {
                return self.remote.start_mode_locked(self, Some(context)).await;
            }
        }
        if config.broker.enabled {
            self.start().await?;
        }
        Ok(())
    }
    pub async fn start(self: &Arc<Self>) -> Result<(), String> {
        let mut current = self.listener.lock().await;
        if current.as_ref().is_some_and(|l| !l.handle.is_finished()) {
            return Ok(());
        }
        let address = self.config().broker.bind_address();
        let mut lan_ips = Vec::new();
        if address.ip().is_unspecified() {
            for interface in
                if_addrs::get_if_addrs().map_err(|e| format!("无法读取本机网卡地址: {e}"))?
            {
                if let std::net::IpAddr::V4(ip) = interface.ip()
                    && !ip.is_loopback()
                    && !ip.is_unspecified()
                {
                    lan_ips.push(ip);
                }
            }
            lan_ips.sort_unstable();
            lan_ips.dedup();
        }
        let listener = tokio::net::TcpListener::bind(address).await.map_err(|e| {
            self.log_level("ERROR", &format!("MCP 启动失败 · 地址 {address} · {e}"));
            format!("Broker 监听地址不可用: {e}")
        })?;
        let broker = self.clone();
        let token = CancellationToken::new();
        let mut config = StreamableHttpServerConfig::default();
        config.cancellation_token = token.child_token();
        // Preserve rmcp's Host validation; permit only addresses owned by this host
        // at listener startup. Never disable the allowlist for LAN access.
        config
            .allowed_hosts
            .extend(lan_ips.iter().map(ToString::to_string));
        config.legacy_session_mode = false;
        config.json_response = true;
        config.allowed_origins = vec![
            format!("http://127.0.0.1:{}", address.port()),
            format!("http://localhost:{}", address.port()),
        ];
        let service = StreamableHttpService::new(
            move || Ok(Handler(broker.clone())),
            Arc::new(LocalSessionManager::default()),
            config,
        );
        let remote = self.remote.clone();
        let service = tower::service_fn(move |request: axum::extract::Request| {
            use tower::ServiceExt;
            let mut service = service.clone();
            // Only local configuration / verified context extends this allowlist.
            if let Some(origin) = remote.public_origin() {
                let url = url::Url::parse(&origin).expect("validated public context");
                let authority = &url[url::Position::BeforeHost..url::Position::AfterPort];
                service.config.allowed_hosts.push(authority.to_owned());
                service.config.allowed_origins.push(origin);
            }
            async move {
                use axum::response::IntoResponse;
                // rmcp 3.2.0 treats an omitted allowed Origin port as a wildcard
                // and reads only the first header. Enforce exact origins first.
                if !origin_allowed(request.headers(), &service.config.allowed_origins) {
                    return Ok::<_, std::convert::Infallible>(
                        (axum::http::StatusCode::FORBIDDEN, "Origin is not allowed")
                            .into_response(),
                    );
                }
                service
                    .oneshot(request)
                    .await
                    .map(IntoResponse::into_response)
            }
        });
        let logging_broker = self.clone();
        let router =
            axum::Router::new()
                .nest_service("/mcp", service)
                .route_layer(axum::middleware::from_fn_with_state(self.remote.clone(), crate::oauth::http::protect))
                .merge(crate::oauth::http::router(self.remote.clone()))
                .layer(axum::middleware::from_fn(
                    move |axum::extract::ConnectInfo(peer): axum::extract::ConnectInfo<std::net::SocketAddr>, request: axum::extract::Request, next: axum::middleware::Next| {
                        let broker = logging_broker.clone();
                        async move {
                            let method = request.method().clone();
                            let path = request.uri().path().to_owned();
                            // Forwarded headers are diagnostic claims, not the TCP peer identity.
                            // Bound and quote values so headers cannot create arbitrary log lines.
                            let headers = request.headers();
                            let header = |name: &str| headers.get(name)
                                .and_then(|value| value.to_str().ok())
                                .unwrap_or("-").chars().take(512).collect::<String>();
                            let host = header("host");
                            let cf_ip = header("cf-connecting-ip");
                            let forwarded_for = header("x-forwarded-for");
                            let forwarded_host = header("x-forwarded-host");
                            let response = next.run(request).await;
                            broker.log_level(if response.status().is_server_error() { "ERROR" } else if response.status().is_client_error() { "WARN" } else { "INFO" }, &format!(
                                "HTTP {method} · {} · path={path:?} peer={peer} host={host:?} cf-connecting-ip={cf_ip:?} x-forwarded-for={forwarded_for:?} x-forwarded-host={forwarded_host:?}",
                                response.status()
                            ));
                            response
                        }
                    },
                ));
        let quit = token.clone();
        let error_broker = self.clone();
        let handle = tokio::spawn(async move {
            if let Err(e) = axum::serve(
                listener,
                router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .with_graceful_shutdown(quit.cancelled_owned())
            .await
            {
                error_broker.log_level("ERROR", &format!("MCP 监听异常 · {e}"));
                *error_broker.error.lock().unwrap() = Some(e.to_string());
            }
        });
        *current = Some(Listener {
            address,
            lan_endpoints: lan_ips
                .iter()
                .map(|ip| format!("http://{ip}:{}/mcp", address.port()))
                .collect(),
            cancel: token,
            handle,
        });
        self.log(&format!("MCP 已监听 · http://{address}/mcp"));
        self.log(&format!(
            "agent contract startup {}",
            registry::agent_contract_diagnostic(
                self.config().agent_enabled,
                &registry::agent_tool()
            )
        ));
        Ok(())
    }
    pub async fn stop(&self) -> Result<(), String> {
        self.remote.stop().await?;
        self.stop_listener().await;
        Ok(())
    }
    pub async fn shutdown(&self) -> Result<(), String> {
        self.remote.shutdown().await?;
        self.stop_listener().await;
        Ok(())
    }
    pub(crate) async fn stop_listener(&self) {
        if let Some(Listener {
            cancel, mut handle, ..
        }) = self.listener.lock().await.take()
        {
            cancel.cancel();
            if tokio::time::timeout(Duration::from_secs(5), &mut handle)
                .await
                .is_err()
            {
                handle.abort();
                let _ = handle.await;
            }
            self.log("MCP 已停止");
        }
        self.clear_workspace(&mut *self.workspace.write().await);
    }
}

#[cfg(test)]
mod quick_tunnel_transport_tests {
    use super::*;
    use crate::config::AppPaths;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn stateless_candidate_returns_json_without_sse_for_broker_requests() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let broker = Arc::new(Broker::new(Arc::new(
            SupervisorState::new(AppPaths {
                runtime_directory: root.join("runtime"),
                config_file: root.join("config.json"),
                log_directory: root.join("logs"),
                app_log: root.join("logs/app.log"),
                serena_log: root.join("logs/serena.log"),
            })
            .unwrap(),
        )));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let cancel = CancellationToken::new();
        let config = StreamableHttpServerConfig::default()
            .with_legacy_session_mode(false)
            .with_json_response(true)
            .with_cancellation_token(cancel.child_token());
        let service = StreamableHttpService::new(
            move || Ok(Handler(broker.clone())),
            Arc::new(LocalSessionManager::default()),
            config,
        );
        let quit = cancel.clone();
        let server = tokio::spawn(async move {
            axum::serve(listener, axum::Router::new().nest_service("/mcp", service))
                .with_graceful_shutdown(quit.cancelled_owned())
                .await
                .unwrap();
        });
        let outcome = tokio::time::timeout(Duration::from_secs(15), async {
            for protocol in ["2025-03-26", "2025-06-18", "2025-11-25"] {
                for (body, expected) in [
                    (json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":protocol,"capabilities":{},"clientInfo":{"name":"transport-contract-test","version":"1"}}}), "initialize"),
                    (json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}), "backend_unavailable"),
                    (json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"workspace_list","arguments":{}}}), "workspace_list"),
                    (json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"git_status","arguments":{}}}), "no_workspace"),
                ] {
                    let request_id = body["id"].clone();
                    let method = body["method"].as_str().unwrap();
                    let text = body.to_string();
                    let request = format!("POST /mcp HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nMCP-Protocol-Version: {protocol}\r\nMcp-Method: {method}\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{text}", text.len());
                    let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
                    socket.write_all(request.as_bytes()).await.unwrap();
                    let mut bytes = Vec::new();
                    socket.read_to_end(&mut bytes).await.unwrap();
                    let response = String::from_utf8(bytes).unwrap();
                    let (headers, body) = response.split_once("\r\n\r\n").unwrap();
                    assert!(headers.starts_with("HTTP/1.1 200"), "{response}");
                    let headers = headers.to_ascii_lowercase();
                    assert!(headers.contains("content-type: application/json"), "{response}");
                    assert!(!headers.contains("text/event-stream"), "{response}");
                    assert!(!headers.contains("mcp-session-id:"), "{response}");
                    let value: Value = serde_json::from_str(body).unwrap();
                    assert_eq!(value["id"], request_id);
                    match expected {
                        "initialize" => assert_eq!(value["result"]["protocolVersion"], protocol),
                        "backend_unavailable" => assert!(value["error"]["message"].as_str().unwrap().contains("BACKEND_UNAVAILABLE")),
                        "workspace_list" => {
                            assert_ne!(value["result"]["isError"], true, "{value}");
                            assert_eq!(value["result"]["structuredContent"]["workspaces"], json!([]), "{value}");
                        },
                        "no_workspace" => assert!(value["result"].to_string().contains("NO_ACTIVE_WORKSPACE")),
                        _ => unreachable!(),
                    }
                }
            }
            let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
            socket.write_all(format!("GET /mcp HTTP/1.1\r\nHost: {address}\r\nAccept: text/event-stream\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
            let mut response = String::new();
            socket.read_to_string(&mut response).await.unwrap();
            assert!(response.starts_with("HTTP/1.1 405"), "{response}");
        }).await;
        cancel.cancel();
        server.await.unwrap();
        outcome.expect("JSON transport candidate timed out");
    }
}
