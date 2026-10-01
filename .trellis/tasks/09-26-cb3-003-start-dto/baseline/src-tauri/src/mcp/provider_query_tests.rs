//! CB3-002：真实 MCP transport 的只读目录契约与副作用边界。
use super::*;
use crate::agent::{
    provider::{
        ProviderCancelContext, ProviderCapabilities, ProviderDescriptor, ProviderError,
        ProviderExecutionContext, ProviderId, ProviderRunResult, ProviderStartupContext,
        port::{
            AgentEventSink, AgentProvider, ProviderAcceptanceSink, ProviderExecutionFailure,
            ProviderFuture, ProviderReconcileSummary,
        },
        registry::{ProviderHealth, ProviderRegistry},
    },
    task_manager::AgentTaskManager,
};
use std::sync::atomic::{AtomicBool, Ordering};

/// 目录可读取元数据；任何执行、Session、ACP 或恢复入口都会立即使测试失败。
struct ReadOnlyProvider {
    invalid_identity: AtomicBool,
}

impl AgentProvider for ReadOnlyProvider {
    /// 只改变返回身份以注入读取失败，Registry 与 health 本身保持不变。
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: ProviderId::new(if self.invalid_identity.load(Ordering::SeqCst) {
                "missing".into()
            } else {
                "fixture".into()
            })
            .unwrap(),
            display_name: "Read-only fixture".into(),
            version: None,
        }
    }
    /// 未证明的能力保持 false；查询不得通过运行 Provider 来探测能力。
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            can_execute: true,
            can_continue: false,
            can_cancel: false,
            can_recover: false,
            activity: false,
            token_usage: false,
        }
    }
    /// 禁止执行，包括 Runtime/ACP/Session 创建。
    fn execute<'a>(
        &'a self,
        _: ProviderExecutionContext,
        _: Arc<dyn ProviderAcceptanceSink>,
        _: Arc<dyn AgentEventSink>,
    ) -> ProviderFuture<'a, Result<ProviderRunResult, ProviderExecutionFailure>> {
        panic!("providers must not execute or connect ACP")
    }
    /// 禁止取消已有执行。
    fn cancel<'a>(
        &'a self,
        _: ProviderCancelContext,
    ) -> ProviderFuture<'a, Result<(), ProviderError>> {
        panic!("providers must not cancel")
    }
    /// 禁止通过恢复刷新 Provider 状态。
    fn startup_reconcile<'a>(
        &'a self,
        _: ProviderStartupContext,
    ) -> ProviderFuture<'a, Result<ProviderReconcileSummary, ProviderError>> {
        panic!("providers must not reconcile")
    }
}

/// 比较完整持久化内容，既能检测创建也能检测已有行被改写。
fn durable_rows(root: &std::path::Path) -> Vec<Vec<Vec<rusqlite::types::Value>>> {
    let connection = rusqlite::Connection::open(root.join("agent-state.db")).unwrap();
    ["executions", "workspace_claims", "runtime_instances"]
        .iter()
        .map(|table| {
            let mut statement = connection
                .prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))
                .unwrap();
            let columns = statement.column_count();
            statement
                .query_map([], |row| {
                    (0..columns).map(|column| row.get(column)).collect()
                })
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap()
        })
        .collect()
}

/// DTO 和实际公布 schema 使用同一正负矩阵，不能只验证 serde 一层。
#[test]
fn providers_strict_dto_schema_and_descriptor_contract() {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let tool = orchestration::descriptors()
        .into_iter()
        .find(|tool| tool.name == "agent_query")
        .unwrap();
    let valid = json!({"action":"providers"});
    let mut invalid = vec![
        json!({}),
        json!({"action":null}),
        json!({"action":"Providers"}),
    ];
    for field in [
        "workspaceId",
        "agentId",
        "executionId",
        "workRunId",
        "limit",
        "enabled",
        "roleRouting",
        "refreshHealth",
        "providerId",
        "unknown",
    ] {
        for value in [Value::Null, json!("W"), json!(true), json!(1), json!({})] {
            let mut args = valid.clone();
            args[field] = value;
            assert_eq!(
                registry::validate("agent_query", &args).unwrap_err(),
                "INVALID_PARAMS",
                "{args}"
            );
            invalid.push(args);
        }
    }
    assert!(registry::validate("agent_query", &valid).is_ok());
    let annotations = tool.annotations.as_ref().unwrap();
    assert_eq!(annotations.read_only_hint, Some(true));
    assert_eq!(annotations.destructive_hint, Some(false));
    assert_eq!(annotations.open_world_hint, Some(false));
    // 将真实 descriptor 写入测试证据；固定 hash 由既有 registry gate 验证。
    println!(
        "CB3_PROVIDER_DESCRIPTOR={}",
        serde_json::to_string(&tool).unwrap()
    );
    let mut child = Command::new("node")
        .current_dir(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap(),
        )
        .args([
            "-e",
            r#"
const Ajv = require('ajv'); let text='';
process.stdin.on('data', c => text += c);
process.stdin.on('end', () => {
  const {schema, valid, invalid} = JSON.parse(text); delete schema.$schema;
  const check = new Ajv({formats:{uint32:{type:'number',validate:n => Number.isInteger(n) && n >= 0 && n <= 4294967295}}}).compile(schema);
  if (!check(valid)) throw Error(JSON.stringify(check.errors));
  for (const value of invalid) if (check(value)) throw Error('accepted '+JSON.stringify(value));
});
"#,
        ])
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(
            json!({"schema":tool.input_schema,"valid":valid,"invalid":invalid})
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

/// Remote registry 不发布任何 Provider mutation，已有 Start 字段仍不能使用新路由参数。
#[test]
fn providers_remote_registry_has_no_mutation_and_start_contract_is_unchanged() {
    for agent_enabled in [false, true] {
        let tools = registry::list_with_capabilities(agent_enabled, true, true);
        assert_eq!(
            tools.iter().any(|tool| tool.name == "agent_query"),
            agent_enabled
        );
        assert!(!tools.iter().any(|tool| tool.name.contains("provider")));
    }
    for action in [
        "set_enabled",
        "set_role_route",
        "refresh_health",
        "refreshHealth",
    ] {
        assert!(registry::validate("agent_query", &json!({"action":action})).is_err());
    }
    assert!(registry::validate("agent_execute", &json!({"action":"start","workRunId":"w","workspaceId":"W","requestKey":"k","prompt":"p","taskRole":"general","providerId":"codex"})).is_err());
}

/// 真实 HTTP call_tool 返回 Product JSON，所有成功、非法输入和读取失败均保持状态不变。
#[tokio::test]
async fn http_providers_product_json_strict_input_and_read_failures_are_side_effect_free() {
    let dir = tempfile::tempdir().unwrap();
    let broker = fixture(dir.path());
    let state = dir.path().join("state");
    let store = StateStore::open(state.clone()).await.unwrap();
    // 保留一个已有 Execution/Claim，防止空表断言漏掉修改已有状态的副作用。
    store
        .product_create_fresh(
            "E".into(),
            "A".into(),
            "K".into(),
            "prompt".into(),
            "W".into(),
            Some(
                crate::agent::store::transactions::product::WorkspaceSnapshot {
                    id: "W".into(),
                    root: dir.path().to_string_lossy().into(),
                    generation: 1,
                },
            ),
            10,
        )
        .await
        .unwrap();
    let adapter = Arc::new(ReadOnlyProvider {
        invalid_identity: AtomicBool::new(false),
    });
    let mut registry = ProviderRegistry::new();
    registry
        .register(adapter.clone(), ProviderHealth::Unavailable)
        .unwrap();
    let mut manager = AgentTaskManager::new(store.clone(), PathBuf::new());
    manager.use_registry(registry);
    let registry = manager.registry().unwrap();
    #[cfg(windows)]
    {
        *manager.runtime_pool.test_connect.lock().unwrap() = Some(Arc::new(|_, _| {
            panic!("providers must not connect Runtime")
        }));
    }
    let product = Arc::new(AgentProductService::new_with_manager_for_test(
        store, manager,
    ));
    assert!(broker.product.set(product.clone()).is_ok());
    let mut config = broker.config();
    config.agent_providers.providers.insert(
        "fixture".into(),
        crate::config::AgentProviderPolicy { enabled: true },
    );
    config
        .agent_providers
        .role_routing
        .insert("review".into(), None);
    broker.supervisor.replace_config(config).unwrap();
    let expected =
        serde_json::to_value(product.provider_catalog(&broker.supervisor).unwrap()).unwrap();
    let before = durable_rows(&state);
    assert_eq!(before[0].len(), 1);
    assert_eq!(before[1].len(), 1);
    assert!(before[2].is_empty());
    let config = broker.config();
    let policy_bytes = std::fs::read(&broker.supervisor.paths.config_file).unwrap();
    assert!(config.workspaces.is_empty());
    assert!(broker.workspace.read().await.is_none());
    broker.start().await.unwrap();
    let client = ()
        .serve(StreamableHttpClientTransport::from_uri(format!(
            "http://127.0.0.1:{}/mcp",
            broker.config().broker.port
        )))
        .await
        .unwrap();
    let request = |args: Value| {
        CallToolRequestParams::new("agent_query").with_arguments(args.as_object().unwrap().clone())
    };
    let tools = client.list_all_tools().await.unwrap();
    assert!(!tools.iter().any(|tool| tool.name.contains("provider")));
    let output = client
        .call_tool(request(json!({"action":"providers"})))
        .await
        .unwrap();
    assert_eq!(output.is_error, Some(false));
    let response = output.structured_content.unwrap();
    assert_eq!(response, json!({"ok":true,"data":expected}));
    // 同一 JSON 同时覆盖 Broker router 与真实 HTTP envelope。
    assert_eq!(
        call(&broker, "agent_query", json!({"action":"providers"})).await,
        response
    );
    let mut responses = vec![response];
    for field in [
        "workspaceId",
        "agentId",
        "executionId",
        "limit",
        "enabled",
        "roleRouting",
        "refreshHealth",
        "unknown",
    ] {
        let mut args = json!({"action":"providers"});
        args[field] = Value::Null;
        let rejected = client.call_tool(request(args)).await.unwrap();
        assert_eq!(rejected.is_error, Some(true));
        let rejected = rejected.structured_content.unwrap();
        assert_eq!(rejected["error"]["code"], "INVALID_PARAMS");
        responses.push(rejected);
    }
    adapter.invalid_identity.store(true, Ordering::SeqCst);
    let failed = client
        .call_tool(request(json!({"action":"providers"})))
        .await
        .unwrap();
    assert_eq!(failed.is_error, Some(true));
    let failed = failed.structured_content.unwrap();
    assert_eq!(
        failed,
        json!({"ok":false,"error":{"code":"AGENT_OPERATION_FAILED","message":"AGENT_OPERATION_FAILED"}})
    );
    responses.push(failed);
    assert_query_output_contract(&responses);
    assert_eq!(durable_rows(&state), before);
    assert_eq!(broker.config(), config);
    assert_eq!(
        std::fs::read(&broker.supervisor.paths.config_file).unwrap(),
        policy_bytes
    );
    assert_eq!(
        registry
            .health(&ProviderId::new("fixture".into()).unwrap())
            .unwrap(),
        ProviderHealth::Unavailable
    );
    assert!(broker.workspace.read().await.is_none());
    client.cancel().await.unwrap();
    broker.stop().await.unwrap();
}

/// HTTP 路径与 CB3-001 的真实 Codex Product fixture 完全一致，不另造目录 JSON。
#[cfg(windows)]
#[tokio::test]
async fn http_providers_reuses_cb3_001_codex_product_fixture_without_runtime() {
    let dir = tempfile::tempdir().unwrap();
    let broker = fixture(dir.path());
    let store = StateStore::open(dir.path().join("state")).await.unwrap();
    let manager = AgentTaskManager::new(store.clone(), PathBuf::new());
    *manager.runtime_pool.test_connect.lock().unwrap() =
        Some(Arc::new(|_, _| panic!("catalog must not connect Codex")));
    assert!(
        broker
            .product
            .set(Arc::new(AgentProductService::new_with_manager_for_test(
                store, manager
            )))
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
    let result = client
        .call_tool(
            CallToolRequestParams::new("agent_query")
                .with_arguments(json!({"action":"providers"}).as_object().unwrap().clone()),
        )
        .await
        .unwrap();
    assert_eq!(result.is_error, Some(false));
    let result = result.structured_content.unwrap();
    let expected: Value = serde_json::from_str(include_str!(
        "../agent/product/fixtures/provider_catalog_codex.json"
    ))
    .unwrap();
    assert_eq!(result, json!({"ok":true,"data":expected}));
    println!("CB3_PROVIDER_INPUT={{\"action\":\"providers\"}}");
    println!("CB3_PROVIDER_OUTPUT={result}");
    assert_query_output_contract(&[result]);
    assert!(
        durable_rows(&dir.path().join("state"))
            .iter()
            .all(Vec::is_empty)
    );
    assert!(broker.workspace.read().await.is_none());
    client.cancel().await.unwrap();
    broker.stop().await.unwrap();
}
