use super::*;
use crate::agent::{product::AgentProductService, store::StateStore};
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
    config.broker.port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
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
                let mut tools: Vec<_> = registry::SOURCES
                    .iter()
                    .map(|(_, name, fields, _)| {
                        let mut properties = serde_json::Map::new();
                        for field in *fields {
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
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
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
    };
    let graph = codegraph::Binding::begin(&workspace, 1, broker.logs.clone());
    *broker.workspace.write().await = Some(Active {
        workspace,
        client,
        pid: if allow_read {
            broker.supervisor.snapshot().process_id.unwrap()
        } else {
            1
        },
        graph,
        generation: 1,
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
        json!({"action":"start","agentId":"a","requestKey":"k","prompt":"p"}),
        json!({"action":"list","limit":101}),
    ] {
        // Internal Tauri/legacy entry still uses the same internal Product contract.
        assert_eq!(
            broker.agent_operation(request.clone()).await,
            broker.product.get().unwrap().operation(request, None).await
        );
    }
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
            json!({"action":"begin","workspaceId":"W","title":"title"}),
            "WORKSPACE_CONTEXT_MISMATCH",
        ),
        (
            "agent_query",
            json!({"action":"get","executionId":"missing"}),
            "AGENT_EXECUTION_NOT_FOUND",
        ),
        (
            "agent_execute",
            json!({"action":"start","workRunId":"missing","requestKey":"k","prompt":"p"}),
            "WORK_NOT_FOUND",
        ),
    ] {
        let result = client.call_tool(request(name, args)).await.unwrap();
        assert_eq!(result.is_error, Some(true));
        assert_eq!(result.structured_content.unwrap()["error"]["code"], code);
        let malformed = client
            .call_tool(request(
                name,
                json!({"action":"invalid","secret":"SQL path secret"}),
            ))
            .await
            .unwrap();
        assert_eq!(malformed.is_error, Some(true));
        assert_eq!(
            malformed.structured_content.unwrap()["error"]["code"],
            "WORK_INVALID_ARGUMENT"
        );
    }
    let server = active(&broker, dir.path()).await;
    assert_eq!(
        call(
            &broker,
            "work_update",
            json!({"action":"begin","workspaceId":"wrong","title":"title"})
        )
        .await["error"]["code"],
        "WORKSPACE_CONTEXT_MISMATCH"
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
    broker.workspace.write().await.take();
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
    server.abort();
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
            json!({"action":"observe","executionId":"e","wakeOn":"activity"}),
        ),
        (
            "agent_query",
            json!({"action":"observe","executionId":"e","knownControlRevision":"r"}),
        ),
        (
            "agent_execute",
            json!({"action":"start","workRunId":"w","requestKey":"k","prompt":"p","delegationContextJson":"{}"}),
        ),
        (
            "agent_execute",
            json!({"action":"resume_pending","workRunId":"w","executionId":"e","context":{}}),
        ),
        (
            "agent_execute",
            json!({"action":"start","workRunId":"w","requestKey":"k","prompt":"p","context":{"unknown":true}}),
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
    let tools = orchestration::descriptors();
    let execute = tools
        .iter()
        .find(|tool| tool.name == "agent_execute")
        .unwrap();
    assert_eq!(
        execute.input_schema["$defs"]["Context"]["properties"]["summary"]["type"],
        "string"
    );
    assert!(registry::validate("agent_execute",&json!({"action":"start","workRunId":"w","requestKey":"k","prompt":"p","context":{"summary":null}})).is_err());
    // This is structurally valid transport input. Phase 6 must reject its values.
    assert!(registry::validate("agent_execute",&json!({"action":"start","workRunId":"w","requestKey":"k","prompt":"p","context":{"files":[{"path":"../escape","sha256":"bad"}]}})).is_ok());
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
