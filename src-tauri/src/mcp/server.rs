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

const MAX_LOG_DETAIL_TEXT: usize = 512;
const MAX_LOG_DETAIL_ITEMS: usize = 32;

/// 内容和凭据类参数即使来自本地客户端也不能写入诊断日志。
fn detail_field_is_sensitive(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "authorization"
            | "access_token"
            | "refresh_token"
            | "token"
            | "secret"
            | "password"
            | "prompt"
            | "query"
            | "text"
            | "content"
            | "newcontent"
            | "oldcontent"
            | "substring_pattern"
            | "message"
    )
}

/// 限制普通诊断字符串，避免单次请求占满本地日志环形缓冲区。
fn bounded_log_text(value: &str) -> String {
    let mut characters = value.chars();
    let bounded: String = characters.by_ref().take(MAX_LOG_DETAIL_TEXT).collect();
    if characters.next().is_some() {
        format!("{bounded}…")
    } else {
        bounded
    }
}

/// 将系统 bind 错误稳定映射为前端可判断的错误码与可执行提示。
fn broker_bind_error(address: std::net::SocketAddr, error: &std::io::Error) -> (String, String) {
    let (code, reason, fix, user_message) = if error.kind() == std::io::ErrorKind::AddrInUse {
        (
            "BROKER_PORT_IN_USE",
            "端口已被其他程序占用",
            "在“设置 → MCP 连接入口”改用其他未占用端口后重试",
            format!(
                "MCP 连接入口端口 {} 已被其他程序占用。请在“设置 → MCP 连接入口”修改端口后重试。",
                address.port()
            ),
        )
    } else {
        (
            "BROKER_BIND_FAILED",
            "监听地址绑定失败",
            "检查端口与本机网络设置，或在“设置 → MCP 连接入口”改用其他端口后重试",
            format!(
                "MCP 连接入口无法监听 {}。请检查端口与本机网络设置后重试。",
                address
            ),
        )
    };
    let raw_os_error = error
        .raw_os_error()
        .map_or_else(|| "none".to_owned(), |value| value.to_string());
    let diagnostic = format!(
        "MCP 连接入口启动失败 · code={code} · address={address} · port={} · error_kind={:?} · raw_os_error={raw_os_error} · reason={reason} · fix={fix} · system_error={}",
        address.port(),
        error.kind(),
        bounded_log_text(&error.to_string())
    );
    (format!("{code}: {user_message}"), diagnostic)
}

/// 仅保留可诊断的调用形状，绝不把请求内容或凭据写进本地日志。
fn safe_log_detail(value: &Value, field_name: Option<&str>) -> Value {
    if field_name.is_some_and(detail_field_is_sensitive) {
        return Value::String("[已隐藏]".into());
    }
    match value {
        Value::Object(object) => {
            let mut safe = serde_json::Map::new();
            for (name, value) in object.iter().take(MAX_LOG_DETAIL_ITEMS) {
                safe.insert(name.clone(), safe_log_detail(value, Some(name)));
            }
            if object.len() > MAX_LOG_DETAIL_ITEMS {
                safe.insert(
                    "_omittedFields".into(),
                    json!(object.len() - MAX_LOG_DETAIL_ITEMS),
                );
            }
            Value::Object(safe)
        }
        Value::Array(items) => {
            let mut safe: Vec<_> = items
                .iter()
                .take(MAX_LOG_DETAIL_ITEMS)
                .map(|value| safe_log_detail(value, None))
                .collect();
            if items.len() > MAX_LOG_DETAIL_ITEMS {
                safe.push(json!({"_omittedItems": items.len() - MAX_LOG_DETAIL_ITEMS}));
            }
            Value::Array(safe)
        }
        Value::String(value) => Value::String(bounded_log_text(value)),
        _ => value.clone(),
    }
}

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

#[cfg(test)]
mod log_detail_tests {
    use super::*;

    #[test]
    fn tool_log_details_keep_safe_arguments_and_hide_request_content() {
        let details = safe_log_detail(
            &json!({
                "workspaceId": "workspace-a",
                "relative_path": "src/lib.rs",
                "startLine": 3,
                "substring_pattern": "PRIVATE_TOOL_ARGUMENT_9291",
                "content": "private source text",
                "access_token": "token-value",
            }),
            None,
        );
        assert_eq!(details["workspaceId"], "workspace-a");
        assert_eq!(details["relative_path"], "src/lib.rs");
        assert_eq!(details["startLine"], 3);
        assert_eq!(details["substring_pattern"], "[已隐藏]");
        assert_eq!(details["content"], "[已隐藏]");
        assert_eq!(details["access_token"], "[已隐藏]");
    }

    #[test]
    fn non_address_in_use_bind_errors_use_generic_stable_code_and_bounded_system_text() {
        let address = std::net::SocketAddr::from(([127, 0, 0, 1], 19120));
        let system_text = "x".repeat(MAX_LOG_DETAIL_TEXT + 100);
        let error = std::io::Error::new(std::io::ErrorKind::PermissionDenied, system_text);
        let (user_error, diagnostic) = broker_bind_error(address, &error);

        assert!(
            user_error.starts_with("BROKER_BIND_FAILED:"),
            "{user_error}"
        );
        assert!(diagnostic.contains("error_kind=PermissionDenied"));
        assert!(diagnostic.contains("raw_os_error=none"));
        assert!(diagnostic.ends_with('…'), "{diagnostic}");
        assert!(!diagnostic.contains(&"x".repeat(MAX_LOG_DETAIL_TEXT + 1)));
    }
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
        info.instructions = Some("所有 Workspace-scoped Tool 都必须显式传入 workspaceId。workspace_list 与 workspace_get 仅用于 Discovery，不建立 Workspace binding。".into());
        info
    }

    async fn list_tools(
        &self,
        _: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        self.0.log("tools/list · 生成本地公开工具描述");
        let config = self.0.config();
        let tools = registry::list_with_source_write(
            config.agent_enabled,
            config.remote_source_write_enabled,
        );
        self.0.log(&registry::orchestration_contract_diagnostic(
            config.agent_enabled,
            &tools,
        ));
        self.0.log("tools/list · 返回本地公开工具列表");
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
        let details = json!({
            "kind": "tool_call",
            "phase": "received",
            "requestId": request_id,
            "tool": request.name,
            "arguments": safe_log_detail(&args, None),
        });
        self.0.log_tool_detail(
            "INFO",
            &format!("tools/call request={request_id:?} tool={:?}", request.name),
            &details,
        );
        // Orchestration uses the same typed validation in Broker, returning its
        // product envelope (including control) rather than a JSON-RPC parameter error.
        if !super::orchestration::contains(&request.name) {
            if let Err(reason) = registry::authorize_source_write(
                self.0.config().remote_source_write_enabled,
                &request.name,
            ) {
                // 保持未公开工具的 JSON-RPC UNKNOWN_TOOL 合约；Dispatcher 仍会再次检查。
                self.0.log_tool_detail(
                    "WARN",
                    &format!("source write rejected before dispatch error_code={reason}"),
                    &json!({
                        "kind": "tool_call",
                        "phase": "authorization",
                        "requestId": request_id,
                        "tool": request.name,
                        "arguments": safe_log_detail(&args, None),
                        "errorCode": reason.to_string(),
                    }),
                );
                return Err(ErrorData::invalid_params("UNKNOWN_TOOL", None));
            }
            registry::validate(&request.name, &args).map_err(|e| {
                self.0.log_tool_detail(
                    "WARN",
                    &format!("tools/call request={request_id:?} tool={:?} error_code=INVALID_PARAMS duration_ms={:.3}", request.name, started.elapsed().as_secs_f64() * 1000.0),
                    &json!({
                        "kind": "tool_call",
                        "phase": "validation",
                        "requestId": request_id,
                        "tool": request.name,
                        "arguments": safe_log_detail(&args, None),
                        "errorCode": "INVALID_PARAMS",
                        "error": bounded_log_text(&e),
                    }),
                );
                ErrorData::invalid_params(e, None)
            })?;
        }
        let mut reported_error = None;
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
                        || (super::orchestration::contains(&request.name) && v["ok"] == false)
                    {
                        reported_error = v.get("error").cloned();
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
        let transport_error = result.as_ref().err().map(ToString::to_string);
        self.0.log_tool_detail(
            if failed { "ERROR" } else { "INFO" },
            &format!(
                "tools/call request={request_id:?} tool={:?} success={} duration_ms={:.3}",
                request.name,
                !failed,
                started.elapsed().as_secs_f64() * 1000.0
            ),
            &json!({
                "kind": "tool_call",
                "phase": "completed",
                "requestId": request_id,
                "tool": request.name,
                "success": !failed,
                "durationMs": (started.elapsed().as_secs_f64() * 1000.0),
                "error": transport_error
                    .map(|error| Value::String(bounded_log_text(&error)))
                    .or_else(|| reported_error.map(|error| safe_log_detail(&error, None))),
            }),
        );
        Ok(result
            .unwrap_or_else(|e| {
                CallToolResult::error(vec![ContentBlock::text(json!({"error":e}).to_string())])
            })
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
            if config.remote_access.self_hosted.provider == crate::remote::SelfHostedProvider::Ngrok
            {
                let result = self.remote.start_managed_ngrok_locked(self).await;
                if result.is_err() && config.broker.enabled {
                    self.start().await?;
                }
                return result;
            }
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
        if config.remote_access.mode == crate::remote::RemoteAccessMode::QuickTunnel
            && config.remote_access.quick_tunnel_desired_running
        {
            // 本地 Broker 的持久偏好先恢复；快捷隧道恢复仅发起一次异步 worker。
            if let Err(error) = self.remote.start_mode_locked(self, None).await {
                let mut inner = self.remote.inner.lock().unwrap();
                inner.status = crate::remote::Status::Error;
                inner.error = Some(error);
            }
        }
        Ok(())
    }
    pub async fn start(self: &Arc<Self>) -> Result<(), String> {
        let mut current = self.listener.lock().await;
        if current.as_ref().is_some_and(|l| !l.handle.is_finished()) {
            *self.error.lock().unwrap() = None;
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
        let listener = tokio::net::TcpListener::bind(address)
            .await
            .map_err(|error| {
                let (user_error, diagnostic) = broker_bind_error(address, &error);
                self.log_level("ERROR", &diagnostic);
                crate::logs::append(&self.supervisor.paths.app_log, "MCP Broker", &diagnostic);
                *self.error.lock().unwrap() = Some(user_error.clone());
                user_error
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
        let router = axum::Router::new()
            .nest_service("/mcp", service)
            .route_layer(axum::middleware::from_fn_with_state(
                self.remote.clone(),
                crate::oauth::http::protect,
            ))
            .merge(crate::oauth::http::router(self.remote.clone()))
            .layer(axum::middleware::from_fn(
                move |axum::extract::ConnectInfo(peer): axum::extract::ConnectInfo<
                    std::net::SocketAddr,
                >,
                      request: axum::extract::Request,
                      next: axum::middleware::Next| {
                    let broker = logging_broker.clone();
                    async move {
                        let method = request.method().clone();
                        let path = request.uri().path().to_owned();
                        // Forwarded headers are diagnostic claims, not the TCP peer identity.
                        // Bound and quote values so headers cannot create arbitrary log lines.
                        let headers = request.headers();
                        let header = |name: &str| {
                            headers
                                .get(name)
                                .and_then(|value| value.to_str().ok())
                                .unwrap_or("-")
                                .chars()
                                .take(512)
                                .collect::<String>()
                        };
                        let host = header("host");
                        let cf_ip = header("cf-connecting-ip");
                        let forwarded_for = header("x-forwarded-for");
                        let forwarded_host = header("x-forwarded-host");
                        let response = next.run(request).await;
                        broker.log_http_detail(
                            if response.status().is_server_error() {
                                "ERROR"
                            } else if response.status().is_client_error() {
                                "WARN"
                            } else {
                                "INFO"
                            },
                            &format!("HTTP {method} · {}", response.status()),
                            &json!({
                                "kind": "http_request",
                                "method": method.to_string(),
                                "path": path,
                                "status": response.status().as_u16(),
                                "peer": peer.to_string(),
                                "host": host,
                                "cfConnectingIp": cf_ip,
                                "forwardedFor": forwarded_for,
                                "forwardedHost": forwarded_host,
                            }),
                        );
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
            started_at: chrono::Utc::now().timestamp_millis(),
            cancel: token,
            handle,
        });
        *self.error.lock().unwrap() = None;
        self.log(&format!("MCP 已监听 · http://{address}/mcp"));
        let enabled = self.config().agent_enabled;
        self.log(&registry::orchestration_contract_diagnostic(
            enabled,
            &if enabled {
                super::orchestration::descriptors()
            } else {
                Vec::new()
            },
        ));
        Ok(())
    }
    pub async fn stop(&self) -> Result<(), String> {
        self.remote.stop().await?;
        self.stop_listener().await;
        *self.error.lock().unwrap() = None;
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
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn stateless_candidate_returns_json_without_sse_for_broker_requests() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let broker =
            super::super::integration_tests::transport_fixture_with_unprepared_codegraph(root);
        // 真实 transport 请求使用已登记的 Workspace；Root 仍只能由服务器 Resolver 导出。
        let workspace = crate::workspace_registry::WorkspaceRegistry::new(&broker.supervisor)
            .register(root.to_path_buf(), Some("transport-codegraph".into()))
            .unwrap();
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
                let mut requests = vec![
                    (json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":protocol,"capabilities":{},"clientInfo":{"name":"transport-contract-test","version":"1"}}}), "initialize"),
                    (json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}), "tools_list"),
                    (json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"workspace_list","arguments":{}}}), "workspace_list"),
                    (json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"git_status","arguments":{}}}), "workspace_context_required"),
                    (json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"codegraph_explore","arguments":{"query":"symbol"}}}), "codegraph_context_required"),
                    (json!({"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"codegraph_explore","arguments":{"workspaceId":"missing","query":"symbol"}}}), "codegraph_workspace_not_found"),
                    (json!({"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"codegraph_explore","arguments":{"workspaceId":workspace.id.clone(),"query":"symbol"}}}), "codegraph_valid_workspace"),
                ];
                // 每个冻结名称都必须经真实 Remote transport 在进入 Broker 前被 registry 拒绝。
                requests.extend(
                    super::super::source_write_domain::SourceWriteTool::ALL
                        .into_iter()
                        .enumerate()
                        .map(|(index, tool)| {
                            (
                                json!({
                                    "jsonrpc":"2.0",
                                    "id":10 + index,
                                    "method":"tools/call",
                                    "params":{"name":tool.code(),"arguments":{}}
                                }),
                                "source_write_remote_disabled",
                            )
                        }),
                );
                for (body, expected) in requests {
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
                        "tools_list" => {
                            let names = value["result"]["tools"]
                                .as_array()
                                .unwrap()
                                .iter()
                                .filter_map(|tool| tool["name"].as_str())
                                .collect::<std::collections::HashSet<_>>();
                            for name in registry::LOCAL_SOURCES.iter().chain(registry::SEMANTIC_SOURCES) {
                                assert!(names.contains(name), "{name}: {value}");
                            }
                            for legacy in ["workspace_activate", "workspace_deactivate", "workspace_current"] {
                                assert!(!names.contains(legacy), "{legacy}: {value}");
                            }
                            assert!(names.contains("codegraph_explore"), "{value}");
                            for tool in super::super::source_write_domain::SourceWriteTool::ALL {
                                assert!(!names.contains(tool.code()), "{}: {value}", tool.code());
                            }
                        }
                        "workspace_list" => {
                            assert_ne!(value["result"]["isError"], true, "{value}");
                            assert_eq!(
                                value["result"]["structuredContent"]["workspaces"]
                                    .as_array()
                                    .unwrap()
                                    .len(),
                                1,
                                "{value}"
                            );
                        },
                        "workspace_context_required" => {
                            assert_eq!(value["error"]["code"], -32602, "{value}");
                            assert_eq!(value["error"]["message"], "WORKSPACE_CONTEXT_REQUIRED");
                        }
                        "codegraph_context_required" => {
                            assert_eq!(value["error"]["code"], -32602, "{value}");
                            assert_eq!(value["error"]["message"], "WORKSPACE_CONTEXT_REQUIRED");
                        }
                        "codegraph_workspace_not_found" => {
                            assert_eq!(value["result"]["isError"], true, "{value}");
                            assert!(value["result"]["content"][0]["text"]
                                .as_str()
                                .unwrap()
                                .contains("WORKSPACE_NOT_FOUND"));
                        }
                        "codegraph_valid_workspace" => {
                            assert_eq!(value["result"]["isError"], true, "{value}");
                            let error = &value["result"]["structuredContent"]["error"];
                            assert_eq!(error["code"], "CODEGRAPH_NOT_INITIALIZED", "{value}");
                            assert_eq!(
                                error["message"],
                                "The active workspace has no initialized CodeGraph index.",
                                "{value}"
                            );
                            assert_eq!(error["recoverable"], false, "{value}");
                            assert!(error["workspace"].is_null(), "{value}");
                            assert!(!serde_json::to_string(error).unwrap().contains("root"));
                        }
                        "source_write_remote_disabled" => {
                            assert_eq!(value["error"]["code"], -32602, "{value}");
                            assert_eq!(value["error"]["message"], "UNKNOWN_TOOL", "{value}");
                        }
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
