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
    })
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
    let value = serde_json::to_value(service.provider_catalog(&supervisor).unwrap()).unwrap();
    let expected: Value =
        serde_json::from_str(include_str!("fixtures/provider_catalog_codex.json")).unwrap();
    assert_eq!(value, expected);
    assert!(durable_snapshot(directory.path()).iter().all(Vec::is_empty));
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
