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
impl ServerHandler for Handler {
    fn supported_protocol_versions(&self) -> std::borrow::Cow<'static, [ProtocolVersion]> {
        // Keep the external Broker on the session-based protocol used by Serena.
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
        let tools =
            registry::list(&client.tools).map_err(|e| ErrorData::internal_error(e, None))?;
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
        registry::validate(&request.name, &args).map_err(|e| {
            self.0.log(&format!(
                "tools/call · {:?} · 参数校验失败 · 耗时 {:.3} ms",
                request.name,
                started.elapsed().as_secs_f64() * 1000.0
            ));
            ErrorData::invalid_params(e, None)
        })?;
        self.0.log(&format!("tools/call · {} · 开始", request.name));
        let result = self.0.call_tool(&request.name, args, context.ct).await;
        self.0.log(&format!(
            "tools/call · {} · {} · 耗时 {:.3} ms",
            request.name,
            if result.as_ref().is_ok_and(|v| v.get("error").is_none()) {
                "成功"
            } else {
                "失败"
            },
            started.elapsed().as_secs_f64() * 1000.0
        ));
        Ok(match result {
            Ok(v) => {
                let content = vec![ContentBlock::text(v.to_string())];
                let mut result = if request.name == "codegraph_explore" && v.get("error").is_some()
                {
                    CallToolResult::error(content)
                } else {
                    CallToolResult::success(content)
                };
                result.structured_content = Some(v);
                result
            }
            Err(e) => {
                CallToolResult::error(vec![ContentBlock::text(json!({"error":e}).to_string())])
            }
        }
        .into())
    }
}
impl Broker {
    pub async fn start(self: &Arc<Self>) -> Result<(), String> {
        let mut current = self.listener.lock().await;
        if current.as_ref().is_some_and(|(_, _, h)| !h.is_finished()) {
            return Ok(());
        }
        let port = self.config().broker.port;
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
            .await
            .map_err(|e| {
                self.log(&format!("MCP 启动失败 · 端口 {port} · {e}"));
                format!("Broker 端口不可用: {e}")
            })?;
        let broker = self.clone();
        let token = CancellationToken::new();
        let mut config = StreamableHttpServerConfig::default();
        config.cancellation_token = token.child_token();
        let service = StreamableHttpService::new(
            move || Ok(Handler(broker.clone())),
            Arc::new(LocalSessionManager::default()),
            config,
        );
        let logging_broker = self.clone();
        let router =
            axum::Router::new()
                .nest_service("/mcp", service)
                .layer(axum::middleware::from_fn(
                    move |axum::extract::ConnectInfo(peer): axum::extract::ConnectInfo<std::net::SocketAddr>, request: axum::extract::Request, next: axum::middleware::Next| {
                        let broker = logging_broker.clone();
                        async move {
                            let method = request.method().clone();
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
                            broker.log(&format!(
                                "HTTP {method} · {} · peer={peer} host={host:?} cf-connecting-ip={cf_ip:?} x-forwarded-for={forwarded_for:?} x-forwarded-host={forwarded_host:?}",
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
                error_broker.log(&format!("MCP 监听异常 · {e}"));
                *error_broker.error.lock().unwrap() = Some(e.to_string());
            }
        });
        *current = Some((port, token, handle));
        self.log(&format!("MCP 已监听 · http://127.0.0.1:{port}/mcp"));
        Ok(())
    }
    pub async fn stop(&self) -> Result<(), String> {
        if let Some((_, token, mut handle)) = self.listener.lock().await.take() {
            token.cancel();
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
        Ok(())
    }
}
