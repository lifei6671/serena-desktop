//! Workspace capability health 的只读、Descriptor 驱动投影。

use super::{
    CapabilityAction, CapabilityAvailability, CapabilityInstallationState, CapabilityObservation,
    CapabilityReadinessState, CapabilityRuntimeState, CapabilityStage, CapabilityStageState,
    RuntimeSlotKey, WorkspaceCapabilityDescriptor, WorkspaceCapabilityManager,
    WorkspaceCapabilityProvider, lock_unpoisoned,
};
use crate::workspace_resolver::WorkspaceLease;
use serde::Serialize;
use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{task::JoinSet, time::timeout};

/// 单个 Provider 的 Health 观察上界；超时只影响该 Provider 的本次 freshness snapshot。
const CAPABILITY_HEALTH_OBSERVE_TIMEOUT: Duration = Duration::from_secs(5);

/// Local-only Workspace health snapshot，不构成 Workspace Registry Authority。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceCapabilityHealth {
    pub(crate) workspace_id: String,
    pub(crate) providers: BTreeMap<String, WorkspaceProviderHealth>,
}

/// Descriptor 与本次观察合成后的单个 Provider 安全投影。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceProviderHealth {
    pub(crate) display_name: String,
    pub(crate) installation: CapabilityInstallationState,
    pub(crate) readiness: CapabilityReadinessState,
    pub(crate) status: CapabilityAvailability,
    pub(crate) runtime_state: CapabilityRuntimeState,
    pub(crate) checked_at: u64,
    pub(crate) stages: Vec<CapabilityStage>,
    pub(crate) actions: Vec<CapabilityAction>,
}

/// Provider task 只保留安全枚举和 observation，不把原始错误跨越投影边界。
enum ProviderObserveResult {
    Installation(CapabilityInstallationState),
    Observation(CapabilityObservation),
    Failed(Option<CapabilityInstallationState>),
}

impl WorkspaceCapabilityManager {
    /// 并行形成指定 Lease 的新鲜 health snapshot，绝不启动 Runtime 或执行准备动作。
    pub(crate) async fn observe_health(&self, lease: WorkspaceLease) -> WorkspaceCapabilityHealth {
        let mut observations = JoinSet::new();
        let mut providers = BTreeMap::new();

        for provider in self.registry.providers() {
            let descriptor = provider.descriptor().clone();
            let provider_id = descriptor.provider_id.as_str().to_owned();
            // 先放入 fail-closed 占位；task panic/cancellation 也只影响自己的 Provider。
            providers.insert(
                provider_id,
                error_projection(&descriptor, self.runtime_state_for(&descriptor, &lease)),
            );
            let task_provider = Arc::clone(provider);
            let task_lease = lease.clone();
            observations.spawn(async move {
                let observed = observe_provider(task_provider, task_lease).await;
                (descriptor, observed)
            });
        }

        while let Some(joined) = observations.join_next().await {
            let Ok((descriptor, observed)) = joined else {
                // 预置 fail-closed 投影已覆盖异常退出的任务，不能影响其他 Provider。
                continue;
            };
            let runtime_state = self.runtime_state_for(&descriptor, &lease);
            providers.insert(
                descriptor.provider_id.as_str().to_owned(),
                project_provider_health(&descriptor, observed, runtime_state),
            );
        }

        WorkspaceCapabilityHealth {
            workspace_id: lease.workspace_id,
            providers,
        }
    }

    /// 只在短暂 Manager lock 内读取准确 Slot lifecycle；没有当前 generation 的 Slot 即 stopped。
    fn runtime_state_for(
        &self,
        descriptor: &WorkspaceCapabilityDescriptor,
        lease: &WorkspaceLease,
    ) -> CapabilityRuntimeState {
        let key = RuntimeSlotKey::new(descriptor.provider_id.clone(), lease);
        let table = lock_unpoisoned(&self.runtime_slots);
        let Some(slot) = table.slots.get(&key) else {
            return CapabilityRuntimeState::Stopped;
        };
        lock_unpoisoned(&slot.state).lifecycle
    }
}

/// 在每个 Provider 自己的有限任务中执行只读 installation/readiness 观察。
async fn observe_provider(
    provider: Arc<dyn WorkspaceCapabilityProvider>,
    lease: WorkspaceLease,
) -> ProviderObserveResult {
    match timeout(CAPABILITY_HEALTH_OBSERVE_TIMEOUT, async move {
        let installation = match provider.probe_installation().await {
            Ok(installation) => installation,
            Err(_) => return ProviderObserveResult::Failed(None),
        };
        if installation.state != CapabilityInstallationState::Installed {
            return ProviderObserveResult::Installation(installation.state);
        }
        match provider.observe_readiness(lease).await {
            Ok(observation) => ProviderObserveResult::Observation(observation),
            Err(_) => ProviderObserveResult::Failed(Some(installation.state)),
        }
    })
    .await
    {
        Ok(observed) => observed,
        Err(_) => ProviderObserveResult::Failed(None),
    }
}

/// 合成静态 Descriptor 与动态 observation；所有 identity 不一致均只让目标 Provider fail-closed。
fn project_provider_health(
    descriptor: &WorkspaceCapabilityDescriptor,
    observed: ProviderObserveResult,
    runtime_state: CapabilityRuntimeState,
) -> WorkspaceProviderHealth {
    match observed {
        ProviderObserveResult::Installation(CapabilityInstallationState::NotInstalled) => {
            unavailable_projection(descriptor, runtime_state)
        }
        ProviderObserveResult::Installation(CapabilityInstallationState::CheckFailed)
        | ProviderObserveResult::Installation(CapabilityInstallationState::Installed) => {
            error_projection(descriptor, runtime_state)
        }
        ProviderObserveResult::Failed(installation) => failure_projection(
            descriptor,
            runtime_state,
            installation.unwrap_or(CapabilityInstallationState::CheckFailed),
        ),
        ProviderObserveResult::Observation(observation)
            if observation_matches_descriptor(descriptor, &observation) =>
        {
            WorkspaceProviderHealth {
                display_name: descriptor.display_name.clone(),
                installation: CapabilityInstallationState::Installed,
                readiness: observation.readiness,
                status: CapabilityAvailability::Ready,
                runtime_state,
                checked_at: observation.checked_at,
                stages: project_stages(descriptor, &observation),
                actions: descriptor_actions(descriptor),
            }
        }
        ProviderObserveResult::Observation(_) => failure_projection(
            descriptor,
            runtime_state,
            CapabilityInstallationState::Installed,
        ),
    }
}

/// 无安装时仍仅显示 Descriptor 的安全静态内容，不能伪造该 Workspace 的准备状态。
fn unavailable_projection(
    descriptor: &WorkspaceCapabilityDescriptor,
    runtime_state: CapabilityRuntimeState,
) -> WorkspaceProviderHealth {
    WorkspaceProviderHealth {
        display_name: descriptor.display_name.clone(),
        installation: CapabilityInstallationState::NotInstalled,
        readiness: CapabilityReadinessState::Unknown,
        status: CapabilityAvailability::Unavailable,
        runtime_state,
        checked_at: current_checked_at(),
        stages: descriptor_stages(descriptor, CapabilityStageState::Unknown),
        actions: descriptor_actions(descriptor),
    }
}

/// Provider 失败与 Descriptor identity 不一致不泄露原始错误，也不改变其他 Provider。
fn error_projection(
    descriptor: &WorkspaceCapabilityDescriptor,
    runtime_state: CapabilityRuntimeState,
) -> WorkspaceProviderHealth {
    failure_projection(
        descriptor,
        runtime_state,
        CapabilityInstallationState::CheckFailed,
    )
}

/// 失败保留已经安全确认的安装事实，但 readiness/status 始终 fail-closed。
fn failure_projection(
    descriptor: &WorkspaceCapabilityDescriptor,
    runtime_state: CapabilityRuntimeState,
    installation: CapabilityInstallationState,
) -> WorkspaceProviderHealth {
    let descriptor_valid = descriptor_identities_are_valid(descriptor);
    WorkspaceProviderHealth {
        display_name: descriptor.display_name.clone(),
        installation,
        readiness: CapabilityReadinessState::Unknown,
        status: CapabilityAvailability::Error,
        runtime_state,
        checked_at: current_checked_at(),
        stages: descriptor_valid
            .then(|| descriptor_stages(descriptor, CapabilityStageState::Unknown))
            .unwrap_or_default(),
        actions: descriptor_valid
            .then(|| descriptor_actions(descriptor))
            .unwrap_or_default(),
    }
}

/// stage/action 必须一一对应 Descriptor；Provider 无权覆盖静态显示和执行元数据。
fn observation_matches_descriptor(
    descriptor: &WorkspaceCapabilityDescriptor,
    observation: &CapabilityObservation,
) -> bool {
    observation.provider_id == descriptor.provider_id
        && observation.installation == CapabilityInstallationState::Installed
        && descriptor_identities_are_valid(descriptor)
        && same_unique_ids(
            descriptor
                .stage_descriptors
                .iter()
                .map(|stage| stage.id.as_str()),
            observation.stages.iter().map(|stage| stage.id.as_str()),
        )
        && same_unique_ids(
            descriptor
                .action_descriptors
                .iter()
                .map(|action| action.action_id.as_str()),
            observation.actions.iter().map(|action| action.id.as_str()),
        )
}

/// 只接受非空且不重复的 Descriptor identity，防止错误 Provider 形成歧义投影。
fn descriptor_identities_are_valid(descriptor: &WorkspaceCapabilityDescriptor) -> bool {
    unique_nonempty(
        descriptor
            .stage_descriptors
            .iter()
            .map(|stage| stage.id.as_str()),
    ) && unique_nonempty(
        descriptor
            .action_descriptors
            .iter()
            .map(|action| action.action_id.as_str()),
    )
}

/// 比较两个 identity 集合并同时拒绝重复或空 identity。
fn same_unique_ids<'a>(
    expected: impl Iterator<Item = &'a str>,
    actual: impl Iterator<Item = &'a str>,
) -> bool {
    let expected: Vec<_> = expected.collect();
    let actual: Vec<_> = actual.collect();
    expected.len() == actual.len()
        && unique_nonempty(expected.iter().copied())
        && unique_nonempty(actual.iter().copied())
        && expected.iter().all(|id| actual.contains(id))
}

/// 验证 identity 不为空且只出现一次。
fn unique_nonempty<'a>(mut ids: impl Iterator<Item = &'a str>) -> bool {
    let mut known = std::collections::HashSet::new();
    ids.all(|id| !id.trim().is_empty() && known.insert(id))
}

/// 动态 state/message 仅来自本次 observation，名称和 requirement 始终来自 Descriptor。
fn project_stages(
    descriptor: &WorkspaceCapabilityDescriptor,
    observation: &CapabilityObservation,
) -> Vec<CapabilityStage> {
    descriptor
        .stage_descriptors
        .iter()
        .map(|descriptor_stage| {
            let observation_stage = observation
                .stages
                .iter()
                .find(|stage| stage.id == descriptor_stage.id)
                .expect("validated stage identities must retain every Descriptor stage");
            CapabilityStage {
                id: descriptor_stage.id.clone(),
                display_name: descriptor_stage.display_name.clone(),
                state: observation_stage.state,
                requirement: descriptor_stage.requirement,
                message_code: observation_stage.message_code.clone(),
            }
        })
        .collect()
}

/// 未观察或失败时只能从 Descriptor 形成 unknown stage 外壳。
fn descriptor_stages(
    descriptor: &WorkspaceCapabilityDescriptor,
    state: CapabilityStageState,
) -> Vec<CapabilityStage> {
    descriptor
        .stage_descriptors
        .iter()
        .map(|stage| CapabilityStage {
            id: stage.id.clone(),
            display_name: stage.display_name.clone(),
            state,
            requirement: stage.requirement,
            message_code: None,
        })
        .collect()
}

/// action 的名称、Authority 与执行方式只由编译期 Descriptor 提供。
fn descriptor_actions(descriptor: &WorkspaceCapabilityDescriptor) -> Vec<CapabilityAction> {
    descriptor
        .action_descriptors
        .iter()
        .map(|action| CapabilityAction {
            id: action.action_id.clone(),
            display_name: action.display_name.clone(),
            authority: action.authority,
            execution: action.execution,
        })
        .collect()
}

/// 失败投影自带本次时间戳，但永不将时间写入 Workspace Authority。
fn current_checked_at() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace_capability::{
        CapabilityActionAuthority, CapabilityActionDescriptor, CapabilityActionExecution,
        CapabilityFuture, CapabilityInstallation, CapabilityPrepareAction, CapabilityPrepareResult,
        CapabilityProviderError, CapabilityProviderErrorCode, CapabilityReadinessProbe,
        CapabilityRuntimeHandle, CapabilityRuntimeModel, CapabilityRuntimePolicy,
        CapabilityStageDescriptor, CapabilityStageRequirement, CapabilityStopFailure, StopEvidence,
        WorkspaceCapabilityProviderId, WorkspaceCapabilityRegistry, WorkspaceToolCall,
        WorkspaceToolResult,
    };
    use std::{
        path::PathBuf,
        sync::{
            Mutex,
            atomic::{AtomicUsize, Ordering},
        },
    };

    /// 用非 Serena provider 验证 projection 不依赖任何内置 Provider ID。
    struct HealthProvider {
        descriptor: WorkspaceCapabilityDescriptor,
        installation: CapabilityInstallationState,
        observation: Mutex<Result<CapabilityObservation, CapabilityProviderError>>,
        probes: AtomicUsize,
        observations: AtomicUsize,
        prepares: AtomicUsize,
        starts: AtomicUsize,
    }

    impl HealthProvider {
        /// 构造一个提供单 stage/action 的纯 observation fake。
        fn new(id: &str, installation: CapabilityInstallationState) -> Arc<Self> {
            let descriptor = test_descriptor(id);
            let observation = ready_observation(&descriptor, CapabilityReadinessState::Ready);
            Arc::new(Self {
                descriptor,
                installation,
                observation: Mutex::new(Ok(observation)),
                probes: AtomicUsize::new(0),
                observations: AtomicUsize::new(0),
                prepares: AtomicUsize::new(0),
                starts: AtomicUsize::new(0),
            })
        }

        /// 覆盖后续 readiness 结果以覆盖 Provider failure 与 identity mismatch。
        fn set_observation(
            &self,
            observation: Result<CapabilityObservation, CapabilityProviderError>,
        ) {
            *lock_unpoisoned(&self.observation) = observation;
        }
    }

    impl WorkspaceCapabilityProvider for HealthProvider {
        fn descriptor(&self) -> &WorkspaceCapabilityDescriptor {
            &self.descriptor
        }

        fn probe_installation(
            &self,
        ) -> CapabilityFuture<'_, Result<CapabilityInstallation, CapabilityProviderError>> {
            self.probes.fetch_add(1, Ordering::SeqCst);
            let state = self.installation;
            Box::pin(async move {
                Ok(CapabilityInstallation {
                    state,
                    detected_version: None,
                })
            })
        }

        fn observe_readiness(
            &self,
            _lease: WorkspaceLease,
        ) -> CapabilityFuture<'_, Result<CapabilityObservation, CapabilityProviderError>> {
            self.observations.fetch_add(1, Ordering::SeqCst);
            let observation = lock_unpoisoned(&self.observation).clone();
            Box::pin(async move { observation })
        }

        fn prepare<'a>(
            &'a self,
            _lease: WorkspaceLease,
            _action: CapabilityPrepareAction,
            _activity: &'a dyn super::super::CapabilityActivitySink,
        ) -> CapabilityFuture<'a, Result<CapabilityPrepareResult, CapabilityProviderError>>
        {
            self.prepares.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { panic!("health observation must not prepare") })
        }

        fn start(
            &self,
            _lease: WorkspaceLease,
        ) -> CapabilityFuture<'_, Result<CapabilityRuntimeHandle, CapabilityProviderError>>
        {
            self.starts.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { panic!("health observation must not start a runtime") })
        }

        fn call<'a>(
            &'a self,
            _lease: &'a WorkspaceLease,
            _runtime: Option<&'a CapabilityRuntimeHandle>,
            _tool: WorkspaceToolCall,
        ) -> CapabilityFuture<'a, Result<WorkspaceToolResult, CapabilityProviderError>> {
            Box::pin(async { panic!("health observation must not call a provider tool") })
        }

        fn stop(
            &self,
            _runtime: CapabilityRuntimeHandle,
        ) -> CapabilityFuture<'_, Result<StopEvidence, CapabilityStopFailure>> {
            Box::pin(async { panic!("health observation must not stop a runtime") })
        }
    }

    /// 构造带 Descriptor display metadata 的 generic provider，避免 Serena 特判。
    fn test_descriptor(id: &str) -> WorkspaceCapabilityDescriptor {
        WorkspaceCapabilityDescriptor {
            provider_id: WorkspaceCapabilityProviderId::new(id),
            display_name: format!("{id} display"),
            tool_names: vec![format!("{id}_tool")],
            runtime_model: CapabilityRuntimeModel::WorkspaceScopedProcess,
            readiness_probe: CapabilityReadinessProbe::Required,
            preparation_policy: super::super::CapabilityPreparationPolicy::ExplicitOnly,
            stage_descriptors: vec![CapabilityStageDescriptor {
                id: "index".into(),
                display_name: "Descriptor index".into(),
                requirement: CapabilityStageRequirement::Required,
            }],
            action_descriptors: vec![CapabilityActionDescriptor {
                action_id: "prepare_index".into(),
                display_name: "Descriptor prepare".into(),
                authority: CapabilityActionAuthority::LocalHuman,
                execution: CapabilityActionExecution::ProviderPrepare,
                warm_runtime: false,
            }],
            runtime_policy: CapabilityRuntimePolicy {
                max_instances: 2,
                idle_timeout_ms: 30_000,
                per_slot_concurrency: 1,
            },
        }
    }

    /// 构造与 Descriptor identity 对齐、但带无权覆盖静态 display metadata 的 observation。
    fn ready_observation(
        descriptor: &WorkspaceCapabilityDescriptor,
        readiness: CapabilityReadinessState,
    ) -> CapabilityObservation {
        CapabilityObservation {
            provider_id: descriptor.provider_id.clone(),
            installation: CapabilityInstallationState::Installed,
            readiness,
            runtime_state: CapabilityRuntimeState::Ready,
            checked_at: 42,
            stages: vec![CapabilityStage {
                id: "index".into(),
                display_name: "Untrusted provider name".into(),
                state: CapabilityStageState::Ready,
                requirement: CapabilityStageRequirement::Optional,
                message_code: Some("INDEX_READY".into()),
            }],
            actions: vec![CapabilityAction {
                id: "prepare_index".into(),
                display_name: "Untrusted provider action".into(),
                authority: CapabilityActionAuthority::LocalHuman,
                execution: CapabilityActionExecution::ManagerEnsureRuntime,
            }],
        }
    }

    /// 构造永远由服务端取得的 Lease，不携带 caller supplied root。
    fn test_lease(id: &str) -> WorkspaceLease {
        WorkspaceLease {
            workspace_id: id.into(),
            canonical_root: PathBuf::from("C:/server-resolved"),
            generation: 7,
        }
    }

    /// 将 generic fake 组合为 immutable Registry/Manager。
    fn health_manager(providers: Vec<Arc<HealthProvider>>) -> WorkspaceCapabilityManager {
        let providers: Vec<Arc<dyn WorkspaceCapabilityProvider>> = providers
            .into_iter()
            .map(|provider| provider as Arc<dyn WorkspaceCapabilityProvider>)
            .collect();
        WorkspaceCapabilityManager::new(Arc::new(
            WorkspaceCapabilityRegistry::new(providers).unwrap(),
        ))
    }

    #[tokio::test]
    async fn health_projection_keeps_ready_and_not_prepared_available_while_stopped() {
        let ready = HealthProvider::new("third-ready", CapabilityInstallationState::Installed);
        let not_prepared =
            HealthProvider::new("third-pending", CapabilityInstallationState::Installed);
        not_prepared.set_observation(Ok(ready_observation(
            not_prepared.descriptor(),
            CapabilityReadinessState::NotPrepared,
        )));
        let manager = health_manager(vec![Arc::clone(&ready), Arc::clone(&not_prepared)]);

        let health = manager.observe_health(test_lease("a")).await;
        let ready_health = &health.providers["third-ready"];
        let pending_health = &health.providers["third-pending"];
        assert_eq!(health.workspace_id, "a");
        assert_eq!(ready_health.status, CapabilityAvailability::Ready);
        assert_eq!(ready_health.readiness, CapabilityReadinessState::Ready);
        assert_eq!(ready_health.runtime_state, CapabilityRuntimeState::Stopped);
        assert_eq!(pending_health.status, CapabilityAvailability::Ready);
        assert_eq!(
            pending_health.readiness,
            CapabilityReadinessState::NotPrepared
        );
        assert_eq!(
            pending_health.runtime_state,
            CapabilityRuntimeState::Stopped
        );
        assert_eq!(ready_health.display_name, "third-ready display");
        assert_eq!(ready_health.stages[0].display_name, "Descriptor index");
        assert_eq!(
            ready_health.stages[0].requirement,
            CapabilityStageRequirement::Required
        );
        assert_eq!(
            ready_health.stages[0].message_code.as_deref(),
            Some("INDEX_READY")
        );
        assert_eq!(ready_health.actions[0].display_name, "Descriptor prepare");
        assert_eq!(ready.prepares.load(Ordering::SeqCst), 0);
        assert_eq!(ready.starts.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn health_projection_isolates_unavailable_and_observation_failure_and_adds_third_provider()
     {
        let unavailable = HealthProvider::new(
            "third-unavailable",
            CapabilityInstallationState::NotInstalled,
        );
        let check_failed = HealthProvider::new(
            "third-check-failed",
            CapabilityInstallationState::CheckFailed,
        );
        let broken = HealthProvider::new("third-broken", CapabilityInstallationState::Installed);
        broken.set_observation(Err(CapabilityProviderError {
            code: CapabilityProviderErrorCode::OperationFailed,
        }));
        let normal = HealthProvider::new("fourth-normal", CapabilityInstallationState::Installed);
        let manager = health_manager(vec![
            Arc::clone(&unavailable),
            Arc::clone(&check_failed),
            Arc::clone(&broken),
            Arc::clone(&normal),
        ]);

        let health = manager.observe_health(test_lease("a")).await;
        assert_eq!(health.providers.len(), 4);
        assert_eq!(
            health.providers["third-unavailable"].status,
            CapabilityAvailability::Unavailable
        );
        assert_eq!(
            health.providers["third-unavailable"].readiness,
            CapabilityReadinessState::Unknown
        );
        assert_eq!(unavailable.observations.load(Ordering::SeqCst), 0);
        assert_eq!(
            health.providers["third-check-failed"].status,
            CapabilityAvailability::Error
        );
        assert_eq!(
            check_failed.observations.load(Ordering::SeqCst),
            0,
            "check_failed must not read Workspace readiness"
        );
        assert_eq!(
            health.providers["third-broken"].status,
            CapabilityAvailability::Error
        );
        assert_eq!(
            health.providers["third-broken"].installation,
            CapabilityInstallationState::Installed
        );
        assert_eq!(
            health.providers["third-broken"].readiness,
            CapabilityReadinessState::Unknown
        );
        assert_eq!(
            health.providers["fourth-normal"].status,
            CapabilityAvailability::Ready
        );
    }

    #[tokio::test]
    async fn health_projection_uses_manager_slot_lifecycle_without_cross_workspace_leakage() {
        let provider = HealthProvider::new("third-runtime", CapabilityInstallationState::Installed);
        let manager = health_manager(vec![Arc::clone(&provider)]);
        let workspace_a = test_lease("a");
        let workspace_b = test_lease("b");
        let key = RuntimeSlotKey::new(provider.descriptor.provider_id.clone(), &workspace_a);
        let slot = Arc::new(super::super::RuntimeSlot::new(
            workspace_a.canonical_root.clone(),
            provider.descriptor.runtime_policy.per_slot_concurrency,
        ));
        lock_unpoisoned(&manager.runtime_slots)
            .slots
            .insert(key, Arc::clone(&slot));

        for lifecycle in [
            CapabilityRuntimeState::Starting,
            CapabilityRuntimeState::Ready,
            CapabilityRuntimeState::Error,
            CapabilityRuntimeState::Stopping,
        ] {
            lock_unpoisoned(&slot.state).lifecycle = lifecycle;
            let health_a = manager.observe_health(workspace_a.clone()).await;
            assert_eq!(health_a.providers["third-runtime"].runtime_state, lifecycle);
            assert_eq!(
                health_a.providers["third-runtime"].status,
                CapabilityAvailability::Ready
            );
        }
        let health_b = manager.observe_health(workspace_b).await;
        assert_eq!(health_b.workspace_id, "b");
        assert_eq!(
            health_b.providers["third-runtime"].runtime_state,
            CapabilityRuntimeState::Stopped
        );
    }

    #[tokio::test]
    async fn health_projection_fails_closed_for_observation_identity_mismatch_and_never_serializes_private_runtime_data()
     {
        let provider =
            HealthProvider::new("third-mismatch", CapabilityInstallationState::Installed);
        let mut mismatch =
            ready_observation(provider.descriptor(), CapabilityReadinessState::Ready);
        mismatch.stages[0].id = "unknown-stage".into();
        provider.set_observation(Ok(mismatch));
        let manager = health_manager(vec![Arc::clone(&provider)]);

        let health = manager.observe_health(test_lease("a")).await;
        let projected = &health.providers["third-mismatch"];
        assert_eq!(projected.status, CapabilityAvailability::Error);
        assert_eq!(projected.readiness, CapabilityReadinessState::Unknown);
        let wire = serde_json::to_string(&health).unwrap().to_ascii_lowercase();
        for forbidden in [
            "canonicalroot",
            "root",
            "path",
            "pid",
            "port",
            "client",
            "handle",
            "serena_home",
            "command",
            "argv",
            "raw",
            "lastusedat",
            "inflight",
        ] {
            assert!(!wire.contains(forbidden), "health wire leaked {forbidden}");
        }
        assert_eq!(provider.prepares.load(Ordering::SeqCst), 0);
        assert_eq!(provider.starts.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn health_projection_isolates_provider_id_and_action_identity_mismatches() {
        let provider_id_mismatch = HealthProvider::new(
            "third-provider-mismatch",
            CapabilityInstallationState::Installed,
        );
        let mut wrong_provider = ready_observation(
            provider_id_mismatch.descriptor(),
            CapabilityReadinessState::Ready,
        );
        wrong_provider.provider_id = WorkspaceCapabilityProviderId::new("another-provider");
        provider_id_mismatch.set_observation(Ok(wrong_provider));

        let action_mismatch = HealthProvider::new(
            "third-action-mismatch",
            CapabilityInstallationState::Installed,
        );
        let mut duplicate_action = ready_observation(
            action_mismatch.descriptor(),
            CapabilityReadinessState::Ready,
        );
        duplicate_action
            .actions
            .push(duplicate_action.actions[0].clone());
        action_mismatch.set_observation(Ok(duplicate_action));

        let normal = HealthProvider::new("fourth-normal", CapabilityInstallationState::Installed);
        let manager = health_manager(vec![
            Arc::clone(&provider_id_mismatch),
            Arc::clone(&action_mismatch),
            Arc::clone(&normal),
        ]);

        let health = manager.observe_health(test_lease("a")).await;
        assert_eq!(
            health.providers["third-provider-mismatch"].status,
            CapabilityAvailability::Error
        );
        assert_eq!(
            health.providers["third-action-mismatch"].status,
            CapabilityAvailability::Error
        );
        assert_eq!(
            health.providers["fourth-normal"].status,
            CapabilityAvailability::Ready
        );
    }

    #[test]
    fn fallback_checked_at_uses_the_same_epoch_millisecond_scale_as_observations() {
        let descriptor = test_descriptor("third-timestamp");
        let before = epoch_millis();
        let unavailable = unavailable_projection(&descriptor, CapabilityRuntimeState::Stopped);
        let failed = error_projection(&descriptor, CapabilityRuntimeState::Stopped);
        let after = epoch_millis();
        let mut observation = ready_observation(&descriptor, CapabilityReadinessState::Ready);
        observation.checked_at = before;
        let successful = project_provider_health(
            &descriptor,
            ProviderObserveResult::Observation(observation),
            CapabilityRuntimeState::Stopped,
        );

        for checked_at in [unavailable.checked_at, failed.checked_at] {
            assert!(
                (before..=after).contains(&checked_at),
                "fallback checkedAt must remain in the current epoch-millisecond window"
            );
            assert!(
                checked_at > 1_000_000_000_000,
                "fallback checkedAt must not use epoch seconds"
            );
        }
        assert_eq!(successful.checked_at, before);
    }

    /// 以同一安全转换取得 wall-clock 边界，避免毫秒测试依赖精确相等时序。
    fn epoch_millis() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
            .unwrap_or(0)
    }
}
