//! CB3-004：真实 Broker→Product→Store 路径的路由、幂等与管理锁竞态。
use super::*;
use crate::agent::{
    notification::noop_agent_terminal_notifier,
    provider::{
        ProviderCancelContext, ProviderCapabilities, ProviderDescriptor, ProviderError,
        ProviderExecutionContext, ProviderId, ProviderOutcome, ProviderResultCompleteness,
        ProviderRunResult, ProviderStartupContext,
        port::{
            AgentEventSink, AgentProvider, ProviderAcceptanceSink, ProviderExecutionFailure,
            ProviderFuture, ProviderReconcileSummary,
        },
        registry::{ProviderHealth, ProviderRegistry},
    },
    task_manager::AgentTaskManager,
};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

#[path = "continuation_routing_tests.rs"]
mod continuation_routing_tests;

struct RoutingProvider {
    id: ProviderId,
    can_execute: bool,
    calls: AtomicUsize,
    store: StateStore,
}

impl AgentProvider for RoutingProvider {
    /// 测试 Provider 不启动进程，身份仅用于持久化与路由断言。
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: self.id.clone(),
            display_name: "Routing fixture".into(),
            version: None,
        }
    }
    /// 用独立能力事实验证 health 与 canExecute 的错误优先级。
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            can_execute: self.can_execute,
            can_continue: false,
            can_cancel: true,
            can_recover: false,
            activity: false,
            token_usage: false,
        }
    }
    /// 通过既有无 Runtime 取消路径收口，不能伪造 Runtime/Claim evidence。
    fn execute<'a>(
        &'a self,
        context: ProviderExecutionContext,
        acceptance: Arc<dyn ProviderAcceptanceSink>,
        _: Arc<dyn AgentEventSink>,
    ) -> ProviderFuture<'a, Result<ProviderRunResult, ProviderExecutionFailure>> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            acceptance.accepted();
            self.store
                .request_cancel(
                    context.execution_id.clone(),
                    crate::agent::coordinator::now(),
                )
                .await?;
            Ok(ProviderRunResult {
                execution_id: context.execution_id,
                outcome: ProviderOutcome::Cancelled,
                result: None,
                result_completeness: ProviderResultCompleteness::Unknown,
                diagnostic_code: None,
            })
        })
    }
    /// 本组测试不请求 Provider 取消。
    fn cancel<'a>(
        &'a self,
        _: ProviderCancelContext,
    ) -> ProviderFuture<'a, Result<(), ProviderError>> {
        Box::pin(async { panic!("unexpected provider cancel") })
    }
    /// 路由测试不启动恢复任务。
    fn startup_reconcile<'a>(
        &'a self,
        _: ProviderStartupContext,
    ) -> ProviderFuture<'a, Result<ProviderReconcileSummary, ProviderError>> {
        Box::pin(async { panic!("unexpected startup reconciliation") })
    }
}

struct RoutingFixture {
    _dir: tempfile::TempDir,
    broker: Arc<Broker>,
    store: StateStore,
    manager: AgentTaskManager,
    provider: Arc<RoutingProvider>,
    state_root: PathBuf,
}

impl RoutingFixture {
    /// 使用真实 Supervisor 配置与同一 admission owner，避免测试快照形成第二 Authority。
    async fn new(
        health: ProviderHealth,
        can_execute: bool,
        handoff: Option<Arc<(tokio::sync::Notify, tokio::sync::Notify)>>,
    ) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let broker = fixture(dir.path());
        let root = dir.path().join("workspace");
        std::fs::create_dir(&root).unwrap();
        let root = std::fs::canonicalize(root).unwrap();
        let mut config = broker.config();
        config.workspaces = vec![Workspace {
            id: "W".into(),
            name: "W".into(),
            root,
            generation: 1,
        }];
        broker.supervisor.replace_config(config).unwrap();
        broker
            .supervisor
            .mutate_provider_settings(|settings| {
                for id in ["fixture", "other"] {
                    settings.providers.insert(
                        id.into(),
                        crate::config::AgentProviderPolicy { enabled: true },
                    );
                }
                for route in settings.role_routing.values_mut() {
                    *route = Some(ProviderId::new("fixture".into()).unwrap());
                }
            })
            .unwrap();
        let state_root = dir.path().join("state");
        let store = StateStore::open(state_root.clone()).await.unwrap();
        let provider = Arc::new(RoutingProvider {
            id: ProviderId::new("fixture".into()).unwrap(),
            can_execute,
            calls: AtomicUsize::new(0),
            store: store.clone(),
        });
        let mut registry = ProviderRegistry::new();
        registry.register(provider.clone(), health).unwrap();
        registry
            .register(
                Arc::new(RoutingProvider {
                    id: ProviderId::new("other".into()).unwrap(),
                    can_execute: true,
                    calls: AtomicUsize::new(0),
                    store: store.clone(),
                }),
                ProviderHealth::Available,
            )
            .unwrap();
        registry
            .register(
                Arc::new(RoutingProvider {
                    id: ProviderId::new("codex".into()).unwrap(),
                    can_execute: true,
                    calls: AtomicUsize::new(0),
                    store: store.clone(),
                }),
                ProviderHealth::Available,
            )
            .unwrap();
        let mut manager = AgentTaskManager::new_with_terminal_notifier_and_provider_settings(
            store.clone(),
            PathBuf::new(),
            noop_agent_terminal_notifier(),
            broker.supervisor.provider_policy(),
        );
        manager.use_registry(registry);
        manager.test_handoff = handoff;
        assert!(
            broker
                .product
                .set(Arc::new(AgentProductService::new_with_manager_for_test(
                    store.clone(),
                    manager.clone()
                )))
                .is_ok()
        );
        Self {
            _dir: dir,
            broker,
            store,
            manager,
            provider,
            state_root,
        }
    }

    /// Work/context preflight 与真实入口一致，不直接插入 Execution。
    async fn begin(&self) -> String {
        let result = self
            .broker
            .orchestration_operation(
                "work_update",
                json!({"action":"begin","workspaceId":"W","title":"Routing"}),
            )
            .await;
        assert_eq!(result["ok"], true, "{result}");
        result["data"]["workRun"]["workRunId"]
            .as_str()
            .unwrap()
            .into()
    }

    /// 完整比较三类持久化副作用，拒绝时要求逐值不变。
    fn rows(&self) -> Vec<Vec<Vec<rusqlite::types::Value>>> {
        let db = rusqlite::Connection::open(self.state_root.join("agent-state.db")).unwrap();
        ["executions", "workspace_claims", "runtime_instances"]
            .iter()
            .map(|table| {
                let mut q = db
                    .prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))
                    .unwrap();
                let columns = q.column_count();
                q.query_map([], |row| (0..columns).map(|i| row.get(i)).collect())
                    .unwrap()
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap()
            })
            .collect()
    }

    /// 测试中的 Human mutation 使用与正式 IPC 相同的 management→operation 顺序。
    async fn policy(&self, mutate: impl FnOnce(&mut crate::config::AgentProviderSettings)) {
        let _management = self.broker.management.lock().await;
        self.broker
            .supervisor
            .mutate_provider_settings(mutate)
            .unwrap();
    }

    /// 仅通过当前 Broker 公共 orchestration 执行已解析请求。
    async fn execute(&self, request: Value) -> Value {
        self.broker
            .orchestration_operation("agent_execute", request)
            .await
    }
}

/// 显式角色固定由调用方给出，Prompt 刻意包含另一种角色的文字。
fn start(work: &str) -> Value {
    json!({"action":"start","workRunId":work,"workspaceId":"W","requestKey":"key",
        "prompt":"请开发实现并评审代码，不要根据这些文字改变角色", "taskRole":"testing","providerId":"fixture"})
}

/// 逐层叠加坏事实，验证显式 Start 的冻结错误优先级及三类零副作用。
#[tokio::test]
async fn explicit_routing_error_priority_has_no_creation_side_effects() {
    let cases = [
        ("disabled-agent", "AGENT_DISABLED"),
        ("missing-provider", "AGENT_PROVIDER_NOT_FOUND"),
        ("disabled-provider", "AGENT_PROVIDER_DISABLED"),
        ("missing-role", "AGENT_ROLE_NOT_CONFIGURED"),
        ("mismatch", "AGENT_ROLE_PROVIDER_MISMATCH"),
        ("unavailable", "AGENT_PROVIDER_UNAVAILABLE"),
        ("capability", "AGENT_PROVIDER_CAPABILITY_UNSUPPORTED"),
    ];
    for (case, expected) in cases {
        let f = RoutingFixture::new(
            if case == "capability" {
                ProviderHealth::Available
            } else {
                ProviderHealth::Unavailable
            },
            false,
            None,
        )
        .await;
        let work = f.begin().await;
        let mut request = start(&work);
        if case == "disabled-agent" {
            let _management = f.broker.management.lock().await;
            let mut config = f.broker.config();
            config.agent_enabled = false;
            f.broker.supervisor.replace_config(config).unwrap();
        }
        if case == "missing-provider" {
            request["providerId"] = json!("unregistered");
        }
        f.policy(|settings| {
            if matches!(
                case,
                "disabled-agent" | "missing-provider" | "disabled-provider"
            ) {
                settings.providers.get_mut("fixture").unwrap().enabled = false;
            }
            settings.role_routing.insert(
                "testing".into(),
                match case {
                    "unavailable" | "capability" => {
                        Some(ProviderId::new("fixture".into()).unwrap())
                    }
                    "mismatch" => Some(ProviderId::new("other".into()).unwrap()),
                    _ => None,
                },
            );
        })
        .await;
        let before = f.rows();
        let response = f.execute(request).await;
        assert_eq!(response["error"]["code"], expected, "{case}: {response}");
        assert_eq!(f.rows(), before, "{case}");
        assert_eq!(f.provider.calls.load(Ordering::SeqCst), 0);
    }
}

/// legacy 必须读取当前 general route，缺失时不能使用默认 Codex。
#[tokio::test]
async fn legacy_general_route_is_required_and_ignores_prompt_classification() {
    let f = RoutingFixture::new(ProviderHealth::Available, true, None).await;
    let work = f.begin().await;
    let mut request = start(&work);
    request.as_object_mut().unwrap().remove("taskRole");
    request.as_object_mut().unwrap().remove("providerId");
    f.policy(|settings| {
        settings.role_routing.insert("general".into(), None);
    })
    .await;
    let before = f.rows();
    assert_eq!(
        f.execute(request.clone()).await["error"]["code"],
        "AGENT_ROLE_NOT_CONFIGURED"
    );
    assert_eq!(f.rows(), before);
    f.policy(|settings| {
        settings.role_routing.insert(
            "general".into(),
            Some(ProviderId::new("fixture".into()).unwrap()),
        );
    })
    .await;
    let response = f.execute(request).await;
    assert_eq!(response["ok"], true, "{response}");
    let rows = f.store.product_read(None, None, None, 10).await.unwrap();
    assert_eq!(rows[0].execution.provider, "fixture");
    assert_eq!(rows[0].task_role, "general");
}

/// 非 general 的 exact retry 不被 preflight 拒绝，最终 pair 参与新请求 identity。
#[tokio::test]
async fn explicit_retry_and_changed_provider_or_role_use_frozen_v3_identity() {
    let f = RoutingFixture::new(ProviderHealth::Available, true, None).await;
    let work = f.begin().await;
    let request = start(&work);
    assert_eq!(f.execute(request.clone()).await["ok"], true);
    let original = f
        .store
        .product_read(None, None, None, 10)
        .await
        .unwrap()
        .remove(0);
    assert_eq!(original.execution.provider, "fixture");
    assert_eq!(original.task_role, "testing");
    assert_eq!(f.execute(request.clone()).await["ok"], true);
    assert_eq!(f.provider.calls.load(Ordering::SeqCst), 1);
    let mut changed_role = request.clone();
    changed_role["taskRole"] = json!("review");
    assert_eq!(
        f.execute(changed_role).await["error"]["code"],
        "EXECUTION_REQUEST_KEY_CONFLICT"
    );
    f.policy(|settings| {
        settings.role_routing.insert(
            "testing".into(),
            Some(ProviderId::new("other".into()).unwrap()),
        );
    })
    .await;
    let mut changed_provider = request.clone();
    changed_provider["providerId"] = json!("other");
    assert_eq!(
        f.execute(changed_provider).await["error"]["code"],
        "EXECUTION_REQUEST_KEY_CONFLICT"
    );
    // exact retry 必须再次检查当前 Authority，不能用 prior row 绕过 mismatch/disabled。
    assert_eq!(
        f.execute(request.clone()).await["error"]["code"],
        "AGENT_ROLE_PROVIDER_MISMATCH"
    );
    f.policy(|settings| {
        settings.providers.get_mut("fixture").unwrap().enabled = false;
    })
    .await;
    assert_eq!(
        f.execute(request).await["error"]["code"],
        "AGENT_PROVIDER_DISABLED"
    );
    let current = f.store.product_read(None, None, None, 10).await.unwrap();
    assert_eq!(current.len(), 1);
    assert_eq!(current[0].execution.provider, original.execution.provider);
    assert_eq!(current[0].task_role, original.task_role);
    assert_eq!(
        current[0].execution.request_hash,
        original.execution.request_hash
    );
}

/// Query 的快照不授予创建权限，查询后的 policy/开关变化在 create 前生效。
#[tokio::test]
async fn query_then_policy_changes_are_rejected_before_creation() {
    for disable in [false, true] {
        let f = RoutingFixture::new(ProviderHealth::Available, true, None).await;
        let work = f.begin().await;
        let catalog = f
            .broker
            .orchestration_operation("agent_query", json!({"action":"providers"}))
            .await;
        assert_eq!(catalog["ok"], true);
        f.policy(|settings| {
            if disable {
                settings.providers.get_mut("fixture").unwrap().enabled = false;
            } else {
                settings.role_routing.insert(
                    "testing".into(),
                    Some(ProviderId::new("other".into()).unwrap()),
                );
            }
        })
        .await;
        let before = f.rows();
        let response = f.execute(start(&work)).await;
        assert_eq!(
            response["error"]["code"],
            if disable {
                "AGENT_PROVIDER_DISABLED"
            } else {
                "AGENT_ROLE_PROVIDER_MISMATCH"
            }
        );
        assert_eq!(f.rows(), before);
    }
}

/// 同步创建钩子制造真实 interleaving；三条管理路径不能穿过最终校验与提交之间。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn management_serializes_policy_health_and_agent_enabled_through_creation() {
    for mutation in ["route", "enabled", "health", "agent-enabled"] {
        let f = RoutingFixture::new(ProviderHealth::Available, true, None).await;
        let work = f.begin().await;
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let release_rx = Arc::new(std::sync::Mutex::new(release_rx));
        *f.broker.supervisor.workspace_start_hook.lock().unwrap() = Some(Arc::new(move || {
            entered_tx.send(()).unwrap();
            release_rx
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(10))
                .unwrap();
        }));
        let broker = f.broker.clone();
        let request = start(&work);
        let start_task = tokio::spawn(async move {
            broker
                .orchestration_operation("agent_execute", request)
                .await
        });
        tokio::task::spawn_blocking(move || {
            entered_rx.recv_timeout(Duration::from_secs(10)).unwrap()
        })
        .await
        .unwrap();
        let broker = f.broker.clone();
        let manager = f.manager.clone();
        let (attempt_tx, attempt_rx) = tokio::sync::oneshot::channel();
        let (mutation_tx, mut mutation_rx) = tokio::sync::oneshot::channel();
        let update = tokio::spawn(async move {
            attempt_tx.send(()).unwrap();
            let _management = broker.management.lock().await;
            mutation_tx.send(()).unwrap();
            match mutation {
                "route" | "enabled" => {
                    broker
                        .supervisor
                        .mutate_provider_settings(|settings| {
                            if mutation == "route" {
                                settings.role_routing.insert("testing".into(), None);
                            } else {
                                settings.providers.get_mut("fixture").unwrap().enabled = false;
                            }
                        })
                        .unwrap();
                }
                "health" => {
                    // 通过既有 discovery 注入点实际发布 Unavailable，绝不启动用户 CLI。
                    let health = crate::agent::codex::TEST_BACKEND_DISCOVERY
                        .scope(
                            Err("TEST_ROUTING_HEALTH_UNAVAILABLE".into()),
                            manager
                                .refresh_provider_health(ProviderId::new("codex".into()).unwrap()),
                        )
                        .await
                        .unwrap();
                    assert_eq!(health, ProviderHealth::Unavailable);
                }
                _ => {
                    let mut config = broker.config();
                    config.agent_enabled = false;
                    broker.supervisor.replace_config(config).unwrap();
                }
            }
        });
        attempt_rx.await.unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(50), &mut mutation_rx)
                .await
                .is_err(),
            "{mutation}"
        );
        assert_eq!(
            f.manager
                .registry()
                .unwrap()
                .health(&ProviderId::new("codex".into()).unwrap())
                .unwrap(),
            ProviderHealth::Available
        );
        assert!(f.rows()[0].is_empty());
        release_tx.send(()).unwrap();
        mutation_rx.await.unwrap();
        update.await.unwrap();
        // 提交后的开关可能拒绝 dispatch；无论 handoff 结果如何，已创建身份不能被改写。
        let _response = start_task.await.unwrap();
        let rows = f.store.product_read(None, None, None, 10).await.unwrap();
        assert_eq!(rows.len(), 1, "{mutation}");
        assert_eq!(rows[0].execution.provider, "fixture");
        assert_eq!(rows[0].task_role, "testing");
    }
}

/// 查询后的真实 health refresh 必须影响随后创建，不能继续使用目录中的 Available。
#[tokio::test]
async fn query_then_health_refresh_rejects_start_without_side_effects() {
    let f = RoutingFixture::new(ProviderHealth::Available, true, None).await;
    let work = f.begin().await;
    f.policy(|settings| {
        settings.role_routing.insert(
            "testing".into(),
            Some(ProviderId::new("codex".into()).unwrap()),
        );
    })
    .await;
    let catalog = f
        .broker
        .orchestration_operation("agent_query", json!({"action":"providers"}))
        .await;
    assert_eq!(catalog["ok"], true);
    {
        let _management = f.broker.management.lock().await;
        let health = crate::agent::codex::TEST_BACKEND_DISCOVERY
            .scope(
                Err("TEST_ROUTING_HEALTH_UNAVAILABLE".into()),
                f.manager
                    .refresh_provider_health(ProviderId::new("codex".into()).unwrap()),
            )
            .await
            .unwrap();
        assert_eq!(health, ProviderHealth::Unavailable);
    }
    let before = f.rows();
    let mut request = start(&work);
    request["providerId"] = json!("codex");
    assert_eq!(
        f.execute(request).await["error"]["code"],
        "AGENT_PROVIDER_UNAVAILABLE"
    );
    assert_eq!(f.rows(), before);
}

/// 在既有 handoff 同步点停住；此时必须已释放 management，允许管理操作完成。
#[tokio::test]
async fn management_is_released_before_provider_handoff() {
    let handoff = Arc::new((tokio::sync::Notify::new(), tokio::sync::Notify::new()));
    let f = RoutingFixture::new(ProviderHealth::Available, true, Some(handoff.clone())).await;
    let work = f.begin().await;
    let broker = f.broker.clone();
    let request = start(&work);
    let task = tokio::spawn(async move {
        broker
            .orchestration_operation("agent_execute", request)
            .await
    });
    tokio::time::timeout(Duration::from_secs(10), handoff.0.notified())
        .await
        .unwrap();
    assert_eq!(f.rows()[0].len(), 1);
    assert_eq!(f.provider.calls.load(Ordering::SeqCst), 0);
    let management = tokio::time::timeout(Duration::from_secs(1), f.broker.management.lock())
        .await
        .unwrap();
    f.broker
        .supervisor
        .mutate_provider_settings(|settings| {
            settings.role_routing.insert("testing".into(), None);
        })
        .unwrap();
    drop(management);
    handoff.1.notify_one();
    assert_eq!(task.await.unwrap()["ok"], true);
    assert_eq!(f.provider.calls.load(Ordering::SeqCst), 1);
}

/// 请求进入 Broker 后等待 management 时，最终读取必须观察随后提交的 agentEnabled。
#[tokio::test]
async fn waiting_start_rechecks_agent_enabled_after_management_acquisition() {
    let f = RoutingFixture::new(ProviderHealth::Available, true, None).await;
    let work = f.begin().await;
    let management = f.broker.management.lock().await;
    let broker = f.broker.clone();
    let request = start(&work);
    let mut task = tokio::spawn(async move {
        broker
            .orchestration_operation("agent_execute", request)
            .await
    });
    assert!(
        tokio::time::timeout(Duration::from_millis(50), &mut task)
            .await
            .is_err()
    );
    let before = f.rows();
    let mut config = f.broker.config();
    config.agent_enabled = false;
    f.broker.supervisor.replace_config(config).unwrap();
    drop(management);
    let response = task.await.unwrap();
    assert_eq!(response["error"]["code"], "AGENT_DISABLED");
    assert_eq!(f.rows(), before);
    assert_eq!(f.provider.calls.load(Ordering::SeqCst), 0);
}
