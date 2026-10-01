use super::*;
use crate::{
    agent::provider::{
        ProviderCancelContext, ProviderCapabilities, ProviderError, ProviderErrorCode,
        ProviderExecutionContext, ProviderRunResult, ProviderStartupContext,
        port::{
            AgentEventSink, AgentProvider, ProviderAcceptanceSink, ProviderExecutionFailure,
            ProviderFuture, ProviderReconcileSummary,
        },
        registry::{ProviderHealth, ProviderRegistry},
    },
    config::{AgentProviderPolicy, AppPaths, ManagerConfig},
    serena::SupervisorState,
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// 测试 Provider 只允许元数据读取；任何生命周期调用立即失败。
struct CatalogProvider {
    descriptor: ProviderDescriptor,
    capabilities: ProviderCapabilities,
    invalid_descriptor: AtomicBool,
    admission_diagnostic: Option<String>,
}
impl AgentProvider for CatalogProvider {
    /// 可控身份漂移用于触发 Registry 读取失败，不修改 Registry 自身。
    fn descriptor(&self) -> ProviderDescriptor {
        let mut descriptor = self.descriptor.clone();
        if self.invalid_descriptor.load(Ordering::SeqCst) {
            descriptor.id = ProviderId::new("missing".into()).unwrap();
        }
        descriptor
    }
    /// 返回冻结声明，覆盖不能提前宣传的能力。
    fn capabilities(&self) -> ProviderCapabilities {
        self.capabilities.clone()
    }
    /// 只返回 fixture 明确声明的 provider-neutral admission diagnostic。
    fn admission_diagnostic(&self) -> Option<String> {
        self.admission_diagnostic.clone()
    }
    /// 目录查询不允许进入执行，包括创建 Session。
    fn execute<'a>(
        &'a self,
        _: ProviderExecutionContext,
        _: Arc<dyn ProviderAcceptanceSink>,
        _: Arc<dyn AgentEventSink>,
    ) -> ProviderFuture<'a, Result<ProviderRunResult, ProviderExecutionFailure>> {
        panic!("catalog must not execute")
    }
    /// 目录查询不允许取消执行。
    fn cancel<'a>(
        &'a self,
        _: ProviderCancelContext,
    ) -> ProviderFuture<'a, Result<(), ProviderError>> {
        panic!("catalog must not cancel")
    }
    /// 目录查询不允许触发恢复或健康更新。
    fn startup_reconcile<'a>(
        &'a self,
        _: ProviderStartupContext,
    ) -> ProviderFuture<'a, Result<ProviderReconcileSummary, ProviderError>> {
        panic!("catalog must not reconcile")
    }
}

/// 写入真实 Local Human 配置，再由 Supervisor 加载。
fn supervisor(directory: &std::path::Path, config: &ManagerConfig) -> SupervisorState {
    let paths = AppPaths {
        runtime_directory: directory.join("runtime"),
        config_file: directory.join("config.json"),
        log_directory: directory.join("logs"),
        app_log: directory.join("logs/app.log"),
        serena_log: directory.join("logs/serena.log"),
    };
    crate::config::save(&paths.config_file, config).unwrap();
    SupervisorState::new(paths).unwrap()
}

/// 构造无 Runtime 的通用 Provider。
fn provider(id: &str, can_execute: bool) -> Arc<CatalogProvider> {
    Arc::new(CatalogProvider {
        descriptor: ProviderDescriptor {
            id: ProviderId::new(id.into()).unwrap(),
            display_name: format!("Catalog {id}"),
            version: None,
            protocol: None,
        },
        capabilities: ProviderCapabilities {
            can_execute,
            can_continue: false,
            can_cancel: true,
            can_recover: false,
            activity: true,
            token_usage: false,
        },
        invalid_descriptor: AtomicBool::new(false),
        admission_diagnostic: None,
    })
}

/// 构造带确定性 admission diagnostic 的通用目录 Provider。
fn provider_with_diagnostic(id: &str, diagnostic: &str) -> Arc<CatalogProvider> {
    let mut provider = Arc::try_unwrap(provider(id, true)).unwrap_or_else(|_| unreachable!());
    provider.admission_diagnostic = Some(diagnostic.into());
    Arc::new(provider)
}

/// 读取完整持久化行，检测已有 Execution/Claim 被修改及新 Runtime 被创建。
fn durable_snapshot(directory: &std::path::Path) -> Vec<Vec<Vec<rusqlite::types::Value>>> {
    let db = rusqlite::Connection::open(directory.join("agent-state.db")).unwrap();
    ["executions", "workspace_claims", "runtime_instances"]
        .iter()
        .map(|table| {
            let mut statement = db
                .prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))
                .unwrap();
            let count = statement.column_count();
            statement
                .query_map([], |row| (0..count).map(|i| row.get(i)).collect())
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap()
        })
        .collect()
}

/// 真实 Codex adapter 的声明必须形成稳定 camelCase JSON，且不触发连接。
#[cfg(windows)]
#[tokio::test]
async fn codex_available_enabled_json_fixture() {
    let directory = tempfile::tempdir().unwrap();
    let store = StateStore::open(directory.path().into()).await.unwrap();
    let service = AgentProductService::new(store);
    *service.manager.runtime_pool.test_connect.lock().unwrap() =
        Some(Arc::new(|_, _| panic!("catalog must not connect runtime")));
    let supervisor = supervisor(
        directory.path(),
        &ManagerConfig {
            agent_enabled: true,
            ..Default::default()
        },
    );
    let value = crate::agent::codebuddy::TEST_DISCOVERY
        .scope(
            Err(crate::agent::codebuddy::discovery::DiscoveryError::not_found(false)),
            async { serde_json::to_value(service.provider_catalog(&supervisor).unwrap()).unwrap() },
        )
        .await;
    let mut expected: Value =
        serde_json::from_str(include_str!("fixtures/provider_catalog_codex.json")).unwrap();
    // Fresh Execute、Activity、Cancel 与 Job recovery 仅在通过 native Windows Gate 的平台声明。
    expected["providers"][0]["capabilities"]["canExecute"] =
        json!(cfg!(any(windows, target_os = "macos")));
    expected["providers"][0]["capabilities"]["canContinue"] =
        json!(cfg!(any(windows, target_os = "macos")));
    expected["providers"][0]["capabilities"]["activity"] =
        json!(cfg!(any(windows, target_os = "macos")));
    expected["providers"][0]["capabilities"]["canRecover"] =
        json!(cfg!(any(windows, target_os = "macos")));
    expected["providers"][0]["capabilities"]["canCancel"] =
        json!(cfg!(any(windows, target_os = "macos")));
    assert_eq!(value, expected);
    assert!(durable_snapshot(directory.path()).iter().all(Vec::is_empty));
}

/// 真实 CodeBuddy 的平台能力快照覆盖 disabled、CLI 缺失以及 refresh 重建。
#[tokio::test]
async fn codebuddy_catalog_and_refresh_preserve_capability_truth() {
    use crate::agent::codebuddy::{
        TEST_DISCOVERY,
        discovery::{DiscoveryError, DiscoveryResult, MetadataStatus},
    };

    for enabled in [true, false] {
        let directory = tempfile::tempdir().unwrap();
        let store = StateStore::open(directory.path().into()).await.unwrap();
        let service = AgentProductService::new(store);
        let mut config = ManagerConfig {
            agent_enabled: true,
            ..Default::default()
        };
        config
            .agent_providers
            .providers
            .insert("codebuddy".into(), AgentProviderPolicy { enabled });
        let supervisor = supervisor(directory.path(), &config);
        let id = ProviderId::new("codebuddy".into()).unwrap();
        let codex_id = ProviderId::new("codex".into()).unwrap();
        let initial = TEST_DISCOVERY
            .scope(Err(DiscoveryError::not_found(false)), async {
                service.manager.registry().unwrap()
            })
            .await;
        let codex = initial.get_registered(&codex_id).unwrap();
        let codex_health = initial.health(&codex_id).unwrap();

        // 同一 Registry 经 missing→found→found-without-version→missing，能力不随 health 改变。
        for (found, version) in [
            (false, None),
            (true, Some("2.158.0")),
            (true, None),
            (false, None),
        ] {
            let discovery = if found {
                let mut discovery =
                    DiscoveryResult::direct_for_test("C:/resolved/build-deadbeef/codebuddy.exe");
                discovery.metadata.product_version = version.map(str::to_owned);
                discovery.metadata.base_version = Some("1.106.1".into());
                discovery.metadata.package_version = Some("0.0.0-deadbeef".into());
                discovery.metadata.status = MetadataStatus::Parsed;
                Ok(discovery)
            } else {
                Err(DiscoveryError::not_found(false))
            };
            let before = service.manager.registry().unwrap();
            let health = TEST_DISCOVERY
                .scope(discovery, service.refresh_provider_health(id.clone()))
                .await
                .unwrap();
            assert_eq!(
                health,
                if found {
                    ProviderHealth::Available
                } else {
                    ProviderHealth::Unavailable
                }
            );
            let registry = service.manager.registry().unwrap();
            let registered = registry.get_registered(&id).unwrap();
            assert!(!Arc::ptr_eq(
                &before.get_registered(&id).unwrap(),
                &registered
            ));
            assert_eq!(registered.descriptor().version.as_deref(), version);
            assert_eq!(registered.descriptor().protocol.as_deref(), Some("ACP v1"));
            // 刷新只替换目标 adapter，Codex 注册、descriptor、capability 与 health 保持原值。
            assert!(Arc::ptr_eq(
                &codex,
                &registry.get_registered(&codex_id).unwrap()
            ));
            assert_eq!(registry.health(&codex_id).unwrap(), codex_health);
            let catalog = service.provider_catalog(&supervisor).unwrap();
            assert_eq!(catalog.providers.len(), 2);
            let entry = catalog
                .providers
                .iter()
                .find(|entry| entry.id == id)
                .unwrap();
            // 平台适配内联快照：Windows Job Gate 不外推到其他编译目标。
            let mut expected = json!({
                "id": "codebuddy",
                "displayName": "CodeBuddy",
                "protocol": "ACP v1",
                "enabled": enabled,
                "health": if found { "available" } else { "unavailable" },
                "availableForNewExecution": cfg!(any(windows, target_os = "macos")) && found && enabled,
                "capabilities": {
                    "canExecute": cfg!(any(windows, target_os = "macos")),
                    "canContinue": cfg!(any(windows, target_os = "macos")),
                    "canCancel": cfg!(any(windows, target_os = "macos")),
                    "canRecover": cfg!(any(windows, target_os = "macos")),
                    "activity": cfg!(any(windows, target_os = "macos")),
                    "tokenUsage": false
                }
            });
            if let Some(version) = version {
                expected["version"] = json!(version);
            }
            assert_eq!(serde_json::to_value(entry).unwrap(), expected);
        }
        assert!(durable_snapshot(directory.path()).iter().all(Vec::is_empty));
    }
}

/// public availableActions 必须组合 core、当前 policy/health 与真实 CodeBuddy private S1 validation。
#[cfg(windows)]
#[tokio::test]
async fn codebuddy_product_continue_action_requires_exact_private_source_and_current_admission() {
    use crate::agent::{
        codebuddy::{
            discovery::DiscoveryResult, provider::register_codebuddy_provider_with_discovery,
        },
        execution::{CreateExecutionInput, canonicalize_request},
    };

    for (private, enabled, health, expected) in [
        ("exact", true, ProviderHealth::Available, true),
        ("missing", true, ProviderHealth::Available, false),
        ("no-session", true, ProviderHealth::Available, false),
        ("exact", false, ProviderHealth::Available, false),
        ("exact", true, ProviderHealth::Unavailable, false),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let store = StateStore::open(directory.path().into()).await.unwrap();
        let root = crate::config::canonicalize_workspace_root(directory.path()).unwrap();
        let input: CreateExecutionInput = serde_json::from_value(json!({
            "agent_id":"codebuddy-agent","request_key":format!("source-{private}-{enabled}"),
            "prompt":"source","execution_profile":{},"workspace_id":"workspace",
            "canonical_workspace_root":root,"workspace_generation":1,
            "provider":"codebuddy","mode":"workspace_write"
        }))
        .unwrap();
        store
            .create_execution("source".into(), canonicalize_request(input).unwrap(), 1)
            .await
            .unwrap();
        let database = directory.path().join("agent-state.db");
        let connection = rusqlite::Connection::open(&database).unwrap();
        connection.execute_batch(
            "INSERT INTO runtime_instances(id,owner_host_instance_id,state,termination_evidence_type,termination_evidence_at,termination_evidence_state,created_at,updated_at,provider) VALUES ('R1','old-host','terminated','job_active_processes_zero',2,'complete',1,2,'codebuddy');
             UPDATE executions SET status='completed',dispatch_state='dispatched',runtime_instance_id='R1',provider_terminal_status='completed',provider_terminal_evidence_at=2,provider_terminal_evidence_runtime_instance_id='R1',release_evidence_state='complete',release_evidence_kind='runtime_terminated',release_evidence_json='{}',result_completeness='complete',final_result_json='{}',completed_at=2 WHERE id='source';
             DELETE FROM workspace_claims WHERE execution_id='source';",
        ).unwrap();
        match private {
            "exact" => connection.execute_batch(
                "INSERT INTO codebuddy_execution_state(execution_id,runtime_instance_id,acp_protocol_version,session_id,conversation_request_id,prompt_state,terminal_stop_reason,terminal_observed_at,recovery_state,revision,created_at,updated_at) VALUES ('source','R1',1,'S1','01900000000070008000000000000001','terminal_observed','end_turn',2,'not_attempted',0,1,2);",
            ).unwrap(),
            "no-session" => connection.execute_batch(
                "INSERT INTO codebuddy_execution_state(execution_id,runtime_instance_id,conversation_request_id,prompt_state,recovery_state,revision,created_at,updated_at) VALUES ('source','R1','01900000000070008000000000000001','prepared','not_attempted',0,1,2);",
            ).unwrap(),
            "missing" => {}
            _ => unreachable!(),
        }
        drop(connection);

        let mut registry = ProviderRegistry::new();
        register_codebuddy_provider_with_discovery(
            &mut registry,
            store.clone(),
            "current-host".into(),
            Ok(DiscoveryResult::direct_for_test(
                "C:/resolved/codebuddy.exe",
            )),
        )
        .unwrap();
        let provider_id = ProviderId::new("codebuddy".into()).unwrap();
        registry.set_health(&provider_id, health).unwrap();
        let mut service = AgentProductService::new(store);
        service.manager.use_registry(registry);
        service
            .manager
            .set_provider_enabled_for_test("codebuddy", enabled);
        let before = durable_snapshot(directory.path());
        let view = service.observe("source".into(), false).await.unwrap();
        assert_eq!(view.available_actions.can_continue, expected, "{private}");
        assert_eq!(durable_snapshot(directory.path()), before, "{private}");
    }
}

/// enabled、health、capability 和总开关独立决定可接收新执行的投影。
#[tokio::test]
async fn availability_matrix_preserves_independent_facts() {
    for (agent_enabled, enabled, health, can_execute, expected) in [
        (true, true, ProviderHealth::Available, true, true),
        (true, false, ProviderHealth::Available, true, false),
        (true, true, ProviderHealth::Unavailable, true, false),
        (true, true, ProviderHealth::Available, false, false),
        (false, true, ProviderHealth::Available, true, false),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let store = StateStore::open(directory.path().into()).await.unwrap();
        let mut service = AgentProductService::new(store);
        let mut registry = ProviderRegistry::new();
        registry
            .register(provider("fake-acp", can_execute), health)
            .unwrap();
        service.manager.use_registry(registry);
        let mut config = ManagerConfig {
            agent_enabled,
            ..Default::default()
        };
        config
            .agent_providers
            .providers
            .insert("fake-acp".into(), AgentProviderPolicy { enabled });
        let supervisor = supervisor(directory.path(), &config);
        let snapshot = service.provider_catalog(&supervisor).unwrap();
        assert_eq!(snapshot.providers.len(), 1);
        let entry = &snapshot.providers[0];
        assert_eq!(entry.enabled, enabled);
        assert_eq!(entry.health, health);
        assert_eq!(entry.capabilities.can_execute, can_execute);
        assert_eq!(entry.available_for_new_execution, expected);
        assert!(!entry.capabilities.can_continue);
        assert!(!entry.capabilities.can_recover);
        assert!(!entry.capabilities.token_usage);
    }
}

/// Catalog 只投影通用 Registry diagnostic，不从身份、版本或普通错误文本猜测。
#[tokio::test]
async fn admission_diagnostic_serializes_exact_code_and_blocks_new_execution() {
    let directory = tempfile::tempdir().unwrap();
    let store = StateStore::open(directory.path().into()).await.unwrap();
    let mut service = AgentProductService::new(store);
    let mut registry = ProviderRegistry::new();
    registry
        .register(
            provider_with_diagnostic("future-acp", "FAKE_PROVIDER_CONTRACT_INCOMPATIBLE"),
            ProviderHealth::Available,
        )
        .unwrap();
    // 相似身份与版本仅是展示数据，不得自行生成或改写 diagnostic。
    let ordinary = provider("codebuddy-compatible-name", true);
    let ordinary_id = ordinary.descriptor.id.clone();
    registry
        .register(ordinary, ProviderHealth::Available)
        .unwrap();
    service.manager.use_registry(registry);
    let mut config = ManagerConfig {
        agent_enabled: true,
        ..Default::default()
    };
    for id in ["future-acp", ordinary_id.as_str()] {
        config
            .agent_providers
            .providers
            .insert(id.into(), AgentProviderPolicy { enabled: true });
    }
    let supervisor = supervisor(directory.path(), &config);

    let value = serde_json::to_value(service.provider_catalog(&supervisor).unwrap()).unwrap();
    let diagnostic = value["providers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["id"] == "future-acp")
        .unwrap();
    assert_eq!(diagnostic["health"], "unavailable");
    assert_eq!(diagnostic["availableForNewExecution"], false);
    assert_eq!(
        diagnostic["diagnosticCode"],
        "FAKE_PROVIDER_CONTRACT_INCOMPATIBLE"
    );
    let ordinary = value["providers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["id"] == ordinary_id.as_str())
        .unwrap();
    assert_eq!(ordinary["health"], "available");
    assert_eq!(
        ordinary["availableForNewExecution"],
        cfg!(any(windows, target_os = "macos"))
    );
    assert!(ordinary.get("diagnosticCode").is_none());
}

/// 生产 Catalog 不允许按 Provider 身份或 CodeBuddy 私有码分支。
#[test]
fn production_catalog_has_no_provider_specific_diagnostic_branch() {
    let source = include_str!("../product.rs");
    assert!(!source.contains("descriptor.id.as_str() == \"codebuddy\""));
    assert!(!source.contains("descriptor.id == \"codebuddy\""));
    assert!(!source.contains("CODEBUDDY_ACP_INCOMPATIBLE"));
}

/// 当前策略更新立即可观察；未知合法 route、null 和未配置注册项保持各自事实。
#[tokio::test]
async fn current_policy_unknown_route_and_missing_policy_json_fixture() {
    let directory = tempfile::tempdir().unwrap();
    let store = StateStore::open(directory.path().into()).await.unwrap();
    let mut service = AgentProductService::new(store);
    let mut registry = ProviderRegistry::new();
    registry
        .register(provider("zeta", true), ProviderHealth::Available)
        .unwrap();
    registry
        .register(provider("alpha", false), ProviderHealth::Unavailable)
        .unwrap();
    service.manager.use_registry(registry);
    let supervisor = supervisor(
        directory.path(),
        &ManagerConfig {
            agent_enabled: true,
            ..Default::default()
        },
    );
    assert_eq!(
        service.provider_catalog(&supervisor).unwrap().role_routing["testing"]
            .as_ref()
            .unwrap()
            .as_str(),
        "codex"
    );
    supervisor
        .mutate_provider_settings(|settings| {
            settings
                .providers
                .insert("future-acp".into(), AgentProviderPolicy { enabled: true });
            settings.role_routing.insert(
                "testing".into(),
                Some(ProviderId::new("future-acp".into()).unwrap()),
            );
            settings.role_routing.insert("review".into(), None);
            settings
                .providers
                .insert("alpha".into(), AgentProviderPolicy { enabled: true });
        })
        .unwrap();
    let expected: Value = serde_json::from_str(include_str!(
        "fixtures/provider_catalog_current_policy.json"
    ))
    .unwrap();
    assert_eq!(
        serde_json::to_value(service.provider_catalog(&supervisor).unwrap()).unwrap(),
        expected
    );
}

/// Product 与本地 getter 成功和失败均不改 Runtime、已有 Execution/Claim、health 或 policy。
#[tokio::test]
async fn success_and_read_failure_have_no_side_effects() {
    let directory = tempfile::tempdir().unwrap();
    let store = StateStore::open(directory.path().into()).await.unwrap();
    store
        .product_create_fresh(
            "e".into(),
            "a".into(),
            "k".into(),
            "prompt".into(),
            "w".into(),
            Some(WorkspaceSnapshot {
                id: "w".into(),
                root: directory.path().to_string_lossy().into(),
                generation: 1,
            }),
            10,
        )
        .await
        .unwrap();
    let mut service = AgentProductService::new(store);
    #[cfg(windows)]
    {
        *service.manager.runtime_pool.test_connect.lock().unwrap() =
            Some(Arc::new(|_, _| panic!("catalog must not connect runtime")));
    }
    let adapter = provider("fake-acp", true);
    let mut registry = ProviderRegistry::new();
    registry
        .register(adapter.clone(), ProviderHealth::Available)
        .unwrap();
    service.manager.use_registry(registry);
    let supervisor = supervisor(
        directory.path(),
        &ManagerConfig {
            agent_enabled: true,
            ..Default::default()
        },
    );
    let before = durable_snapshot(directory.path());
    assert_eq!(before[0].len(), 1);
    assert_eq!(before[1].len(), 1);
    assert!(before[2].is_empty());
    let policy = supervisor.workspace_registry_config();
    let config_bytes = std::fs::read(&supervisor.paths.config_file).unwrap();
    let registry = service.manager.registry().unwrap();
    let expected = serde_json::to_value(service.provider_catalog(&supervisor).unwrap()).unwrap();
    let local = serde_json::to_value(
        crate::commands::agent_provider_catalog_get_impl(&service, &supervisor).unwrap(),
    )
    .unwrap();
    assert_eq!(local, expected);
    // 未知注册 Provider 原样保留，不会被替换为某个已知品牌。
    assert_eq!(local["providers"][0]["id"], "fake-acp");
    assert_eq!(local["providers"][0]["displayName"], "Catalog fake-acp");
    assert!(local["providers"][0].get("version").is_none());
    adapter.invalid_descriptor.store(true, Ordering::SeqCst);
    assert_eq!(
        service.provider_catalog(&supervisor).unwrap_err().code,
        ProviderErrorCode::AgentProviderNotFound
    );
    assert!(crate::commands::agent_provider_catalog_get_impl(&service, &supervisor).is_err());
    assert_eq!(durable_snapshot(directory.path()), before);
    assert_eq!(supervisor.workspace_registry_config(), policy);
    assert_eq!(
        std::fs::read(&supervisor.paths.config_file).unwrap(),
        config_bytes
    );
    assert!(Arc::ptr_eq(&registry, &service.manager.registry().unwrap()));
    assert_eq!(
        registry
            .health(&ProviderId::new("fake-acp".into()).unwrap())
            .unwrap(),
        ProviderHealth::Available
    );
    assert!(!service.manager.runtime_pool.stop.is_cancelled());
}

/// 目录按 adapter 事实投影任意身份的 metadata，缺失协议不制造默认值。
#[tokio::test]
async fn catalog_projects_provider_owned_version_and_protocol() {
    for protocol in [Some("Custom RPC v3"), None] {
        let directory = tempfile::tempdir().unwrap();
        let store = StateStore::open(directory.path().into()).await.unwrap();
        let mut service = AgentProductService::new(store);
        let mut adapter =
            Arc::try_unwrap(provider("future-provider", true)).unwrap_or_else(|_| unreachable!());
        adapter.descriptor.version = Some("2.160.0".into());
        adapter.descriptor.protocol = protocol.map(str::to_owned);
        let mut registry = ProviderRegistry::new();
        registry
            .register(Arc::new(adapter), ProviderHealth::Available)
            .unwrap();
        service.manager.use_registry(registry);
        let supervisor = supervisor(directory.path(), &ManagerConfig::default());
        let snapshot = service.provider_catalog(&supervisor).unwrap();
        let entry = &snapshot.providers[0];
        assert_eq!(entry.version.as_deref(), Some("2.160.0"));
        assert_eq!(entry.protocol.as_deref(), protocol);
        let value = serde_json::to_value(entry).unwrap();
        assert_eq!(value["version"], "2.160.0");
        if let Some(protocol) = protocol {
            assert_eq!(value["protocol"], protocol);
        } else {
            assert!(value.get("protocol").is_none());
        }
    }
}
