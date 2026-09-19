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
    time::{Duration, Instant},
};
#[cfg(test)]
use tokio::sync::Notify;
use tokio::sync::{OwnedSemaphorePermit, Semaphore, watch};
use tokio_util::sync::CancellationToken;

mod actions;
mod health;
pub(crate) use actions::CapabilityActionResult;
pub(crate) use health::WorkspaceCapabilityHealth;

/// Provider stop 的最大等待时间；超时后后台 single-flight 仍保有 handle 并继续完成清理。
#[cfg(not(test))]
const CAPABILITY_STOP_WAIT_TIMEOUT: Duration = Duration::from_secs(30);
/// 测试使用较短上界验证超时语义，避免把生产超时纳入单元测试时长。
#[cfg(test)]
const CAPABILITY_STOP_WAIT_TIMEOUT: Duration = Duration::from_millis(250);

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
    pub(crate) display_name: String,
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
    pub(crate) operation_id: String,
    pub(crate) workspace_id: String,
    pub(crate) provider_id: WorkspaceCapabilityProviderId,
    pub(crate) action_id: String,
    pub(crate) stage_code: &'static str,
    pub(crate) state: &'static str,
    pub(crate) revision: u64,
    pub(crate) message_code: &'static str,
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

    /// 仅供 Provider 在 stop 时定位自己的私有 Runtime，不暴露进程或端点细节。
    pub(crate) fn matches_identity(
        &self,
        provider_id: &WorkspaceCapabilityProviderId,
        workspace_id: &str,
        workspace_generation: u64,
    ) -> bool {
        self.provider_id == *provider_id
            && self.workspace_id == workspace_id
            && self.workspace_generation == workspace_generation
    }
}

/// Provider 安全错误的封闭代码集，避免暴露命令、路径或原始进程错误。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum CapabilityProviderErrorCode {
    RuntimeIdentityMismatch,
    NotFound,
    NotPrepared,
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

    /// 返回容量不足或 Slot 正在停止时的统一忙碌错误。
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

    /// 返回显式准备后才可启动 Runtime 的统一错误，绝不由 acquire 隐式执行 prepare。
    fn preparation_required() -> Self {
        Self {
            code: WorkspaceCapabilityErrorCode::PreparationRequired,
        }
    }

    /// 返回 Provider stop 失败但 Slot 已安全保留 Runtime ownership 的错误。
    fn stop_failed() -> Self {
        Self {
            code: WorkspaceCapabilityErrorCode::StopFailed,
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
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceToolCall {
    pub(crate) tool_name: String,
    pub(crate) arguments: Value,
    /// 仅在进程内传递请求取消；不进入 Provider wire payload，也不承载 Workspace authority。
    #[serde(skip)]
    pub(crate) cancellation: CancellationToken,
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
    stop_flight: Option<Arc<StopFlight>>,
    in_flight: usize,
    pending_acquires: usize,
    last_used_sequence: u64,
    idle_since: Option<Instant>,
}

/// 单次 startup epoch 专属的 completion，不与后续 retry 共用状态。
struct StartupFlight {
    epoch: u64,
    completion:
        watch::Sender<Option<Result<Arc<CapabilityRuntimeHandle>, WorkspaceCapabilityError>>>,
}

/// 单次 stop epoch 的完成信号；Remove 与 shutdown 只能加入同一结果，不能重复调用 Provider。
struct StopFlight {
    completion: watch::Sender<Option<Result<(), WorkspaceCapabilityError>>>,
    deadline: Instant,
}

impl StopFlight {
    /// 创建仅服务于本次 stop 的 completion channel。
    fn new() -> Self {
        let (completion, _) = watch::channel(None);
        Self {
            completion,
            deadline: Instant::now() + CAPABILITY_STOP_WAIT_TIMEOUT,
        }
    }

    /// 返回同一 stop epoch 剩余的等待预算，确保所有加入者共享同一个有界结果。
    fn remaining(&self) -> Duration {
        self.deadline.saturating_duration_since(Instant::now())
    }
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
    state_revision: watch::Sender<u64>,
    #[cfg(test)]
    startup_waiter_registered: tokio::sync::Notify,
    #[cfg(test)]
    stop_completion_published: tokio::sync::Notify,
}

/// acquire 对 Slot 当前状态的无 await 决策。
enum RuntimeAcquireDecision {
    Ready(RuntimeAcquireReservation),
    Start {
        flight: Arc<StartupFlight>,
        reservation: RuntimeAcquireReservation,
    },
    Wait {
        completion:
            watch::Receiver<Option<Result<Arc<CapabilityRuntimeHandle>, WorkspaceCapabilityError>>>,
        reservation: RuntimeAcquireReservation,
    },
    Busy,
    Evict(RuntimeStop),
}

impl RuntimeSlot {
    /// 为首次 acquire 创建 stopped Slot，未启动任何 Provider Runtime。
    fn new(canonical_root: PathBuf, per_slot_concurrency: usize) -> Self {
        let (state_revision, _) = watch::channel(0_u64);
        Self {
            canonical_root,
            state: Mutex::new(RuntimeSlotState {
                lifecycle: CapabilityRuntimeState::Stopped,
                epoch: 0,
                runtime: None,
                startup_flight: None,
                stop_flight: None,
                in_flight: 0,
                pending_acquires: 0,
                last_used_sequence: 0,
                idle_since: None,
            }),
            permits: Arc::new(Semaphore::new(per_slot_concurrency)),
            state_revision,
            #[cfg(test)]
            startup_waiter_registered: tokio::sync::Notify::new(),
            #[cfg(test)]
            stop_completion_published: tokio::sync::Notify::new(),
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
        self.publish_state_change();
    }

    /// 仅供测试绑定当前 startup flight，验证旧 wave 不会读取后续 retry 的结果。
    #[cfg(test)]
    fn startup_completion(
        &self,
    ) -> watch::Receiver<Option<Result<Arc<CapabilityRuntimeHandle>, WorkspaceCapabilityError>>>
    {
        lock_unpoisoned(&self.state)
            .startup_flight
            .as_ref()
            .expect("starting RuntimeSlot must retain its startup flight")
            .completion
            .subscribe()
    }

    /// 在拿到 permit 后把已线性化的 acquire reservation 转为 in-flight guard。
    async fn acquire_reserved_guard(
        self: &Arc<Self>,
        mut reservation: RuntimeAcquireReservation,
    ) -> Result<RuntimeInFlightGuard, WorkspaceCapabilityError> {
        let permit = Arc::clone(&self.permits)
            .acquire_owned()
            .await
            .expect("RuntimeSlot semaphore is owned by the Slot");
        let runtime = {
            let mut state = lock_unpoisoned(&self.state);
            if state.lifecycle != CapabilityRuntimeState::Ready {
                return Err(WorkspaceCapabilityError::busy());
            }
            state.pending_acquires = state
                .pending_acquires
                .checked_sub(1)
                .expect("Runtime acquire reservation must be registered");
            state.in_flight += 1;
            Arc::clone(
                state
                    .runtime
                    .as_ref()
                    .expect("ready RuntimeSlot must retain its runtime"),
            )
        };
        reservation.disarm();
        Ok(RuntimeInFlightGuard {
            runtime: Some(runtime),
            slot: Arc::clone(self),
            _permit: permit,
        })
    }

    /// 由 guard Drop 调用，保证 cancellation/panic 展开时归还 in-flight。
    fn release_in_flight(&self) {
        let mut state = lock_unpoisoned(&self.state);
        state.in_flight = state
            .in_flight
            .checked_sub(1)
            .expect("RuntimeInFlightGuard must correspond to one in-flight acquisition");
        if state.in_flight == 0 {
            state.idle_since = Some(Instant::now());
        }
        drop(state);
        self.publish_state_change();
    }

    /// 取消尚未取得 permit 的 acquire reservation，重新允许 idle eviction。
    fn release_pending_acquire(&self) {
        let mut state = lock_unpoisoned(&self.state);
        state.pending_acquires = state
            .pending_acquires
            .checked_sub(1)
            .expect("Runtime acquire reservation must correspond to one pending acquire");
        drop(state);
        self.publish_state_change();
    }

    /// 发布会影响 shutdown drain 判断的 Slot 状态变更，避免 Notify 注册窗口丢失唤醒。
    fn publish_state_change(&self) {
        self.state_revision
            .send_modify(|revision| *revision = revision.wrapping_add(1));
    }
}

/// 已经在 Manager 调度边界登记、但尚未成为 in-flight guard 的 acquire。
struct RuntimeAcquireReservation {
    slot: Arc<RuntimeSlot>,
    active: bool,
}

impl RuntimeAcquireReservation {
    /// 创建与单一 Slot 绑定的 pending acquire reservation。
    fn new(slot: Arc<RuntimeSlot>) -> Self {
        Self { slot, active: true }
    }

    /// 成功转为 guard 后，Drop 不再归还 pending 计数。
    fn disarm(&mut self) {
        self.active = false;
    }
}

impl Drop for RuntimeAcquireReservation {
    fn drop(&mut self) {
        if self.active {
            self.slot.release_pending_acquire();
        }
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
    runtime: Option<Arc<CapabilityRuntimeHandle>>,
    slot: Arc<RuntimeSlot>,
    _permit: OwnedSemaphorePermit,
}

impl RuntimeInFlightGuard {
    /// 返回受 guard 生命周期约束的 Runtime handle 借用，不暴露进程内部信息。
    pub(crate) fn runtime(&self) -> &CapabilityRuntimeHandle {
        self.runtime
            .as_deref()
            .expect("RuntimeInFlightGuard must retain its runtime until Drop")
    }
}

impl Drop for RuntimeInFlightGuard {
    fn drop(&mut self) {
        // 必须先释放额外 Arc，再让 Slot 变为可驱逐，保证 stop 可取得唯一 handle。
        drop(self.runtime.take());
        self.slot.release_in_flight();
    }
}

/// 处理 poisoned Mutex 仍恢复内部状态，避免 Provider panic 使 Slot 永久不可用。
fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Manager 内部的 Slot 表与全局单调使用序号，共同构成容量调度线性化边界。
struct RuntimeSlotTable {
    slots: HashMap<RuntimeSlotKey, Arc<RuntimeSlot>>,
    operations: HashMap<(RuntimeSlotKey, String), Arc<actions::ActionFlight>>,
    next_used_sequence: u64,
    removing_workspaces: HashSet<(String, u64)>,
    shutting_down: bool,
}

/// Workspace Remove 完成前持续持有 capability admission exclusion 的 guard。
pub(crate) struct WorkspaceRuntimeRemoval<'a> {
    manager: &'a WorkspaceCapabilityManager,
    workspace_id: String,
    generation: u64,
}

impl Drop for WorkspaceRuntimeRemoval<'_> {
    fn drop(&mut self) {
        let mut table = lock_unpoisoned(&self.manager.runtime_slots);
        table
            .removing_workspaces
            .remove(&(self.workspace_id.clone(), self.generation));
    }
}

/// 已在调度边界内完成 ownership 转移、等待 Provider stop 的单 Slot 操作。
struct RuntimeStop {
    key: RuntimeSlotKey,
    slot: Arc<RuntimeSlot>,
    runtime: CapabilityRuntimeHandle,
    flight: Arc<StopFlight>,
}

/// stop 成功后由同一调度边界执行的后续动作，避免容量交接出现可见窗口。
enum StopCompletion {
    /// idle timeout 仅将 Slot 收敛为 stopped，不产生 replacement。
    Idle,
    /// capacity eviction 在释放 victim 容量后立即为原请求继续 acquire。
    Eviction {
        provider_id: WorkspaceCapabilityProviderId,
        lease: WorkspaceLease,
        max_instances: usize,
        per_slot_concurrency: usize,
        operation_id: Option<String>,
    },
}

/// 单 Slot stop 完成后的内部结果；仅 eviction 会交付新的 acquire 决策。
enum StopResult {
    /// idle stop 已完成，没有后续 acquire。
    IdleStopped,
    /// capacity eviction 已在同一 Manager 锁内完成 replacement admission。
    ResumeAcquire(RuntimeAcquireDecision),
}

/// 具备 workspace-scoped Runtime Slot 的 Registry lookup 与 acquire Manager。
pub(crate) struct WorkspaceCapabilityManager {
    registry: Arc<WorkspaceCapabilityRegistry>,
    runtime_slots: Arc<Mutex<RuntimeSlotTable>>,
    /// 仅测试同步点：确认 shutdown 已封闭 admission，避免并发测试依赖调度时机。
    #[cfg(test)]
    shutdown_admission: Arc<Notify>,
}

impl WorkspaceCapabilityManager {
    /// 以 immutable Registry 创建无进程 Manager shell。
    pub(crate) fn new(registry: Arc<WorkspaceCapabilityRegistry>) -> Self {
        Self {
            registry,
            runtime_slots: Arc::new(Mutex::new(RuntimeSlotTable {
                slots: HashMap::new(),
                operations: HashMap::new(),
                next_used_sequence: 0,
                removing_workspaces: HashSet::new(),
                shutting_down: false,
            })),
            #[cfg(test)]
            shutdown_admission: Arc::new(Notify::new()),
        }
    }

    /// 返回 shutdown admission 已封闭后的测试同步通知，不形成生产运行时契约。
    #[cfg(test)]
    pub(crate) fn shutdown_admission_notifier(&self) -> Arc<Notify> {
        Arc::clone(&self.shutdown_admission)
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
        self.acquire_action_runtime(provider_id, lease, None).await
    }

    /// action 自身可在 exclusive claim 内 warm；外部 acquire 永远不持有此 operation identity。
    async fn acquire_action_runtime(
        &self,
        provider_id: &str,
        lease: WorkspaceLease,
        operation_id: Option<&str>,
    ) -> Result<RuntimeInFlightGuard, WorkspaceCapabilityError> {
        let provider = self.provider(provider_id)?;
        let descriptor = provider.descriptor();
        if descriptor.runtime_model != CapabilityRuntimeModel::WorkspaceScopedProcess {
            return Err(WorkspaceCapabilityError::contract_error());
        }

        let mut decision = self.begin_capacity_acquire(
            &descriptor.provider_id,
            &lease,
            descriptor.runtime_policy.max_instances,
            descriptor.runtime_policy.per_slot_concurrency,
            operation_id,
        )?;
        loop {
            match decision {
                RuntimeAcquireDecision::Ready(reservation) => {
                    let slot = Arc::clone(&reservation.slot);
                    return slot.acquire_reserved_guard(reservation).await;
                }
                RuntimeAcquireDecision::Start {
                    flight,
                    reservation,
                } => {
                    Self::start_runtime(
                        Arc::clone(&reservation.slot),
                        flight,
                        Arc::clone(&provider),
                        lease.clone(),
                    )
                    .await?;
                    let slot = Arc::clone(&reservation.slot);
                    return slot.acquire_reserved_guard(reservation).await;
                }
                RuntimeAcquireDecision::Wait {
                    completion,
                    reservation,
                } => {
                    Self::wait_for_startup(completion).await?;
                    let slot = Arc::clone(&reservation.slot);
                    return slot.acquire_reserved_guard(reservation).await;
                }
                RuntimeAcquireDecision::Busy => return Err(WorkspaceCapabilityError::busy()),
                RuntimeAcquireDecision::Evict(eviction) => {
                    decision = self
                        .stop_lru_and_resume_acquire(
                            eviction,
                            Arc::clone(&provider),
                            &lease,
                            operation_id,
                        )
                        .await?;
                }
            }
        }
    }

    /// 在单个 in-flight guard 的完整生命周期内，将 Tool 交给指定的 workspace-scoped Provider。
    ///
    /// 入口只接受服务端已解析的 Lease；guard 会跨 Provider await 保持 Slot 不可停止，取消或
    /// future Drop 时则由其 Drop 自动归还 in-flight 计数与 permit。
    pub(crate) async fn call(
        &self,
        provider_id: &str,
        lease: WorkspaceLease,
        tool: WorkspaceToolCall,
    ) -> Result<WorkspaceToolResult, WorkspaceCapabilityError> {
        let provider = self.provider(provider_id)?;
        let tool_owner = self.tool_owner(&tool.tool_name)?;
        if tool_owner != provider.descriptor().provider_id {
            return Err(WorkspaceCapabilityError::contract_error());
        }
        match provider.descriptor().runtime_model {
            // In-process 与 stateless command Adapter 没有 Provider Runtime，不能创建 RuntimeSlot。
            CapabilityRuntimeModel::InProcess | CapabilityRuntimeModel::StatelessCommand => {
                call_with_checked_runtime(provider.as_ref(), &lease, None, tool)
                    .await
                    .map_err(Self::map_provider_call_error)
            }
            CapabilityRuntimeModel::WorkspaceScopedProcess => {
                let guard = self.acquire_runtime(provider_id, lease.clone()).await?;
                call_with_checked_runtime(provider.as_ref(), &lease, Some(guard.runtime()), tool)
                    .await
                    .map_err(Self::map_provider_call_error)
            }
        }
    }

    /// 依据 Registry 的唯一 Tool owner 执行调用；调用方不需要、也不能自行选择 Provider ID。
    pub(crate) async fn call_tool(
        &self,
        lease: WorkspaceLease,
        tool: WorkspaceToolCall,
    ) -> Result<WorkspaceToolResult, WorkspaceCapabilityError> {
        let provider_id = self.tool_owner(&tool.tool_name)?;
        self.call(provider_id.as_str(), lease, tool).await
    }

    /// 在 Manager 锁内创建或读取目标 Slot，并将本次 acquire 线性化到容量调度。
    fn begin_capacity_acquire(
        &self,
        provider_id: &WorkspaceCapabilityProviderId,
        lease: &WorkspaceLease,
        max_instances: usize,
        per_slot_concurrency: usize,
        operation_id: Option<&str>,
    ) -> Result<RuntimeAcquireDecision, WorkspaceCapabilityError> {
        let key = RuntimeSlotKey::new(provider_id.clone(), lease);
        let mut table = lock_unpoisoned(&self.runtime_slots);
        Self::begin_capacity_acquire_locked(
            &mut table,
            key,
            lease,
            max_instances,
            per_slot_concurrency,
            operation_id,
        )
    }

    /// 在已持有 Manager 锁时执行容量检查、LRU 选择及 Slot 状态切换。
    fn begin_capacity_acquire_locked(
        table: &mut RuntimeSlotTable,
        key: RuntimeSlotKey,
        lease: &WorkspaceLease,
        max_instances: usize,
        per_slot_concurrency: usize,
        operation_id: Option<&str>,
    ) -> Result<RuntimeAcquireDecision, WorkspaceCapabilityError> {
        if table.shutting_down
            || table.operations.iter().any(|((operation_key, _), flight)| {
                operation_key == &key
                    && flight.exclusive
                    && Some(flight.operation_id.as_str()) != operation_id
            })
            || table
                .removing_workspaces
                .contains(&(key.workspace_id.clone(), key.generation))
        {
            return Ok(RuntimeAcquireDecision::Busy);
        }
        let target = if let Some(slot) = table.slots.get(&key) {
            (slot.canonical_root == lease.canonical_root)
                .then(|| Arc::clone(slot))
                .ok_or_else(WorkspaceCapabilityError::contract_error)?
        } else {
            let slot = Arc::new(RuntimeSlot::new(
                lease.canonical_root.clone(),
                per_slot_concurrency,
            ));
            table.slots.insert(key.clone(), Arc::clone(&slot));
            slot
        };

        let state = lock_unpoisoned(&target.state);
        match state.lifecycle {
            CapabilityRuntimeState::Ready => {
                drop(state);
                Ok(Self::reserve_ready_acquire(table, target))
            }
            CapabilityRuntimeState::Starting => {
                #[cfg(test)]
                target.startup_waiter_registered.notify_one();
                let completion = state
                    .startup_flight
                    .as_ref()
                    .expect("starting RuntimeSlot must retain its startup flight")
                    .completion
                    .subscribe();
                drop(state);
                Ok(RuntimeAcquireDecision::Wait {
                    completion,
                    reservation: Self::reserve_acquire(table, target),
                })
            }
            CapabilityRuntimeState::Stopping => {
                drop(state);
                Ok(RuntimeAcquireDecision::Busy)
            }
            CapabilityRuntimeState::Error if state.runtime.is_some() => {
                // stop failure 返还的 handle 仍由 Slot 持有；不得在后续 acquire 中清空或替代。
                drop(state);
                Ok(RuntimeAcquireDecision::Busy)
            }
            CapabilityRuntimeState::Stopped | CapabilityRuntimeState::Error => {
                drop(state);
                if Self::allocated_capacity(table, &key.provider_id) < max_instances {
                    let flight = {
                        let mut state = lock_unpoisoned(&target.state);
                        state.epoch = state.epoch.wrapping_add(1);
                        state.lifecycle = CapabilityRuntimeState::Starting;
                        state.runtime = None;
                        let flight = Arc::new(StartupFlight::new(state.epoch));
                        state.startup_flight = Some(Arc::clone(&flight));
                        flight
                    };
                    Ok(RuntimeAcquireDecision::Start {
                        flight,
                        reservation: Self::reserve_acquire(table, target),
                    })
                } else {
                    Self::select_lru_eviction(table, &key.provider_id)
                        .map(RuntimeAcquireDecision::Evict)
                        .ok_or_else(WorkspaceCapabilityError::busy)
                }
            }
        }
    }

    /// 为本次 acquire 分配稳定的使用序号，并防止 permit 等待期间被误判为 idle。
    fn reserve_acquire(
        table: &mut RuntimeSlotTable,
        slot: Arc<RuntimeSlot>,
    ) -> RuntimeAcquireReservation {
        table.next_used_sequence = table
            .next_used_sequence
            .checked_add(1)
            .expect("Runtime LRU usage sequence must not overflow");
        let mut state = lock_unpoisoned(&slot.state);
        state.pending_acquires += 1;
        state.last_used_sequence = table.next_used_sequence;
        state.idle_since = None;
        drop(state);
        RuntimeAcquireReservation::new(slot)
    }

    /// Ready Slot 的 acquire 也必须走 reservation，避免 eviction 在 await permit 时夺走 handle。
    fn reserve_ready_acquire(
        table: &mut RuntimeSlotTable,
        slot: Arc<RuntimeSlot>,
    ) -> RuntimeAcquireDecision {
        RuntimeAcquireDecision::Ready(Self::reserve_acquire(table, slot))
    }

    /// 仅统计同一 Provider 的 live/allocated Slot，以及仍持有 stop-failure handle 的 error Slot。
    fn allocated_capacity(
        table: &RuntimeSlotTable,
        provider_id: &WorkspaceCapabilityProviderId,
    ) -> usize {
        table
            .slots
            .iter()
            .filter(|(key, slot)| {
                if key.provider_id != *provider_id {
                    return false;
                }
                let state = lock_unpoisoned(&slot.state);
                matches!(
                    state.lifecycle,
                    CapabilityRuntimeState::Starting
                        | CapabilityRuntimeState::Ready
                        | CapabilityRuntimeState::Stopping
                ) || (state.lifecycle == CapabilityRuntimeState::Error && state.runtime.is_some())
            })
            .count()
    }

    /// 选中唯一最小使用序号的 idle Ready Slot，并在同一同步边界内转移 stop ownership。
    fn select_lru_eviction(
        table: &RuntimeSlotTable,
        provider_id: &WorkspaceCapabilityProviderId,
    ) -> Option<RuntimeStop> {
        let victim = table
            .slots
            .iter()
            .filter_map(|(key, slot)| {
                if key.provider_id != *provider_id {
                    return None;
                }
                if table
                    .operations
                    .keys()
                    .any(|(operation_key, _)| operation_key == key)
                {
                    return None;
                }
                let state = lock_unpoisoned(&slot.state);
                (state.lifecycle == CapabilityRuntimeState::Ready
                    && state.in_flight == 0
                    && state.pending_acquires == 0)
                    .then(|| (state.last_used_sequence, key.clone(), Arc::clone(slot)))
            })
            .min_by_key(|(last_used_sequence, _, _)| *last_used_sequence)?;
        let (_, key, slot) = victim;
        Self::begin_slot_stop(key, slot)
    }

    /// 在 Manager 调度边界内转移唯一 handle，令同一 Slot 后续 stop 请求只能加入既有结果。
    fn begin_slot_stop(key: RuntimeSlotKey, slot: Arc<RuntimeSlot>) -> Option<RuntimeStop> {
        let mut state = lock_unpoisoned(&slot.state);
        if !matches!(
            state.lifecycle,
            CapabilityRuntimeState::Ready | CapabilityRuntimeState::Error
        ) || state.in_flight != 0
            || state.pending_acquires != 0
            || state.runtime.is_none()
        {
            return None;
        }
        let runtime = state
            .runtime
            .take()
            .expect("eligible Ready RuntimeSlot must retain its runtime");
        let runtime = Arc::try_unwrap(runtime)
            .expect("idle RuntimeSlot must have no Runtime handle outside Manager ownership");
        state.lifecycle = CapabilityRuntimeState::Stopping;
        state.idle_since = None;
        let flight = Arc::new(StopFlight::new());
        state.stop_flight = Some(Arc::clone(&flight));
        drop(state);
        slot.publish_state_change();
        Some(RuntimeStop {
            key,
            slot,
            runtime,
            flight,
        })
    }

    /// 扫描并停止超过 Descriptor idle timeout 的 Slot；不接入后台 sweeper、Remove 或 shutdown。
    pub(crate) async fn stop_idle_runtimes(&self) -> Result<(), WorkspaceCapabilityError> {
        while let Some((stop, provider)) = self.begin_idle_stop()? {
            match self
                .stop_runtime(stop, provider, StopCompletion::Idle)
                .await?
            {
                StopResult::IdleStopped => {}
                StopResult::ResumeAcquire(_) => {
                    return Err(WorkspaceCapabilityError::contract_error());
                }
            }
        }
        Ok(())
    }

    /// 在 Manager 锁内选中一个已超过 timeout 的 idle Slot，并原子转移其 stop ownership。
    fn begin_idle_stop(
        &self,
    ) -> Result<Option<(RuntimeStop, Arc<dyn WorkspaceCapabilityProvider>)>, WorkspaceCapabilityError>
    {
        let now = Instant::now();
        let table = lock_unpoisoned(&self.runtime_slots);
        let mut candidate = None;
        for (key, slot) in &table.slots {
            if table
                .operations
                .keys()
                .any(|(operation_key, _)| operation_key == key)
            {
                continue;
            }
            let provider = self
                .registry
                .provider(key.provider_id.as_str())
                .map_err(Self::map_registry_error)?;
            let state = lock_unpoisoned(&slot.state);
            let timed_out = state.idle_since.is_some_and(|idle_since| {
                now.saturating_duration_since(idle_since)
                    >= Duration::from_millis(provider.descriptor().runtime_policy.idle_timeout_ms)
            });
            if state.lifecycle == CapabilityRuntimeState::Ready
                && state.in_flight == 0
                && state.pending_acquires == 0
                && timed_out
            {
                candidate = Some((key.clone(), Arc::clone(slot), provider));
                break;
            }
        }
        let Some((key, slot, provider)) = candidate else {
            return Ok(None);
        };
        let stop = Self::begin_slot_stop(key, slot)
            .expect("idle RuntimeSlot selected under Manager lock must remain stoppable");
        Ok(Some((stop, provider)))
    }

    /// capacity eviction 复用通用 stop primitive，并在 stop success 后原子恢复原 acquire。
    async fn stop_lru_and_resume_acquire(
        &self,
        eviction: RuntimeStop,
        provider: Arc<dyn WorkspaceCapabilityProvider>,
        lease: &WorkspaceLease,
        operation_id: Option<&str>,
    ) -> Result<RuntimeAcquireDecision, WorkspaceCapabilityError> {
        let descriptor = provider.descriptor();
        let completion = StopCompletion::Eviction {
            provider_id: descriptor.provider_id.clone(),
            lease: lease.clone(),
            max_instances: descriptor.runtime_policy.max_instances,
            per_slot_concurrency: descriptor.runtime_policy.per_slot_concurrency,
            operation_id: operation_id.map(str::to_owned),
        };
        match self.stop_runtime(eviction, provider, completion).await? {
            StopResult::ResumeAcquire(decision) => Ok(decision),
            StopResult::IdleStopped => Err(WorkspaceCapabilityError::contract_error()),
        }
    }

    /// 启动取消安全的 per-Slot stop single-flight；Provider stop 从不持有 Manager 锁。
    async fn stop_runtime(
        &self,
        stop: RuntimeStop,
        provider: Arc<dyn WorkspaceCapabilityProvider>,
        completion: StopCompletion,
    ) -> Result<StopResult, WorkspaceCapabilityError> {
        let (completion_sender, completion_receiver) = tokio::sync::oneshot::channel();
        let runtime_slots = Arc::clone(&self.runtime_slots);
        let flight = Arc::clone(&stop.flight);
        tokio::spawn(async move {
            let result = Self::complete_slot_stop(runtime_slots, stop, provider, completion).await;
            if let Err(result) = completion_sender.send(result) {
                if let Ok(StopResult::ResumeAcquire(decision)) = result {
                    Self::cancel_unclaimed_acquire(decision);
                }
            }
        });
        tokio::time::timeout(flight.remaining(), completion_receiver)
            .await
            .map_err(|_| WorkspaceCapabilityError::stop_failed())?
            .map_err(|_| WorkspaceCapabilityError::contract_error())?
    }

    /// 完成单 Slot stop，并在成功时于同一 Manager 锁内执行可选的 eviction replacement admission。
    async fn complete_slot_stop(
        runtime_slots: Arc<Mutex<RuntimeSlotTable>>,
        stop: RuntimeStop,
        provider: Arc<dyn WorkspaceCapabilityProvider>,
        completion: StopCompletion,
    ) -> Result<StopResult, WorkspaceCapabilityError> {
        match provider.stop(stop.runtime).await {
            Ok(evidence) if evidence.runtime_state == CapabilityRuntimeState::Stopped => {
                let mut table = lock_unpoisoned(&runtime_slots);
                let mut state = lock_unpoisoned(&stop.slot.state);
                if state.lifecycle != CapabilityRuntimeState::Stopping
                    || state.runtime.is_some()
                    || !state
                        .stop_flight
                        .as_ref()
                        .is_some_and(|flight| Arc::ptr_eq(flight, &stop.flight))
                {
                    stop.flight
                        .completion
                        .send_replace(Some(Err(WorkspaceCapabilityError::contract_error())));
                    return Err(WorkspaceCapabilityError::contract_error());
                }
                state.lifecycle = CapabilityRuntimeState::Stopped;
                state.idle_since = None;
                state.stop_flight = None;
                drop(state);
                stop.flight.completion.send_replace(Some(Ok(())));
                stop.slot.publish_state_change();
                let result = match completion {
                    StopCompletion::Idle => StopResult::IdleStopped,
                    StopCompletion::Eviction {
                        provider_id,
                        lease,
                        max_instances,
                        per_slot_concurrency,
                        operation_id,
                    } => StopResult::ResumeAcquire(Self::begin_capacity_acquire_locked(
                        &mut table,
                        RuntimeSlotKey::new(provider_id, &lease),
                        &lease,
                        max_instances,
                        per_slot_concurrency,
                        operation_id.as_deref(),
                    )?),
                };
                drop(table);
                #[cfg(test)]
                stop.slot.stop_completion_published.notify_waiters();
                Ok(result)
            }
            Ok(_) => {
                let _table = lock_unpoisoned(&runtime_slots);
                let mut state = lock_unpoisoned(&stop.slot.state);
                if state.lifecycle != CapabilityRuntimeState::Stopping
                    || state.runtime.is_some()
                    || !state
                        .stop_flight
                        .as_ref()
                        .is_some_and(|flight| Arc::ptr_eq(flight, &stop.flight))
                {
                    stop.flight
                        .completion
                        .send_replace(Some(Err(WorkspaceCapabilityError::contract_error())));
                    return Err(WorkspaceCapabilityError::contract_error());
                }
                state.lifecycle = CapabilityRuntimeState::Error;
                state.stop_flight = None;
                drop(state);
                stop.flight
                    .completion
                    .send_replace(Some(Err(WorkspaceCapabilityError::contract_error())));
                stop.slot.publish_state_change();
                #[cfg(test)]
                stop.slot.stop_completion_published.notify_waiters();
                Err(WorkspaceCapabilityError::contract_error())
            }
            Err(failure) => {
                let _table = lock_unpoisoned(&runtime_slots);
                let mut state = lock_unpoisoned(&stop.slot.state);
                if failure.runtime.provider_id != stop.key.provider_id
                    || failure.runtime.workspace_id != stop.key.workspace_id
                    || failure.runtime.workspace_generation != stop.key.generation
                    || state.lifecycle != CapabilityRuntimeState::Stopping
                    || state.runtime.is_some()
                    || !state
                        .stop_flight
                        .as_ref()
                        .is_some_and(|flight| Arc::ptr_eq(flight, &stop.flight))
                {
                    stop.flight
                        .completion
                        .send_replace(Some(Err(WorkspaceCapabilityError::contract_error())));
                    return Err(WorkspaceCapabilityError::contract_error());
                }
                state.runtime = Some(Arc::new(failure.runtime));
                state.lifecycle = CapabilityRuntimeState::Error;
                state.idle_since = None;
                state.stop_flight = None;
                drop(state);
                stop.flight
                    .completion
                    .send_replace(Some(Err(WorkspaceCapabilityError::stop_failed())));
                stop.slot.publish_state_change();
                #[cfg(test)]
                stop.slot.stop_completion_published.notify_waiters();
                Err(WorkspaceCapabilityError::stop_failed())
            }
        }
    }

    /// 回滚接收方取消后尚未执行的 admission，避免遗留 pending acquire 或无 leader startup。
    fn cancel_unclaimed_acquire(decision: RuntimeAcquireDecision) {
        match decision {
            RuntimeAcquireDecision::Start {
                flight,
                reservation,
            } => {
                reservation
                    .slot
                    .complete_startup(&flight, Err(WorkspaceCapabilityError::start_failed()));
            }
            RuntimeAcquireDecision::Evict(eviction) => {
                let mut state = lock_unpoisoned(&eviction.slot.state);
                if state.lifecycle == CapabilityRuntimeState::Stopping && state.runtime.is_none() {
                    state.runtime = Some(Arc::new(eviction.runtime));
                    state.lifecycle = CapabilityRuntimeState::Ready;
                }
            }
            RuntimeAcquireDecision::Ready(_)
            | RuntimeAcquireDecision::Wait { .. }
            | RuntimeAcquireDecision::Busy => {}
        }
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
            .map_err(|error| match error.code {
                CapabilityProviderErrorCode::NotPrepared => {
                    WorkspaceCapabilityError::preparation_required()
                }
                _ => WorkspaceCapabilityError::start_failed(),
            })
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

    /// 为目标 Workspace 关闭新的 capability admission，并在 Registry 删除前收敛全部 RuntimeSlot。
    pub(crate) async fn begin_workspace_remove(
        &self,
        lease: &WorkspaceLease,
    ) -> Result<WorkspaceRuntimeRemoval<'_>, WorkspaceCapabilityError> {
        let removal = self.begin_workspace_remove_admission(lease)?;
        if let Err(error) = self.drain_workspace_remove(lease).await {
            drop(removal);
            return Err(error);
        }
        Ok(removal)
    }

    /// 同步建立 Remove admission；Supervisor 必须在其 operation 临界区内调用它。
    pub(crate) fn begin_workspace_remove_admission(
        &self,
        lease: &WorkspaceLease,
    ) -> Result<WorkspaceRuntimeRemoval<'_>, WorkspaceCapabilityError> {
        self.mark_workspace_removing(lease)
    }

    /// 在 admission 已建立后异步 drain Runtime；调用期间不持有 Supervisor operation mutex。
    pub(crate) async fn drain_workspace_remove(
        &self,
        lease: &WorkspaceLease,
    ) -> Result<(), WorkspaceCapabilityError> {
        let keys = {
            let table = lock_unpoisoned(&self.runtime_slots);
            if !table
                .removing_workspaces
                .contains(&(lease.workspace_id.clone(), lease.generation))
            {
                return Err(WorkspaceCapabilityError::contract_error());
            }
            table
                .slots
                .keys()
                .filter(|key| {
                    key.workspace_id == lease.workspace_id && key.generation == lease.generation
                })
                .cloned()
                .collect::<Vec<_>>()
        };
        for key in keys {
            self.drain_workspace_remove_slot(&key).await?;
        }
        Ok(())
    }

    /// 在 Supervisor operation 临界区内查询同一 admission，阻止 Agent Start 创建新的 Claim。
    pub(crate) fn workspace_remove_admission_active(&self, lease: &WorkspaceLease) -> bool {
        lock_unpoisoned(&self.runtime_slots)
            .removing_workspaces
            .contains(&(lease.workspace_id.clone(), lease.generation))
    }

    /// 在同一 Slot 表线性化边界登记 Remove，并拒绝已有 startup、调用或 admission 的 Workspace。
    fn mark_workspace_removing(
        &self,
        lease: &WorkspaceLease,
    ) -> Result<WorkspaceRuntimeRemoval<'_>, WorkspaceCapabilityError> {
        let mut table = lock_unpoisoned(&self.runtime_slots);
        let identity = (lease.workspace_id.clone(), lease.generation);
        if table.shutting_down || table.removing_workspaces.contains(&identity) {
            return Err(WorkspaceCapabilityError::busy());
        }
        if table.operations.keys().any(|(key, _)| {
            key.workspace_id == lease.workspace_id && key.generation == lease.generation
        }) {
            return Err(WorkspaceCapabilityError::busy());
        }
        for (key, slot) in &table.slots {
            if key.workspace_id != lease.workspace_id || key.generation != lease.generation {
                continue;
            }
            if slot.canonical_root != lease.canonical_root {
                return Err(WorkspaceCapabilityError::contract_error());
            }
            let state = lock_unpoisoned(&slot.state);
            if state.lifecycle == CapabilityRuntimeState::Starting
                || state.in_flight != 0
                || state.pending_acquires != 0
            {
                return Err(WorkspaceCapabilityError::busy());
            }
        }
        table.removing_workspaces.insert(identity.clone());
        Ok(WorkspaceRuntimeRemoval {
            manager: self,
            workspace_id: identity.0,
            generation: identity.1,
        })
    }

    /// 收敛 Remove 已排他的单个 Slot；已 Stopping 的调用方只等待其既有完成结果。
    async fn drain_workspace_remove_slot(
        &self,
        key: &RuntimeSlotKey,
    ) -> Result<(), WorkspaceCapabilityError> {
        loop {
            enum Next {
                Done,
                WaitStop(Arc<StopFlight>),
                Stop(RuntimeStop, Arc<dyn WorkspaceCapabilityProvider>),
            }
            let next = {
                let table = lock_unpoisoned(&self.runtime_slots);
                let slot = Arc::clone(
                    table
                        .slots
                        .get(key)
                        .expect("workspace remove Slot must remain in its Manager table"),
                );
                let state = lock_unpoisoned(&slot.state);
                match state.lifecycle {
                    CapabilityRuntimeState::Stopped | CapabilityRuntimeState::Error
                        if state.runtime.is_none() =>
                    {
                        Next::Done
                    }
                    CapabilityRuntimeState::Stopping => Next::WaitStop(Arc::clone(
                        state
                            .stop_flight
                            .as_ref()
                            .expect("stopping RuntimeSlot must retain its stop flight"),
                    )),
                    CapabilityRuntimeState::Ready | CapabilityRuntimeState::Error
                        if state.runtime.is_some() =>
                    {
                        if state.in_flight != 0 || state.pending_acquires != 0 {
                            return Err(WorkspaceCapabilityError::busy());
                        }
                        drop(state);
                        let stop = Self::begin_slot_stop(key.clone(), Arc::clone(&slot))
                            .expect("idle remove RuntimeSlot must transfer its stop ownership");
                        let provider = self
                            .registry
                            .provider(key.provider_id.as_str())
                            .map_err(Self::map_registry_error)?;
                        Next::Stop(stop, provider)
                    }
                    CapabilityRuntimeState::Starting => {
                        return Err(WorkspaceCapabilityError::busy());
                    }
                    CapabilityRuntimeState::Stopped
                    | CapabilityRuntimeState::Ready
                    | CapabilityRuntimeState::Error => {
                        return Err(WorkspaceCapabilityError::contract_error());
                    }
                }
            };
            match next {
                Next::Done => return Ok(()),
                Next::WaitStop(flight) => Self::wait_for_stop(flight).await?,
                Next::Stop(stop, provider) => match self
                    .stop_runtime(stop, provider, StopCompletion::Idle)
                    .await?
                {
                    StopResult::IdleStopped => return Ok(()),
                    StopResult::ResumeAcquire(_) => {
                        return Err(WorkspaceCapabilityError::contract_error());
                    }
                },
            }
        }
    }

    /// 等待指定 stop epoch 的结果；后续 retry 不能伪造本次完成。
    async fn wait_for_stop(flight: Arc<StopFlight>) -> Result<(), WorkspaceCapabilityError> {
        let mut completion = flight.completion.subscribe();
        loop {
            if let Some(result) = completion.borrow_and_update().clone() {
                return result;
            }
            tokio::time::timeout(flight.remaining(), completion.changed())
                .await
                .map_err(|_| WorkspaceCapabilityError::stop_failed())?
                .map_err(|_| WorkspaceCapabilityError::contract_error())?;
        }
    }

    /// Host shutdown 先封闭新的 admission，再逐一停止所有 live 或 retained-handle Slot。
    pub(crate) async fn shutdown_runtimes(&self) -> Result<(), WorkspaceCapabilityError> {
        let operations = {
            let mut table = lock_unpoisoned(&self.runtime_slots);
            table.shutting_down = true;
            table
                .operations
                .values()
                .map(|operation| operation.completion.subscribe())
                .collect::<Vec<_>>()
        };
        #[cfg(test)]
        self.shutdown_admission.notify_waiters();
        // 已获授权的 bounded operation 先完成并归还 claim，随后统一停止 warm Runtime。
        for mut completion in operations {
            while completion.borrow_and_update().is_none() {
                if completion.changed().await.is_err() {
                    break;
                }
            }
        }
        let keys = {
            let mut table = lock_unpoisoned(&self.runtime_slots);
            table.shutting_down = true;
            table.slots.keys().cloned().collect::<Vec<_>>()
        };
        let mut failure = None;
        for key in keys {
            if let Err(error) = self.drain_shutdown_slot(&key).await {
                failure.get_or_insert(error);
            }
        }
        failure.map_or(Ok(()), Err)
    }

    /// shutdown 等待既有 guard、startup 或 stop 结束，但从不在 MutexGuard 存活时 await。
    async fn drain_shutdown_slot(
        &self,
        key: &RuntimeSlotKey,
    ) -> Result<(), WorkspaceCapabilityError> {
        loop {
            enum Next {
                Done,
                WaitState(watch::Receiver<u64>),
                WaitStart(
                    watch::Receiver<
                        Option<Result<Arc<CapabilityRuntimeHandle>, WorkspaceCapabilityError>>,
                    >,
                ),
                WaitStop(Arc<StopFlight>),
                Stop(RuntimeStop, Arc<dyn WorkspaceCapabilityProvider>),
            }
            let next = {
                let table = lock_unpoisoned(&self.runtime_slots);
                let slot = Arc::clone(
                    table
                        .slots
                        .get(key)
                        .expect("shutdown RuntimeSlot must remain in its Manager table"),
                );
                let state_changed = slot.state_revision.subscribe();
                let state = lock_unpoisoned(&slot.state);
                match state.lifecycle {
                    CapabilityRuntimeState::Stopped | CapabilityRuntimeState::Error
                        if state.runtime.is_none() =>
                    {
                        Next::Done
                    }
                    CapabilityRuntimeState::Starting => Next::WaitStart(
                        state
                            .startup_flight
                            .as_ref()
                            .expect("starting RuntimeSlot must retain its startup flight")
                            .completion
                            .subscribe(),
                    ),
                    CapabilityRuntimeState::Stopping => Next::WaitStop(Arc::clone(
                        state
                            .stop_flight
                            .as_ref()
                            .expect("stopping RuntimeSlot must retain its stop flight"),
                    )),
                    CapabilityRuntimeState::Ready | CapabilityRuntimeState::Error
                        if state.runtime.is_some() =>
                    {
                        if state.in_flight != 0 || state.pending_acquires != 0 {
                            Next::WaitState(state_changed)
                        } else {
                            drop(state);
                            let stop = Self::begin_slot_stop(key.clone(), Arc::clone(&slot))
                                .expect(
                                    "idle shutdown RuntimeSlot must transfer its stop ownership",
                                );
                            let provider = self
                                .registry
                                .provider(key.provider_id.as_str())
                                .map_err(Self::map_registry_error)?;
                            Next::Stop(stop, provider)
                        }
                    }
                    CapabilityRuntimeState::Stopped
                    | CapabilityRuntimeState::Ready
                    | CapabilityRuntimeState::Error => {
                        return Err(WorkspaceCapabilityError::contract_error());
                    }
                }
            };
            match next {
                Next::Done => return Ok(()),
                Next::WaitState(mut revision) => revision
                    .changed()
                    .await
                    .map_err(|_| WorkspaceCapabilityError::contract_error())?,
                Next::WaitStart(completion) => {
                    let _ = Self::wait_for_startup(completion).await;
                }
                Next::WaitStop(flight) => Self::wait_for_stop(flight).await?,
                Next::Stop(stop, provider) => match self
                    .stop_runtime(stop, provider, StopCompletion::Idle)
                    .await?
                {
                    StopResult::IdleStopped => return Ok(()),
                    StopResult::ResumeAcquire(_) => {
                        return Err(WorkspaceCapabilityError::contract_error());
                    }
                },
            }
        }
    }

    /// 将 Registry 内部错误收敛为 Manager 的安全 Workspace Capability 错误。
    fn map_registry_error(error: CapabilityProviderError) -> WorkspaceCapabilityError {
        match error.code {
            CapabilityProviderErrorCode::NotFound => WorkspaceCapabilityError::not_found(),
            _ => WorkspaceCapabilityError::contract_error(),
        }
    }

    /// 将 Provider 的封闭安全代码继续收敛为 Manager 对外的冻结错误，不传播实现细节。
    fn map_provider_call_error(error: CapabilityProviderError) -> WorkspaceCapabilityError {
        match error.code {
            CapabilityProviderErrorCode::NotFound
            | CapabilityProviderErrorCode::Unavailable
            | CapabilityProviderErrorCode::OperationFailed => WorkspaceCapabilityError {
                code: WorkspaceCapabilityErrorCode::RuntimeLost,
            },
            CapabilityProviderErrorCode::NotPrepared => {
                WorkspaceCapabilityError::preparation_required()
            }
            CapabilityProviderErrorCode::RuntimeIdentityMismatch
            | CapabilityProviderErrorCode::ContractError => {
                WorkspaceCapabilityError::contract_error()
            }
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
    // 子模块复用既有 fake Provider；所有 action 测试仍属于 workspace_capability::tests。
    include!("workspace_capability/action_tests.rs");
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
                display_name: "Index".into(),
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

    /// 受 channel 控制的 stop 结果，确保 Stopping 竞态测试不依赖 sleep。
    #[derive(Clone, Copy, Debug)]
    enum StopPlan {
        Success,
        Failure,
    }

    /// 受 channel 控制的 Tool 调用结果，用于验证 Manager guard 的取消释放语义。
    #[derive(Clone, Copy, Debug)]
    enum CallPlan {
        Success,
        Failure,
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
        call_fails: std::sync::atomic::AtomicBool,
        start_plans: Mutex<VecDeque<oneshot::Receiver<StartPlan>>>,
        start_entered: Arc<Notify>,
        call_plans: Mutex<VecDeque<oneshot::Receiver<CallPlan>>>,
        prepare_plans: Mutex<VecDeque<oneshot::Receiver<CallPlan>>>,
        prepare_entered: Arc<Notify>,
        prepared_leases: Mutex<Vec<WorkspaceLease>>,
        call_entered: Arc<Notify>,
        stop_plans: Mutex<VecDeque<oneshot::Receiver<StopPlan>>>,
        stop_entered: Arc<Notify>,
        stopped_workspaces: Arc<Mutex<Vec<String>>>,
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
                call_fails: std::sync::atomic::AtomicBool::new(false),
                start_plans: Mutex::new(VecDeque::new()),
                start_entered: Arc::new(Notify::new()),
                call_plans: Mutex::new(VecDeque::new()),
                prepare_plans: Mutex::new(VecDeque::new()),
                prepare_entered: Arc::new(Notify::new()),
                prepared_leases: Mutex::new(Vec::new()),
                call_entered: Arc::new(Notify::new()),
                stop_plans: Mutex::new(VecDeque::new()),
                stop_entered: Arc::new(Notify::new()),
                stopped_workspaces: Arc::new(Mutex::new(Vec::new())),
            }
        }

        /// 为下一次 start 安排一个由测试 channel 放行的确定性结果。
        fn enqueue_start(&self) -> oneshot::Sender<StartPlan> {
            let (sender, receiver) = oneshot::channel();
            lock_unpoisoned(&self.start_plans).push_back(receiver);
            sender
        }

        /// 为下一次 stop 安排一个由测试 channel 放行的确定性结果。
        fn enqueue_stop(&self) -> oneshot::Sender<StopPlan> {
            let (sender, receiver) = oneshot::channel();
            lock_unpoisoned(&self.stop_plans).push_back(receiver);
            sender
        }

        /// 在显式 prepare 内确定性暂停，以验证 duplicate/cancel/remove。
        fn enqueue_prepare(&self) -> oneshot::Sender<CallPlan> {
            let (sender, receiver) = oneshot::channel();
            lock_unpoisoned(&self.prepare_plans).push_back(receiver);
            sender
        }

        /// 令后续 stop 确定性失败，用于验证 failure carrier 的所有权归还。
        fn fail_stop(&self) {
            self.stop_fails
                .store(true, std::sync::atomic::Ordering::SeqCst);
        }

        /// 令后续 Tool 调用返回安全 Provider 失败，验证 Manager 不泄露其内部分类。
        fn fail_call(&self) {
            self.call_fails
                .store(true, std::sync::atomic::Ordering::SeqCst);
        }

        /// 为下一次 Tool 调用安排一个由测试 channel 放行的确定性结果。
        fn enqueue_call(&self) -> oneshot::Sender<CallPlan> {
            let (sender, receiver) = oneshot::channel();
            lock_unpoisoned(&self.call_plans).push_back(receiver);
            sender
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
            lease: WorkspaceLease,
            _action: CapabilityPrepareAction,
            _activity: &'a dyn CapabilityActivitySink,
        ) -> CapabilityFuture<'a, Result<CapabilityPrepareResult, CapabilityProviderError>>
        {
            self.prepares
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            lock_unpoisoned(&self.prepared_leases).push(lease);
            let plan = lock_unpoisoned(&self.prepare_plans).pop_front();
            Box::pin(async move {
                self.prepare_entered.notify_one();
                if let Some(plan) = plan {
                    if matches!(plan.await, Ok(CallPlan::Failure) | Err(_)) {
                        return Err(CapabilityProviderError {
                            code: CapabilityProviderErrorCode::OperationFailed,
                        });
                    }
                }
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
            let call_fails = self.call_fails.load(std::sync::atomic::Ordering::SeqCst);
            let plan = lock_unpoisoned(&self.call_plans).pop_front();
            let call_entered = Arc::clone(&self.call_entered);
            Box::pin(async move {
                self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let call_fails = if let Some(plan) = plan {
                    call_entered.notify_one();
                    matches!(plan.await.unwrap_or(CallPlan::Failure), CallPlan::Failure)
                } else {
                    call_fails
                };
                if call_fails {
                    return Err(CapabilityProviderError {
                        code: CapabilityProviderErrorCode::OperationFailed,
                    });
                }
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
            let plan = lock_unpoisoned(&self.stop_plans).pop_front();
            let stop_entered = Arc::clone(&self.stop_entered);
            let stopped_workspaces = Arc::clone(&self.stopped_workspaces);
            Box::pin(async move {
                let stop_fails = if let Some(plan) = plan {
                    stop_entered.notify_one();
                    matches!(plan.await.unwrap_or(StopPlan::Failure), StopPlan::Failure)
                } else {
                    stop_fails
                };
                if stop_fails {
                    return Err(CapabilityStopFailure {
                        runtime,
                        error: CapabilityProviderError {
                            code: CapabilityProviderErrorCode::OperationFailed,
                        },
                    });
                }
                lock_unpoisoned(&stopped_workspaces).push(runtime.workspace_id.clone());
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
        runtime_provider_with_capacity(provider_id, per_slot_concurrency, 2)
    }

    /// 创建指定并发度和容量上限的通用 workspace-scoped fake Provider。
    fn runtime_provider_with_capacity(
        provider_id: &str,
        per_slot_concurrency: usize,
        max_instances: usize,
    ) -> Arc<FakeProvider> {
        let mut provider_descriptor = descriptor(provider_id);
        provider_descriptor.runtime_policy.per_slot_concurrency = per_slot_concurrency;
        provider_descriptor.runtime_policy.max_instances = max_instances;
        Arc::new(FakeProvider::with_descriptor(provider_descriptor))
    }

    /// 创建指定 idle timeout 的 fake Provider，避免 timeout 测试依赖真实等待。
    fn runtime_provider_with_idle_timeout(
        provider_id: &str,
        per_slot_concurrency: usize,
        idle_timeout_ms: u64,
    ) -> Arc<FakeProvider> {
        let mut provider_descriptor = descriptor(provider_id);
        provider_descriptor.runtime_policy.per_slot_concurrency = per_slot_concurrency;
        provider_descriptor.runtime_policy.idle_timeout_ms = idle_timeout_ms;
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
            .slots
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
        assert_eq!(descriptor["stageDescriptors"][0]["displayName"], "Index");
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
                    cancellation: CancellationToken::new(),
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
            cancellation: CancellationToken::new(),
        };
        let value = serde_json::to_value(tool).unwrap();

        assert_eq!(value["toolName"], "fake_tool");
        assert_eq!(value["arguments"]["relativePath"], "src/lib.rs");
        assert!(value.get("root").is_none());
        assert!(value.get("canonicalRoot").is_none());
        assert!(value.get("cancellation").is_none());
    }

    #[tokio::test]
    /// 验证 Manager 在 guard 生命周期内只向指定 Provider 交付一次正确的 Tool 调用。
    async fn manager_call_acquires_checked_runtime_and_delegates_once() {
        let provider = runtime_provider("generic", 1);
        let manager = runtime_manager(Arc::clone(&provider));
        let workspace_lease = lease("workspace-a", 7);

        let result = manager
            .call(
                "generic",
                workspace_lease.clone(),
                WorkspaceToolCall {
                    tool_name: "fake_tool".into(),
                    arguments: json!({"relative_path":"src/lib.rs"}),
                    cancellation: CancellationToken::new(),
                },
            )
            .await
            .unwrap();

        assert_eq!(result.result, json!({"ok":true}));
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            lock_unpoisoned(&runtime_slot(&manager, "generic", &workspace_lease).state).in_flight,
            0
        );
    }

    #[tokio::test]
    /// 第三个 in-process Provider 只由 descriptor 选中，既不需要 Core 分支也不分配 RuntimeSlot。
    async fn manager_routes_third_in_process_provider_without_a_runtime_slot() {
        let mut third_descriptor = descriptor("third");
        third_descriptor.tool_names = vec!["third_tool".into()];
        third_descriptor.runtime_model = CapabilityRuntimeModel::InProcess;
        third_descriptor.runtime_policy.max_instances = 0;
        let third = Arc::new(FakeProvider::with_descriptor(third_descriptor));
        let manager = WorkspaceCapabilityManager::new(Arc::new(
            WorkspaceCapabilityRegistry::new(vec![as_provider(Arc::clone(&third))]).unwrap(),
        ));

        let result = manager
            .call_tool(
                lease("workspace-a", 7),
                WorkspaceToolCall {
                    tool_name: "third_tool".into(),
                    arguments: json!({}),
                    cancellation: CancellationToken::new(),
                },
            )
            .await
            .unwrap();

        assert_eq!(result.result, json!({"ok":true}));
        assert_eq!(third.calls.load(Ordering::SeqCst), 1);
        assert_eq!(third.starts.load(Ordering::SeqCst), 0);
        assert!(lock_unpoisoned(&manager.runtime_slots).slots.is_empty());
    }

    #[tokio::test]
    /// Source 的 in-process 与 Git 的 stateless command 均由 Descriptor 通用路由，绝不启动或保留 RuntimeSlot。
    async fn manager_routes_source_and_git_stateless_models_without_runtime_slots() {
        let mut source_descriptor = descriptor("source");
        source_descriptor.tool_names = vec!["source_tool".into()];
        source_descriptor.runtime_model = CapabilityRuntimeModel::InProcess;
        source_descriptor.runtime_policy.max_instances = 0;
        let source = Arc::new(FakeProvider::with_descriptor(source_descriptor));

        let mut git_descriptor = descriptor("git");
        git_descriptor.tool_names = vec!["git_tool".into()];
        git_descriptor.runtime_model = CapabilityRuntimeModel::StatelessCommand;
        git_descriptor.runtime_policy.max_instances = 0;
        let git = Arc::new(FakeProvider::with_descriptor(git_descriptor));

        let manager = WorkspaceCapabilityManager::new(Arc::new(
            WorkspaceCapabilityRegistry::new(vec![
                as_provider(Arc::clone(&source)),
                as_provider(Arc::clone(&git)),
            ])
            .unwrap(),
        ));
        for tool_name in ["source_tool", "git_tool"] {
            manager
                .call_tool(
                    lease("workspace-a", 7),
                    WorkspaceToolCall {
                        tool_name: tool_name.into(),
                        arguments: json!({}),
                        cancellation: CancellationToken::new(),
                    },
                )
                .await
                .unwrap();
        }

        assert_eq!(source.starts.load(Ordering::SeqCst), 0);
        assert_eq!(git.starts.load(Ordering::SeqCst), 0);
        assert!(lock_unpoisoned(&manager.runtime_slots).slots.is_empty());
    }

    #[tokio::test]
    /// 验证错误 Provider 或启动返回的任意错误 Runtime identity 都不会进入 Provider call。
    async fn manager_call_rejects_wrong_provider_or_runtime_identity_before_delegation() {
        let provider = runtime_provider("generic", 1);
        let manager = runtime_manager(Arc::clone(&provider));
        let workspace_lease = lease("workspace-a", 7);

        let unknown = manager
            .call(
                "unknown",
                workspace_lease.clone(),
                WorkspaceToolCall {
                    tool_name: "fake_tool".into(),
                    arguments: json!({}),
                    cancellation: CancellationToken::new(),
                },
            )
            .await
            .unwrap_err();
        assert_eq!(unknown.code, WorkspaceCapabilityErrorCode::NotFound);

        for plan in [
            StartPlan::WrongProvider,
            StartPlan::WrongWorkspace,
            StartPlan::WrongGeneration,
        ] {
            let sender = provider.enqueue_start();
            sender.send(plan).unwrap();
            let error = manager
                .call(
                    "generic",
                    workspace_lease.clone(),
                    WorkspaceToolCall {
                        tool_name: "fake_tool".into(),
                        arguments: json!({}),
                        cancellation: CancellationToken::new(),
                    },
                )
                .await
                .unwrap_err();
            assert_eq!(error.code, WorkspaceCapabilityErrorCode::ContractError);
        }
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    /// 验证 Provider 调用失败只映射为冻结的 Manager 错误，绝不向上层泄露 Provider 原因。
    async fn manager_call_maps_provider_failure_to_runtime_lost() {
        let provider = runtime_provider("generic", 1);
        provider.fail_call();
        let manager = runtime_manager(Arc::clone(&provider));

        let error = manager
            .call(
                "generic",
                lease("workspace-a", 7),
                WorkspaceToolCall {
                    tool_name: "fake_tool".into(),
                    arguments: json!({}),
                    cancellation: CancellationToken::new(),
                },
            )
            .await
            .unwrap_err();

        assert_eq!(error.code, WorkspaceCapabilityErrorCode::RuntimeLost);
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    /// 验证取消中的 Manager call 会释放 guard，后续 acquire 与 stop 均可继续推进。
    async fn cancelled_manager_call_releases_in_flight_for_following_acquire_and_stop() {
        let provider = runtime_provider_with_idle_timeout("generic", 1, 0);
        let manager = runtime_manager(Arc::clone(&provider));
        let workspace_lease = lease("workspace-a", 7);
        let _call = provider.enqueue_call();
        let calling_manager = Arc::clone(&manager);
        let calling_lease = workspace_lease.clone();
        let calling = tokio::spawn(async move {
            calling_manager
                .call(
                    "generic",
                    calling_lease,
                    WorkspaceToolCall {
                        tool_name: "fake_tool".into(),
                        arguments: json!({}),
                        cancellation: CancellationToken::new(),
                    },
                )
                .await
        });

        provider.call_entered.notified().await;
        let slot = runtime_slot(&manager, "generic", &workspace_lease);
        assert_eq!(lock_unpoisoned(&slot.state).in_flight, 1);
        calling.abort();
        assert!(calling.await.unwrap_err().is_cancelled());
        assert_eq!(lock_unpoisoned(&slot.state).in_flight, 0);

        drop(
            manager
                .acquire_runtime("generic", workspace_lease)
                .await
                .unwrap(),
        );
        manager.stop_idle_runtimes().await.unwrap();
        assert_eq!(provider.stops.load(Ordering::SeqCst), 1);
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
        let old_completion = slot.startup_completion();
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
        assert_eq!(lock_unpoisoned(&manager.runtime_slots).slots.len(), 2);
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

    #[tokio::test]
    /// 验证已 idle 到 Descriptor timeout 的 ready Slot 会停止，但不删除其 Registry/Slot 记录。
    async fn idle_timeout_stops_ready_slot_without_removing_its_slot() {
        let provider = runtime_provider_with_idle_timeout("generic", 1, 0);
        let manager = runtime_manager(Arc::clone(&provider));
        let workspace_lease = lease("workspace-a", 7);

        drop(
            manager
                .acquire_runtime("generic", workspace_lease.clone())
                .await
                .unwrap(),
        );
        manager.stop_idle_runtimes().await.unwrap();

        assert_eq!(provider.stops.load(Ordering::SeqCst), 1);
        assert_eq!(
            *lock_unpoisoned(&provider.stopped_workspaces),
            vec!["workspace-a"]
        );
        assert_eq!(
            lock_unpoisoned(&runtime_slot(&manager, "generic", &workspace_lease).state).lifecycle,
            CapabilityRuntimeState::Stopped
        );
        assert_eq!(lock_unpoisoned(&manager.runtime_slots).slots.len(), 1);
    }

    #[tokio::test]
    /// 验证 in-flight Slot 不会被 timeout 停止，guard 释放后重新扫描才可停止。
    async fn idle_timeout_defers_in_flight_slot_until_guard_release() {
        let provider = runtime_provider_with_idle_timeout("generic", 1, 0);
        let manager = runtime_manager(Arc::clone(&provider));
        let workspace_lease = lease("workspace-a", 7);
        let guard = manager
            .acquire_runtime("generic", workspace_lease.clone())
            .await
            .unwrap();

        manager.stop_idle_runtimes().await.unwrap();
        assert_eq!(provider.stops.load(Ordering::SeqCst), 0);
        assert_eq!(
            lock_unpoisoned(&runtime_slot(&manager, "generic", &workspace_lease).state).lifecycle,
            CapabilityRuntimeState::Ready
        );

        drop(guard);
        manager.stop_idle_runtimes().await.unwrap();
        assert_eq!(provider.stops.load(Ordering::SeqCst), 1);
        assert_eq!(
            lock_unpoisoned(&runtime_slot(&manager, "generic", &workspace_lease).state).lifecycle,
            CapabilityRuntimeState::Stopped
        );
    }

    #[tokio::test]
    /// 验证并发 idle stop 请求对同一 Slot 只调用一次 Provider stop。
    async fn concurrent_idle_stop_requests_are_single_flight_per_slot() {
        let provider = runtime_provider_with_idle_timeout("generic", 1, 0);
        let manager = runtime_manager(Arc::clone(&provider));
        let workspace_lease = lease("workspace-a", 7);
        drop(
            manager
                .acquire_runtime("generic", workspace_lease.clone())
                .await
                .unwrap(),
        );
        let stop = provider.enqueue_stop();
        let first_manager = Arc::clone(&manager);
        let first = tokio::spawn(async move { first_manager.stop_idle_runtimes().await });

        provider.stop_entered.notified().await;
        manager.stop_idle_runtimes().await.unwrap();
        assert_eq!(provider.stops.load(Ordering::SeqCst), 1);
        assert_eq!(
            lock_unpoisoned(&runtime_slot(&manager, "generic", &workspace_lease).state).lifecycle,
            CapabilityRuntimeState::Stopping
        );
        stop.send(StopPlan::Success).unwrap();
        first.await.unwrap().unwrap();
        assert_eq!(provider.stops.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    /// 验证容量满时会停止 idle LRU Slot，stop 成功后才启动 replacement。
    async fn capacity_evicts_idle_lru_before_starting_replacement() {
        let provider = runtime_provider_with_capacity("generic", 1, 1);
        let manager = runtime_manager(Arc::clone(&provider));
        let workspace_a = lease("workspace-a", 7);
        let workspace_b = lease("workspace-b", 7);

        drop(
            manager
                .acquire_runtime("generic", workspace_a.clone())
                .await
                .unwrap(),
        );
        let replacement = manager
            .acquire_runtime("generic", workspace_b.clone())
            .await
            .unwrap();

        assert_eq!(provider.starts.load(Ordering::SeqCst), 2);
        assert_eq!(provider.stops.load(Ordering::SeqCst), 1);
        assert_eq!(
            *lock_unpoisoned(&provider.stopped_workspaces),
            vec!["workspace-a"]
        );
        assert_eq!(
            lock_unpoisoned(&runtime_slot(&manager, "generic", &workspace_a).state).lifecycle,
            CapabilityRuntimeState::Stopped
        );
        assert_eq!(
            lock_unpoisoned(&runtime_slot(&manager, "generic", &workspace_b).state).lifecycle,
            CapabilityRuntimeState::Ready
        );
        drop(replacement);
    }

    #[tokio::test]
    /// 验证 busy 的较旧 Slot 不可驱逐，较新的 idle Slot 也不会抢占其容量。
    async fn capacity_skips_busy_lru_and_evicts_the_only_idle_slot() {
        let provider = runtime_provider_with_capacity("generic", 1, 2);
        let manager = runtime_manager(Arc::clone(&provider));
        let workspace_a = lease("workspace-a", 7);
        let workspace_b = lease("workspace-b", 7);
        let workspace_c = lease("workspace-c", 7);

        let busy = manager
            .acquire_runtime("generic", workspace_a.clone())
            .await
            .unwrap();
        drop(
            manager
                .acquire_runtime("generic", workspace_b.clone())
                .await
                .unwrap(),
        );
        let replacement = manager
            .acquire_runtime("generic", workspace_c.clone())
            .await
            .unwrap();

        assert_eq!(
            *lock_unpoisoned(&provider.stopped_workspaces),
            vec!["workspace-b"]
        );
        assert_eq!(
            lock_unpoisoned(&runtime_slot(&manager, "generic", &workspace_a).state).in_flight,
            1
        );
        assert_eq!(
            lock_unpoisoned(&runtime_slot(&manager, "generic", &workspace_c).state).lifecycle,
            CapabilityRuntimeState::Ready
        );
        drop(busy);
        drop(replacement);
    }

    #[tokio::test]
    /// 验证容量满但所有 Slot 均 busy 时不调用 stop 或 start replacement。
    async fn full_capacity_without_eligible_victim_returns_busy_without_side_effects() {
        let provider = runtime_provider_with_capacity("generic", 1, 1);
        let manager = runtime_manager(Arc::clone(&provider));
        let workspace_a = lease("workspace-a", 7);
        let workspace_b = lease("workspace-b", 7);
        let busy = manager
            .acquire_runtime("generic", workspace_a)
            .await
            .unwrap();

        let error = match manager.acquire_runtime("generic", workspace_b).await {
            Err(error) => error,
            Ok(_) => panic!("all busy Slots must not produce a replacement guard"),
        };
        assert_eq!(error.code, WorkspaceCapabilityErrorCode::Busy);
        assert_eq!(provider.starts.load(Ordering::SeqCst), 1);
        assert_eq!(provider.stops.load(Ordering::SeqCst), 0);
        drop(busy);
    }

    #[tokio::test]
    /// 验证 stop failure 将同一 handle 留在 error Slot，容量不释放且后续 acquire 不会清空它。
    async fn failed_lru_stop_retains_capacity_and_runtime_handle_without_replacement() {
        let provider = runtime_provider_with_capacity("generic", 1, 1);
        let manager = runtime_manager(Arc::clone(&provider));
        let workspace_a = lease("workspace-a", 7);
        let workspace_b = lease("workspace-b", 7);
        drop(
            manager
                .acquire_runtime("generic", workspace_a.clone())
                .await
                .unwrap(),
        );
        provider.fail_stop();

        let error = match manager
            .acquire_runtime("generic", workspace_b.clone())
            .await
        {
            Err(error) => error,
            Ok(_) => panic!("stop failure must not produce a replacement guard"),
        };
        assert_eq!(error.code, WorkspaceCapabilityErrorCode::StopFailed);
        assert_eq!(provider.starts.load(Ordering::SeqCst), 1);
        assert_eq!(provider.stops.load(Ordering::SeqCst), 1);
        let original = runtime_slot(&manager, "generic", &workspace_a);
        let original_state = lock_unpoisoned(&original.state);
        assert_eq!(original_state.lifecycle, CapabilityRuntimeState::Error);
        assert!(original_state.runtime.is_some());
        drop(original_state);
        assert_eq!(
            WorkspaceCapabilityManager::allocated_capacity(
                &lock_unpoisoned(&manager.runtime_slots),
                &WorkspaceCapabilityProviderId::new("generic"),
            ),
            1
        );
        assert_eq!(
            lock_unpoisoned(&runtime_slot(&manager, "generic", &workspace_b).state).lifecycle,
            CapabilityRuntimeState::Stopped
        );

        let retained_error = match manager.acquire_runtime("generic", workspace_a).await {
            Err(error) => error,
            Ok(_) => panic!("retained stop-failure handle must not be replaced by acquire"),
        };
        assert_eq!(retained_error.code, WorkspaceCapabilityErrorCode::Busy);
        assert_eq!(provider.starts.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    /// 验证 victim 进入 Stopping 后并发 acquire 立即 Busy，不能重新取得被转移的 handle。
    async fn stopping_victim_rejects_concurrent_acquire_without_reentry_window() {
        let provider = runtime_provider_with_capacity("generic", 1, 1);
        let manager = runtime_manager(Arc::clone(&provider));
        let workspace_a = lease("workspace-a", 7);
        let workspace_b = lease("workspace-b", 7);
        drop(
            manager
                .acquire_runtime("generic", workspace_a.clone())
                .await
                .unwrap(),
        );
        let stop = provider.enqueue_stop();
        let evicting_manager = Arc::clone(&manager);
        let evicting_lease = workspace_b.clone();
        let evicting = tokio::spawn(async move {
            evicting_manager
                .acquire_runtime("generic", evicting_lease)
                .await
        });

        provider.stop_entered.notified().await;
        let error = match manager
            .acquire_runtime("generic", workspace_a.clone())
            .await
        {
            Err(error) => error,
            Ok(_) => panic!("Stopping Slot must not re-enter acquire"),
        };
        assert_eq!(error.code, WorkspaceCapabilityErrorCode::Busy);
        assert_eq!(provider.starts.load(Ordering::SeqCst), 1);
        stop.send(StopPlan::Success).unwrap();
        drop(evicting.await.unwrap().unwrap());
        assert_eq!(provider.starts.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    /// 验证等待 eviction 的 caller 被取消后，后台 stop 仍返还 failure handle 并保留容量。
    async fn cancelled_eviction_waiter_does_not_drop_stop_failure_handle() {
        let provider = runtime_provider_with_capacity("generic", 1, 1);
        let manager = runtime_manager(Arc::clone(&provider));
        let workspace_a = lease("workspace-a", 7);
        let workspace_b = lease("workspace-b", 7);
        drop(
            manager
                .acquire_runtime("generic", workspace_a.clone())
                .await
                .unwrap(),
        );
        let stop = provider.enqueue_stop();
        let evicting_manager = Arc::clone(&manager);
        let evicting = tokio::spawn(async move {
            evicting_manager
                .acquire_runtime("generic", workspace_b)
                .await
        });

        provider.stop_entered.notified().await;
        evicting.abort();
        let join_error = match evicting.await {
            Err(error) => error,
            Ok(_) => panic!("cancelled eviction caller must not complete"),
        };
        assert!(join_error.is_cancelled());
        let original = runtime_slot(&manager, "generic", &workspace_a);
        let mut completion = Box::pin(original.stop_completion_published.notified());
        assert!(
            std::future::poll_fn(|context| match completion.as_mut().poll(context) {
                Poll::Pending => Poll::Ready(true),
                Poll::Ready(_) => Poll::Ready(false),
            })
            .await
        );
        stop.send(StopPlan::Failure).unwrap();
        completion.await;
        let state = lock_unpoisoned(&original.state);
        assert_eq!(state.lifecycle, CapabilityRuntimeState::Error);
        assert!(state.runtime.is_some());
        drop(state);

        let retained_error = match manager.acquire_runtime("generic", workspace_a).await {
            Err(error) => error,
            Ok(_) => panic!("cancelled caller must not allow retained handle replacement"),
        };
        assert_eq!(retained_error.code, WorkspaceCapabilityErrorCode::Busy);
        assert_eq!(provider.starts.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    /// 验证每次 admitted acquire 的单调序号决定 LRU，而非 HashMap 迭代顺序。
    async fn lru_eviction_order_is_deterministic_after_reuse() {
        let provider = runtime_provider_with_capacity("generic", 1, 2);
        let manager = runtime_manager(Arc::clone(&provider));
        let workspace_a = lease("workspace-a", 7);
        let workspace_b = lease("workspace-b", 7);
        let workspace_c = lease("workspace-c", 7);

        drop(
            manager
                .acquire_runtime("generic", workspace_a.clone())
                .await
                .unwrap(),
        );
        drop(
            manager
                .acquire_runtime("generic", workspace_b.clone())
                .await
                .unwrap(),
        );
        drop(
            manager
                .acquire_runtime("generic", workspace_a)
                .await
                .unwrap(),
        );
        drop(
            manager
                .acquire_runtime("generic", workspace_c)
                .await
                .unwrap(),
        );

        assert_eq!(
            *lock_unpoisoned(&provider.stopped_workspaces),
            vec!["workspace-b"]
        );
    }

    #[tokio::test]
    /// 验证 Remove 登记后拒绝同一 Workspace 的新 acquire，直到 Registry 删除阶段释放排他权。
    async fn workspace_remove_excludes_concurrent_acquire_until_its_guard_is_released() {
        let provider = runtime_provider_with_capacity("generic", 1, 1);
        let manager = runtime_manager(Arc::clone(&provider));
        let workspace = lease("workspace-a", 7);
        drop(
            manager
                .acquire_runtime("generic", workspace.clone())
                .await
                .unwrap(),
        );
        let stop = provider.enqueue_stop();
        let (release_tx, release_rx) = oneshot::channel();
        let (done_tx, done_rx) = oneshot::channel();
        let remove_manager = Arc::clone(&manager);
        let remove_workspace = workspace.clone();
        tokio::spawn(async move {
            let removal = remove_manager
                .begin_workspace_remove(&remove_workspace)
                .await
                .unwrap();
            let _ = done_tx.send(());
            let _ = release_rx.await;
            drop(removal);
        });

        provider.stop_entered.notified().await;
        let error = match manager.acquire_runtime("generic", workspace.clone()).await {
            Err(error) => error,
            Ok(_) => panic!("removing Workspace must reject acquire"),
        };
        assert_eq!(error.code, WorkspaceCapabilityErrorCode::Busy);
        assert_eq!(provider.stops.load(Ordering::SeqCst), 1);
        stop.send(StopPlan::Success).unwrap();
        done_rx.await.unwrap();
        release_tx.send(()).unwrap();
    }

    #[tokio::test]
    /// 验证 starting、in-flight 与 pending acquire 都会让 Remove fail closed。
    async fn workspace_remove_rejects_starting_and_live_runtime_owners() {
        let provider = runtime_provider_with_capacity("generic", 1, 1);
        let manager = runtime_manager(Arc::clone(&provider));
        let workspace = lease("workspace-a", 7);
        let start = provider.enqueue_start();
        let starting_manager = Arc::clone(&manager);
        let starting_workspace = workspace.clone();
        let starting = tokio::spawn(async move {
            starting_manager
                .acquire_runtime("generic", starting_workspace)
                .await
        });
        provider.start_entered.notified().await;
        let starting_error = match manager.begin_workspace_remove(&workspace).await {
            Err(error) => error,
            Ok(_) => panic!("starting Workspace must block Remove"),
        };
        assert_eq!(starting_error.code, WorkspaceCapabilityErrorCode::Busy);
        start.send(StartPlan::Success).unwrap();
        let guard = starting.await.unwrap().unwrap();
        let in_flight_error = match manager.begin_workspace_remove(&workspace).await {
            Err(error) => error,
            Ok(_) => panic!("in-flight Runtime must block Remove"),
        };
        assert_eq!(in_flight_error.code, WorkspaceCapabilityErrorCode::Busy);
        drop(guard);
        drop(manager.begin_workspace_remove(&workspace).await.unwrap());
    }

    #[tokio::test]
    /// 验证 Remove stop failure 保留同一 handle，并在清除 removing 后仍拒绝替换 acquire。
    async fn workspace_remove_stop_failure_retains_runtime_handle_and_clears_exclusion() {
        let provider = runtime_provider_with_capacity("generic", 1, 1);
        let manager = runtime_manager(Arc::clone(&provider));
        let workspace = lease("workspace-a", 7);
        drop(
            manager
                .acquire_runtime("generic", workspace.clone())
                .await
                .unwrap(),
        );
        provider.fail_stop();

        let error = match manager.begin_workspace_remove(&workspace).await {
            Err(error) => error,
            Ok(_) => panic!("stop failure must preserve removal ownership"),
        };
        assert_eq!(error.code, WorkspaceCapabilityErrorCode::StopFailed);
        let slot = runtime_slot(&manager, "generic", &workspace);
        let state = lock_unpoisoned(&slot.state);
        assert_eq!(state.lifecycle, CapabilityRuntimeState::Error);
        assert!(state.runtime.is_some());
        drop(state);
        let acquire_error = match manager.acquire_runtime("generic", workspace).await {
            Err(error) => error,
            Ok(_) => panic!("retained handle must reject replacement acquire"),
        };
        assert_eq!(acquire_error.code, WorkspaceCapabilityErrorCode::Busy);
    }

    #[tokio::test]
    /// 验证 stop 超时只返回有界失败；后台 single-flight 继续持有 handle，随后仍可安全收敛。
    async fn workspace_remove_stop_timeout_keeps_single_flight_handle_ownership() {
        let provider = runtime_provider_with_capacity("generic", 1, 1);
        let manager = runtime_manager(Arc::clone(&provider));
        let workspace = lease("workspace-a", 7);
        drop(
            manager
                .acquire_runtime("generic", workspace.clone())
                .await
                .unwrap(),
        );
        let held_stop = provider.enqueue_stop();
        let error = match manager.begin_workspace_remove(&workspace).await {
            Err(error) => error,
            Ok(_) => panic!("unresolved stop must return a bounded failure"),
        };
        assert_eq!(error.code, WorkspaceCapabilityErrorCode::StopFailed);
        let slot = runtime_slot(&manager, "generic", &workspace);
        let state = lock_unpoisoned(&slot.state);
        assert_eq!(state.lifecycle, CapabilityRuntimeState::Stopping);
        assert!(state.runtime.is_none());
        drop(state);
        let acquire_error = match manager.acquire_runtime("generic", workspace.clone()).await {
            Err(error) => error,
            Ok(_) => panic!("timed-out stop must retain its admission ownership"),
        };
        assert_eq!(acquire_error.code, WorkspaceCapabilityErrorCode::Busy);

        let completion = slot.stop_completion_published.notified();
        held_stop.send(StopPlan::Success).unwrap();
        completion.await;
        let state = lock_unpoisoned(&slot.state);
        assert_eq!(state.lifecycle, CapabilityRuntimeState::Stopped);
        assert!(state.runtime.is_none());
        assert_eq!(provider.stops.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    /// 验证 eviction、Remove 与 idle sweep 同时遇到同一 Slot 时只执行一次 Provider stop。
    async fn remove_eviction_and_idle_stop_share_one_slot_stop_flight() {
        let provider = runtime_provider_with_capacity("generic", 1, 1);
        let manager = runtime_manager(Arc::clone(&provider));
        let workspace_a = lease("workspace-a", 7);
        let workspace_b = lease("workspace-b", 7);
        drop(
            manager
                .acquire_runtime("generic", workspace_a.clone())
                .await
                .unwrap(),
        );
        let stop = provider.enqueue_stop();
        let evict_manager = Arc::clone(&manager);
        let evicting =
            tokio::spawn(
                async move { evict_manager.acquire_runtime("generic", workspace_b).await },
            );
        provider.stop_entered.notified().await;
        let remove_manager = Arc::clone(&manager);
        let remove_workspace = workspace_a.clone();
        let removing = tokio::spawn(async move {
            let removal = remove_manager
                .begin_workspace_remove(&remove_workspace)
                .await
                .unwrap();
            drop(removal);
        });
        manager.stop_idle_runtimes().await.unwrap();
        assert_eq!(provider.stops.load(Ordering::SeqCst), 1);
        stop.send(StopPlan::Success).unwrap();
        drop(evicting.await.unwrap().unwrap());
        removing.await.unwrap();
        assert_eq!(provider.stops.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    /// 验证 Host shutdown 停止全部 live 与 retained-handle Slot，且不会遗留 fake Runtime ownership。
    async fn host_shutdown_drains_live_and_retained_handles_without_orphans() {
        let provider = runtime_provider_with_capacity("generic", 1, 2);
        let manager = runtime_manager(Arc::clone(&provider));
        let workspace_a = lease("workspace-a", 7);
        let workspace_b = lease("workspace-b", 7);
        drop(
            manager
                .acquire_runtime("generic", workspace_a.clone())
                .await
                .unwrap(),
        );
        drop(
            manager
                .acquire_runtime("generic", workspace_b.clone())
                .await
                .unwrap(),
        );
        let failed_stop = provider.enqueue_stop();
        let failed_manager = Arc::clone(&manager);
        let failed_workspace = workspace_a.clone();
        let failed_remove = tokio::spawn(async move {
            failed_manager
                .begin_workspace_remove(&failed_workspace)
                .await
                .map(|_| ())
        });
        provider.stop_entered.notified().await;
        failed_stop.send(StopPlan::Failure).unwrap();
        assert_eq!(
            failed_remove.await.unwrap().unwrap_err().code,
            WorkspaceCapabilityErrorCode::StopFailed
        );

        manager.shutdown_runtimes().await.unwrap();
        assert_eq!(provider.stops.load(Ordering::SeqCst), 3);
        let stopped = lock_unpoisoned(&provider.stopped_workspaces).clone();
        assert!(stopped.contains(&"workspace-a".into()));
        assert!(stopped.contains(&"workspace-b".into()));
        for workspace in [&workspace_a, &workspace_b] {
            let slot = runtime_slot(&manager, "generic", workspace);
            let state = lock_unpoisoned(&slot.state);
            assert_eq!(state.lifecycle, CapabilityRuntimeState::Stopped);
            assert!(state.runtime.is_none());
        }
        let acquire_error = match manager.acquire_runtime("generic", workspace_a).await {
            Err(error) => error,
            Ok(_) => panic!("shutdown Manager must reject new acquire"),
        };
        assert_eq!(acquire_error.code, WorkspaceCapabilityErrorCode::Busy);
    }

    #[tokio::test]
    /// 验证 Remove stop failure 不会删除 Registry entry，且 fake handle 仍由原 Slot 保留。
    async fn coordinated_remove_preserves_registry_entry_and_handle_after_stop_failure() {
        use crate::{
            agent::{product::AgentProductService, store::StateStore},
            config::{self, AppPaths, ManagerConfig, Workspace},
            serena::SupervisorState,
            workspace_registry::WorkspaceRegistry,
            workspace_resolver::WorkspaceResolver,
        };

        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("workspace");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("marker.txt"), b"preserve").unwrap();
        let paths = AppPaths {
            runtime_directory: directory.path().join("runtime"),
            config_file: directory.path().join("config.json"),
            log_directory: directory.path().join("logs"),
            app_log: directory.path().join("logs/app.log"),
            serena_log: directory.path().join("logs/serena.log"),
        };
        let workspace = Workspace {
            id: "workspace".into(),
            name: "Workspace".into(),
            root: std::fs::canonicalize(&root).unwrap(),
            generation: 7,
        };
        config::save(
            &paths.config_file,
            &ManagerConfig {
                workspace_registry_revision: 1,
                workspaces: vec![workspace.clone()],
                ..ManagerConfig::default()
            },
        )
        .unwrap();
        let provider = runtime_provider_with_capacity("generic", 1, 1);
        let manager = runtime_manager(Arc::clone(&provider));
        let mut supervisor = SupervisorState::new(paths.clone()).unwrap();
        supervisor.replace_workspace_capability_manager_for_test(Arc::clone(&manager));
        let lease = WorkspaceResolver::new(&supervisor)
            .resolve(&workspace.id)
            .unwrap();
        drop(
            manager
                .acquire_runtime("generic", lease.clone())
                .await
                .unwrap(),
        );
        provider.fail_stop();
        let product = AgentProductService::new(
            StateStore::open(directory.path().join("agent-state"))
                .await
                .unwrap(),
        );

        assert_eq!(
            supervisor
                .remove_workspace_coordinated(&product, &workspace.id)
                .await,
            Err("WORKSPACE_CAPABILITY_STOP_FAILED".into())
        );
        assert_eq!(
            WorkspaceRegistry::new(&supervisor).get(&workspace.id),
            Ok(workspace)
        );
        assert_eq!(std::fs::read(root.join("marker.txt")).unwrap(), b"preserve");
        let slot = runtime_slot(&manager, "generic", &lease);
        let state = lock_unpoisoned(&slot.state);
        assert_eq!(state.lifecycle, CapabilityRuntimeState::Error);
        assert!(state.runtime.is_some());
    }

    #[tokio::test]
    /// 验证 Remove admission 已建立且 stop 被阻塞时，Agent Start/Claim 与 capability acquire 都不能越过。
    async fn coordinated_remove_admission_blocks_agent_claim_and_capability_acquire() {
        use crate::{
            agent::{product::AgentProductService, store::StateStore},
            config::{self, AppPaths, ManagerConfig, Workspace},
            serena::{SupervisorState, WorkspaceStartCreation},
            workspace_registry::{WORKSPACE_IN_USE, WorkspaceRegistry},
            workspace_resolver::WorkspaceResolver,
        };

        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("workspace");
        std::fs::create_dir_all(&root).unwrap();
        let paths = AppPaths {
            runtime_directory: directory.path().join("runtime"),
            config_file: directory.path().join("config.json"),
            log_directory: directory.path().join("logs"),
            app_log: directory.path().join("logs/app.log"),
            serena_log: directory.path().join("logs/serena.log"),
        };
        let workspace = Workspace {
            id: "workspace".into(),
            name: "Workspace".into(),
            root: std::fs::canonicalize(&root).unwrap(),
            generation: 7,
        };
        config::save(
            &paths.config_file,
            &ManagerConfig {
                workspace_registry_revision: 1,
                workspaces: vec![workspace.clone()],
                ..ManagerConfig::default()
            },
        )
        .unwrap();
        let provider = runtime_provider_with_capacity("generic", 1, 1);
        let manager = runtime_manager(Arc::clone(&provider));
        let mut supervisor = SupervisorState::new(paths).unwrap();
        supervisor.replace_workspace_capability_manager_for_test(Arc::clone(&manager));
        let supervisor = Arc::new(supervisor);
        let lease = WorkspaceResolver::new(supervisor.as_ref())
            .resolve(&workspace.id)
            .unwrap();
        drop(
            manager
                .acquire_runtime("generic", lease.clone())
                .await
                .unwrap(),
        );
        let store = StateStore::open(directory.path().join("agent-state"))
            .await
            .unwrap();
        let product = Arc::new(AgentProductService::new(store.clone()));
        let held_stop = provider.enqueue_stop();
        let remove_supervisor = Arc::clone(&supervisor);
        let remove_product = Arc::clone(&product);
        let remove_id = workspace.id.clone();
        let removing = tokio::spawn(async move {
            remove_supervisor
                .remove_workspace_coordinated(remove_product.as_ref(), &remove_id)
                .await
        });

        provider.stop_entered.notified().await;
        assert_eq!(
            supervisor.create_workspace_start(
                &store,
                WorkspaceStartCreation {
                    execution_id: "execution".into(),
                    agent_id: "agent".into(),
                    request_key: "request".into(),
                    prompt: "prompt".into(),
                    workspace_id: workspace.id.clone(),
                    work: None,
                    now: 1,
                },
            ),
            Err(WORKSPACE_IN_USE.into())
        );
        assert!(
            store
                .workspace_claim(lease.canonical_root.to_string_lossy().into_owned())
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(
            supervisor
                .resolve_workspace_write_guard(&workspace.id)
                .map(|_| ()),
            Err(WORKSPACE_IN_USE.into())
        );
        let acquire_error = match manager.acquire_runtime("generic", lease.clone()).await {
            Err(error) => error,
            Ok(_) => panic!("Remove admission must reject capability acquire"),
        };
        assert_eq!(acquire_error.code, WorkspaceCapabilityErrorCode::Busy);

        held_stop.send(StopPlan::Success).unwrap();
        assert_eq!(removing.await.unwrap(), Ok(workspace.clone()));
        assert_eq!(
            WorkspaceRegistry::new(supervisor.as_ref()).get(&workspace.id),
            Err(crate::workspace_registry::WORKSPACE_NOT_FOUND.into())
        );
    }
}
