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
        let request_id = &context.id;
        self.0.log_tool(
            "INFO",
            &format!(
                "tools/call request={request_id:?} tool={:?} · 入参={}",
                request.name,
                log_value(&args)
            ),
        );
        registry::validate(&request.name, &args).map_err(|e| {
            self.0.log_tool("WARN", &format!(
                "tools/call request={request_id:?} tool={:?} · 参数校验失败 · error={} · 耗时 {:.3} ms",
                request.name, log_value(&json!(e)), started.elapsed().as_secs_f64() * 1000.0
            ));
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
                    let mut result =
                        if request.name == "codegraph_explore" && v.get("error").is_some() {
                            CallToolResult::error(content)
                        } else {
                            CallToolResult::success(content)
                        };
                    result.structured_content = Some(v);
                    result
                })
        };
        let error = match &result {
            Ok(value) => value
                .structured_content
                .as_ref()
                .and_then(|v| v.get("error"))
                .cloned(),
            Err(error) => Some(json!(error)),
        };
        self.0.log_tool(
            if error.is_some() { "ERROR" } else { "INFO" },
            &format!(
                "tools/call request={request_id:?} tool={:?} · {} · 耗时 {:.3} ms{}",
                request.name,
                if error.is_some() { "失败" } else { "成功" },
                started.elapsed().as_secs_f64() * 1000.0,
                error
                    .map(|e| format!(" · error={}", log_value(&e)))
                    .unwrap_or_default()
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
        Ok(())
    }
    pub async fn stop(&self) -> Result<(), String> {
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
        Ok(())
    }
}

// JSON keeps user-controlled newlines escaped; bound each diagnostic payload.
fn log_value(value: &Value) -> String {
    let text = value.to_string();
    if text.chars().count() <= 8192 {
        return text;
    }
    format!(
        "{}… [已截断，最多 8192 字符]",
        text.chars().take(8192).collect::<String>()
    )
}

#[cfg(test)]
mod log_tests {
    use super::*;
    #[test]
    fn arguments_remain_readable_and_cannot_forge_log_lines() {
        let args =
            json!({"relative_path":"src/main.rs", "query":"你好\nERROR forged", "maxFiles":12});
        let logged = log_value(&args);
        assert_eq!(serde_json::from_str::<Value>(&logged).unwrap(), args);
        assert!(!logged.contains('\n'));
    }
    #[test]
    fn oversized_unicode_arguments_are_explicitly_truncated() {
        let logged = log_value(&json!({"query":"中".repeat(9000)}));
        assert!(logged.ends_with("[已截断，最多 8192 字符]"));
        assert!(logged.chars().count() < 8250);
    }
}
