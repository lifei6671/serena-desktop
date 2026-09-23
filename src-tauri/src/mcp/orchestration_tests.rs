use super::*;
use crate::agent::{product::AgentProductService, store::StateStore};
use crate::workspace_registry::WorkspaceRegistry;
use rmcp::{ServiceExt, model::CallToolRequestParams, transport::StreamableHttpClientTransport};

pub(crate) fn fixture(root: &std::path::Path) -> Arc<Broker> {
    let paths = crate::config::AppPaths {
        runtime_directory: root.join("runtime"),
        config_file: root.join("config.json"),
        log_directory: root.join("logs"),
        app_log: root.join("app.log"),
        serena_log: root.join("serena.log"),
    };
    let broker = Arc::new(Broker::new(Arc::new(SupervisorState::new(paths).unwrap())));
    let mut config = broker.config();
    config.agent_enabled = true;
    config.broker.port = crate::test_support::broker_loopback_listener()
        .local_addr()
        .unwrap()
        .port();
    broker.supervisor.replace_config(config).unwrap();
    broker
}

// Only an idle Serena connection for the current Workspace snapshot. Every call
// would fail the fixture; orchestration must not use Serena to perform its work.
pub(crate) async fn active(broker: &Broker, root: &std::path::Path) -> tokio::task::JoinHandle<()> {
    active_fixture(broker, root, false).await
}

/// 构造依赖 Windows 受管进程的 Source 读取测试夹具。
#[cfg(windows)]
pub(crate) async fn active_with_read_file(
    broker: &Broker,
    root: &std::path::Path,
) -> (
    crate::serena::remote_fixture::Fixture,
    tokio::task::JoinHandle<()>,
) {
    // Reuse the test-only managed process so Source's real Supervisor/PID guard runs.
    let process = crate::serena::remote_fixture::attach(broker.supervisor.clone()).await;
    let server = active_fixture(broker, root, true).await;
    (process, server)
}

async fn active_fixture(
    broker: &Broker,
    root: &std::path::Path,
    allow_read: bool,
) -> tokio::task::JoinHandle<()> {
    async fn rpc(
        axum::extract::State(read_root): axum::extract::State<Option<PathBuf>>,
        axum::Json(request): axum::Json<Value>,
    ) -> axum::response::Response {
        use axum::response::IntoResponse;
        if request.get("id").is_none() {
            return axum::http::StatusCode::ACCEPTED.into_response();
        }
        let result = match request["method"].as_str().unwrap() {
            "initialize" => {
                json!({"protocolVersion":request["params"]["protocolVersion"],"capabilities":{"tools":{}},"serverInfo":{"name":"fixture","version":"1"}})
            }
            "tools/list" => {
                let mut tools: Vec<_> = [
                    ("get_symbols_overview", &["relative_path", "depth"][..]),
                    (
                        "find_symbol",
                        &[
                            "relative_path",
                            "name_path_pattern",
                            "depth",
                            "include_body",
                        ][..],
                    ),
                    (
                        "find_referencing_symbols",
                        &["relative_path", "name_path"][..],
                    ),
                ]
                .into_iter()
                .map(|(name, fields)| {
                    let mut properties = serde_json::Map::new();
                    for field in fields {
                        properties.insert((*field).into(), json!({}));
                    }
                    properties.insert("max_answer_chars".into(), json!({}));
                    json!({"name":name,"inputSchema":{"type":"object","properties":properties}})
                })
                .collect();
                tools.push(json!({"name":"activate_project","inputSchema":{"type":"object","properties":{"project":{}}}}));
                tools.push(json!({"name":"get_current_config","inputSchema":{"type":"object"}}));
                json!({"tools":tools})
            }
            "tools/call" if read_root.is_some() => {
                assert_eq!(request["params"]["name"], "read_file");
                let args = &request["params"]["arguments"];
                assert_eq!(args["relative_path"], "context.txt");
                let raw = std::fs::read_to_string(read_root.unwrap().join("context.txt")).unwrap();
                let start = args["start_line"].as_u64().unwrap_or(0) as usize;
                let end = args["end_line"].as_u64().map(|line| line as usize);
                // Only upstream text; production Source computes the raw-file version.
                let text = raw
                    .lines()
                    .enumerate()
                    .filter(|(line, _)| *line >= start && end.is_none_or(|end| *line <= end))
                    .map(|(_, text)| text)
                    .collect::<Vec<_>>()
                    .join("\n");
                json!({"content":[{"type":"text","text":text}],"isError":false})
            }
            _ => panic!("orchestration called Serena"),
        };
        axum::Json(json!({"jsonrpc":"2.0","id":request["id"],"result":result})).into_response()
    }
    let listener = crate::test_support::broker_loopback_listener();
    listener.set_nonblocking(true).unwrap();
    let listener = tokio::net::TcpListener::from_std(listener).unwrap();
    let port = listener.local_addr().unwrap().port();
    let read_root = allow_read.then(|| root.to_path_buf());
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            axum::Router::new()
                .route("/mcp", axum::routing::post(rpc))
                .with_state(read_root),
        )
        .await
        .unwrap();
    });
    let client = Arc::new(serena::Client::connect(port).await.unwrap());
    let workspace = Workspace {
        id: "W".into(),
        name: "fixture".into(),
        root: root.into(),
        generation: 1,
    };
    *broker.workspace.write().await = Some(Active {
        workspace,
        client,
        pid: if allow_read {
            broker.supervisor.snapshot().process_id.unwrap()
        } else {
            1
        },
    });
    server
}
async fn call(broker: &Broker, name: &str, args: Value) -> Value {
    broker
        .call_tool(name, args, CancellationToken::new())
        .await
        .unwrap()
}

#[tokio::test]
async fn agent_execute_start_resolves_the_work_snapshot_without_active_workspace_authority() {
    let dir = tempfile::tempdir().unwrap();
    let root_a = dir.path().join("a");
    let root_b = dir.path().join("b");
    let root_a_replaced = dir.path().join("a-replaced");
    std::fs::create_dir_all(&root_a).unwrap();
    std::fs::create_dir_all(&root_b).unwrap();
    std::fs::create_dir_all(&root_a_replaced).unwrap();
    let root_a = std::fs::canonicalize(root_a).unwrap();
    let root_b = std::fs::canonicalize(root_b).unwrap();
    let root_a_replaced = std::fs::canonicalize(root_a_replaced).unwrap();
    let broker = fixture(dir.path());
    let store = StateStore::open(dir.path().join("state")).await.unwrap();
    assert!(
        broker
            .product
            .set(Arc::new(
                AgentProductService::new_with_rejected_dispatch_for_test(store.clone()),
            ))
            .is_ok()
    );
    let mut config = broker.config();
    config.workspaces = vec![
        Workspace {
            id: "A".into(),
            name: "A".into(),
            root: root_a.clone(),
            generation: 3,
        },
        Workspace {
            id: "B".into(),
            name: "B".into(),
            root: root_b.clone(),
            generation: 7,
        },
    ];
    config.desktop_selected_workspace_id = Some("B".into());
    broker.supervisor.replace_config(config).unwrap();
    let server = active(&broker, &root_b).await;
    {
        let mut active = broker.workspace.write().await;
        let active = active.as_mut().unwrap();
        active.workspace.id = "B".into();
        active.workspace.generation = 7;
        assert_eq!(active.workspace.id, "B");
        assert_eq!(
            broker.supervisor.desktop_selected_workspace().unwrap().id,
            "B"
        );
    }
    for (id, workspace_id, root, generation) in [
        ("work-a", "A", root_a.clone(), 3),
        ("work-b", "B", root_b.clone(), 7),
        ("work-stale", "A", root_a.clone(), 3),
        ("work-unknown", "unknown", root_a.clone(), 3),
        ("work-mismatch", "A", root_a.clone(), 3),
        ("work-no-context", "A", root_a.clone(), 3),
    ] {
        store
            .create_work_run(
                id.into(),
                workspace_id.into(),
                root.to_string_lossy().into(),
                generation,
                id.into(),
                None,
                1,
            )
            .await
            .unwrap();
    }

    // Global ActiveWorkspace 和 Desktop selection 均为 B，Start 仍只服从显式 workspaceId。
    for (work_run_id, workspace_id, root, generation) in [
        ("work-a", "A", root_a.clone(), 3),
        ("work-b", "B", root_b.clone(), 7),
    ] {
        let response = call(
            &broker,
            "agent_execute",
            json!({"action":"start","workRunId":work_run_id,"workspaceId":workspace_id,"requestKey":"key","prompt":"p"}),
        )
        .await;
        // 测试专用 Provider 在接受前拒绝；durable 创建先于派发失败完成。
        assert_eq!(response["ok"], false);
        assert_eq!(response["error"]["code"], "AGENT_OPERATION_FAILED");
        assert_eq!(response["control"]["requestAccepted"], true);
        let link = store
            .work_execution_links(work_run_id.into())
            .await
            .unwrap()
            .pop()
            .unwrap();
        assert_eq!(response["error"]["executionId"], link.execution_id);
        let execution = store.execution(link.execution_id).await.unwrap().unwrap();
        assert_eq!(execution.workspace_id, workspace_id);
        assert_eq!(execution.canonical_workspace_root, root.to_string_lossy());
        assert_eq!(execution.workspace_generation, generation);
    }

    WorkspaceRegistry::new(&broker.supervisor)
        .rename("A", "renamed A".into())
        .unwrap();
    WorkspaceRegistry::new(&broker.supervisor)
        .reorder(vec!["B".into(), "A".into()])
        .unwrap();
    broker.supervisor.select_desktop_workspace("B").unwrap();
    let before_retry = store.work_execution_links("work-a".into()).await.unwrap();
    let retry = call(
        &broker,
        "agent_execute",
        json!({"action":"start","workRunId":"work-a","workspaceId":"A","requestKey":"key","prompt":"p"}),
    )
    .await;
    assert_eq!(retry["ok"], true);
    assert_eq!(before_retry.len(), 1);
    assert_eq!(retry["data"]["executionId"], before_retry[0].execution_id);
    assert_eq!(
        store.work_execution_links("work-a".into()).await.unwrap(),
        before_retry
    );
    let frozen = store
        .execution(before_retry[0].execution_id.clone())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(frozen.canonical_workspace_root, root_a.to_string_lossy());
    assert_eq!(frozen.workspace_generation, 3);

    let mut changed = broker.config();
    let workspace_a = changed
        .workspaces
        .iter_mut()
        .find(|workspace| workspace.id == "A")
        .unwrap();
    workspace_a.root = root_a_replaced;
    workspace_a.generation = 4;
    broker.supervisor.replace_config(changed).unwrap();
    let before_rejected = store
        .product_read(None, None, None, 100)
        .await
        .unwrap()
        .into_iter()
        .map(|snapshot| snapshot.execution)
        .collect::<Vec<_>>();
    let stale = call(
        &broker,
        "agent_execute",
        json!({"action":"start","workRunId":"work-stale","workspaceId":"A","requestKey":"key","prompt":"p"}),
    )
    .await;
    assert_eq!(stale["error"]["code"], "WORKSPACE_CONTEXT_MISMATCH");
    assert_eq!(
        store
            .product_read(None, None, None, 100)
            .await
            .unwrap()
            .into_iter()
            .map(|snapshot| snapshot.execution)
            .collect::<Vec<_>>(),
        before_rejected
    );
    assert!(
        store
            .work_execution_links("work-stale".into())
            .await
            .unwrap()
            .is_empty()
    );

    let unknown = call(
        &broker,
        "agent_execute",
        json!({"action":"start","workRunId":"work-unknown","workspaceId":"unknown","requestKey":"key","prompt":"p"}),
    )
    .await;
    assert_eq!(unknown["error"]["code"], "WORKSPACE_NOT_FOUND");
    assert_eq!(
        store
            .product_read(None, None, None, 100)
            .await
            .unwrap()
            .into_iter()
            .map(|snapshot| snapshot.execution)
            .collect::<Vec<_>>(),
        before_rejected
    );
    assert!(
        store
            .work_execution_links("work-unknown".into())
            .await
            .unwrap()
            .is_empty()
    );

    let mismatch = call(
        &broker,
        "agent_execute",
        json!({"action":"start","workRunId":"work-mismatch","workspaceId":"B","requestKey":"key","prompt":"p"}),
    )
    .await;
    assert_eq!(mismatch["error"]["code"], "WORKSPACE_CONTEXT_MISMATCH");
    assert_eq!(
        store
            .work_execution_links("work-mismatch".into())
            .await
            .unwrap()
            .len(),
        0
    );
    assert_eq!(
        store
            .product_read(None, None, None, 100)
            .await
            .unwrap()
            .into_iter()
            .map(|snapshot| snapshot.execution)
            .collect::<Vec<_>>(),
        before_rejected
    );

    // 省略字段不得回退到 WorkRun、ActiveWorkspace 或 Desktop selection。
    let missing_context = call(
        &broker,
        "agent_execute",
        json!({"action":"start","workRunId":"work-no-context","requestKey":"key","prompt":"p"}),
    )
    .await;
    assert_eq!(
        missing_context["error"]["code"],
        "WORKSPACE_CONTEXT_REQUIRED"
    );
    assert!(
        store
            .work_execution_links("work-no-context".into())
            .await
            .unwrap()
            .is_empty()
    );
    for workspace_id in [json!(" \n"), json!(7)] {
        let invalid = call(
            &broker,
            "agent_execute",
            json!({"action":"start","workRunId":"work-no-context","workspaceId":workspace_id,"requestKey":"key","prompt":"p"}),
        )
        .await;
        assert_eq!(invalid["error"]["code"], "INVALID_PARAMS");
        assert!(
            store
                .work_execution_links("work-no-context".into())
                .await
                .unwrap()
                .is_empty()
        );
    }
    server.abort();
}

#[tokio::test]
/// 验证 Local Start 仅信任显式 workspaceId，且重试保持单一执行记录。
async fn local_agent_start_resolves_explicit_workspace_without_global_active_workspace() {
    let dir = tempfile::tempdir().unwrap();
    let root_a = dir.path().join("a");
    let root_b = dir.path().join("b");
    std::fs::create_dir_all(&root_a).unwrap();
    std::fs::create_dir_all(&root_b).unwrap();
    let root_a = std::fs::canonicalize(root_a).unwrap();
    let root_b = std::fs::canonicalize(root_b).unwrap();
    let broker = fixture(dir.path());
    let store = StateStore::open(dir.path().join("state")).await.unwrap();
    assert!(
        broker
            .product
            .set(Arc::new(
                AgentProductService::new_with_rejected_dispatch_for_test(store.clone()),
            ))
            .is_ok()
    );
    let mut config = broker.config();
    config.workspaces = vec![
        Workspace {
            id: "A".into(),
            name: "A".into(),
            root: root_a.clone(),
            generation: 3,
        },
        Workspace {
            id: "B".into(),
            name: "B".into(),
            root: root_b.clone(),
            generation: 7,
        },
    ];
    config.desktop_selected_workspace_id = Some("B".into());
    broker.supervisor.replace_config(config).unwrap();
    let server = active(&broker, &root_b).await;
    {
        let mut active = broker.workspace.write().await;
        let active = active.as_mut().unwrap();
        active.workspace.id = "B".into();
        active.workspace.generation = 7;
    }

    // Local Start 缺少 ID 时不能从 Workstation 的全局或 Desktop 状态补齐。
    let missing = broker
        .agent_operation(
            json!({"action":"start","agentId":"local","requestKey":"missing","prompt":"p"}),
        )
        .await;
    assert_eq!(missing["error"]["code"], "WORKSPACE_CONTEXT_REQUIRED");
    assert_eq!(missing["control"]["providerInvoked"], false);
    let unknown = broker
        .agent_operation(json!({"action":"start","workspaceId":"unknown","agentId":"local","requestKey":"unknown","prompt":"p"}))
        .await;
    assert_eq!(unknown["error"]["code"], "WORKSPACE_NOT_FOUND");
    assert_eq!(unknown["control"]["providerInvoked"], false);
    assert!(
        store
            .product_read(None, None, None, 100)
            .await
            .unwrap()
            .is_empty()
    );

    let response = broker
        .agent_operation(json!({"action":"start","workspaceId":"A","agentId":"local","requestKey":"key","prompt":"p"}))
        .await;
    // 测试专用 Provider 在接受前拒绝；创建已冻结后，派发失败是预期边界。
    assert_eq!(response["ok"], false);
    assert_eq!(response["error"]["code"], "AGENT_OPERATION_FAILED");
    assert_eq!(response["control"]["requestAccepted"], true);
    let execution = store
        .product_read(None, None, None, 100)
        .await
        .unwrap()
        .pop()
        .unwrap()
        .execution;
    assert_eq!(response["error"]["executionId"], execution.id);
    assert_eq!(execution.workspace_id, "A");
    assert_eq!(execution.canonical_workspace_root, root_a.to_string_lossy());
    assert_eq!(execution.workspace_generation, 3);
    let replay = broker
        .agent_operation(json!({"action":"start","workspaceId":"A","agentId":"local","requestKey":"key","prompt":"p"}))
        .await;
    assert_eq!(replay["data"]["executionId"], execution.id);
    let executions = store.product_read(None, None, None, 100).await.unwrap();
    assert_eq!(executions.len(), 1);
    assert_eq!(executions[0].execution.id, execution.id);
    server.abort();
}

#[tokio::test]
/// 验证 Local Start 从 Lease 解析到执行创建始终受同一 Supervisor 操作锁线性化保护。
async fn local_agent_start_holds_supervisor_operation_mutex_from_lease_to_create() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("workspace");
    std::fs::create_dir_all(&root).unwrap();
    let root = std::fs::canonicalize(root).unwrap();
    let broker = fixture(dir.path());
    let store = StateStore::open(dir.path().join("state")).await.unwrap();
    let product = Arc::new(AgentProductService::new_with_rejected_dispatch_for_test(
        store.clone(),
    ));
    assert!(broker.product.set(product.clone()).is_ok());
    let mut config = broker.config();
    config.workspaces = vec![Workspace {
        id: "W".into(),
        name: "Workspace".into(),
        root: root.clone(),
        generation: 9,
    }];
    broker.supervisor.replace_config(config).unwrap();

    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let release_rx = Arc::new(std::sync::Mutex::new(release_rx));
    let hook_release = release_rx.clone();
    *broker.supervisor.workspace_start_hook.lock().unwrap() = Some(Arc::new(move || {
        entered_tx.send(()).unwrap();
        hook_release.lock().unwrap().recv().unwrap();
    }));

    let start_broker = broker.clone();
    let start = std::thread::spawn(move || {
        tokio::runtime::Runtime::new().unwrap().block_on(async move {
            start_broker
                .agent_operation(json!({"action":"start","workspaceId":"W","agentId":"local","requestKey":"linear","prompt":"p"}))
                .await
        })
    });
    tokio::task::spawn_blocking(move || entered_rx.recv().unwrap())
        .await
        .unwrap();

    let remove_supervisor = broker.supervisor.clone();
    let remove_product = product.clone();
    let mut remove = tokio::task::spawn_blocking(move || {
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(remove_supervisor.remove_workspace_coordinated(remove_product.as_ref(), "W"))
    });
    // Hook is after Resolver and before the Store transaction; Remove must not mutate Registry here.
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(50), &mut remove)
            .await
            .is_err()
    );
    assert_eq!(
        WorkspaceRegistry::new(&broker.supervisor)
            .get("W")
            .unwrap()
            .generation,
        9
    );

    release_tx.send(()).unwrap();
    let response = tokio::task::spawn_blocking(move || start.join().unwrap())
        .await
        .unwrap();
    assert_eq!(response["ok"], false);
    assert_eq!(response["error"]["code"], "AGENT_OPERATION_FAILED");
    assert_eq!(response["control"]["requestAccepted"], true);
    let execution = store
        .product_read(None, None, None, 10)
        .await
        .unwrap()
        .pop()
        .unwrap()
        .execution;
    assert_eq!(response["error"]["executionId"], execution.id);
    assert_eq!(execution.workspace_id, "W");
    assert_eq!(execution.canonical_workspace_root, root.to_string_lossy());
    assert_eq!(execution.workspace_generation, 9);
    // Remove may proceed only after the frozen execution exists, then sees its Claim or a terminal release.
    let _ = remove.await.unwrap();
}

#[tokio::test]
async fn four_tools_enforce_disabled_and_uninitialized_policy_and_legacy_is_unknown() {
    let dir = tempfile::tempdir().unwrap();
    let broker = fixture(dir.path());
    for enabled in [false, true] {
        let mut config = broker.config();
        config.agent_enabled = enabled;
        broker.supervisor.replace_config(config).unwrap();
        for name in orchestration::NAMES {
            let response = call(&broker, name, json!({"action":"invalid"})).await;
            assert_eq!(response["ok"], false);
            assert_eq!(response.get("control").is_some(), name == "agent_execute");
            assert_eq!(
                response["error"]["code"],
                if enabled {
                    "BACKEND_UNAVAILABLE"
                } else {
                    "AGENT_DISABLED"
                }
            );
        }
        assert_eq!(
            broker
                .dispatch("agent", json!({"action":"start"}), CancellationToken::new())
                .await
                .unwrap_err(),
            "UNKNOWN_TOOL"
        );
    }
    let store = StateStore::open(dir.path().join("state")).await.unwrap();
    assert!(
        broker
            .product
            .set(Arc::new(AgentProductService::new(store.clone())))
            .is_ok()
    );
    for request in [
        json!({"action":"list"}),
        json!({"action":"observe","executionId":"missing"}),
        json!({"action":"list","limit":101}),
    ] {
        // 非 Start 的 Local 兼容入口仍只透传既有 Product 契约。
        assert_eq!(
            broker.agent_operation(request.clone()).await,
            broker.product.get().unwrap().operation(request, None).await
        );
    }
    // Local 兼容 DTO 同样不接受 Continue workspaceId，避免形成双 Authority。
    assert_eq!(
        broker
            .agent_operation(json!({"action":"continue","executionId":"missing","workspaceId":"W","requestKey":"k","prompt":"p"}))
            .await["error"]["code"],
        "AGENT_INVALID_ARGUMENT"
    );
    let mut config = broker.config();
    config.agent_enabled = false;
    broker.supervisor.replace_config(config).unwrap();
    for name in orchestration::NAMES {
        assert_eq!(
            call(&broker, name, json!({"action":"list"})).await["error"]["code"],
            "AGENT_DISABLED"
        );
    }
    assert_eq!(
        broker.agent_operation(json!({"action":"list"})).await["ok"],
        true
    );
    assert!(
        store
            .product_read(None, None, None, 100)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn http_work_projection_guards_errors_and_reopen_preserve_public_contract() {
    let dir = tempfile::tempdir().unwrap();
    let broker = fixture(dir.path());
    let store = StateStore::open(dir.path().join("state")).await.unwrap();
    assert!(
        broker
            .product
            .set(Arc::new(AgentProductService::new(store.clone())))
            .is_ok()
    );
    let root = std::fs::canonicalize(dir.path()).unwrap();
    let mut config = broker.config();
    config.workspaces = vec![Workspace {
        id: "W".into(),
        name: "Workspace".into(),
        root,
        generation: 1,
    }];
    broker.supervisor.replace_config(config).unwrap();
    broker.start().await.unwrap();
    let client = ()
        .serve(StreamableHttpClientTransport::from_uri(format!(
            "http://127.0.0.1:{}/mcp",
            broker.config().broker.port
        )))
        .await
        .unwrap();
    let request = |name: &str, args: Value| {
        CallToolRequestParams::new(name.to_string())
            .with_arguments(args.as_object().unwrap().clone())
    };
    assert!(
        client
            .call_tool(request("agent", json!({"action":"list"})))
            .await
            .unwrap_err()
            .to_string()
            .contains("UNKNOWN_TOOL")
    );
    for (name, args, code) in [
        (
            "work_query",
            json!({"action":"get","workRunId":"missing"}),
            "WORK_NOT_FOUND",
        ),
        (
            "work_update",
            json!({"action":"begin","workspaceId":"missing","title":"title"}),
            "WORKSPACE_NOT_FOUND",
        ),
        (
            "agent_query",
            json!({"action":"get","executionId":"missing"}),
            "AGENT_EXECUTION_NOT_FOUND",
        ),
        (
            "agent_execute",
            json!({"action":"start","workRunId":"missing","workspaceId":"W","requestKey":"k","prompt":"p"}),
            "WORK_NOT_FOUND",
        ),
    ] {
        let result = client.call_tool(request(name, args)).await.unwrap();
        assert_eq!(result.is_error, Some(true));
        let response = result.structured_content.unwrap();
        assert_eq!(response["error"]["code"], code);
        assert_eq!(response.get("control").is_some(), name == "agent_execute");
        let malformed = client
            .call_tool(request(
                name,
                json!({"action":"invalid","secret":"SQL path secret"}),
            ))
            .await
            .unwrap();
        assert_eq!(malformed.is_error, Some(true));
        assert_eq!(
            malformed
                .structured_content
                .as_ref()
                .unwrap()
                .get("control")
                .is_some(),
            name == "agent_execute"
        );
        assert_eq!(
            malformed.structured_content.unwrap()["error"]["code"],
            "WORK_INVALID_ARGUMENT"
        );
    }
    assert_eq!(
        call(
            &broker,
            "work_update",
            json!({"action":"begin","workspaceId":"wrong","title":"title"})
        )
        .await["error"]["code"],
        "WORKSPACE_NOT_FOUND"
    );
    let created = client
        .call_tool(request(
            "work_update",
            json!({"action":"begin","workspaceId":"W","title":"  title "}),
        ))
        .await
        .unwrap();
    assert_eq!(created.is_error, Some(false));
    let row = created.structured_content.unwrap()["data"]["workRun"].clone();
    let id = row["workRunId"].as_str().unwrap();
    for field in ["goal", "acceptance", "completedAt"] {
        assert!(row.as_object().unwrap().contains_key(field));
        assert!(row[field].is_null());
    }
    assert_eq!(row.as_object().unwrap().len(), 10);
    assert!(!row.to_string().contains(dir.path().to_str().unwrap()));
    let queried = client
        .call_tool(request(
            "work_query",
            json!({"action":"get","workRunId":id}),
        ))
        .await
        .unwrap();
    assert_eq!(queried.is_error, Some(false));
    assert_eq!(queried.structured_content.unwrap()["data"]["workRun"], row);
    let finished=client.call_tool(request("work_update",json!({"action":"finish","workRunId":id,"outcome":"completed","acceptance":{"summary":" reviewed ","executionIds":[]}}))).await.unwrap();
    assert_eq!(finished.is_error, Some(false));
    let row = finished.structured_content.unwrap()["data"]["workRun"].clone();
    assert_eq!(row["acceptance"]["summary"], "reviewed");
    assert_eq!(row["acceptance"]["acceptedAt"], row["completedAt"]);
    let db = rusqlite::Connection::open(dir.path().join("state/agent-state.db")).unwrap();
    for corrupt in [
        "invalid SQL E:/private/path",
        r#"{"decision":"accepted","summary":"x","executionIds":[],"acceptedAt":1,"systemVerified":true}"#,
    ] {
        db.execute(
            "UPDATE work_runs SET acceptance_json=?1 WHERE id=?2",
            [corrupt, id],
        )
        .unwrap();
        let failure = client
            .call_tool(request(
                "work_query",
                json!({"action":"get","workRunId":id}),
            ))
            .await
            .unwrap();
        assert_eq!(failure.is_error, Some(true));
        assert_eq!(
            failure.structured_content.unwrap(),
            json!({"ok":false,"error":{"code":"WORK_OPERATION_FAILED","message":"WORK_OPERATION_FAILED"}})
        );
    }
    db.execute(
        "UPDATE work_runs SET acceptance_json=?1 WHERE id=?2",
        [row["acceptance"].to_string(), id.into()],
    )
    .unwrap();
    for name in orchestration::NAMES {
        let mut config = broker.config();
        config.agent_enabled = false;
        broker.supervisor.replace_config(config).unwrap();
        let result = client
            .call_tool(request(name, json!({"action":"list"})))
            .await
            .unwrap();
        assert_eq!(result.is_error, Some(true));
        assert_eq!(
            result.structured_content.unwrap()["error"]["code"],
            "AGENT_DISABLED"
        );
    }
    client.cancel().await.unwrap();
    broker.stop().await.unwrap();
    drop(broker);
    drop(store);
    drop(db);
    let broker = fixture(dir.path());
    let store = StateStore::open(dir.path().join("state")).await.unwrap();
    assert!(
        broker
            .product
            .set(Arc::new(AgentProductService::new(store)))
            .is_ok()
    );
    assert_eq!(
        call(
            &broker,
            "work_query",
            json!({"action":"get","workRunId":id})
        )
        .await["data"]["workRun"],
        row
    );
    assert_eq!(
        call(&broker, "work_query", json!({"action":"list"})).await["data"]["workRuns"],
        json!([row])
    );
}

fn assert_compact_observation(response: &Value) {
    assert_eq!(response["ok"], true);
    assert_eq!(response.as_object().unwrap().len(), 2);
    assert!(response.get("control").is_none());
    let data = &response["data"];
    for field in [
        "prompt",
        "canonicalWorkspaceRoot",
        "agentId",
        "workspaceId",
        "dispatchState",
        "controlRevision",
        "availableActions",
        "threadId",
        "threadName",
        "turnId",
        "providerTerminalStatus",
        "createdAt",
        "updatedAt",
        "completedAt",
        "interruptRequested",
        "interruptAcknowledged",
        "interruptTimedOut",
        "errorCode",
        "errorMessage",
    ] {
        assert!(
            data.get(field).is_none(),
            "unexpected observe field: {field}"
        );
    }
    for field in [
        "executionId",
        "status",
        "revision",
        "activityRevision",
        "resultCompleteness",
    ] {
        assert!(data[field].is_string(), "missing {field}");
    }
    assert!(data["usage"].is_object(), "missing public usage");
    assert!(data["usage"]["completeness"].is_string());
    assert!(data["usage"]["usageRevision"].is_u64());
    assert!(data["unchanged"].is_boolean());
    assert!(data["resultAvailable"].is_boolean());
    assert!(data["progress"]["phase"].is_string());
    assert!(
        data["progress"]["summaryCode"].is_null() || data["progress"]["summaryCode"].is_string(),
        "missing nullable summaryCode"
    );
    for field in [
        "activityPhase",
        "lastActivityAt",
        "activityAgeMs",
        "activityRevision",
    ] {
        assert!(data["progress"].get(field).is_none());
    }
}

fn assert_query_output_contract(responses: &[Value]) {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let tool = orchestration::descriptors()
        .into_iter()
        .find(|t| t.name == "agent_query")
        .unwrap();
    let mut child = Command::new("node")
        .current_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap())
        .args(["-e", r#"
const Ajv = require('ajv');
let input = '';
process.stdin.on('data', chunk => input += chunk);
process.stdin.on('end', () => {
  const {schema, responses} = JSON.parse(input);
  const validate = new Ajv({allErrors:true, formats:{
    int64: {type:'number', validate:Number.isInteger},
    uint32: {type:'number', validate:n => Number.isInteger(n) && n >= 0 && n <= 4294967295},
    uint64: {type:'number', validate:n => Number.isInteger(n) && n >= 0}
  }}).compile(schema);
  for (const response of responses) {
    if (!validate(response)) throw new Error(JSON.stringify(validate.errors));
    const invalid = [{...response, control:null}, {...response, ok:!response.ok}];
    if (!response.ok) invalid.push({...response, error:{...response.error, executionId:null}});
    if (response.ok && response.data.executions) {
      const row = response.data.executions[0];
      invalid.push({...response, data:{executions:[{...row, prompt:'must be absent'}]}});
      const missing = {...row}; delete missing.revision;
      invalid.push({...response, data:{executions:[missing]}});
    } else if (response.ok && !('prompt' in response.data)) {
      for (const field of ['unchanged', 'revision', 'resultAvailable', 'progress']) {
        const missing = {...response.data}; delete missing[field];
        invalid.push({...response, data:missing});
      }
      for (const extra of [{prompt:'forbidden'}, {unchanged:null}, {error:{}}, {nextAction:null}]) {
        invalid.push({...response, data:{...response.data, ...extra}});
      }
      invalid.push({...response, data:{...response.data, progress:{phase:'running', toolCategory:null}}});
    }
    for (const changed of invalid) if (validate(changed)) throw new Error('invalid response accepted: '+JSON.stringify(changed));
  }
});
"#])
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped())
        .spawn().expect("Query output contract tests require npm install and Node");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(
            json!({"schema":tool.output_schema,"responses":responses})
                .to_string()
                .as_bytes(),
        )
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test]
async fn http_agent_query_compact_views_preserve_revision_persisted_result_and_execute_receipts() {
    use crate::agent::{
        product::AgentQueryAction,
        store::transactions::product::{WorkExecutionContext, WorkspaceSnapshot},
    };
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_string_lossy().into_owned();
    let store = StateStore::open(dir.path().join("state")).await.unwrap();
    store
        .create_work_run(
            "work".into(),
            "W".into(),
            root.clone(),
            1,
            "query fixture".into(),
            None,
            1,
        )
        .await
        .unwrap();
    let prompt = "long prompt excluded from frequent observations / ".repeat(2048);
    store
        .product_create_fresh_with_work(
            "E".into(),
            "work".into(),
            "key".into(),
            prompt.clone(),
            "W".into(),
            Some(WorkspaceSnapshot {
                id: "W".into(),
                root: root.clone(),
                generation: 1,
            }),
            Some(WorkExecutionContext {
                work_run_id: "work".into(),
                parent_execution_id: None,
                delegation_context_json: None,
            }),
            1,
        )
        .await
        .unwrap();
    let broker = fixture(dir.path());
    let product = Arc::new(AgentProductService::new(store.clone()));
    assert!(broker.product.set(product.clone()).is_ok());
    broker.start().await.unwrap();
    let client = ()
        .serve(StreamableHttpClientTransport::from_uri(format!(
            "http://127.0.0.1:{}/mcp",
            broker.config().broker.port
        )))
        .await
        .unwrap();
    let request = |name: &str, args: Value| {
        CallToolRequestParams::new(name.to_string())
            .with_arguments(args.as_object().unwrap().clone())
    };
    let mut samples = Vec::new();
    let before = store.execution("E".into()).await.unwrap();
    let claim = store.workspace_claim(root.clone()).await.unwrap();
    let work = store.work_run("work".into()).await.unwrap();
    let detail = client
        .call_tool(request(
            "agent_query",
            json!({"action":"get","executionId":"E"}),
        ))
        .await
        .unwrap();
    let detail = detail.structured_content.unwrap();
    let internal = product
        .agent_query(AgentQueryAction::Get {
            execution_id: "E".into(),
            include_result: None,
        })
        .await
        .unwrap();
    assert_eq!(detail["data"], serde_json::to_value(internal).unwrap());
    assert_eq!(detail["data"]["prompt"], prompt);
    assert_eq!(detail["data"]["canonicalWorkspaceRoot"], root);
    assert!(detail.get("control").is_none());
    assert!(detail["data"].get("finalResult").is_none());
    samples.push(detail.clone());
    let first = client
        .call_tool(request(
            "agent_query",
            json!({"action":"observe","executionId":"E","waitMs":0,"includeResult":true}),
        ))
        .await
        .unwrap();
    assert_eq!(first.is_error, Some(false));
    let first = first.structured_content.unwrap();
    assert_compact_observation(&first);
    assert_eq!(first["data"]["revision"], detail["data"]["controlRevision"]);
    assert_eq!(first["data"]["unchanged"], false);
    assert_eq!(
        first["data"]["progress"],
        json!({"phase":"pending","summaryCode":null})
    );
    assert_eq!(first["data"]["attention"], "pending_explicit_resume");
    assert!(first["data"].get("finalResult").is_none());
    assert!(first["data"].get("error").is_none());
    assert!(!first.to_string().contains(&prompt));
    let started = tokio::time::Instant::now();
    let same = client.call_tool(request("agent_query", json!({"action":"observe","executionId":"E","knownRevision":first["data"]["revision"],"waitMs":80,"includeResult":false}))).await.unwrap();
    assert!(started.elapsed() >= std::time::Duration::from_millis(80));
    let same = same.structured_content.unwrap();
    assert_compact_observation(&same);
    assert_eq!(same["data"]["unchanged"], true);
    assert_eq!(same["data"]["revision"], first["data"]["revision"]);
    assert!(same["data"].get("finalResult").is_none());
    samples.extend([first, same]);
    let list = client
        .call_tool(request(
            "agent_query",
            json!({"action":"list","workRunId":"work"}),
        ))
        .await
        .unwrap()
        .structured_content
        .unwrap();
    assert_eq!(
        list,
        json!({"ok":true,"data":{"executions":[{
            "executionId":"E","provider":{"id":"codex","displayName":"Codex"},
            "usage":{"inputTokens":null,"cachedInputTokens":null,"cacheWriteInputTokens":null,
                "outputTokens":null,"reasoningTokens":null,"totalTokens":null,"modelContextWindow":null,
                "completeness":"unknown","usageRevision":0,"updatedAt":null},
            "status":"dispatch_pending","dispatchState":"not_dispatched",
            "revision":detail["data"]["controlRevision"],"resultAvailable":false,"resultCompleteness":"unknown",
            "attention":"pending_explicit_resume","nextAction":{"action":"resume_pending"},"createdAt":1
        }]}})
    );
    samples.push(list);
    assert_eq!(store.execution("E".into()).await.unwrap(), before);
    assert_eq!(store.workspace_claim(root.clone()).await.unwrap(), claim);
    assert_eq!(store.work_run("work".into()).await.unwrap(), work);
    assert!(!store.product_worker_owned("E"));

    let db = rusqlite::Connection::open(dir.path().join("state/agent-state.db")).unwrap();
    db.execute("UPDATE executions SET status='running',dispatch_state='dispatched',thread_id='T',turn_id='TURN',last_activity_at=?1,activity_phase='tool',tool_category='test',activity_summary_code='tool.test' WHERE id='E'", [chrono::Utc::now().timestamp_millis()]).unwrap();
    store
        .save_thread_name("T".into(), Some("Test thread".into()))
        .await
        .unwrap();
    let running = client
        .call_tool(request(
            "agent_query",
            json!({"action":"observe","executionId":"E","waitMs":0}),
        ))
        .await
        .unwrap()
        .structured_content
        .unwrap();
    assert_compact_observation(&running);
    assert_eq!(
        running["data"]["progress"],
        json!({"phase":"running","summaryCode":"tool.test","toolCategory":"test","silenceLevel":"fresh"})
    );
    assert_eq!(
        running["data"]["nextAction"],
        json!({"action":"observe","waitMs":20000})
    );
    assert!(running["data"].get("attention").is_none());
    let known_control = running["data"]["revision"].clone();
    let known_activity = running["data"]["activityRevision"].clone();
    let activity_wait = client.call_tool(request(
        "agent_query",
        json!({
            "action":"observe","executionId":"E",
            "knownControlRevision":known_control,
            "knownActivityRevision":known_activity,
            "wakeOn":"activity","waitMs":20000
        }),
    ));
    let activity_change = async {
        tokio::time::sleep(std::time::Duration::from_millis(60)).await;
        store
            .execution_activity(
                "E".into(),
                "T".into(),
                "TURN".into(),
                crate::agent::activity::ActivityPhase::Tool,
                Some(crate::agent::activity::ToolCategory::Read),
                chrono::Utc::now().timestamp_millis(),
            )
            .await
            .unwrap();
    };
    let (activity_wait, ()) = tokio::join!(activity_wait, activity_change);
    let activity_wait = activity_wait.unwrap().structured_content.unwrap();
    assert_compact_observation(&activity_wait);
    assert_eq!(activity_wait["data"]["wakeReason"], "activity");
    assert_eq!(activity_wait["data"]["unchanged"], true);
    assert_eq!(
        activity_wait["data"]["progress"]["summaryCode"],
        "tool.read"
    );
    assert!(activity_wait["data"].get("mismatchKind").is_none());
    samples.push(activity_wait);
    samples.push(running);

    // Restore an undispatched fixture so execute.cancel exercises a real control receipt without a Provider.
    db.execute("UPDATE executions SET status='dispatch_pending',dispatch_state='not_dispatched',thread_id=NULL,turn_id=NULL,last_activity_at=NULL,activity_phase=NULL,tool_category=NULL,activity_summary_code=NULL WHERE id='E'", []).unwrap();
    let cancelled = client
        .call_tool(request(
            "agent_execute",
            json!({"action":"cancel","workRunId":"work","executionId":"E"}),
        ))
        .await
        .unwrap();
    #[cfg(any(windows, target_os = "macos"))]
    {
        assert_eq!(cancelled.is_error, Some(false));
        let cancelled = cancelled.structured_content.unwrap();
        assert_eq!(cancelled["data"]["prompt"], prompt);
        assert_eq!(cancelled["data"]["canonicalWorkspaceRoot"], root);
        assert_eq!(
            cancelled["control"],
            json!({"requestAccepted":true,"providerInvoked":false,"dispatchCertainty":"not_dispatched","nextAction":null})
        );
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        // 未支持平台不伪造取消能力，未派发记录保持原状并返回稳定 unavailable。
        assert_eq!(cancelled.is_error, Some(true));
        let cancelled = cancelled.structured_content.unwrap();
        assert_eq!(cancelled["error"]["code"], "AGENT_PROVIDER_UNAVAILABLE");
        assert_eq!(cancelled["control"]["requestAccepted"], false);
    }

    // Seed an exact persisted terminal result. Reads must neither execute nor consume it.
    let persisted = json!({"text":"exact final result\n中文", "items":[{"id":"item-1","content":"original"}],"nested":{"zero":0,"null":null}});
    db.execute("UPDATE executions SET status='completed',dispatch_state='dispatched',thread_id='T',turn_id='TURN',provider_terminal_status='completed',result_completeness='complete',final_result_json=?1,completed_at=42 WHERE id='E'", [persisted.to_string()]).unwrap();
    let terminal_before = store.execution("E".into()).await.unwrap();
    for include in [false, true, true, false] {
        for action in ["get", "observe"] {
            let mut args = json!({"action":action,"executionId":"E","includeResult":include});
            if action == "observe" {
                args["waitMs"] = json!(0);
            }
            let response = client
                .call_tool(request("agent_query", args))
                .await
                .unwrap()
                .structured_content
                .unwrap();
            if action == "observe" {
                assert_compact_observation(&response);
            } else {
                assert_eq!(response["data"]["prompt"], prompt);
                assert_eq!(response["data"]["canonicalWorkspaceRoot"], root);
            }
            assert!(response.get("control").is_none());
            assert_eq!(response["data"]["status"], "completed");
            assert_eq!(response["data"]["resultAvailable"], true);
            assert_eq!(response["data"]["resultCompleteness"], "complete");
            if include {
                assert_eq!(response["data"]["finalResult"], persisted);
            } else {
                assert!(response["data"].get("finalResult").is_none());
            }
            if action == "observe" && include {
                assert!(response["data"].get("nextAction").is_none());
            }
            samples.push(response);
        }
    }
    let terminal_detail = samples.last().unwrap();
    let terminal_list = client
        .call_tool(request(
            "agent_query",
            json!({"action":"list","workRunId":"work"}),
        ))
        .await
        .unwrap()
        .structured_content
        .unwrap();
    assert_eq!(
        terminal_list,
        json!({"ok":true,"data":{"executions":[{
            "executionId":"E","provider":{"id":"codex","displayName":"Codex"},
            "usage":{"inputTokens":null,"cachedInputTokens":null,"cacheWriteInputTokens":null,
                "outputTokens":null,"reasoningTokens":null,"totalTokens":null,"modelContextWindow":null,
                "completeness":"unknown","usageRevision":0,"updatedAt":null},
            "status":"completed","dispatchState":"dispatched","revision":terminal_detail["data"]["revision"],
            "resultAvailable":true,"resultCompleteness":"complete","attention":"none","threadName":"Test thread",
            "nextAction":{"action":"review_result","includeResult":true},"createdAt":1,"completedAt":42
        }]}})
    );
    samples.push(terminal_list);
    assert_eq!(store.execution("E".into()).await.unwrap(), terminal_before);
    assert_eq!(store.work_run("work".into()).await.unwrap(), work);
    #[cfg(any(windows, target_os = "macos"))]
    assert!(store.workspace_claim(root.clone()).await.unwrap().is_none());
    // 未支持平台的后端拒绝取消后不得释放缺少 Runtime 终止证据的 Claim。
    #[cfg(not(any(windows, target_os = "macos")))]
    assert!(store.workspace_claim(root).await.unwrap().is_some());
    assert_eq!(
        db.query_row("SELECT count(*) FROM runtime_instances", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        0
    );

    for (args, code) in [
        (
            json!({"action":"get","executionId":"missing"}),
            "AGENT_EXECUTION_NOT_FOUND",
        ),
        (
            json!({"action":"observe","executionId":"E","waitMs":20001}),
            "AGENT_OBSERVE_INVALID_ARGUMENT",
        ),
        (
            json!({"action":"observe","executionId":"bad id","waitMs":0}),
            "AGENT_OBSERVE_INVALID_ARGUMENT",
        ),
        (
            json!({"action":"observe","executionId":"bad\nid","waitMs":0}),
            "AGENT_OBSERVE_INVALID_ARGUMENT",
        ),
        (
            json!({"action":"observe","executionId":"E","knownRevision":""}),
            "AGENT_OBSERVE_INVALID_ARGUMENT",
        ),
        (
            json!({"action":"observe","executionId":"E","knownControlRevision":""}),
            "AGENT_OBSERVE_INVALID_ARGUMENT",
        ),
        (
            json!({"action":"observe","executionId":"E","knownActivityRevision":""}),
            "AGENT_OBSERVE_INVALID_ARGUMENT",
        ),
        (
            json!({"action":"observe","executionId":"E","wakeOn":"unknown"}),
            "AGENT_OBSERVE_INVALID_ARGUMENT",
        ),
    ] {
        let failure = client
            .call_tool(request("agent_query", args))
            .await
            .unwrap();
        assert_eq!(failure.is_error, Some(true));
        let failure = failure.structured_content.unwrap();
        assert_eq!(failure.as_object().unwrap().len(), 2);
        assert!(failure.get("control").is_none());
        assert_eq!(failure["error"]["code"], code);
        assert_eq!(failure["error"]["message"], failure["error"]["code"]);
        samples.push(failure);
    }
    assert_query_output_contract(&samples);
    client.cancel().await.unwrap();
    broker.stop().await.unwrap();
}

#[test]
fn typed_registry_validation_rejects_cross_action_fields_and_preserves_context_authority() {
    for (name, args) in [
        (
            "work_query",
            json!({"action":"get","workRunId":"w","limit":1}),
        ),
        (
            "work_update",
            json!({"action":"finish","workRunId":"w","outcome":"cancelled"}),
        ),
        (
            "work_update",
            json!({"action":"finish","workRunId":"w","outcome":"completed","acceptance":{"summary":"s","executionIds":[],"acceptedAt":1}}),
        ),
        (
            "agent_query",
            json!({"action":"observe","executionId":"e","wakeOn":"unknown"}),
        ),
        (
            "agent_execute",
            json!({"action":"start","workRunId":"w","workspaceId":"W","requestKey":"k","prompt":"p","delegationContextJson":"{}"}),
        ),
        (
            "agent_execute",
            json!({"action":"resume_pending","workRunId":"w","executionId":"e","context":{}}),
        ),
        (
            "agent_execute",
            json!({"action":"start","workRunId":"w","workspaceId":"W","requestKey":"k","prompt":"p","context":{"unknown":true}}),
        ),
    ] {
        assert!(registry::validate(name, &args).is_err(), "{name} {args}");
    }
    for n in [0, 101] {
        for name in ["work_query", "agent_query"] {
            let mut args = json!({"action":"list","limit":n});
            if name == "agent_query" {
                args["workRunId"] = json!("w");
            }
            assert!(registry::validate(name, &args).is_err());
        }
    }
    assert!(
        registry::validate(
            "agent_query",
            &json!({"action":"observe","executionId":"e","waitMs":20001})
        )
        .is_err()
    );
    for args in [
        json!({"action":"observe","executionId":"e","knownControlRevision":"control","knownActivityRevision":"activity","wakeOn":"activity","waitMs":0}),
        json!({"action":"observe","executionId":"e","waitMs":20000}),
    ] {
        assert!(registry::validate("agent_query", &args).is_ok(), "{args}");
    }
    let tools = orchestration::descriptors();
    let execute = tools
        .iter()
        .find(|tool| tool.name == "agent_execute")
        .unwrap();
    assert_eq!(
        execute.input_schema["$defs"]["Context"]["properties"]["summary"]["type"],
        "string"
    );
    assert_eq!(
        registry::validate(
            "agent_execute",
            &json!({"action":"start","workRunId":"w","requestKey":"k","prompt":"p"})
        )
        .unwrap_err(),
        "WORKSPACE_CONTEXT_REQUIRED"
    );
    for workspace_id in [json!(""), json!(false)] {
        assert!(
            registry::validate("agent_execute", &json!({"action":"start","workRunId":"w","workspaceId":workspace_id,"requestKey":"k","prompt":"p"}))
                .unwrap_err()
                .starts_with("INVALID_PARAMS")
        );
    }
    // Continue 的 Authority 仅来自 parentExecutionId；公共 DTO 不接受第二个 Workspace 输入。
    assert!(registry::validate("agent_execute", &json!({"action":"continue","workRunId":"w","parentExecutionId":"e","workspaceId":"W","requestKey":"k","prompt":"p"})).is_err());
    assert!(registry::validate("agent_execute",&json!({"action":"start","workRunId":"w","workspaceId":"W","requestKey":"k","prompt":"p","context":{"summary":null}})).is_err());
    // This is structurally valid transport input. Phase 6 must reject its values.
    assert!(registry::validate("agent_execute",&json!({"action":"start","workRunId":"w","workspaceId":"W","requestKey":"k","prompt":"p","context":{"files":[{"path":"../escape","sha256":"bad"}]}})).is_ok());
    assert_eq!(
        registry::validate("agent", &json!({"action":"list"})).unwrap_err(),
        "UNKNOWN_TOOL"
    );
}

#[test]
fn work_output_requires_all_fields_and_preserves_nullable_values() {
    for tool in orchestration::descriptors()
        .into_iter()
        .filter(|t| t.name.starts_with("work_"))
    {
        let schema = serde_json::to_value(tool.output_schema.unwrap()).unwrap();
        let view = &schema["$defs"]["WorkRunView"];
        let fields = [
            "workRunId",
            "workspaceId",
            "title",
            "goal",
            "status",
            "revision",
            "acceptance",
            "createdAt",
            "updatedAt",
            "completedAt",
        ];
        assert_eq!(view["properties"].as_object().unwrap().len(), fields.len());
        assert_eq!(view["required"].as_array().unwrap().len(), fields.len());
        for field in fields {
            assert!(view["required"].as_array().unwrap().contains(&json!(field)));
        }
        for field in ["goal", "completedAt"] {
            assert!(
                view["properties"][field]["type"]
                    .as_array()
                    .unwrap()
                    .contains(&json!("null"))
            );
        }
        assert!(
            view["properties"]["acceptance"]["anyOf"]
                .as_array()
                .unwrap()
                .iter()
                .any(|s| s["type"] == "null")
        );
        assert_eq!(schema["anyOf"][0]["properties"]["ok"]["const"], true);
        assert_eq!(schema["anyOf"][1]["properties"]["ok"]["const"], false);
    }
}

#[test]
fn manual_resolution_is_not_a_remote_mcp_mutation() {
    assert!(
        !orchestration::descriptors()
            .iter()
            .any(|tool| tool.name.contains("manual_resolve"))
    );
    assert!(
        registry::validate(
            "agent_execute",
            &json!({"action":"manual_resolution","executionId":"e"}),
        )
        .is_err()
    );
}
