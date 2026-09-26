//! CB2-003 本地 IPC 实现与持久化、门禁隔离契约。
use super::*;
use crate::agent::{
    execution::AgentTaskRole,
    notification::noop_agent_terminal_notifier,
    provider::{ProviderId, control::ProviderAdmissionCapability, registry::ProviderHealth},
    store::StateStore,
    task_manager::AgentTaskManager,
};
use std::sync::Arc;

#[cfg(windows)]
#[path = "provider_policy_drain_tests.rs"]
mod drain;

/// 真实 Supervisor、Broker 与 Store；不初始化后台恢复或 Runtime。
async fn fixture() -> (tempfile::TempDir, Arc<crate::mcp::Broker>, AgentTaskManager) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let supervisor = Arc::new(
        crate::serena::SupervisorState::new(crate::config::AppPaths {
            runtime_directory: root.join("runtime"),
            config_file: root.join("config.json"),
            log_directory: root.join("logs"),
            app_log: root.join("logs/app.log"),
            serena_log: root.join("logs/serena.log"),
        })
        .unwrap(),
    );
    let store = StateStore::open(root.into()).await.unwrap();
    let manager = AgentTaskManager::new_with_terminal_notifier_and_provider_settings(
        store,
        "unused-codex.exe".into(),
        noop_agent_terminal_notifier(),
        supervisor.provider_policy(),
    );
    (
        directory,
        Arc::new(crate::mcp::Broker::new(supervisor)),
        manager,
    )
}

/// 使用与 Tauri 参数相同的 ProviderId 反序列化边界。
fn id(value: &str) -> ProviderId {
    serde_json::from_value(serde_json::json!(value)).unwrap()
}

/// 查询真实持久化表，禁止测试只依赖返回值推断无执行副作用。
fn assert_no_execution(directory: &tempfile::TempDir) {
    let db = rusqlite::Connection::open(directory.path().join("agent-state.db")).unwrap();
    for table in ["executions", "runtime_instances", "workspace_claims"] {
        let count: i64 = db
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 0, "{table}");
    }
}

/// 持久化成功立即更新同一 admission，重启恢复未知 Provider 和显式空路由。
#[tokio::test]
async fn provider_policy_persists_restarts_and_updates_admission_without_runtime() {
    let (directory, broker, manager) = fixture().await;
    let registry = manager.registry().unwrap();
    assert!(
        broker
            .supervisor
            .provider_policy()
            .admit(
                &registry,
                &id("codex"),
                ProviderAdmissionCapability::Execute
            )
            .is_ok()
    );
    agent_provider_set_enabled_impl(&broker, id("codex"), false)
        .await
        .unwrap();
    assert_eq!(
        broker
            .supervisor
            .provider_policy()
            .admit(
                &registry,
                &id("codex"),
                ProviderAdmissionCapability::Execute
            )
            .err()
            .unwrap()
            .code,
        crate::agent::provider::ProviderErrorCode::AgentProviderDisabled
    );
    // Manager 必须观察同一成功提交策略，拒绝发生在创建 Execution/Claim 之前。
    let input = serde_json::from_value(serde_json::json!({
        "agent_id": "policy-agent", "request_key": "disabled-start", "prompt": "no runtime",
        "execution_profile": {}, "workspace_id": "workspace",
        "canonical_workspace_root": directory.path().to_str().unwrap(),
        "provider": "codex", "mode": "read_only"
    }))
    .unwrap();
    assert_eq!(
        manager.execute(input).await.unwrap_err(),
        crate::agent::provider::port::ProviderExecutionFailure::State(
            "AGENT_PROVIDER_DISABLED".into()
        )
    );
    agent_provider_set_enabled_impl(&broker, id("future-provider"), true)
        .await
        .unwrap();
    agent_provider_set_role_route_impl(&broker, AgentTaskRole::Review, Some(id("future-provider")))
        .await
        .unwrap();
    agent_provider_set_role_route_impl(&broker, AgentTaskRole::General, None)
        .await
        .unwrap();
    let settings = agent_provider_settings_get_impl(&broker);
    assert_eq!(settings.role_routing["general"], None);
    assert_eq!(settings.role_routing["review"], Some(id("future-provider")));
    assert!(settings.providers["future-provider"].enabled);
    let reopened = crate::serena::SupervisorState::new(broker.supervisor.paths.clone()).unwrap();
    assert_eq!(reopened.snapshot().config.agent_providers, settings);
    assert_eq!(
        crate::config::load(&broker.supervisor.paths.config_file)
            .unwrap()
            .agent_providers,
        settings
    );
    assert!(manager.runtime_pool.is_empty());
    assert_no_execution(&directory);
}

/// IPC 的既有领域类型拒绝非法 Provider/Role，null 仍可表示清空。
#[test]
fn provider_policy_ipc_validation_reuses_domain_types() {
    for value in ["", " codex", "codex ", "a\nb", "a b"] {
        assert!(serde_json::from_value::<ProviderId>(serde_json::json!(value)).is_err());
    }
    for value in ["", "Review", "future", "review "] {
        assert!(serde_json::from_value::<AgentTaskRole>(serde_json::json!(value)).is_err());
    }
    assert_eq!(
        serde_json::from_value::<Option<ProviderId>>(serde_json::Value::Null).unwrap(),
        None
    );
}

/// 原子落盘失败不改变磁盘、Supervisor 或 admission Authority。
#[tokio::test]
async fn provider_policy_persist_failure_preserves_authority() {
    let (directory, broker, manager) = fixture().await;
    agent_provider_set_enabled_impl(&broker, id("codex"), true)
        .await
        .unwrap();
    let before = agent_provider_settings_get_impl(&broker);
    // 将配置文件替换为目录，稳定触发 atomic replace 失败。
    std::fs::remove_file(&broker.supervisor.paths.config_file).unwrap();
    std::fs::create_dir(&broker.supervisor.paths.config_file).unwrap();
    assert!(
        agent_provider_set_enabled_impl(&broker, id("codex"), false)
            .await
            .is_err()
    );
    assert!(
        agent_provider_set_role_route_impl(&broker, AgentTaskRole::Review, None)
            .await
            .is_err()
    );
    assert_eq!(agent_provider_settings_get_impl(&broker), before);
    assert!(
        broker
            .supervisor
            .provider_policy()
            .admit(
                &manager.registry().unwrap(),
                &id("codex"),
                ProviderAdmissionCapability::Execute
            )
            .is_ok()
    );
    assert!(broker.supervisor.paths.config_file.is_dir());
    assert_no_execution(&directory);
}

/// 两个 mutation 都等待既有 management 锁，随后分别保留对方最新字段。
#[tokio::test]
async fn provider_policy_concurrent_mutations_wait_for_management_lock() {
    let (directory, broker, _) = fixture().await;
    let guard = broker.management.lock().await;
    let first_broker = broker.clone();
    let first = tokio::spawn(async move {
        agent_provider_set_enabled_impl(&first_broker, id("codex"), false).await
    });
    let second_broker = broker.clone();
    let second = tokio::spawn(async move {
        agent_provider_set_role_route_impl(&second_broker, AgentTaskRole::Testing, None).await
    });
    tokio::task::yield_now().await;
    assert!(!first.is_finished());
    assert!(!second.is_finished());
    assert!(!broker.supervisor.paths.config_file.exists());
    drop(guard);
    first.await.unwrap().unwrap();
    second.await.unwrap().unwrap();
    let settings = agent_provider_settings_get_impl(&broker);
    assert!(!settings.providers["codex"].enabled);
    assert_eq!(settings.role_routing["testing"], None);
    assert_eq!(
        crate::config::load(&broker.supervisor.paths.config_file)
            .unwrap()
            .agent_providers,
        settings
    );
    assert_no_execution(&directory);
}

/// 整配置保存携带旧 Provider 快照时不覆盖专用入口，普通配置仍保存。
#[tokio::test]
async fn provider_policy_stale_whole_config_cannot_overwrite_local_policy() {
    let (_, broker, manager) = fixture().await;
    let mut stale = broker.config();
    stale.minimize_to_tray = !stale.minimize_to_tray;
    agent_provider_set_enabled_impl(&broker, id("codex"), false)
        .await
        .unwrap();
    save_config_impl(&broker, stale.clone()).await.unwrap();
    assert!(!broker.config().agent_providers.providers["codex"].enabled);
    assert_eq!(broker.config().minimize_to_tray, stale.minimize_to_tray);
    assert!(
        broker
            .supervisor
            .provider_policy()
            .admit(
                &manager.registry().unwrap(),
                &id("codex"),
                ProviderAdmissionCapability::Execute
            )
            .is_err()
    );
}

/// 健康刷新可使 unavailable 恢复；enabled 独立，原 Registry 句柄不变。
#[tokio::test]
async fn provider_policy_health_probe_has_no_execution_session_or_runtime() {
    let (directory, broker, manager) = fixture().await;
    agent_provider_set_enabled_impl(&broker, id("codex"), false)
        .await
        .unwrap();
    let original = manager.registry().unwrap();
    for (probe, expected) in [
        (
            Err("BACKEND_UNAVAILABLE".into()),
            ProviderHealth::Unavailable,
        ),
        (Ok("new-codex.exe".into()), ProviderHealth::Available),
    ] {
        let health = crate::agent::codex::TEST_BACKEND_DISCOVERY
            .scope(probe, manager.refresh_provider_health(id("codex")))
            .await
            .unwrap();
        assert_eq!(health, expected);
        assert_eq!(
            manager.registry().unwrap().health(&id("codex")).unwrap(),
            expected
        );
        assert!(!agent_provider_settings_get_impl(&broker).providers["codex"].enabled);
        assert!(manager.runtime_pool.is_empty());
        assert_no_execution(&directory);
    }
    assert_eq!(
        original.health(&id("codex")).unwrap(),
        ProviderHealth::Available
    );
    assert_eq!(
        manager
            .refresh_provider_health(id("codebuddy"))
            .await
            .unwrap_err(),
        "AGENT_PROVIDER_NOT_FOUND"
    );
    assert_no_execution(&directory);
}

/// Remote catalog 与 validation 均不能接触本地 mutation。
#[test]
fn provider_policy_mutations_are_absent_from_remote_registry() {
    for enabled in [false, true] {
        let tools = crate::mcp::registry::list_with_capabilities(enabled, true, true);
        for name in [
            "agent_provider_settings_get",
            "agent_provider_set_enabled",
            "agent_provider_set_role_route",
            "agent_provider_refresh_health",
        ] {
            assert!(!tools.iter().any(|tool| tool.name == name));
            assert_eq!(
                crate::mcp::registry::validate(name, &serde_json::json!({})),
                Err("UNKNOWN_TOOL".into())
            );
        }
    }
}

/// 已持久化的在途 Execution 保留原 Provider、Role、状态与 Claim。
#[tokio::test]
async fn provider_policy_route_changes_leave_running_execution_frozen() {
    let (directory, broker, manager) = fixture().await;
    let store = StateStore::open(directory.path().into()).await.unwrap();
    let root = directory.path().to_string_lossy().into_owned();
    store
        .product_create_fresh(
            "running".into(),
            "agent".into(),
            "request".into(),
            "prompt".into(),
            "workspace".into(),
            Some(
                crate::agent::store::transactions::product::WorkspaceSnapshot {
                    id: "workspace".into(),
                    root: root.clone(),
                    generation: 1,
                },
            ),
            1,
        )
        .await
        .unwrap();
    // 固定已在途行，不通过 Runtime 启动制造测试副作用。
    rusqlite::Connection::open(directory.path().join("agent-state.db"))
        .unwrap()
        .execute(
            "UPDATE executions SET status='running' WHERE id='running'",
            [],
        )
        .unwrap();
    let before = store.execution("running".into()).await.unwrap().unwrap();
    let claim = store.workspace_claim(root.clone()).await.unwrap();
    agent_provider_set_role_route_impl(
        &broker,
        AgentTaskRole::General,
        Some(id("future-provider")),
    )
    .await
    .unwrap();
    agent_provider_set_enabled_impl(&broker, id("codex"), false)
        .await
        .unwrap();
    assert_eq!(
        store.execution("running".into()).await.unwrap().unwrap(),
        before
    );
    assert_eq!(store.workspace_claim(root).await.unwrap(), claim);
    assert!(manager.runtime_pool.is_empty());
}
