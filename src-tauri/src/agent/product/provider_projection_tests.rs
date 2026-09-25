use super::*;
use crate::agent::{
    execution::{CreateExecutionInput, canonicalize_request},
    provider::{
        ProviderCancelContext, ProviderCapabilities, ProviderDescriptor, ProviderError,
        ProviderExecutionContext, ProviderId, ProviderRunResult, ProviderStartupContext,
        port::{
            AgentEventSink, AgentProvider, ProviderAcceptanceSink, ProviderExecutionFailure,
            ProviderFuture, ProviderReconcileSummary,
        },
        registry::{ProviderHealth, ProviderRegistry},
    },
};

/// 只暴露测试 descriptor；任何执行、取消或恢复调用都表示 Product 越过了只读边界。
struct DescriptorOnlyProvider(ProviderDescriptor);

impl AgentProvider for DescriptorOnlyProvider {
    /// 返回测试冻结的展示元数据。
    fn descriptor(&self) -> ProviderDescriptor {
        self.0.clone()
    }

    /// Product 投影不得消费 Provider capabilities。
    fn capabilities(&self) -> ProviderCapabilities {
        panic!("Product must not inspect Provider capabilities")
    }

    /// Product 投影不得派发 Provider。
    fn execute<'a>(
        &'a self,
        _: ProviderExecutionContext,
        _: Arc<dyn ProviderAcceptanceSink>,
        _: Arc<dyn AgentEventSink>,
    ) -> ProviderFuture<'a, Result<ProviderRunResult, ProviderExecutionFailure>> {
        Box::pin(async { panic!("Product must not execute Provider") })
    }

    /// Product 投影不得取消 Provider。
    fn cancel<'a>(
        &'a self,
        _: ProviderCancelContext,
    ) -> ProviderFuture<'a, Result<(), ProviderError>> {
        Box::pin(async { panic!("Product must not cancel Provider") })
    }

    /// Product 投影不得调用 Provider recovery。
    fn startup_reconcile<'a>(
        &'a self,
        _: ProviderStartupContext,
    ) -> ProviderFuture<'a, Result<ProviderReconcileSummary, ProviderError>> {
        Box::pin(async { panic!("Product must not reconcile Provider") })
    }
}

/// 冻结 Product 使用真实 Registry 的 descriptor，并允许健康状态不可用时展示。
#[tokio::test]
async fn registered_and_unavailable_provider_descriptors_project_in_all_read_paths() {
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::open(dir.path().into()).await.unwrap();
    store
        .product_create_fresh(
            "e".into(),
            "a".into(),
            "k".into(),
            "prompt".into(),
            "W".into(),
            w(dir.path(), "W"),
            10,
        )
        .await
        .unwrap();
    let db = rusqlite::Connection::open(dir.path().join("agent-state.db")).unwrap();
    let mut registry = ProviderRegistry::new();
    for (id, name, version, health) in [
        (
            "fake-acp",
            "Fake ACP",
            Some("1.2.3"),
            ProviderHealth::Available,
        ),
        (
            "disabled-acp",
            "Disabled ACP",
            None,
            ProviderHealth::Unavailable,
        ),
    ] {
        registry
            .register(
                Arc::new(DescriptorOnlyProvider(ProviderDescriptor {
                    id: ProviderId::new(id.into()).unwrap(),
                    display_name: name.into(),
                    version: version.map(str::to_owned),
                })),
                health,
            )
            .unwrap();
    }
    let mut service = AgentProductService::new(store);
    service.manager.use_registry(registry);
    let mut original_non_provider_view = None;

    for (id, expected) in [
        (
            "fake-acp",
            json!({"id":"fake-acp","displayName":"Fake ACP","version":"1.2.3"}),
        ),
        (
            "disabled-acp",
            json!({"id":"disabled-acp","displayName":"Disabled ACP","version":null}),
        ),
        (
            "historical-acp",
            json!({"id":"historical-acp","displayName":"historical-acp","version":null}),
        ),
    ] {
        db.execute("UPDATE executions SET provider=?1 WHERE id='e'", [id])
            .unwrap();
        let detail =
            serde_json::to_value(service.observe("e".into(), false).await.unwrap()).unwrap();
        let queried = success(
            service
                .agent_query(AgentQueryAction::Get {
                    execution_id: "e".into(),
                    include_result: Some(false),
                })
                .await
                .unwrap(),
        );
        let observed = service
            .operation(
                json!({"action":"observe","executionId":"e","waitMs":0}),
                None,
            )
            .await;
        let listed = service.operation(json!({"action":"list"}), None).await;
        assert_eq!(detail["provider"], expected);
        assert_eq!(queried["data"]["provider"], expected);
        assert_eq!(observed["data"]["provider"], expected);
        assert_eq!(listed["data"]["executions"][0]["provider"], expected);
        assert_eq!(detail["status"], observed["data"]["status"]);
        assert_eq!(
            detail["progress"],
            listed["data"]["executions"][0]["progress"]
        );
        assert_eq!(detail["usage"], observed["data"]["usage"]);
        assert_eq!(detail["attention"], observed["data"]["attention"]);
        assert_eq!(
            detail["resultAvailable"],
            observed["data"]["resultAvailable"]
        );
        let mut non_provider_view = detail;
        non_provider_view
            .as_object_mut()
            .unwrap()
            .remove("provider");
        if let Some(original) = &original_non_provider_view {
            assert_eq!(&non_provider_view, original);
        } else {
            original_non_provider_view = Some(non_provider_view);
        }
    }
}

async fn assert_fake_provider_product(service: &AgentProductService) {
    let detail = success(
        service
            .agent_query(AgentQueryAction::Get {
                execution_id: "fake-restart".into(),
                include_result: Some(false),
            })
            .await
            .unwrap(),
    );
    let observed = service
        .operation(
            json!({"action":"observe","executionId":"fake-restart","waitMs":0}),
            None,
        )
        .await;
    let listed = service.operation(json!({"action":"list"}), None).await;
    let listed = listed["data"]["executions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["executionId"] == "fake-restart")
        .unwrap();

    for view in [&detail["data"], &observed["data"], listed] {
        assert_eq!(
            view["provider"],
            json!({"id":"fake-acp","displayName":"Fake ACP","version":"1.2.3"})
        );
        assert_eq!(view["taskRole"], "general");
        assert_eq!(view["usage"]["completeness"], "unknown");
        assert_eq!(view["usage"]["usageRevision"], 0);
        assert!(view["usage"]["totalTokens"].is_null());
    }
}

/// 从 provider-aware 创建入口持久化的第二 Provider，重启后仍能被所有 Product 只读路径稳定读取。
#[tokio::test]
async fn fake_provider_create_and_restart_preserve_product_identity_and_unknown_usage() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("fake-workspace");
    std::fs::create_dir_all(&root).unwrap();

    let make_registry = || {
        let mut registry = ProviderRegistry::new();
        registry
            .register(
                Arc::new(DescriptorOnlyProvider(ProviderDescriptor {
                    id: ProviderId::new("fake-acp".into()).unwrap(),
                    display_name: "Fake ACP".into(),
                    version: Some("1.2.3".into()),
                })),
                ProviderHealth::Available,
            )
            .unwrap();
        registry
    };

    let store = StateStore::open(dir.path().into()).await.unwrap();
    let input: CreateExecutionInput = serde_json::from_value(json!({
        "agent_id":"fake-restart-agent",
        "request_key":"fake-restart-key",
        "prompt":"provider persistence",
        "execution_profile":{},
        "workspace_id":"fake-workspace",
        "canonical_workspace_root":root.to_string_lossy(),
        "workspace_generation":1,
        "provider":"fake-acp",
        "task_role":"general",
        "mode":"read_only"
    }))
    .unwrap();
    store
        .create_execution(
            "fake-restart".into(),
            canonicalize_request(input).unwrap(),
            1,
        )
        .await
        .unwrap();

    let persisted = store
        .execution("fake-restart".into())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(persisted.provider.as_str(), "fake-acp");

    let mut service = AgentProductService::new(store.clone());
    service.manager.use_registry(make_registry());
    assert_fake_provider_product(&service).await;
    drop(service);
    drop(store);

    let reopened = StateStore::open(dir.path().into()).await.unwrap();
    let persisted = reopened
        .execution("fake-restart".into())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(persisted.provider.as_str(), "fake-acp");

    let mut service = AgentProductService::new(reopened);
    service.manager.use_registry(make_registry());
    assert_fake_provider_product(&service).await;
}

/// descriptor 身份失配或读取缺失都不能覆盖持久化 Provider ID。
#[test]
fn mismatched_descriptor_falls_back_to_persisted_identity() {
    let provider = ProviderProduct::from_execution_provider(
        "historical-acp",
        Some(ProviderDescriptor {
            id: ProviderId::new("other-acp".into()).unwrap(),
            display_name: "Other ACP".into(),
            version: Some("9".into()),
        }),
    );
    assert_eq!(
        serde_json::to_value(provider).unwrap(),
        json!({"id":"historical-acp","displayName":"historical-acp","version":null})
    );
}
