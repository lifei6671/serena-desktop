//! Workspace Capability 的 provider-agnostic domain foundation。
#![allow(
    dead_code,
    reason = "P2A3-001 freezes the domain port before Registry and Manager consumers exist."
)]

use crate::workspace_resolver::WorkspaceLease;
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    future::Future,
    path::PathBuf,
    pin::Pin,
    sync::{Arc, Mutex, MutexGuard},
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, watch};

/// Capability 异步接口使用的标准库 boxed future，避免新增 futures 依赖。
pub(crate) type CapabilityFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// 编译期注册 capability 的稳定 provider identity。
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub(crate) struct WorkspaceCapabilityProviderId(String);

impl WorkspaceCapabilityProviderId {
    /// 创建由编译期注册表使用的 provider identity。
    pub(crate) fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// 返回 identity 的字符串值，供 Registry 做唯一性校验。
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

/// Provider 的 Runtime 模型，不推导具体进程实现。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CapabilityRuntimeModel {
    InProcess,
    StatelessCommand,
    WorkspaceScopedProcess,
}

/// Provider 是否需要 Workspace readiness 探测。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum CapabilityReadinessProbe {
    Required,
    None,
}

/// 首次 Tool 调用前的准备策略。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CapabilityPreparationPolicy {
    None,
    AutoOnFirstToolCall,
    ExplicitOnly,
}

/// Stage 的准备要求，供未来 UI 以通用方式投影。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CapabilityStageRequirement {
    Required,
    AutoPreparable,
    Optional,
}

/// Descriptor 中声明的单个 provider-agnostic stage。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CapabilityStageDescriptor {
    pub(crate) id: String,
    pub(crate) requirement: CapabilityStageRequirement,
}

/// 显式准备动作的授权边界。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CapabilityActionAuthority {
    LocalHuman,
}

/// Manager 与 Provider 分别负责的动作执行模式。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CapabilityActionExecution {
    ManagerEnsureRuntime,
    ProviderPrepare,
}

/// Descriptor 中声明的单个准备动作。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CapabilityActionDescriptor {
    pub(crate) action_id: String,
    pub(crate) display_name: String,
    pub(crate) authority: CapabilityActionAuthority,
    pub(crate) execution: CapabilityActionExecution,
    pub(crate) warm_runtime: bool,
}

/// Provider Runtime 的调度上限；实际容量和 LRU 由后续 Manager 负责。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CapabilityRuntimePolicy {
    pub(crate) max_instances: usize,
    pub(crate) idle_timeout_ms: u64,
    pub(crate) per_slot_concurrency: usize,
}

/// 编译期 Provider 注册所需的最小 capability descriptor。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceCapabilityDescriptor {
    pub(crate) provider_id: WorkspaceCapabilityProviderId,
    pub(crate) display_name: String,
    pub(crate) tool_names: Vec<String>,
    pub(crate) runtime_model: CapabilityRuntimeModel,
    pub(crate) readiness_probe: CapabilityReadinessProbe,
    pub(crate) preparation_policy: CapabilityPreparationPolicy,
    pub(crate) stage_descriptors: Vec<CapabilityStageDescriptor>,
    pub(crate) action_descriptors: Vec<CapabilityActionDescriptor>,
    pub(crate) runtime_policy: CapabilityRuntimePolicy,
}

/// 外部安装探测的安全状态，与 Provider availability 分离。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CapabilityInstallationState {
    Installed,
    NotInstalled,
    CheckFailed,
}

/// Provider 安装探测的内部结果，不包含命令行或本地路径。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CapabilityInstallation {
    pub(crate) state: CapabilityInstallationState,
    pub(crate) detected_version: Option<String>,
}

/// Workspace 持久化准备的状态，独立于 capability availability。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CapabilityReadinessState {
    NotPrepared,
    Preparing,
    Ready,
    Degraded,
    Error,
    Unknown,
}

/// 单个 stage 的 provider-agnostic 运行时投影。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CapabilityStage {
    pub(crate) id: String,
    pub(crate) display_name: String,
    pub(crate) state: CapabilityStageState,
    pub(crate) requirement: CapabilityStageRequirement,
    pub(crate) message_code: Option<String>,
}

/// Stage 的公共状态不暴露 Provider 内部进程证据。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CapabilityStageState {
    Absent,
    Pending,
    Running,
    Ready,
    Stale,
    Error,
    Unknown,
}

/// Capability availability 与 Runtime lifecycle 分离的公共状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CapabilityAvailability {
    Ready,
    Unavailable,
    Error,
}

/// Manager Runtime Slot 的生命周期投影，不携带 Runtime 内部对象。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CapabilityRuntimeState {
    Stopped,
    Starting,
    Ready,
    Error,
    Stopping,
}

/// §8.2 冻结的单个 Provider Workspace observation 外壳。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CapabilityObservation {
    pub(crate) provider_id: WorkspaceCapabilityProviderId,
    pub(crate) installation: CapabilityInstallationState,
    pub(crate) readiness: CapabilityReadinessState,
    pub(crate) runtime_state: CapabilityRuntimeState,
    pub(crate) checked_at: u64,
    pub(crate) stages: Vec<CapabilityStage>,
    pub(crate) actions: Vec<CapabilityAction>,
}

/// §8.2 observation 内的可执行动作投影。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CapabilityAction {
    pub(crate) id: String,
    pub(crate) display_name: String,
    pub(crate) authority: CapabilityActionAuthority,
    pub(crate) execution: CapabilityActionExecution,
}

/// 由 Local Human Authority 发起的已声明准备动作。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CapabilityPrepareAction {
    pub(crate) action_id: String,
}

/// Provider prepare 的最小结果，仅回传统一 readiness 投影。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CapabilityPrepareResult {
    pub(crate) readiness: CapabilityReadinessState,
}

/// 提供给准备流程的 provider-agnostic 活动事件。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CapabilityActivity {
    pub(crate) phase: String,
}

/// Provider 向后续统一 activity 投影发布状态的 object-safe port。
pub(crate) trait CapabilityActivitySink: Send + Sync {
    /// 发布不包含 Provider 私有 Runtime 证据的活动事件。
    fn publish<'a>(&'a self, _activity: CapabilityActivity) -> CapabilityFuture<'a, ()> {
        Box::pin(async {})
    }
}

/// Manager-owned Runtime identity；其字段不形成 serde wire，也不暴露进程细节。
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct CapabilityRuntimeHandle {
    provider_id: WorkspaceCapabilityProviderId,
    workspace_id: String,
    workspace_generation: u64,
}

impl CapabilityRuntimeHandle {
    /// 由 Runtime 创建路径封装 server-resolved Lease identity。
    pub(crate) fn new(provider_id: WorkspaceCapabilityProviderId, lease: &WorkspaceLease) -> Self {
        Self {
            provider_id,
            workspace_id: lease.workspace_id.clone(),
            workspace_generation: lease.generation,
        }
    }
}

/// Provider 安全错误的封闭代码集，避免暴露命令、路径或原始进程错误。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum CapabilityProviderErrorCode {
    RuntimeIdentityMismatch,
    NotFound,
    ContractError,
    Unavailable,
    OperationFailed,
}

/// Provider Port 的安全错误 envelope。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CapabilityProviderError {
    pub(crate) code: CapabilityProviderErrorCode,
}

impl CapabilityProviderError {
    /// 返回 Runtime identity 不匹配的 fail-closed 错误。
    fn runtime_identity_mismatch() -> Self {
        Self {
            code: CapabilityProviderErrorCode::RuntimeIdentityMismatch,
        }
    }

    /// 返回不泄露 Provider 私有信息的统一未找到错误。
    fn not_found() -> Self {
        Self {
            code: CapabilityProviderErrorCode::NotFound,
        }
    }

    /// 返回编译期注册表的统一契约错误。
    fn contract_error() -> Self {
        Self {
            code: CapabilityProviderErrorCode::ContractError,
        }
    }
}

/// Workspace Capability Manager 对外只使用的安全错误代码，不携带 Provider 原始错误。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub(crate) enum WorkspaceCapabilityErrorCode {
    #[serde(rename = "WORKSPACE_CAPABILITY_NOT_FOUND")]
    NotFound,
    #[serde(rename = "WORKSPACE_CAPABILITY_NOT_INSTALLED")]
    NotInstalled,
    #[serde(rename = "WORKSPACE_CAPABILITY_NOT_PREPARED")]
    NotPrepared,
    #[serde(rename = "WORKSPACE_CAPABILITY_PREPARATION_REQUIRED")]
    PreparationRequired,
    #[serde(rename = "WORKSPACE_CAPABILITY_PREPARING")]
    Preparing,
    #[serde(rename = "WORKSPACE_CAPABILITY_PREPARE_FAILED")]
    PrepareFailed,
    #[serde(rename = "WORKSPACE_CAPABILITY_OBSERVE_FAILED")]
    ObserveFailed,
    #[serde(rename = "WORKSPACE_CAPABILITY_BUSY")]
    Busy,
    #[serde(rename = "WORKSPACE_CAPABILITY_START_FAILED")]
    StartFailed,
    #[serde(rename = "WORKSPACE_CAPABILITY_RUNTIME_LOST")]
    RuntimeLost,
    #[serde(rename = "WORKSPACE_CAPABILITY_STOP_FAILED")]
    StopFailed,
    #[serde(rename = "WORKSPACE_CAPABILITY_CONTRACT_ERROR")]
    ContractError,
}

/// Workspace Capability Manager 的统一错误 envelope，禁止存放 Provider 原始错误。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceCapabilityError {
    pub(crate) code: WorkspaceCapabilityErrorCode,
}

impl WorkspaceCapabilityError {
    /// 返回不存在 Provider 时的统一错误。
    fn not_found() -> Self {
        Self {
            code: WorkspaceCapabilityErrorCode::NotFound,
        }
    }

    /// 返回尚未实现 stop 流程时的统一忙碌错误。
    fn busy() -> Self {
        Self {
            code: WorkspaceCapabilityErrorCode::Busy,
        }
    }

    /// 返回不泄露 Provider 启动细节的统一启动失败错误。
    fn start_failed() -> Self {
        Self {
            code: WorkspaceCapabilityErrorCode::StartFailed,
        }
    }

    /// 返回 identity 或内部调用契约不一致时的 fail-closed 错误。
    fn contract_error() -> Self {
        Self {
            code: WorkspaceCapabilityErrorCode::ContractError,
        }
    }
}

/// 后续 Adapter 交给 Provider 的最小 Tool envelope，不承载 caller-provided root。
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceToolCall {
    pub(crate) tool_name: String,
    pub(crate) arguments: Value,
}

/// Provider 返回给后续 Adapter 的最小结果 envelope。
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceToolResult {
    pub(crate) result: Value,
}

/// Runtime 停止的通用安全证据。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StopEvidence {
    pub(crate) runtime_state: CapabilityRuntimeState,
}

/// Runtime 停止失败时由 Provider 返还给 Manager 的内部所有权与安全错误。
///
/// 此类型不是公开 wire，且不推导 Clone、Debug 或 serde，避免复制 opaque handle 或泄露私有 Runtime 细节。
pub(crate) struct CapabilityStopFailure {
    pub(crate) runtime: CapabilityRuntimeHandle,
    pub(crate) error: CapabilityProviderError,
}

/// Workspace-scoped capability 的 object-safe provider port。
pub(crate) trait WorkspaceCapabilityProvider: Send + Sync {
    /// 返回编译期注册且稳定的 Provider descriptor。
    fn descriptor(&self) -> &WorkspaceCapabilityDescriptor;

    /// 探测 Provider 安装状态，不触发 Runtime 或 Workspace 准备。
    fn probe_installation(
        &self,
    ) -> CapabilityFuture<'_, Result<CapabilityInstallation, CapabilityProviderError>>;

    /// 观察一个 server-resolved WorkspaceLease 的持久化 readiness。
    fn observe_readiness(
        &self,
        lease: WorkspaceLease,
    ) -> CapabilityFuture<'_, Result<CapabilityObservation, CapabilityProviderError>>;

    /// 按已声明动作执行 Provider 准备，不承担 Agent lifecycle 权限。
    fn prepare<'a>(
        &'a self,
        lease: WorkspaceLease,
        action: CapabilityPrepareAction,
        activity: &'a dyn CapabilityActivitySink,
    ) -> CapabilityFuture<'a, Result<CapabilityPrepareResult, CapabilityProviderError>>;

    /// 为 server-resolved Lease 启动或取得 Runtime handle。
    fn start(
        &self,
        lease: WorkspaceLease,
    ) -> CapabilityFuture<'_, Result<CapabilityRuntimeHandle, CapabilityProviderError>>;

    /// 在已验证 identity 的 Lease 与可选 Runtime handle 下执行 Tool。
    fn call<'a>(
        &'a self,
        lease: &'a WorkspaceLease,
        runtime: Option<&'a CapabilityRuntimeHandle>,
        tool: WorkspaceToolCall,
    ) -> CapabilityFuture<'a, Result<WorkspaceToolResult, CapabilityProviderError>>;

    /// 尝试消费 Runtime handle 并停止对应 Runtime；失败时必须返还同一 handle 给 Manager。
    fn stop(
        &self,
        runtime: CapabilityRuntimeHandle,
    ) -> CapabilityFuture<'_, Result<StopEvidence, CapabilityStopFailure>>;
}

/// 编译期组合完成后保持只读的 Workspace capability Provider 注册快照。
pub(crate) struct WorkspaceCapabilityRegistry {
    providers: Vec<Arc<dyn WorkspaceCapabilityProvider>>,
    providers_by_id: HashMap<WorkspaceCapabilityProviderId, Arc<dyn WorkspaceCapabilityProvider>>,
    tool_owners: HashMap<String, WorkspaceCapabilityProviderId>,
}

impl WorkspaceCapabilityRegistry {
    /// 从编译期内置 Provider 组合构造完整 Registry；任一契约错误均不发布部分结果。
    pub(crate) fn new(
        providers: impl IntoIterator<Item = Arc<dyn WorkspaceCapabilityProvider>>,
    ) -> Result<Self, CapabilityProviderError> {
        let mut ordered_providers = Vec::new();
        let mut providers_by_id = HashMap::new();
        let mut tool_owners = HashMap::new();

        for provider in providers {
            let descriptor = provider.descriptor();
            let provider_id = descriptor.provider_id.clone();

            if provider_id.as_str().trim().is_empty()
                || descriptor.runtime_policy.per_slot_concurrency == 0
                || (descriptor.runtime_model == CapabilityRuntimeModel::WorkspaceScopedProcess
                    && descriptor.runtime_policy.max_instances == 0)
                || providers_by_id.contains_key(&provider_id)
            {
                return Err(CapabilityProviderError::contract_error());
            }

            let mut provider_tools = HashSet::new();
            for tool_name in &descriptor.tool_names {
                if tool_name.trim().is_empty()
                    || !provider_tools.insert(tool_name.clone())
                    || tool_owners.contains_key(tool_name)
                {
                    return Err(CapabilityProviderError::contract_error());
                }
            }

            for tool_name in provider_tools {
                tool_owners.insert(tool_name, provider_id.clone());
            }
            providers_by_id.insert(provider_id, Arc::clone(&provider));
            ordered_providers.push(provider);
        }

        Ok(Self {
            providers: ordered_providers,
            providers_by_id,
            tool_owners,
        })
    }

    /// 按编译期注册顺序返回 Provider 列表，供通用能力投影遍历。
    pub(crate) fn providers(&self) -> &[Arc<dyn WorkspaceCapabilityProvider>] {
        &self.providers
    }

    /// 解析稳定 Provider ID 对应的 descriptor，不存在时返回统一未找到错误。
    pub(crate) fn descriptor(
        &self,
        provider_id: &str,
    ) -> Result<&WorkspaceCapabilityDescriptor, CapabilityProviderError> {
        self.providers_by_id
            .get(&WorkspaceCapabilityProviderId::new(provider_id))
            .map(|provider| provider.descriptor())
            .ok_or_else(CapabilityProviderError::not_found)
    }

    /// 解析稳定 Provider ID 对应的 Provider handle，不存在时返回统一未找到错误。
    pub(crate) fn provider(
        &self,
        provider_id: &str,
    ) -> Result<Arc<dyn WorkspaceCapabilityProvider>, CapabilityProviderError> {
        self.providers_by_id
            .get(&WorkspaceCapabilityProviderId::new(provider_id))
            .cloned()
            .ok_or_else(CapabilityProviderError::not_found)
    }

    /// 解析 Tool 的唯一 owner，不存在时返回统一未找到错误。
    pub(crate) fn tool_owner(
        &self,
        tool_name: &str,
    ) -> Result<WorkspaceCapabilityProviderId, CapabilityProviderError> {
        self.tool_owners
            .get(tool_name)
            .cloned()
            .ok_or_else(CapabilityProviderError::not_found)
    }
}

/// Runtime Slot 的稳定 identity，不接受 caller payload 中的 Root。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct RuntimeSlotKey {
    provider_id: WorkspaceCapabilityProviderId,
    workspace_id: String,
    generation: u64,
}

impl RuntimeSlotKey {
    /// 从已由服务端解析的 Provider 与 Lease 生成 Slot key。
    fn new(provider_id: WorkspaceCapabilityProviderId, lease: &WorkspaceLease) -> Self {
        Self {
            provider_id,
            workspace_id: lease.workspace_id.clone(),
            generation: lease.generation,
        }
    }
}

/// Slot 内部状态；Runtime 本体永不形成公共 DTO。
struct RuntimeSlotState {
    lifecycle: CapabilityRuntimeState,
    epoch: u64,
    runtime: Option<Arc<CapabilityRuntimeHandle>>,
    startup_flight: Option<Arc<StartupFlight>>,
    in_flight: usize,
}

/// 单次 startup epoch 专属的 completion，不与后续 retry 共用状态。
struct StartupFlight {
    epoch: u64,
    completion:
        watch::Sender<Option<Result<Arc<CapabilityRuntimeHandle>, WorkspaceCapabilityError>>>,
}

impl StartupFlight {
    /// 创建只服务于一个 startup epoch 的 completion channel。
    fn new(epoch: u64) -> Self {
        let (completion, _) = watch::channel(None);
        Self { epoch, completion }
    }
}

/// Runtime Slot 保留服务端 Root 不变量、状态和 per-slot 并发许可。
struct RuntimeSlot {
    canonical_root: PathBuf,
    state: Mutex<RuntimeSlotState>,
    permits: Arc<Semaphore>,
    #[cfg(test)]
    startup_waiter_registered: tokio::sync::Notify,
}

/// acquire 对 Slot 当前状态的无 await 决策。
enum RuntimeAcquireDecision {
    Ready(Arc<CapabilityRuntimeHandle>),
    Start(Arc<StartupFlight>),
    Wait {
        completion:
            watch::Receiver<Option<Result<Arc<CapabilityRuntimeHandle>, WorkspaceCapabilityError>>>,
    },
    Busy,
}

impl RuntimeSlot {
    /// 为首次 acquire 创建 stopped Slot，未启动任何 Provider Runtime。
    fn new(canonical_root: PathBuf, per_slot_concurrency: usize) -> Self {
        Self {
            canonical_root,
            state: Mutex::new(RuntimeSlotState {
                lifecycle: CapabilityRuntimeState::Stopped,
                epoch: 0,
                runtime: None,
                startup_flight: None,
                in_flight: 0,
            }),
            permits: Arc::new(Semaphore::new(per_slot_concurrency)),
            #[cfg(test)]
            startup_waiter_registered: tokio::sync::Notify::new(),
        }
    }

    /// 在短锁内选择 ready、leader startup 或同轮等待，不跨 await 持有 MutexGuard。
    fn begin_acquire(&self) -> RuntimeAcquireDecision {
        let mut state = lock_unpoisoned(&self.state);
        match state.lifecycle {
            CapabilityRuntimeState::Ready => RuntimeAcquireDecision::Ready(Arc::clone(
                state
                    .runtime
                    .as_ref()
                    .expect("ready RuntimeSlot must retain its runtime"),
            )),
            CapabilityRuntimeState::Starting => {
                #[cfg(test)]
                self.startup_waiter_registered.notify_one();
                RuntimeAcquireDecision::Wait {
                    completion: state
                        .startup_flight
                        .as_ref()
                        .expect("starting RuntimeSlot must retain its startup flight")
                        .completion
                        .subscribe(),
                }
            }
            CapabilityRuntimeState::Stopping => RuntimeAcquireDecision::Busy,
            CapabilityRuntimeState::Stopped | CapabilityRuntimeState::Error => {
                state.epoch = state.epoch.wrapping_add(1);
                state.lifecycle = CapabilityRuntimeState::Starting;
                state.runtime = None;
                let flight = Arc::new(StartupFlight::new(state.epoch));
                state.startup_flight = Some(Arc::clone(&flight));
                RuntimeAcquireDecision::Start(flight)
            }
        }
    }

    /// 向所属 flight 发布结果，并仅在该 flight 仍是当前 epoch 时更新 Slot。
    fn complete_startup(
        &self,
        flight: &Arc<StartupFlight>,
        result: Result<Arc<CapabilityRuntimeHandle>, WorkspaceCapabilityError>,
    ) {
        flight.completion.send_replace(Some(result.clone()));
        {
            let mut state = lock_unpoisoned(&self.state);
            if state.lifecycle != CapabilityRuntimeState::Starting
                || state.epoch != flight.epoch
                || !state
                    .startup_flight
                    .as_ref()
                    .is_some_and(|current| Arc::ptr_eq(current, flight))
            {
                return;
            }
            match &result {
                Ok(runtime) => {
                    state.lifecycle = CapabilityRuntimeState::Ready;
                    state.runtime = Some(Arc::clone(runtime));
                }
                Err(_) => {
                    state.lifecycle = CapabilityRuntimeState::Error;
                    state.runtime = None;
                }
            }
            state.startup_flight = None;
        }
    }

    /// 在返回 guard 前取得 permit，再精确记录 in-flight。
    async fn acquire_guard(
        self: &Arc<Self>,
        runtime: Arc<CapabilityRuntimeHandle>,
    ) -> RuntimeInFlightGuard {
        let permit = Arc::clone(&self.permits)
            .acquire_owned()
            .await
            .expect("RuntimeSlot semaphore is owned by the Slot");
        lock_unpoisoned(&self.state).in_flight += 1;
        RuntimeInFlightGuard {
            runtime,
            slot: Arc::clone(self),
            _permit: permit,
        }
    }

    /// 由 guard Drop 调用，保证 cancellation/panic 展开时归还 in-flight。
    fn release_in_flight(&self) {
        let mut state = lock_unpoisoned(&self.state);
        state.in_flight = state
            .in_flight
            .checked_sub(1)
            .expect("RuntimeInFlightGuard must correspond to one in-flight acquisition");
    }
}

/// leader startup 被取消或 panic 展开时，确保 Slot 不会永久停在 starting。
struct StartupCompletionGuard {
    slot: Arc<RuntimeSlot>,
    flight: Arc<StartupFlight>,
    completed: bool,
}

impl StartupCompletionGuard {
    /// 绑定单一 leader flight，除非显式完成否则 Drop 发布安全失败。
    fn new(slot: Arc<RuntimeSlot>, flight: Arc<StartupFlight>) -> Self {
        Self {
            slot,
            flight,
            completed: false,
        }
    }

    /// 标记正常 completion 已发布，避免 Drop 覆盖结果。
    fn disarm(&mut self) {
        self.completed = true;
    }
}

impl Drop for StartupCompletionGuard {
    fn drop(&mut self) {
        if !self.completed {
            self.slot
                .complete_startup(&self.flight, Err(WorkspaceCapabilityError::start_failed()));
        }
    }
}

/// Tool 层只可借用 opaque Runtime handle；guard 不可 Clone，Drop 自动释放计数与许可。
pub(crate) struct RuntimeInFlightGuard {
    runtime: Arc<CapabilityRuntimeHandle>,
    slot: Arc<RuntimeSlot>,
    _permit: OwnedSemaphorePermit,
}

impl RuntimeInFlightGuard {
    /// 返回受 guard 生命周期约束的 Runtime handle 借用，不暴露进程内部信息。
    pub(crate) fn runtime(&self) -> &CapabilityRuntimeHandle {
        &self.runtime
    }
}

impl Drop for RuntimeInFlightGuard {
    fn drop(&mut self) {
        self.slot.release_in_flight();
    }
}

/// 处理 poisoned Mutex 仍恢复内部状态，避免 Provider panic 使 Slot 永久不可用。
fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// 具备 workspace-scoped Runtime Slot 的 Registry lookup 与 acquire Manager。
pub(crate) struct WorkspaceCapabilityManager {
    registry: Arc<WorkspaceCapabilityRegistry>,
    runtime_slots: Mutex<HashMap<RuntimeSlotKey, Arc<RuntimeSlot>>>,
}

impl WorkspaceCapabilityManager {
    /// 以 immutable Registry 创建无进程 Manager shell。
    pub(crate) fn new(registry: Arc<WorkspaceCapabilityRegistry>) -> Self {
        Self {
            registry,
            runtime_slots: Mutex::new(HashMap::new()),
        }
    }

    /// 返回 Manager 持有的 immutable Registry，供后续阶段读取统一注册表。
    pub(crate) fn registry(&self) -> &WorkspaceCapabilityRegistry {
        &self.registry
    }

    /// 按编译期注册顺序委托 Provider 列表 lookup。
    pub(crate) fn providers(&self) -> &[Arc<dyn WorkspaceCapabilityProvider>] {
        self.registry.providers()
    }

    /// 委托 Provider descriptor lookup，不触发 probe 或 Runtime 行为。
    pub(crate) fn descriptor(
        &self,
        provider_id: &str,
    ) -> Result<&WorkspaceCapabilityDescriptor, WorkspaceCapabilityError> {
        self.registry
            .descriptor(provider_id)
            .map_err(Self::map_registry_error)
    }

    /// 委托 Provider handle lookup，不触发 probe 或 Runtime 行为。
    pub(crate) fn provider(
        &self,
        provider_id: &str,
    ) -> Result<Arc<dyn WorkspaceCapabilityProvider>, WorkspaceCapabilityError> {
        self.registry
            .provider(provider_id)
            .map_err(Self::map_registry_error)
    }

    /// 委托 Tool owner lookup，不触发 probe 或 Runtime 行为。
    pub(crate) fn tool_owner(
        &self,
        tool_name: &str,
    ) -> Result<WorkspaceCapabilityProviderId, WorkspaceCapabilityError> {
        self.registry
            .tool_owner(tool_name)
            .map_err(Self::map_registry_error)
    }

    /// 为已进入 Runtime startup 层的 Lease lazy acquire workspace-scoped Runtime。
    pub(crate) async fn acquire_runtime(
        &self,
        provider_id: &str,
        lease: WorkspaceLease,
    ) -> Result<RuntimeInFlightGuard, WorkspaceCapabilityError> {
        let provider = self.provider(provider_id)?;
        let descriptor = provider.descriptor();
        if descriptor.runtime_model != CapabilityRuntimeModel::WorkspaceScopedProcess {
            return Err(WorkspaceCapabilityError::contract_error());
        }

        let slot = self.runtime_slot(
            &descriptor.provider_id,
            &lease,
            descriptor.runtime_policy.per_slot_concurrency,
        )?;
        let runtime = match slot.begin_acquire() {
            RuntimeAcquireDecision::Ready(runtime) => runtime,
            RuntimeAcquireDecision::Busy => return Err(WorkspaceCapabilityError::busy()),
            RuntimeAcquireDecision::Start(flight) => {
                Self::start_runtime(Arc::clone(&slot), flight, provider, lease.clone()).await?
            }
            RuntimeAcquireDecision::Wait { completion } => {
                Self::wait_for_startup(completion).await?
            }
        };

        Ok(slot.acquire_guard(runtime).await)
    }

    /// 只为 workspace_scoped_process 在第一次 acquire 时创建并验证 Slot。
    fn runtime_slot(
        &self,
        provider_id: &WorkspaceCapabilityProviderId,
        lease: &WorkspaceLease,
        per_slot_concurrency: usize,
    ) -> Result<Arc<RuntimeSlot>, WorkspaceCapabilityError> {
        let key = RuntimeSlotKey::new(provider_id.clone(), lease);
        let mut slots = lock_unpoisoned(&self.runtime_slots);
        if let Some(slot) = slots.get(&key) {
            return (slot.canonical_root == lease.canonical_root)
                .then(|| Arc::clone(slot))
                .ok_or_else(WorkspaceCapabilityError::contract_error);
        }

        let slot = Arc::new(RuntimeSlot::new(
            lease.canonical_root.clone(),
            per_slot_concurrency,
        ));
        slots.insert(key, Arc::clone(&slot));
        Ok(slot)
    }

    /// 由单一 leader 执行 Provider start，所有错误均映射为 Manager 安全错误。
    async fn start_runtime(
        slot: Arc<RuntimeSlot>,
        flight: Arc<StartupFlight>,
        provider: Arc<dyn WorkspaceCapabilityProvider>,
        lease: WorkspaceLease,
    ) -> Result<Arc<CapabilityRuntimeHandle>, WorkspaceCapabilityError> {
        let mut completion_guard =
            StartupCompletionGuard::new(Arc::clone(&slot), Arc::clone(&flight));
        let result = provider
            .start(lease.clone())
            .await
            .map(Arc::new)
            .map_err(|_| WorkspaceCapabilityError::start_failed())
            .and_then(|runtime| {
                runtime_matches(&provider.descriptor().provider_id, &lease, Some(&runtime))
                    .then_some(runtime)
                    .ok_or_else(WorkspaceCapabilityError::contract_error)
            });
        slot.complete_startup(&flight, result.clone());
        completion_guard.disarm();
        result
    }

    /// 等待已绑定 flight 的 completion，后续 retry 不会覆盖该结果。
    async fn wait_for_startup(
        mut completion: watch::Receiver<
            Option<Result<Arc<CapabilityRuntimeHandle>, WorkspaceCapabilityError>>,
        >,
    ) -> Result<Arc<CapabilityRuntimeHandle>, WorkspaceCapabilityError> {
        loop {
            if let Some(result) = completion.borrow_and_update().clone() {
                return result;
            }
            completion
                .changed()
                .await
                .map_err(|_| WorkspaceCapabilityError::start_failed())?;
        }
    }

    /// 将 Registry 内部错误收敛为 Manager 的安全 Workspace Capability 错误。
    fn map_registry_error(error: CapabilityProviderError) -> WorkspaceCapabilityError {
        match error.code {
            CapabilityProviderErrorCode::NotFound => WorkspaceCapabilityError::not_found(),
            _ => WorkspaceCapabilityError::contract_error(),
        }
    }
}

/// 在进入 Provider 前验证 Runtime handle 与 Provider/Lease identity 的统一 helper。
pub(crate) fn call_with_checked_runtime<'a>(
    provider: &'a dyn WorkspaceCapabilityProvider,
    lease: &'a WorkspaceLease,
    runtime: Option<&'a CapabilityRuntimeHandle>,
    tool: WorkspaceToolCall,
) -> CapabilityFuture<'a, Result<WorkspaceToolResult, CapabilityProviderError>> {
    let provider_id = provider.descriptor().provider_id.clone();
    if !runtime_matches(&provider_id, lease, runtime) {
        return Box::pin(async { Err(CapabilityProviderError::runtime_identity_mismatch()) });
    }

    provider.call(lease, runtime, tool)
}

/// 将 handle 限定为同一 provider、Workspace 与 generation；缺失 handle 始终允许。
fn runtime_matches(
    provider_id: &WorkspaceCapabilityProviderId,
    lease: &WorkspaceLease,
    runtime: Option<&CapabilityRuntimeHandle>,
) -> bool {
    runtime.is_none_or(|handle| {
        handle.provider_id == *provider_id
            && handle.workspace_id == lease.workspace_id
            && handle.workspace_generation == lease.generation
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::{
        collections::VecDeque,
        future::Future,
        path::PathBuf,
        sync::{Arc, atomic::Ordering},
        task::Poll,
    };
    use tokio::sync::{Barrier, Notify, oneshot};

    /// 构造独立 fake Provider 所需的 descriptor，避免任何 Serena/CodeGraph 类型。
    fn descriptor(provider_id: &str) -> WorkspaceCapabilityDescriptor {
        WorkspaceCapabilityDescriptor {
            provider_id: WorkspaceCapabilityProviderId::new(provider_id),
            display_name: "Fake capability".into(),
            tool_names: vec!["fake_tool".into()],
            runtime_model: CapabilityRuntimeModel::WorkspaceScopedProcess,
            readiness_probe: CapabilityReadinessProbe::Required,
            preparation_policy: CapabilityPreparationPolicy::ExplicitOnly,
            stage_descriptors: vec![CapabilityStageDescriptor {
                id: "index".into(),
                requirement: CapabilityStageRequirement::Required,
            }],
            action_descriptors: vec![CapabilityActionDescriptor {
                action_id: "prepare_index".into(),
                display_name: "Prepare index".into(),
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

    /// 构造 server-resolved Lease 的测试替身，调用方不从 Tool payload 提供 root。
    fn lease(workspace_id: &str, generation: u64) -> WorkspaceLease {
        WorkspaceLease {
            workspace_id: workspace_id.into(),
            canonical_root: PathBuf::from("C:/server-resolved-workspace"),
            generation,
        }
    }

    /// 受 channel 控制的 start 结果，确保并发测试不依赖 sleep。
    #[derive(Clone, Copy, Debug)]
    enum StartPlan {
        Success,
        Failure,
        WrongProvider,
        WrongWorkspace,
        WrongGeneration,
    }

    /// 第三方 fake Provider 只依赖本 module 的通用 domain 类型。
    struct FakeProvider {
        descriptor: WorkspaceCapabilityDescriptor,
        probes: std::sync::atomic::AtomicUsize,
        observations: std::sync::atomic::AtomicUsize,
        prepares: std::sync::atomic::AtomicUsize,
        starts: std::sync::atomic::AtomicUsize,
        calls: std::sync::atomic::AtomicUsize,
        stops: std::sync::atomic::AtomicUsize,
        stop_fails: std::sync::atomic::AtomicBool,
        start_plans: Mutex<VecDeque<oneshot::Receiver<StartPlan>>>,
        start_entered: Arc<Notify>,
    }

    impl FakeProvider {
        /// 创建不依赖内置 Provider 的 fake capability。
        fn new(provider_id: &str) -> Self {
            Self::with_descriptor(descriptor(provider_id))
        }

        /// 以指定 descriptor 创建 fake capability，用于验证 Registry 契约边界。
        fn with_descriptor(descriptor: WorkspaceCapabilityDescriptor) -> Self {
            Self {
                descriptor,
                probes: std::sync::atomic::AtomicUsize::new(0),
                observations: std::sync::atomic::AtomicUsize::new(0),
                prepares: std::sync::atomic::AtomicUsize::new(0),
                starts: std::sync::atomic::AtomicUsize::new(0),
                calls: std::sync::atomic::AtomicUsize::new(0),
                stops: std::sync::atomic::AtomicUsize::new(0),
                stop_fails: std::sync::atomic::AtomicBool::new(false),
                start_plans: Mutex::new(VecDeque::new()),
                start_entered: Arc::new(Notify::new()),
            }
        }

        /// 为下一次 start 安排一个由测试 channel 放行的确定性结果。
        fn enqueue_start(&self) -> oneshot::Sender<StartPlan> {
            let (sender, receiver) = oneshot::channel();
            lock_unpoisoned(&self.start_plans).push_back(receiver);
            sender
        }

        /// 令后续 stop 确定性失败，用于验证 failure carrier 的所有权归还。
        fn fail_stop(&self) {
            self.stop_fails
                .store(true, std::sync::atomic::Ordering::SeqCst);
        }

        /// 断言 Registry/Manager lookup 未调用任何 Provider 生命周期方法。
        fn assert_no_lifecycle_side_effects(&self) {
            for counter in [
                &self.probes,
                &self.observations,
                &self.prepares,
                &self.starts,
                &self.calls,
                &self.stops,
            ] {
                assert_eq!(counter.load(std::sync::atomic::Ordering::SeqCst), 0);
            }
        }
    }

    impl WorkspaceCapabilityProvider for FakeProvider {
        fn descriptor(&self) -> &WorkspaceCapabilityDescriptor {
            &self.descriptor
        }

        fn probe_installation(
            &self,
        ) -> CapabilityFuture<'_, Result<CapabilityInstallation, CapabilityProviderError>> {
            self.probes
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Box::pin(async {
                Ok(CapabilityInstallation {
                    state: CapabilityInstallationState::Installed,
                    detected_version: Some("1.0.0".into()),
                })
            })
        }

        fn observe_readiness(
            &self,
            _lease: WorkspaceLease,
        ) -> CapabilityFuture<'_, Result<CapabilityObservation, CapabilityProviderError>> {
            self.observations
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let provider_id = self.descriptor.provider_id.clone();
            Box::pin(async move {
                Ok(CapabilityObservation {
                    provider_id,
                    installation: CapabilityInstallationState::Installed,
                    readiness: CapabilityReadinessState::Ready,
                    runtime_state: CapabilityRuntimeState::Stopped,
                    checked_at: 0,
                    stages: vec![],
                    actions: vec![],
                })
            })
        }

        fn prepare<'a>(
            &'a self,
            _lease: WorkspaceLease,
            _action: CapabilityPrepareAction,
            _activity: &'a dyn CapabilityActivitySink,
        ) -> CapabilityFuture<'a, Result<CapabilityPrepareResult, CapabilityProviderError>>
        {
            self.prepares
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Box::pin(async {
                Ok(CapabilityPrepareResult {
                    readiness: CapabilityReadinessState::Ready,
                })
            })
        }

        fn start(
            &self,
            lease: WorkspaceLease,
        ) -> CapabilityFuture<'_, Result<CapabilityRuntimeHandle, CapabilityProviderError>>
        {
            self.starts
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let provider_id = self.descriptor.provider_id.clone();
            let plan = lock_unpoisoned(&self.start_plans).pop_front();
            let start_entered = Arc::clone(&self.start_entered);
            Box::pin(async move {
                let plan = if let Some(plan) = plan {
                    start_entered.notify_one();
                    plan.await.unwrap_or(StartPlan::Failure)
                } else {
                    StartPlan::Success
                };
                match plan {
                    StartPlan::Success => Ok(CapabilityRuntimeHandle::new(provider_id, &lease)),
                    StartPlan::Failure => Err(CapabilityProviderError {
                        code: CapabilityProviderErrorCode::OperationFailed,
                    }),
                    StartPlan::WrongProvider => Ok(CapabilityRuntimeHandle::new(
                        WorkspaceCapabilityProviderId::new("wrong-provider"),
                        &lease,
                    )),
                    StartPlan::WrongWorkspace => {
                        let mut wrong_lease = lease.clone();
                        wrong_lease.workspace_id = "wrong-workspace".into();
                        Ok(CapabilityRuntimeHandle::new(provider_id, &wrong_lease))
                    }
                    StartPlan::WrongGeneration => {
                        let mut wrong_lease = lease.clone();
                        wrong_lease.generation = wrong_lease.generation.saturating_add(1);
                        Ok(CapabilityRuntimeHandle::new(provider_id, &wrong_lease))
                    }
                }
            })
        }

        fn call<'a>(
            &'a self,
            _lease: &'a WorkspaceLease,
            _runtime: Option<&'a CapabilityRuntimeHandle>,
            _tool: WorkspaceToolCall,
        ) -> CapabilityFuture<'a, Result<WorkspaceToolResult, CapabilityProviderError>> {
            Box::pin(async move {
                self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(WorkspaceToolResult {
                    result: json!({ "ok": true }),
                })
            })
        }

        fn stop(
            &self,
            runtime: CapabilityRuntimeHandle,
        ) -> CapabilityFuture<'_, Result<StopEvidence, CapabilityStopFailure>> {
            self.stops.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let stop_fails = self.stop_fails.load(std::sync::atomic::Ordering::SeqCst);
            Box::pin(async move {
                if stop_fails {
                    return Err(CapabilityStopFailure {
                        runtime,
                        error: CapabilityProviderError {
                            code: CapabilityProviderErrorCode::OperationFailed,
                        },
                    });
                }
                Ok(StopEvidence {
                    runtime_state: CapabilityRuntimeState::Stopped,
                })
            })
        }
    }

    /// 编译期断言 trait 可作为 Arc trait object 使用。
    fn assert_object_safe(_provider: Arc<dyn WorkspaceCapabilityProvider>) {}

    /// 构造带独立 Tool 名称的 fake Provider，模拟不依赖 Core 分支的第三方接入。
    fn fake_provider(provider_id: &str, tool_names: &[&str]) -> Arc<FakeProvider> {
        let mut fake_descriptor = descriptor(provider_id);
        fake_descriptor.tool_names = tool_names
            .iter()
            .map(|tool_name| (*tool_name).into())
            .collect();
        Arc::new(FakeProvider::with_descriptor(fake_descriptor))
    }

    /// 创建指定 per-slot concurrency 的通用 workspace-scoped fake Provider。
    fn runtime_provider(provider_id: &str, per_slot_concurrency: usize) -> Arc<FakeProvider> {
        let mut provider_descriptor = descriptor(provider_id);
        provider_descriptor.runtime_policy.per_slot_concurrency = per_slot_concurrency;
        Arc::new(FakeProvider::with_descriptor(provider_descriptor))
    }

    /// 构造持有单个通用 fake Provider 的 Runtime Manager。
    fn runtime_manager(provider: Arc<FakeProvider>) -> Arc<WorkspaceCapabilityManager> {
        Arc::new(WorkspaceCapabilityManager::new(Arc::new(
            WorkspaceCapabilityRegistry::new(vec![as_provider(provider)]).unwrap(),
        )))
    }

    /// 读取测试目标 Slot，验证内部计数但不形成公共 DTO。
    fn runtime_slot(
        manager: &WorkspaceCapabilityManager,
        provider_id: &str,
        workspace_lease: &WorkspaceLease,
    ) -> Arc<RuntimeSlot> {
        lock_unpoisoned(&manager.runtime_slots)
            .get(&RuntimeSlotKey::new(
                WorkspaceCapabilityProviderId::new(provider_id),
                workspace_lease,
            ))
            .cloned()
            .expect("test acquire must create its RuntimeSlot")
    }

    /// 将测试 fake Provider 转为 Registry 所需的通用 trait object。
    fn as_provider(provider: Arc<FakeProvider>) -> Arc<dyn WorkspaceCapabilityProvider> {
        provider
    }

    /// 断言构造失败只返回统一契约错误，且不会产生可用 Registry 值。
    fn assert_contract_error(result: Result<WorkspaceCapabilityRegistry, CapabilityProviderError>) {
        match result {
            Err(error) => assert_eq!(error.code, CapabilityProviderErrorCode::ContractError),
            Ok(_) => panic!("expected Registry construction to fail"),
        }
    }

    #[test]
    fn provider_port_is_object_safe_without_builtin_provider_types() {
        let provider: Arc<dyn WorkspaceCapabilityProvider> = Arc::new(FakeProvider::new("third"));

        assert_object_safe(provider);
    }

    #[tokio::test]
    /// 验证 stop failure 返还原 handle，且 carrier 不构成可序列化的公开 wire。
    async fn stop_failure_returns_the_same_opaque_runtime_handle() {
        let provider = FakeProvider::new("third");
        let workspace_lease = lease("workspace-a", 7);
        provider.fail_stop();

        let failure = match provider
            .stop(CapabilityRuntimeHandle::new(
                WorkspaceCapabilityProviderId::new("third"),
                &workspace_lease,
            ))
            .await
        {
            Err(failure) => failure,
            Ok(_) => panic!("configured fake stop must fail"),
        };

        assert_eq!(
            failure.error.code,
            CapabilityProviderErrorCode::OperationFailed
        );
        assert_eq!(failure.runtime.provider_id.as_str(), "third");
        assert_eq!(failure.runtime.workspace_id, "workspace-a");
        assert_eq!(failure.runtime.workspace_generation, 7);
        assert_eq!(provider.stops.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn registry_lists_and_resolves_three_generic_providers_in_registration_order() {
        let first = fake_provider("first", &["first_lookup"]);
        let second = fake_provider("second", &["second_lookup"]);
        let third = fake_provider("third", &["third_lookup"]);
        let registry = WorkspaceCapabilityRegistry::new(vec![
            as_provider(Arc::clone(&first)),
            as_provider(Arc::clone(&second)),
            as_provider(Arc::clone(&third)),
        ])
        .unwrap();

        let provider_ids: Vec<_> = registry
            .providers()
            .iter()
            .map(|provider| provider.descriptor().provider_id.as_str())
            .collect();
        assert_eq!(provider_ids, ["first", "second", "third"]);
        assert_eq!(
            registry.descriptor("second").unwrap().tool_names,
            ["second_lookup"]
        );
        assert!(Arc::ptr_eq(
            &registry.provider("third").unwrap(),
            &as_provider(third)
        ));
        assert_eq!(
            registry.tool_owner("first_lookup").unwrap().as_str(),
            "first"
        );
        assert_eq!(
            registry.tool_owner("third_lookup").unwrap().as_str(),
            "third"
        );
    }

    #[test]
    fn registry_unknown_provider_and_tool_return_the_same_generic_not_found_error() {
        let registry = WorkspaceCapabilityRegistry::new(vec![as_provider(fake_provider(
            "third",
            &["third_lookup"],
        ))])
        .unwrap();

        for result in [
            registry.provider("missing").map(|_| ()),
            registry.descriptor("missing").map(|_| ()),
            registry.tool_owner("missing_tool").map(|_| ()),
        ] {
            assert_eq!(
                result.unwrap_err().code,
                CapabilityProviderErrorCode::NotFound
            );
        }
    }

    #[test]
    fn registry_rejects_duplicate_provider_ids_and_tool_names() {
        assert_contract_error(WorkspaceCapabilityRegistry::new(vec![
            as_provider(fake_provider("same", &["first_tool"])),
            as_provider(fake_provider("same", &["second_tool"])),
        ]));
        assert_contract_error(WorkspaceCapabilityRegistry::new(vec![as_provider(
            fake_provider("same", &["duplicate_tool", "duplicate_tool"]),
        )]));
        assert_contract_error(WorkspaceCapabilityRegistry::new(vec![
            as_provider(fake_provider("first", &["shared_tool"])),
            as_provider(fake_provider("second", &["shared_tool"])),
        ]));
    }

    #[test]
    fn registry_rejects_invalid_identifiers_and_minimum_runtime_policy_values() {
        assert_contract_error(WorkspaceCapabilityRegistry::new(vec![as_provider(
            fake_provider("   ", &["tool"]),
        )]));
        assert_contract_error(WorkspaceCapabilityRegistry::new(vec![as_provider(
            fake_provider("provider", &[" \t "]),
        )]));

        let mut zero_per_slot = descriptor("in-process");
        zero_per_slot.runtime_model = CapabilityRuntimeModel::InProcess;
        zero_per_slot.runtime_policy.per_slot_concurrency = 0;
        assert_contract_error(WorkspaceCapabilityRegistry::new(vec![as_provider(
            Arc::new(FakeProvider::with_descriptor(zero_per_slot)),
        )]));

        let mut zero_workspace_instances = descriptor("workspace-process");
        zero_workspace_instances.runtime_model = CapabilityRuntimeModel::WorkspaceScopedProcess;
        zero_workspace_instances.runtime_policy.max_instances = 0;
        assert_contract_error(WorkspaceCapabilityRegistry::new(vec![as_provider(
            Arc::new(FakeProvider::with_descriptor(zero_workspace_instances)),
        )]));
    }

    #[test]
    fn failed_registry_construction_cannot_publish_a_partial_registry() {
        let result = WorkspaceCapabilityRegistry::new(vec![
            as_provider(fake_provider("valid", &["valid_tool"])),
            as_provider(fake_provider("invalid", &["valid_tool"])),
        ]);

        assert_contract_error(result);
    }

    #[test]
    fn manager_shell_delegates_lookups_without_provider_lifecycle_side_effects() {
        let provider = fake_provider("third", &["third_lookup"]);
        let registry = Arc::new(
            WorkspaceCapabilityRegistry::new(vec![as_provider(Arc::clone(&provider))]).unwrap(),
        );
        let manager = WorkspaceCapabilityManager::new(Arc::clone(&registry));

        assert_eq!(manager.registry().providers().len(), 1);
        assert_eq!(manager.providers().len(), 1);
        assert_eq!(
            manager.descriptor("third").unwrap().display_name,
            "Fake capability"
        );
        assert!(Arc::ptr_eq(
            &manager.provider("third").unwrap(),
            &as_provider(Arc::clone(&provider))
        ));
        assert_eq!(
            manager.tool_owner("third_lookup").unwrap().as_str(),
            "third"
        );
        provider.assert_no_lifecycle_side_effects();
    }

    #[test]
    /// 验证 Manager 的三个 lookup 均不泄露 Registry 的 Provider 错误类型。
    fn manager_lookup_errors_use_the_workspace_capability_boundary() {
        let provider = fake_provider("third", &["third_lookup"]);
        let manager = WorkspaceCapabilityManager::new(Arc::new(
            WorkspaceCapabilityRegistry::new(vec![as_provider(provider)]).unwrap(),
        ));

        for result in [
            manager.descriptor("missing").map(|_| ()),
            manager.provider("missing").map(|_| ()),
            manager.tool_owner("missing_tool").map(|_| ()),
        ] {
            let error = result.unwrap_err();
            assert_eq!(error.code, WorkspaceCapabilityErrorCode::NotFound);
            assert_eq!(
                serde_json::to_value(error.code).unwrap(),
                "WORKSPACE_CAPABILITY_NOT_FOUND"
            );
        }
    }

    #[test]
    fn descriptor_enums_and_observation_use_the_frozen_wire_shape() {
        let descriptor = serde_json::to_value(descriptor("third")).unwrap();
        assert_eq!(descriptor["providerId"], "third");
        assert_eq!(descriptor["displayName"], "Fake capability");
        assert_eq!(descriptor["runtimeModel"], "workspace_scoped_process");
        assert_eq!(descriptor["readinessProbe"], "required");
        assert_eq!(descriptor["preparationPolicy"], "explicit_only");
        assert_eq!(descriptor["stageDescriptors"][0]["id"], "index");
        assert_eq!(descriptor["stageDescriptors"][0]["requirement"], "required");
        assert_eq!(
            descriptor["actionDescriptors"][0]["actionId"],
            "prepare_index"
        );
        assert_eq!(
            descriptor["actionDescriptors"][0]["displayName"],
            "Prepare index"
        );
        assert_eq!(
            descriptor["actionDescriptors"][0]["authority"],
            "local_human"
        );
        assert_eq!(
            descriptor["actionDescriptors"][0]["execution"],
            "provider_prepare"
        );
        assert_eq!(descriptor["actionDescriptors"][0]["warmRuntime"], false);
        assert_eq!(descriptor["runtimePolicy"]["maxInstances"], 2);
        assert_eq!(descriptor["runtimePolicy"]["idleTimeoutMs"], 30_000);
        assert_eq!(descriptor["runtimePolicy"]["perSlotConcurrency"], 1);
        assert!(descriptor.get("provider_id").is_none());

        let observation = serde_json::to_value(CapabilityObservation {
            provider_id: WorkspaceCapabilityProviderId::new("third"),
            installation: CapabilityInstallationState::Installed,
            readiness: CapabilityReadinessState::NotPrepared,
            runtime_state: CapabilityRuntimeState::Stopped,
            checked_at: 0,
            stages: vec![CapabilityStage {
                id: "index".into(),
                display_name: "Index".into(),
                state: CapabilityStageState::Absent,
                requirement: CapabilityStageRequirement::Required,
                message_code: Some("CAPABILITY_STAGE_NOT_PREPARED".into()),
            }],
            actions: vec![CapabilityAction {
                id: "prepare".into(),
                display_name: "Prepare".into(),
                authority: CapabilityActionAuthority::LocalHuman,
                execution: CapabilityActionExecution::ManagerEnsureRuntime,
            }],
        })
        .unwrap();
        assert_eq!(observation["providerId"], "third");
        assert_eq!(observation["installation"], "installed");
        assert_eq!(observation["readiness"], "not_prepared");
        assert_eq!(observation["runtimeState"], "stopped");
        assert_eq!(observation["checkedAt"], 0);
        assert_eq!(observation["stages"][0]["displayName"], "Index");
        assert_eq!(observation["stages"][0]["state"], "absent");
        assert_eq!(
            observation["stages"][0]["messageCode"],
            "CAPABILITY_STAGE_NOT_PREPARED"
        );
        assert_eq!(observation["actions"][0]["id"], "prepare");
        assert_eq!(observation["actions"][0]["authority"], "local_human");
        assert_eq!(
            observation["actions"][0]["execution"],
            "manager_ensure_runtime"
        );
        assert!(observation.get("status").is_none());
        assert!(observation.get("availability").is_none());

        assert_eq!(
            serde_json::to_value(CapabilityInstallationState::NotInstalled).unwrap(),
            "not_installed"
        );
        assert_eq!(
            serde_json::to_value(CapabilityInstallationState::CheckFailed).unwrap(),
            "check_failed"
        );
        assert_eq!(
            serde_json::to_value(CapabilityReadinessState::Preparing).unwrap(),
            "preparing"
        );
        assert_eq!(
            serde_json::to_value(CapabilityReadinessState::Degraded).unwrap(),
            "degraded"
        );
        assert_eq!(
            serde_json::to_value(CapabilityReadinessState::Unknown).unwrap(),
            "unknown"
        );
        assert_eq!(
            serde_json::to_value(CapabilityStageState::Pending).unwrap(),
            "pending"
        );
        assert_eq!(
            serde_json::to_value(CapabilityStageState::Running).unwrap(),
            "running"
        );
        assert_eq!(
            serde_json::to_value(CapabilityStageState::Stale).unwrap(),
            "stale"
        );
        assert_eq!(
            serde_json::to_value(CapabilityRuntimeModel::InProcess).unwrap(),
            "in_process"
        );
        assert_eq!(
            serde_json::to_value(CapabilityRuntimeModel::StatelessCommand).unwrap(),
            "stateless_command"
        );
        assert_eq!(
            serde_json::to_value(CapabilityPreparationPolicy::AutoOnFirstToolCall).unwrap(),
            "auto_on_first_tool_call"
        );
    }

    #[test]
    fn frozen_enum_wire_values_are_exact() {
        for (value, expected) in [
            (CapabilityInstallationState::Installed, "installed"),
            (CapabilityInstallationState::NotInstalled, "not_installed"),
            (CapabilityInstallationState::CheckFailed, "check_failed"),
        ] {
            assert_eq!(serde_json::to_value(value).unwrap(), expected);
        }
        for (value, expected) in [
            (CapabilityReadinessState::NotPrepared, "not_prepared"),
            (CapabilityReadinessState::Preparing, "preparing"),
            (CapabilityReadinessState::Ready, "ready"),
            (CapabilityReadinessState::Degraded, "degraded"),
            (CapabilityReadinessState::Error, "error"),
            (CapabilityReadinessState::Unknown, "unknown"),
        ] {
            assert_eq!(serde_json::to_value(value).unwrap(), expected);
        }
        for (value, expected) in [
            (CapabilityRuntimeState::Stopped, "stopped"),
            (CapabilityRuntimeState::Starting, "starting"),
            (CapabilityRuntimeState::Ready, "ready"),
            (CapabilityRuntimeState::Error, "error"),
            (CapabilityRuntimeState::Stopping, "stopping"),
        ] {
            assert_eq!(serde_json::to_value(value).unwrap(), expected);
        }
        for (value, expected) in [
            (CapabilityStageState::Absent, "absent"),
            (CapabilityStageState::Pending, "pending"),
            (CapabilityStageState::Running, "running"),
            (CapabilityStageState::Ready, "ready"),
            (CapabilityStageState::Stale, "stale"),
            (CapabilityStageState::Error, "error"),
            (CapabilityStageState::Unknown, "unknown"),
        ] {
            assert_eq!(serde_json::to_value(value).unwrap(), expected);
        }
        for (value, expected) in [
            (CapabilityStageRequirement::Required, "required"),
            (
                CapabilityStageRequirement::AutoPreparable,
                "auto_preparable",
            ),
            (CapabilityStageRequirement::Optional, "optional"),
        ] {
            assert_eq!(serde_json::to_value(value).unwrap(), expected);
        }
        for (value, expected) in [
            (CapabilityRuntimeModel::InProcess, "in_process"),
            (
                CapabilityRuntimeModel::StatelessCommand,
                "stateless_command",
            ),
            (
                CapabilityRuntimeModel::WorkspaceScopedProcess,
                "workspace_scoped_process",
            ),
        ] {
            assert_eq!(serde_json::to_value(value).unwrap(), expected);
        }
        for (value, expected) in [
            (CapabilityPreparationPolicy::None, "none"),
            (
                CapabilityPreparationPolicy::AutoOnFirstToolCall,
                "auto_on_first_tool_call",
            ),
            (CapabilityPreparationPolicy::ExplicitOnly, "explicit_only"),
        ] {
            assert_eq!(serde_json::to_value(value).unwrap(), expected);
        }
        for (value, expected) in [(CapabilityActionAuthority::LocalHuman, "local_human")] {
            assert_eq!(serde_json::to_value(value).unwrap(), expected);
        }
        for (value, expected) in [
            (
                CapabilityActionExecution::ManagerEnsureRuntime,
                "manager_ensure_runtime",
            ),
            (
                CapabilityActionExecution::ProviderPrepare,
                "provider_prepare",
            ),
        ] {
            assert_eq!(serde_json::to_value(value).unwrap(), expected);
        }
    }

    #[tokio::test]
    async fn mismatched_runtime_identity_fails_before_fake_provider_call() {
        let provider = FakeProvider::new("third");
        let matching_lease = lease("workspace-a", 7);
        let mismatched_workspace = CapabilityRuntimeHandle::new(
            WorkspaceCapabilityProviderId::new("third"),
            &lease("workspace-b", 7),
        );
        let mismatched_generation = CapabilityRuntimeHandle::new(
            WorkspaceCapabilityProviderId::new("third"),
            &lease("workspace-a", 8),
        );
        let mismatched_provider = CapabilityRuntimeHandle::new(
            WorkspaceCapabilityProviderId::new("other"),
            &matching_lease,
        );

        for runtime in [
            &mismatched_workspace,
            &mismatched_generation,
            &mismatched_provider,
        ] {
            let error = call_with_checked_runtime(
                &provider,
                &matching_lease,
                Some(runtime),
                WorkspaceToolCall {
                    tool_name: "fake_tool".into(),
                    arguments: json!({}),
                },
            )
            .await
            .unwrap_err();
            assert_eq!(
                error.code,
                CapabilityProviderErrorCode::RuntimeIdentityMismatch
            );
        }

        assert_eq!(provider.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[test]
    fn workspace_tool_call_has_no_root_field_or_root_wire_value() {
        let tool = WorkspaceToolCall {
            tool_name: "fake_tool".into(),
            arguments: json!({ "relativePath": "src/lib.rs" }),
        };
        let value = serde_json::to_value(tool).unwrap();

        assert_eq!(value["toolName"], "fake_tool");
        assert_eq!(value["arguments"]["relativePath"], "src/lib.rs");
        assert!(value.get("root").is_none());
        assert!(value.get("canonicalRoot").is_none());
    }

    #[test]
    /// 验证冻结的 Manager 错误代码序列化为公开安全 wire 值。
    fn workspace_capability_error_codes_keep_the_frozen_wire_values() {
        for (code, wire) in [
            (
                WorkspaceCapabilityErrorCode::NotFound,
                "WORKSPACE_CAPABILITY_NOT_FOUND",
            ),
            (
                WorkspaceCapabilityErrorCode::NotInstalled,
                "WORKSPACE_CAPABILITY_NOT_INSTALLED",
            ),
            (
                WorkspaceCapabilityErrorCode::NotPrepared,
                "WORKSPACE_CAPABILITY_NOT_PREPARED",
            ),
            (
                WorkspaceCapabilityErrorCode::PreparationRequired,
                "WORKSPACE_CAPABILITY_PREPARATION_REQUIRED",
            ),
            (
                WorkspaceCapabilityErrorCode::Preparing,
                "WORKSPACE_CAPABILITY_PREPARING",
            ),
            (
                WorkspaceCapabilityErrorCode::PrepareFailed,
                "WORKSPACE_CAPABILITY_PREPARE_FAILED",
            ),
            (
                WorkspaceCapabilityErrorCode::ObserveFailed,
                "WORKSPACE_CAPABILITY_OBSERVE_FAILED",
            ),
            (
                WorkspaceCapabilityErrorCode::Busy,
                "WORKSPACE_CAPABILITY_BUSY",
            ),
            (
                WorkspaceCapabilityErrorCode::StartFailed,
                "WORKSPACE_CAPABILITY_START_FAILED",
            ),
            (
                WorkspaceCapabilityErrorCode::RuntimeLost,
                "WORKSPACE_CAPABILITY_RUNTIME_LOST",
            ),
            (
                WorkspaceCapabilityErrorCode::StopFailed,
                "WORKSPACE_CAPABILITY_STOP_FAILED",
            ),
            (
                WorkspaceCapabilityErrorCode::ContractError,
                "WORKSPACE_CAPABILITY_CONTRACT_ERROR",
            ),
        ] {
            assert_eq!(serde_json::to_value(code).unwrap(), wire);
        }
    }

    #[tokio::test]
    /// 验证同 Slot 的并发首次 acquire 只启动一次并共享同一 opaque Runtime。
    async fn concurrent_first_acquires_share_one_start_and_one_runtime() {
        let provider = runtime_provider("generic", 2);
        let start_result = provider.enqueue_start();
        let manager = runtime_manager(Arc::clone(&provider));
        let workspace_lease = lease("workspace-a", 7);
        let barrier = Arc::new(Barrier::new(3));

        let first_manager = Arc::clone(&manager);
        let first_lease = workspace_lease.clone();
        let first_barrier = Arc::clone(&barrier);
        let first = tokio::spawn(async move {
            first_barrier.wait().await;
            first_manager.acquire_runtime("generic", first_lease).await
        });
        let second_manager = Arc::clone(&manager);
        let second_lease = workspace_lease.clone();
        let second_barrier = Arc::clone(&barrier);
        let second = tokio::spawn(async move {
            second_barrier.wait().await;
            second_manager
                .acquire_runtime("generic", second_lease)
                .await
        });

        barrier.wait().await;
        provider.start_entered.notified().await;
        let slot = runtime_slot(&manager, "generic", &workspace_lease);
        slot.startup_waiter_registered.notified().await;
        assert_eq!(provider.starts.load(Ordering::SeqCst), 1);

        start_result.send(StartPlan::Success).unwrap();
        let first = first.await.unwrap().unwrap();
        let second = second.await.unwrap().unwrap();
        assert!(std::ptr::eq(first.runtime(), second.runtime()));
        assert_eq!(provider.starts.load(Ordering::SeqCst), 1);
        assert_eq!(provider.observations.load(Ordering::SeqCst), 0);
        assert_eq!(provider.prepares.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    /// 验证同轮 startup failure 由全部等待者共享，下一轮 acquire 可重试。
    async fn startup_failure_is_shared_by_waiting_wave_and_fresh_acquire_retries() {
        let provider = runtime_provider("generic", 2);
        let failed_start = provider.enqueue_start();
        let manager = runtime_manager(Arc::clone(&provider));
        let workspace_lease = lease("workspace-a", 7);
        let barrier = Arc::new(Barrier::new(3));

        let first_manager = Arc::clone(&manager);
        let first_lease = workspace_lease.clone();
        let first_barrier = Arc::clone(&barrier);
        let first = tokio::spawn(async move {
            first_barrier.wait().await;
            first_manager.acquire_runtime("generic", first_lease).await
        });
        let second_manager = Arc::clone(&manager);
        let second_lease = workspace_lease.clone();
        let second_barrier = Arc::clone(&barrier);
        let second = tokio::spawn(async move {
            second_barrier.wait().await;
            second_manager
                .acquire_runtime("generic", second_lease)
                .await
        });

        barrier.wait().await;
        provider.start_entered.notified().await;
        let slot = runtime_slot(&manager, "generic", &workspace_lease);
        slot.startup_waiter_registered.notified().await;
        failed_start.send(StartPlan::Failure).unwrap();

        for result in [first.await.unwrap(), second.await.unwrap()] {
            let error = match result {
                Err(error) => error,
                Ok(_) => panic!("failed startup wave must not return a guard"),
            };
            assert_eq!(error.code, WorkspaceCapabilityErrorCode::StartFailed);
        }
        assert_eq!(provider.starts.load(Ordering::SeqCst), 1);

        let retry = provider.enqueue_start();
        let retry_manager = Arc::clone(&manager);
        let retry_lease = workspace_lease.clone();
        let fresh =
            tokio::spawn(
                async move { retry_manager.acquire_runtime("generic", retry_lease).await },
            );
        provider.start_entered.notified().await;
        retry.send(StartPlan::Success).unwrap();
        drop(fresh.await.unwrap().unwrap());
        assert_eq!(provider.starts.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    /// 验证旧 wave 的 waiter 即使晚于 retry poll，也只能收到自己 flight 的失败结果。
    async fn old_startup_flight_result_survives_a_later_successful_retry() {
        let provider = runtime_provider("generic", 1);
        let first_start = provider.enqueue_start();
        let manager = runtime_manager(Arc::clone(&provider));
        let workspace_lease = lease("workspace-a", 7);
        let leader_manager = Arc::clone(&manager);
        let leader_lease = workspace_lease.clone();
        let leader = tokio::spawn(async move {
            leader_manager
                .acquire_runtime("generic", leader_lease)
                .await
        });

        provider.start_entered.notified().await;
        let slot = runtime_slot(&manager, "generic", &workspace_lease);
        let old_completion = match slot.begin_acquire() {
            RuntimeAcquireDecision::Wait { completion } => completion,
            _ => panic!("second epoch-one caller must bind the current startup flight"),
        };
        first_start.send(StartPlan::Failure).unwrap();
        let first_error = match leader.await.unwrap() {
            Err(error) => error,
            Ok(_) => panic!("epoch-one startup must fail"),
        };
        assert_eq!(first_error.code, WorkspaceCapabilityErrorCode::StartFailed);

        let second_start = provider.enqueue_start();
        let retry_manager = Arc::clone(&manager);
        let retry_lease = workspace_lease.clone();
        let retry =
            tokio::spawn(
                async move { retry_manager.acquire_runtime("generic", retry_lease).await },
            );
        provider.start_entered.notified().await;
        second_start.send(StartPlan::Success).unwrap();
        drop(retry.await.unwrap().unwrap());

        let old_error = match WorkspaceCapabilityManager::wait_for_startup(old_completion).await {
            Err(error) => error,
            Ok(_) => panic!("epoch-one waiter must not observe epoch-two runtime"),
        };
        assert_eq!(old_error.code, WorkspaceCapabilityErrorCode::StartFailed);
        assert_eq!(provider.starts.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    /// 验证取消 startup leader 会将目标 Slot 安全收敛为 error，并允许后续 retry。
    async fn cancelled_startup_leader_does_not_leave_slot_starting() {
        let provider = runtime_provider("generic", 1);
        let cancelled_start = provider.enqueue_start();
        let manager = runtime_manager(Arc::clone(&provider));
        let workspace_lease = lease("workspace-a", 7);
        let leader_manager = Arc::clone(&manager);
        let leader_lease = workspace_lease.clone();
        let leader = tokio::spawn(async move {
            leader_manager
                .acquire_runtime("generic", leader_lease)
                .await
        });
        provider.start_entered.notified().await;
        leader.abort();
        let join_error = match leader.await {
            Err(error) => error,
            Ok(_) => panic!("aborted startup leader must not complete"),
        };
        assert!(join_error.is_cancelled());
        drop(cancelled_start);

        let slot = runtime_slot(&manager, "generic", &workspace_lease);
        assert_eq!(
            lock_unpoisoned(&slot.state).lifecycle,
            CapabilityRuntimeState::Error
        );
        let retry = provider.enqueue_start();
        let retry_manager = Arc::clone(&manager);
        let retry_lease = workspace_lease.clone();
        let retry_acquire =
            tokio::spawn(
                async move { retry_manager.acquire_runtime("generic", retry_lease).await },
            );
        provider.start_entered.notified().await;
        retry.send(StartPlan::Success).unwrap();
        drop(retry_acquire.await.unwrap().unwrap());
        assert_eq!(provider.starts.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    /// 验证 guard Drop 与取消 permit 等待均不会泄漏 in-flight 计数。
    async fn guards_count_precisely_and_cancelled_permit_wait_does_not_leak() {
        let provider = runtime_provider("generic", 1);
        let manager = runtime_manager(Arc::clone(&provider));
        let workspace_lease = lease("workspace-a", 7);
        let first = manager
            .acquire_runtime("generic", workspace_lease.clone())
            .await
            .unwrap();
        let slot = runtime_slot(&manager, "generic", &workspace_lease);
        assert_eq!(lock_unpoisoned(&slot.state).in_flight, 1);

        let waiting_manager = Arc::clone(&manager);
        let waiting_lease = workspace_lease.clone();
        let mut waiting = Box::pin(async move {
            waiting_manager
                .acquire_runtime("generic", waiting_lease)
                .await
        });
        assert!(
            std::future::poll_fn(|context| match waiting.as_mut().poll(context) {
                Poll::Pending => Poll::Ready(true),
                Poll::Ready(_) => Poll::Ready(false),
            })
            .await
        );
        assert_eq!(lock_unpoisoned(&slot.state).in_flight, 1);
        drop(waiting);
        assert_eq!(lock_unpoisoned(&slot.state).in_flight, 1);

        drop(first);
        assert_eq!(lock_unpoisoned(&slot.state).in_flight, 0);
        let second = manager
            .acquire_runtime("generic", workspace_lease.clone())
            .await
            .unwrap();
        assert_eq!(lock_unpoisoned(&slot.state).in_flight, 1);
        drop(second);
        assert_eq!(lock_unpoisoned(&slot.state).in_flight, 0);
        assert_eq!(provider.starts.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    /// 验证 descriptor 的 per-slot concurrency 允许两个 guard 而不重复 start。
    async fn two_guards_use_configured_per_slot_concurrency_without_restarting() {
        let provider = runtime_provider("generic", 2);
        let manager = runtime_manager(Arc::clone(&provider));
        let workspace_lease = lease("workspace-a", 7);
        let first = manager
            .acquire_runtime("generic", workspace_lease.clone())
            .await
            .unwrap();
        let second = manager
            .acquire_runtime("generic", workspace_lease.clone())
            .await
            .unwrap();
        let slot = runtime_slot(&manager, "generic", &workspace_lease);
        assert_eq!(lock_unpoisoned(&slot.state).in_flight, 2);
        drop(first);
        drop(second);
        assert_eq!(lock_unpoisoned(&slot.state).in_flight, 0);
        assert_eq!(provider.starts.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    /// 验证 per-slot concurrency 为一时，第二个 acquire 仅等待 guard Drop 而不重复 start。
    async fn per_slot_one_waits_for_guard_drop_without_restarting() {
        let provider = runtime_provider("generic", 1);
        let manager = runtime_manager(Arc::clone(&provider));
        let workspace_lease = lease("workspace-a", 7);
        let first = manager
            .acquire_runtime("generic", workspace_lease.clone())
            .await
            .unwrap();
        let waiting_manager = Arc::clone(&manager);
        let waiting_lease = workspace_lease.clone();
        let mut waiting = Box::pin(async move {
            waiting_manager
                .acquire_runtime("generic", waiting_lease)
                .await
        });
        assert!(
            std::future::poll_fn(|context| match waiting.as_mut().poll(context) {
                Poll::Pending => Poll::Ready(true),
                Poll::Ready(_) => Poll::Ready(false),
            })
            .await
        );
        assert_eq!(provider.starts.load(Ordering::SeqCst), 1);
        drop(first);
        drop(waiting.await.unwrap());
        assert_eq!(provider.starts.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    /// 验证错误 Runtime identity 将目标 Slot 标 error 且不交付 guard。
    async fn wrong_runtime_handle_identity_marks_only_its_slot_error() {
        for plan in [
            StartPlan::WrongProvider,
            StartPlan::WrongWorkspace,
            StartPlan::WrongGeneration,
        ] {
            let provider = runtime_provider("generic", 1);
            let wrong_start = provider.enqueue_start();
            let manager = runtime_manager(Arc::clone(&provider));
            let workspace_lease = lease("workspace-a", 7);
            let failing_manager = Arc::clone(&manager);
            let failing_lease = workspace_lease.clone();
            let acquire = tokio::spawn(async move {
                failing_manager
                    .acquire_runtime("generic", failing_lease)
                    .await
            });
            provider.start_entered.notified().await;
            wrong_start.send(plan).unwrap();
            let error = match acquire.await.unwrap() {
                Err(error) => error,
                Ok(_) => panic!("mismatched handle must not produce a guard"),
            };
            assert_eq!(error.code, WorkspaceCapabilityErrorCode::ContractError);
            let slot = runtime_slot(&manager, "generic", &workspace_lease);
            let state = lock_unpoisoned(&slot.state);
            assert_eq!(state.lifecycle, CapabilityRuntimeState::Error);
            assert_eq!(state.in_flight, 0);
        }
    }

    #[tokio::test]
    /// 验证 generation、canonical Root 与 workspace 均不能复用或串线 RuntimeSlot。
    async fn generation_root_and_workspace_slot_identity_never_crosses() {
        let provider = runtime_provider("generic", 2);
        let manager = runtime_manager(Arc::clone(&provider));
        let generation_one = lease("workspace-a", 1);
        let generation_two = lease("workspace-a", 2);
        let first = manager
            .acquire_runtime("generic", generation_one.clone())
            .await
            .unwrap();
        let second = manager
            .acquire_runtime("generic", generation_two.clone())
            .await
            .unwrap();
        assert!(!std::ptr::eq(first.runtime(), second.runtime()));
        assert_eq!(lock_unpoisoned(&manager.runtime_slots).len(), 2);
        assert_eq!(provider.starts.load(Ordering::SeqCst), 2);
        drop(first);
        drop(second);

        let mut conflicting_root = generation_one;
        conflicting_root.canonical_root = PathBuf::from("C:/different-server-root");
        let error = match manager.acquire_runtime("generic", conflicting_root).await {
            Err(error) => error,
            Ok(_) => panic!("conflicting canonical root must not produce a guard"),
        };
        assert_eq!(error.code, WorkspaceCapabilityErrorCode::ContractError);
        assert_eq!(provider.starts.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    /// 验证一个 Slot 的 start failure 不影响另一 Workspace Slot 的独立 ready。
    async fn failed_start_for_one_slot_does_not_block_another_slot() {
        let provider = runtime_provider("generic", 1);
        let first_start = provider.enqueue_start();
        let second_start = provider.enqueue_start();
        let manager = runtime_manager(Arc::clone(&provider));
        let workspace_a = lease("workspace-a", 7);
        let workspace_b = lease("workspace-b", 7);

        let failing_manager = Arc::clone(&manager);
        let failing_lease = workspace_a.clone();
        let failing = tokio::spawn(async move {
            failing_manager
                .acquire_runtime("generic", failing_lease)
                .await
        });
        provider.start_entered.notified().await;
        first_start.send(StartPlan::Failure).unwrap();
        let error = match failing.await.unwrap() {
            Err(error) => error,
            Ok(_) => panic!("workspace A start should fail"),
        };
        assert_eq!(error.code, WorkspaceCapabilityErrorCode::StartFailed);

        let ready_manager = Arc::clone(&manager);
        let ready_lease = workspace_b.clone();
        let ready =
            tokio::spawn(
                async move { ready_manager.acquire_runtime("generic", ready_lease).await },
            );
        provider.start_entered.notified().await;
        second_start.send(StartPlan::Success).unwrap();
        drop(ready.await.unwrap().unwrap());
        assert_eq!(
            lock_unpoisoned(&runtime_slot(&manager, "generic", &workspace_a).state).lifecycle,
            CapabilityRuntimeState::Error
        );
        assert_eq!(
            lock_unpoisoned(&runtime_slot(&manager, "generic", &workspace_b).state).lifecycle,
            CapabilityRuntimeState::Ready
        );
    }
}
