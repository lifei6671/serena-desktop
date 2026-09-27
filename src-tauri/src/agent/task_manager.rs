//! Internal execution and exact-id cancellation entry points. No scheduler.
#[cfg(all(test, any(windows, target_os = "macos")))]
use super::codex::provider::CodexProvider;
use super::{
    codebuddy::provider::{
        register_codebuddy_provider, register_codebuddy_provider_with_discovery,
    },
    codex::provider::register_codex_provider_with_discovery,
    coordinator::WorkspaceExecutionCoordinator,
    execution::{CreateExecutionInput, ExecutionMode, canonicalize_request},
    notification::{
        AgentTerminalNotifier, AgentTerminalStatus, noop_agent_terminal_notifier, task_title,
    },
    provider::{
        ProviderCancelContext, ProviderError, ProviderErrorCode, ProviderExecutionContext,
        ProviderId, ProviderStartupContext,
        control::{ProviderAdmissionCapability, ProviderAdmissionPolicy},
        port::{
            ProviderAcceptanceSink, ProviderContinuationContext, ProviderContinuationDecision,
            ProviderExecutionFailure, ProviderReconcileItem,
        },
        registry::{ProviderHealth, ProviderRegistry},
    },
    store::{
        StateStore,
        transactions::{
            CreateOutcome,
            product::{ContinuationCandidate, ContinuationPreflight},
        },
    },
    telemetry_projector::ExecutionTelemetryProjector,
};
use crate::config::AgentProviderSettings;
use crate::serena::WorkspaceStartCreation;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

mod automatic_recovery;
#[cfg(any(windows, target_os = "macos"))]
pub mod recovery;

#[derive(Clone)]
pub struct AgentTaskManager {
    store: StateStore,
    executable: PathBuf,
    pub(crate) backend_error: Option<String>,
    owner: String,
    pub(crate) runtime_pool: std::sync::Arc<super::codex::pool::CodexRuntimePool>,
    registry: Arc<Mutex<Option<Arc<ProviderRegistry>>>>,
    admission: ProviderAdmissionPolicy,
    auto_recovery: Arc<AutoRecoveryWorker>,
    terminal_notifier: Arc<dyn AgentTerminalNotifier>,
    #[cfg(test)]
    pub(crate) test_handoff: Option<std::sync::Arc<(tokio::sync::Notify, tokio::sync::Notify)>>,
    #[cfg(test)]
    pub(crate) test_client: Option<(std::sync::Arc<super::codex::app_server::Client>, PathBuf)>,
}

/// Compatibility probe 只携带当前 Manager 已有的正式 ownership authority。
#[derive(Clone)]
pub(crate) struct ProbeContext {
    store: StateStore,
    owner: String,
    runtime_pool: Arc<super::codex::pool::CodexRuntimePool>,
}

impl ProbeContext {
    /// 从已有 Host authority 构造 crate-private probe context，不创建 Store 或 Pool。
    pub(crate) fn from_existing(
        store: StateStore,
        owner: String,
        runtime_pool: Arc<super::codex::pool::CodexRuntimePool>,
    ) -> Self {
        Self {
            store,
            owner,
            runtime_pool,
        }
    }

    /// 返回正式 StateStore clone，所有 probe Runtime 都写入该 Store。
    pub(crate) fn store(&self) -> StateStore {
        self.store.clone()
    }

    /// 返回当前 Host owner，probe 不使用独立身份。
    pub(crate) fn owner(&self) -> &str {
        &self.owner
    }

    /// 返回已有全局 Runtime Pool，cleanup failure 交由原 quarantine 管理。
    pub(crate) fn runtime_pool(&self) -> Arc<super::codex::pool::CodexRuntimePool> {
        self.runtime_pool.clone()
    }
}

/// Host 唯一持有恢复消息接收端；Clone 的 Manager 仅共享同步投递端。
struct AutoRecoveryWorker {
    sender: tokio::sync::mpsc::UnboundedSender<String>,
    receiver: Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<String>>>,
    join: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl Default for AutoRecoveryWorker {
    fn default() -> Self {
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        Self {
            sender,
            receiver: Mutex::new(Some(receiver)),
            join: Mutex::new(None),
        }
    }
}

enum AcceptanceState {
    Pending(Option<tokio::sync::oneshot::Sender<Result<(), String>>>),
    Accepted,
    Closed,
}

struct HostAcceptanceSink(Mutex<AcceptanceState>);

impl HostAcceptanceSink {
    fn new(receipt: Option<tokio::sync::oneshot::Sender<Result<(), String>>>) -> Self {
        Self(Mutex::new(AcceptanceState::Pending(receipt)))
    }

    fn is_accepted(&self) -> bool {
        matches!(*self.0.lock().unwrap(), AcceptanceState::Accepted)
    }

    fn reject(&self, error: String) {
        let receipt = {
            let mut state = self.0.lock().unwrap();
            match std::mem::replace(&mut *state, AcceptanceState::Closed) {
                AcceptanceState::Pending(receipt) => receipt,
                AcceptanceState::Accepted => {
                    *state = AcceptanceState::Accepted;
                    None
                }
                AcceptanceState::Closed => None,
            }
        };
        if let Some(receipt) = receipt {
            let _ = receipt.send(Err(error));
        }
    }
}

impl ProviderAcceptanceSink for HostAcceptanceSink {
    fn accepted(&self) {
        let receipt = {
            let mut state = self.0.lock().unwrap();
            match std::mem::replace(&mut *state, AcceptanceState::Accepted) {
                AcceptanceState::Pending(receipt) => receipt,
                AcceptanceState::Accepted => {
                    *state = AcceptanceState::Accepted;
                    None
                }
                AcceptanceState::Closed => {
                    *state = AcceptanceState::Closed;
                    None
                }
            }
        };
        if let Some(receipt) = receipt {
            let _ = receipt.send(Ok(()));
        }
    }
}

fn provider_error_code(code: ProviderErrorCode) -> &'static str {
    match code {
        ProviderErrorCode::AgentProviderNotFound => "AGENT_PROVIDER_NOT_FOUND",
        ProviderErrorCode::AgentProviderDisabled => "AGENT_PROVIDER_DISABLED",
        ProviderErrorCode::AgentProviderUnavailable => "AGENT_PROVIDER_UNAVAILABLE",
        ProviderErrorCode::AgentProviderCapabilityUnsupported => {
            "AGENT_PROVIDER_CAPABILITY_UNSUPPORTED"
        }
        ProviderErrorCode::AgentProviderContractError => "AGENT_PROVIDER_CONTRACT_ERROR",
        ProviderErrorCode::AgentProviderOperationFailed => "AGENT_PROVIDER_OPERATION_FAILED",
    }
}

fn provider_failure(error: ProviderError) -> ProviderExecutionFailure {
    provider_error_code(error.code).to_string().into()
}

fn provider_id(value: String) -> Result<ProviderId, ProviderExecutionFailure> {
    ProviderId::new(value)
        .map_err(|_| ProviderExecutionFailure::State("AGENT_PROVIDER_CONTRACT_ERROR".to_string()))
}

fn continuation_provider_error(error: ProviderError) -> String {
    match error.code {
        ProviderErrorCode::AgentProviderNotFound
        | ProviderErrorCode::AgentProviderCapabilityUnsupported => {
            "AGENT_CONTINUE_NOT_ALLOWED".into()
        }
        code => provider_error_code(code).into(),
    }
}

impl AgentTaskManager {
    async fn validate_continuation_candidate(
        &self,
        candidate: &ContinuationCandidate,
    ) -> Result<(), String> {
        self.validate_continuation_source(&candidate.source_execution_id, &candidate.provider_id)
            .await
    }

    async fn validate_continuation_source(
        &self,
        source_execution_id: &str,
        provider_value: &str,
    ) -> Result<(), String> {
        let provider_id = ProviderId::new(provider_value.into())
            .map_err(|_| "AGENT_CONTINUE_NOT_ALLOWED".to_string())?;
        let provider = self
            .admit_provider(&provider_id, ProviderAdmissionCapability::Continue)
            .map_err(continuation_provider_error)?;
        match provider
            .validate_continuation(ProviderContinuationContext {
                source_execution_id: source_execution_id.into(),
            })
            .await
            .map_err(continuation_provider_error)?
        {
            ProviderContinuationDecision::Eligible => Ok(()),
            ProviderContinuationDecision::Ineligible => Err("AGENT_CONTINUE_NOT_ALLOWED".into()),
        }
    }

    pub(crate) async fn can_continue(
        &self,
        source_execution_id: String,
        provider_id: String,
    ) -> bool {
        self.validate_continuation_source(&source_execution_id, &provider_id)
            .await
            .is_ok()
    }

    pub async fn cancel(
        &self,
        execution_id: &str,
    ) -> Result<super::store::ExecutionRecord, String> {
        let row = self
            .store
            .execution(execution_id.into())
            .await?
            .ok_or("EXECUTION_NOT_FOUND")?;
        let provider_id = ProviderId::new(row.provider)
            .map_err(|_| "AGENT_PROVIDER_CONTRACT_ERROR".to_string())?;
        let provider = self
            .registry()
            .map_err(|error| provider_error_code(error.code).to_string())?
            .get_registered(&provider_id)
            .map_err(|error| provider_error_code(error.code).to_string())?;
        if !provider.capabilities().can_cancel {
            // 生产初始化已确认后端不可用时保留明确诊断；普通能力缺失仍沿用既有错误码。
            if self
                .backend_error
                .as_deref()
                .is_some_and(|error| error.starts_with("BACKEND_UNAVAILABLE"))
            {
                return Err("AGENT_PROVIDER_UNAVAILABLE".into());
            }
            return Err("AGENT_PROVIDER_CAPABILITY_UNSUPPORTED".into());
        }
        provider
            .cancel(ProviderCancelContext {
                execution_id: execution_id.into(),
            })
            .await
            .map_err(|error| provider_error_code(error.code).to_string())?;
        self.store
            .execution(execution_id.into())
            .await?
            .ok_or_else(|| "EXECUTION_NOT_FOUND".into())
    }
    pub fn new(store: StateStore, executable: PathBuf) -> Self {
        Self::new_with_terminal_notifier(store, executable, noop_agent_terminal_notifier())
    }

    /// 注入产品层终态副作用；默认构造函数保持无副作用以兼容既有调用方。
    pub(crate) fn new_with_terminal_notifier(
        store: StateStore,
        executable: PathBuf,
        terminal_notifier: Arc<dyn AgentTerminalNotifier>,
    ) -> Self {
        Self::new_with_terminal_notifier_and_provider_settings(
            store,
            executable,
            terminal_notifier,
            AgentProviderSettings::default(),
        )
    }

    /// 注入已经验证的本地 Provider 设置；该快照不新增远程或运行时 mutation authority。
    pub(crate) fn new_with_terminal_notifier_and_provider_settings(
        store: StateStore,
        executable: PathBuf,
        terminal_notifier: Arc<dyn AgentTerminalNotifier>,
        provider_settings: impl Into<ProviderAdmissionPolicy>,
    ) -> Self {
        Self {
            store,
            executable,
            backend_error: None,
            owner: Self::id("host"),
            runtime_pool: Default::default(),
            registry: Default::default(),
            admission: provider_settings.into(),
            auto_recovery: Default::default(),
            terminal_notifier,
            #[cfg(test)]
            test_handoff: None,
            #[cfg(test)]
            test_client: None,
        }
    }

    /// 从同一 Manager 导出 probe authority，不暴露 PID/PGID 等进程细节。
    pub(crate) fn probe_context(&self) -> ProbeContext {
        ProbeContext::from_existing(
            self.store.clone(),
            self.owner.clone(),
            self.runtime_pool.clone(),
        )
    }

    /// 使用当前 Manager 的正式 authority 执行平台 discovery。
    pub(crate) async fn discover_backend(&self) -> Result<PathBuf, String> {
        super::codex::discover(self.probe_context()).await
    }

    /// 在 Provider 发布前安装 discovery 结果，不改变 dispatch/state graph。
    pub(crate) fn install_backend_resolution(&mut self, resolution: Result<PathBuf, String>) {
        match resolution {
            Ok(executable) => {
                self.executable = executable;
                self.backend_error = None;
            }
            Err(error) => {
                self.executable = PathBuf::new();
                self.backend_error = Some(error);
            }
        }
        *self.registry.lock().unwrap() = None;
    }

    /// macOS Desktop 发布时延后 backend 探测，首次 Provider execute 仍从空 executable 发现。
    #[cfg(target_os = "macos")]
    pub(crate) fn defer_backend_resolution(&mut self) {
        self.executable = PathBuf::new();
        self.backend_error = None;
        *self.registry.lock().unwrap() = None;
    }

    /// 在已存在 Tokio Runtime 的 Host 发布屏障后启动唯一恢复 worker。
    pub(crate) fn start_auto_recovery_worker(&self) -> bool {
        if self.runtime_pool.stop.is_cancelled() {
            return false;
        }
        let receiver = self.auto_recovery.receiver.lock().unwrap().take();
        let Some(mut receiver) = receiver else {
            return false;
        };
        let manager = self.clone();
        let join = tokio::spawn(async move {
            loop {
                tokio::select! {
                    biased;
                    _ = manager.runtime_pool.stop.cancelled() => break,
                    Some(execution_id) = receiver.recv() => {
                        if manager.schedule_auto_recovery(&execution_id).await.is_err() {
                            // 只记录固定安全诊断，避免将 Provider 原始错误写入公共边界。
                            eprintln!("AGENT_AUTO_RECOVERY_SCHEDULE_FAILED");
                        }
                    }
                    else => break,
                }
            }
        });
        *self.auto_recovery.join.lock().unwrap() = Some(join);
        true
    }

    /// dispatch 终态 hook 只投递 ID；它不读 Store、不创建 child，也不会 await。
    fn notify_auto_recovery(&self, execution_id: &str) {
        let _ = self.auto_recovery.sender.send(execution_id.into());
    }

    /// Runtime shutdown 已发出取消后等待 Host-owned worker 退出。
    pub(crate) async fn wait_auto_recovery_worker(&self) {
        let join = self.auto_recovery.join.lock().unwrap().take();
        if let Some(join) = join {
            let _ = join.await;
        }
    }

    #[cfg(test)]
    pub(crate) async fn wait_for_auto_recovery_worker_for_test(&self) {
        self.wait_auto_recovery_worker().await;
    }

    #[cfg(test)]
    pub(crate) fn notify_auto_recovery_for_test(&self, execution_id: &str) {
        self.notify_auto_recovery(execution_id);
    }
    fn build_registry(&self) -> Result<ProviderRegistry, ProviderError> {
        let mut registry = ProviderRegistry::new();
        let discovery = self
            .backend_error
            .clone()
            .map_or_else(|| Ok(self.executable.clone()), Err);
        register_codex_provider_with_discovery(
            &mut registry,
            self.store.clone(),
            self.owner.clone(),
            self.runtime_pool.clone(),
            discovery,
        )?;
        // CodeBuddy discovery 失败只影响其自身 health，不能阻断 Desktop 或 Codex 注册。
        register_codebuddy_provider(&mut registry)?;
        Ok(registry)
    }
    /// Product 只读复用唯一 Registry；Registry 初始化与派发语义保持原样。
    pub(crate) fn registry(&self) -> Result<Arc<ProviderRegistry>, ProviderError> {
        let mut current = self.registry.lock().unwrap();
        if let Some(registry) = current.as_ref() {
            return Ok(registry.clone());
        }
        let registry = Arc::new(self.build_registry()?);
        *current = Some(registry.clone());
        Ok(registry)
    }

    /// 只重新执行 admission discovery，不创建 Execution、Session 或 Agent Runtime。
    /// macOS 复用受管 CLI probe 的 ownership 记录；只导出 schema，不启动 app-server。
    pub(crate) async fn refresh_provider_health(
        &self,
        id: ProviderId,
    ) -> Result<super::provider::registry::ProviderHealth, String> {
        let registry = self
            .registry()
            .map_err(|e| provider_error_code(e.code).to_string())?;
        registry
            .get_registered(&id)
            .map_err(|e| provider_error_code(e.code).to_string())?;
        let mut refreshed = ProviderRegistry::new();
        match id.as_str() {
            "codex" => register_codex_provider_with_discovery(
                &mut refreshed,
                self.store.clone(),
                self.owner.clone(),
                self.runtime_pool.clone(),
                self.discover_backend().await,
            ),
            // CodeBuddy refresh 只重复无进程 discovery，不创建 ACP 或 Runtime。
            "codebuddy" => register_codebuddy_provider_with_discovery(
                &mut refreshed,
                super::codebuddy::discover(),
            ),
            _ => {
                return Err("AGENT_PROVIDER_CAPABILITY_UNSUPPORTED".into());
            }
        }
        .map_err(|e| provider_error_code(e.code).to_string())?;
        let health = refreshed
            .health(&id)
            .map_err(|e| provider_error_code(e.code).to_string())?;
        let provider = refreshed
            .get_registered(&id)
            .map_err(|e| provider_error_code(e.code).to_string())?;
        let mut current = self.registry.lock().unwrap();
        let mut next = current
            .as_ref()
            .expect("registry initialized")
            .as_ref()
            .clone();
        next.replace_registered(provider, health)
            .map_err(|e| provider_error_code(e.code).to_string())?;
        *current = Some(Arc::new(next));
        Ok(health)
    }

    /// 使用单一 policy owner 解析新工作，避免调用方自行重排 enabled/health/capability。
    fn admit_provider(
        &self,
        provider_id: &ProviderId,
        capability: ProviderAdmissionCapability,
    ) -> Result<Arc<dyn super::provider::port::AgentProvider>, ProviderError> {
        let registry = self.registry()?;
        self.admission
            .admit(registry.as_ref(), provider_id, capability)
    }

    /// Start 创建前只拒绝未注册或 disabled；health/capability 仍在 dispatch 前判定。
    fn ensure_provider_enabled(&self, provider_id: &ProviderId) -> Result<(), ProviderError> {
        let registry = self.registry()?;
        self.admission
            .registered_enabled(registry.as_ref(), provider_id)
            .map(|_| ())
    }

    /// Product Start 在创建 Execution 与 Claim 前完成 registered→enabled 前缀。
    #[cfg(test)]
    fn ensure_product_start_enabled(&self) -> Result<(), super::product::ProductError> {
        let provider_id =
            ProviderId::new("codex".into()).expect("static Codex provider id is valid");
        self.ensure_provider_enabled(&provider_id).map_err(|error| {
            super::product::ProductError::new(provider_error_code(error.code).into(), None)
        })
    }

    /// Resume 在 backend/health 前按 Execution 冻结身份完成 registered→enabled 前缀。
    async fn ensure_persisted_execution_enabled(
        &self,
        execution_id: &str,
    ) -> Result<(), ProviderExecutionFailure> {
        let row = self
            .store
            .execution(execution_id.to_owned())
            .await?
            .ok_or_else(|| "EXECUTION_NOT_FOUND".to_string())?;
        let provider_id = provider_id(row.provider)?;
        self.ensure_provider_enabled(&provider_id)
            .map_err(provider_failure)
    }
    pub(crate) async fn reconcile_startup(&mut self) -> Result<Vec<ProviderReconcileItem>, String> {
        let registry = self
            .registry
            .lock()
            .unwrap()
            .take()
            .map_or_else(
                || self.build_registry(),
                |registry| {
                    Arc::try_unwrap(registry).map_err(|_| ProviderError {
                        code: ProviderErrorCode::AgentProviderContractError,
                    })
                },
            )
            .map_err(|error| provider_error_code(error.code).to_string())?;
        let mut registry = registry;
        let mut report = Vec::new();
        for descriptor in registry.list_descriptors() {
            let id = descriptor.id;
            let provider = registry
                .get_registered(&id)
                .map_err(|error| provider_error_code(error.code).to_string())?;
            if !provider.capabilities().can_recover {
                continue;
            }
            match provider.startup_reconcile(ProviderStartupContext {}).await {
                Ok(summary) => {
                    // 仅消费本轮 Provider 新收敛出的 interrupted，绝不扫描历史终态。
                    for item in &summary.items {
                        if matches!(
                            item.kind,
                            super::provider::port::ProviderReconcileKind::ExecutionInterrupted
                        ) {
                            self.notify_persisted_terminal(&item.subject_id).await;
                        }
                    }
                    report.extend(summary.items);
                }
                Err(_) => registry
                    .set_health(&id, ProviderHealth::Unavailable)
                    .map_err(|error| provider_error_code(error.code).to_string())?,
            }
        }
        *self.registry.lock().unwrap() = Some(Arc::new(registry));
        Ok(report)
    }
    #[cfg(test)]
    /// 测试注入已注册 Provider，验证 Product 与路由读取同一 Registry。
    pub(crate) fn use_registry(&mut self, registry: ProviderRegistry) {
        *self.registry.lock().unwrap() = Some(Arc::new(registry));
    }

    /// 测试专用开关用于验证 Drain；生产设置变更入口不属于 CB2-002。
    #[cfg(test)]
    pub(crate) fn set_provider_enabled_for_test(&self, provider_id: &str, enabled: bool) {
        self.admission.set_enabled_for_test(provider_id, enabled);
    }
    pub(crate) fn id(prefix: &str) -> String {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        format!(
            "{prefix}-{}-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_micros(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        )
    }
    pub(crate) async fn create(
        &self,
        input: CreateExecutionInput,
    ) -> Result<CreateOutcome, String> {
        // One fresh read-only slice uses the already accepted client policy.
        if input.mode != ExecutionMode::ReadOnly
            || input.parent_execution_id.is_some()
            || input.thread_id.is_some()
            || input.execution_profile != serde_json::json!({})
        {
            return Err("TASK006_REQUIRES_FRESH_READ_ONLY_DEFAULT_PROFILE".into());
        }
        WorkspaceExecutionCoordinator {
            store: self.store.clone(),
        }
        .create(Self::id("execution"), canonicalize_request(input)?)
        .await
    }
    pub async fn execute(
        &self,
        input: CreateExecutionInput,
    ) -> Result<CreateOutcome, ProviderExecutionFailure> {
        // Start 必须在创建 Execution/Claim 前拒绝未注册或 disabled Provider。
        self.ensure_provider_enabled(&input.provider)
            .map_err(provider_failure)?;
        let mut outcome = self.create(input).await?;
        if !outcome.created {
            return Ok(outcome);
        }
        outcome.execution = self
            .dispatch_pending_execution(&outcome.execution_id)
            .await?;
        Ok(outcome)
    }
    pub async fn resume_pending_execution(
        &self,
        execution_id: &str,
    ) -> Result<super::store::ExecutionRecord, ProviderExecutionFailure> {
        self.dispatch_pending_execution(execution_id).await
    }
    pub(crate) async fn product_submit(
        &self,
        action: super::product::Action,
        workspace: Option<super::store::transactions::product::WorkspaceSnapshot>,
    ) -> Result<String, super::product::ProductError> {
        self.product_submit_with_work(action, workspace, None).await
    }

    /// 在 Broker management 内重读当前路由与准入事实，不使用早先查询的目录快照。
    fn resolve_start_routing(
        &self,
        config: &crate::config::ManagerConfig,
        intent: super::product::StartRoutingIntent,
    ) -> Result<super::store::transactions::product::FrozenStartRouting, super::product::ProductError>
    {
        use super::{execution::AgentTaskRole, product::StartRoutingIntent};
        if !config.agent_enabled {
            return Err("AGENT_DISABLED".to_string().into());
        }
        // Start 保留 Provider 的稳定码；不改变 Cancel/Continue 既有错误投影。
        let provider_error = |error: ProviderError| {
            let code = provider_error_code(error.code);
            let mut error = super::product::ProductError::from(code.to_string());
            error.code = code.into();
            error
        };
        let route = |role: AgentTaskRole| {
            config
                .agent_providers
                .role_routing
                .get(role.as_str())
                .and_then(Option::as_ref)
                .cloned()
                .ok_or_else(|| {
                    super::product::ProductError::from("AGENT_ROLE_NOT_CONFIGURED".to_string())
                })
        };
        let (task_role, provider) = match intent {
            StartRoutingIntent::LegacyGeneral => {
                (AgentTaskRole::General, route(AgentTaskRole::General)?)
            }
            StartRoutingIntent::Explicit {
                task_role,
                provider_id,
            } => {
                // 显式请求必须先 registered/enabled，再判定角色配置和精确匹配。
                self.ensure_provider_enabled(&provider_id)
                    .map_err(provider_error)?;
                if route(task_role)? != provider_id {
                    return Err("AGENT_ROLE_PROVIDER_MISMATCH".to_string().into());
                }
                (task_role, provider_id)
            }
        };
        self.admit_provider(&provider, ProviderAdmissionCapability::Execute)
            .map_err(provider_error)?;
        Ok(super::store::transactions::product::FrozenStartRouting {
            provider,
            task_role,
        })
    }

    /// management→operation 覆盖最终 Authority 到提交；交接 Runtime 前释放 management。
    pub(crate) async fn product_submit_resolved_workspace_start(
        &self,
        authority: super::product::StartCreationAuthority<'_>,
        action: super::product::Action,
        work: Option<super::store::transactions::product::WorkExecutionContext>,
    ) -> Result<String, super::product::ProductError> {
        use super::product::Action;
        if self.runtime_pool.stop.is_cancelled() {
            return Err("AGENT_SHUTTING_DOWN".to_string().into());
        }
        let Action::Start {
            workspace_id,
            agent_id,
            request_key,
            prompt,
        } = action
        else {
            return Err("AGENT_INVALID_ARGUMENT".to_string().into());
        };
        let management = authority.management.lock().await;
        let routing = self.resolve_start_routing(
            &authority.supervisor.workspace_registry_config(),
            authority.routing,
        )?;
        let outcome = authority.supervisor.create_workspace_start(
            &self.store,
            WorkspaceStartCreation {
                routing,
                execution_id: Self::id("execution"),
                agent_id,
                request_key,
                prompt,
                workspace_id,
                work,
                now: super::coordinator::now(),
            },
        )?;
        drop(management);
        self.handoff_created_outcome(outcome, false).await
    }

    pub(crate) async fn product_submit_with_work(
        &self,
        action: super::product::Action,
        workspace: Option<super::store::transactions::product::WorkspaceSnapshot>,
        work: Option<super::store::transactions::product::WorkExecutionContext>,
    ) -> Result<String, super::product::ProductError> {
        let manager = self.clone();
        // Host owns creation through handoff even if the adapter drops its wait.
        tokio::spawn(async move {
            use super::product::Action;
            if manager.runtime_pool.stop.is_cancelled() {
                return Err("AGENT_SHUTTING_DOWN".to_string().into());
            }
            let continuation = matches!(&action, Action::Continue { .. });
            let outcome = match action {
                Action::Start {
                    workspace_id,
                    agent_id,
                    request_key,
                    prompt,
                } => {
                    // 无 Broker Authority 的 snapshot-only Start 仅用于历史 fixture。
                    #[cfg(not(test))]
                    {
                        let _ = (workspace_id, agent_id, request_key, prompt, workspace, work);
                        return Err("AGENT_INVALID_ARGUMENT".to_string().into());
                    }
                    #[cfg(test)]
                    {
                        manager.ensure_product_start_enabled()?;
                        Some(
                            manager
                                .store
                                .product_create_fresh_with_work(
                                    Self::id("execution"),
                                    agent_id,
                                    request_key,
                                    prompt,
                                    workspace_id,
                                    workspace,
                                    work,
                                    super::coordinator::now(),
                                )
                                .await?,
                        )
                    }
                }
                Action::Continue {
                    execution_id,
                    request_key,
                    prompt,
                } => {
                    let mut work = work;
                    if let Some(context) = &mut work {
                        if context
                            .parent_execution_id
                            .as_ref()
                            .is_some_and(|parent| parent != &execution_id)
                        {
                            return Err("WORK_INVALID_ARGUMENT".to_string().into());
                        }
                        context.parent_execution_id = Some(execution_id.clone());
                    }
                    let candidate = match manager
                        .store
                        .product_continuation_preflight(
                            execution_id.clone(),
                            request_key.clone(),
                            prompt.clone(),
                            work.clone(),
                        )
                        .await?
                    {
                        ContinuationPreflight::Existing(id) => return Ok(id),
                        ContinuationPreflight::Candidate(candidate) => candidate,
                    };
                    manager.validate_continuation_candidate(&candidate).await?;
                    Some(
                        manager
                            .store
                            .product_create_continuation_with_work(
                                Self::id("execution"),
                                execution_id,
                                request_key,
                                prompt,
                                work,
                                Some(candidate.source_revision),
                                super::coordinator::now(),
                            )
                            .await?,
                    )
                }
                Action::ResumePending { execution_id } => {
                    if let Err(error) = manager
                        .ensure_persisted_execution_enabled(&execution_id)
                        .await
                    {
                        let error = match error {
                            ProviderExecutionFailure::State(error) => error,
                            ProviderExecutionFailure::Runtime { code, message } => {
                                format!("{code}: {message}")
                            }
                        };
                        return Err(super::product::ProductError::new(error, Some(execution_id)));
                    }
                    // 健康刷新后的 Registry/admission 是当前准入 Authority；
                    // dispatch 自身会校验 health，不再用启动时的 backend_error 拒绝恢复。
                    let (tx, rx) = tokio::sync::oneshot::channel();
                    let id = execution_id.clone();
                    tokio::spawn(async move {
                        manager.dispatch_with_receipt(&id, Some(tx), false).await
                    });
                    rx.await.map_err(|e| e.to_string())?.map_err(|e| {
                        super::product::ProductError::new(
                            if e == "WORKSPACE_CLAIM_INCONSISTENT" {
                                "AGENT_RESUME_NOT_ALLOWED".into()
                            } else {
                                e
                            },
                            Some(execution_id.clone()),
                        )
                    })?;
                    return Ok(execution_id);
                }
                _ => return Err("AGENT_INVALID_ARGUMENT".to_string().into()),
            }
            .unwrap();
            manager.handoff_created_outcome(outcome, continuation).await
        })
        .await
        .map_err(|e| e.to_string())?
    }

    /// 仅依据已持久化事实判断自动恢复资格；本函数不会创建或派发 child Execution。
    pub(crate) async fn evaluate_auto_recovery(
        &self,
        execution_id: &str,
    ) -> Result<automatic_recovery::AutoRecoveryDecision, String> {
        let Some(execution) = self.store.execution(execution_id.into()).await? else {
            return Ok(automatic_recovery::AutoRecoveryDecision::NotEligible(
                automatic_recovery::AutoRecoveryIneligibleReason::ExecutionMissing,
            ));
        };
        let Some(link) = self.store.work_execution_link(execution.id.clone()).await? else {
            return Ok(automatic_recovery::AutoRecoveryDecision::NotEligible(
                automatic_recovery::AutoRecoveryIneligibleReason::WorkLinkMissing,
            ));
        };
        let Some(work) = self.store.work_run(link.work_run_id.clone()).await? else {
            return Ok(automatic_recovery::AutoRecoveryDecision::NotEligible(
                automatic_recovery::AutoRecoveryIneligibleReason::WorkRunMissing,
            ));
        };
        let parent = if let Some(parent_execution_id) = link.parent_execution_id.as_deref() {
            let parent_execution = self.store.execution(parent_execution_id.into()).await?;
            let parent_link = self
                .store
                .work_execution_link(parent_execution_id.into())
                .await?;
            match (parent_execution, parent_link) {
                (Some(execution), Some(link)) => Some(automatic_recovery::AutoRecoveryParent {
                    execution_id: execution.id,
                    work_run_id: link.work_run_id,
                    parent_execution_id: link.parent_execution_id,
                    delegation_context_json: link.delegation_context_json,
                }),
                _ => None,
            }
        } else {
            None
        };
        let claim_absent = self
            .store
            .workspace_claim(execution.canonical_workspace_root.clone())
            .await?
            .is_none();
        Ok(automatic_recovery::evaluate(
            &execution,
            &link,
            &work,
            claim_absent,
            parent.as_ref(),
        ))
    }

    /// Worker 仅通过既有 Continue 管线创建恢复 child；父 Execution 不在此处被修改。
    async fn schedule_auto_recovery(
        &self,
        execution_id: &str,
    ) -> Result<automatic_recovery::AutoRecoverySchedule, String> {
        let decision = self.evaluate_auto_recovery(execution_id).await?;
        let automatic_recovery::AutoRecoveryDecision::Eligible {
            work_run_id,
            parent_execution_id,
            ..
        } = &decision
        else {
            let automatic_recovery::AutoRecoveryDecision::NotEligible(reason) = decision else {
                unreachable!("auto recovery decision must be eligible or skipped");
            };
            return Ok(automatic_recovery::AutoRecoverySchedule::Skipped(reason));
        };
        let Some(work) = self.store.work_run(work_run_id.clone()).await? else {
            return Ok(automatic_recovery::AutoRecoverySchedule::Skipped(
                automatic_recovery::AutoRecoveryIneligibleReason::WorkRunMissing,
            ));
        };
        if work.status != "active" {
            return Ok(automatic_recovery::AutoRecoverySchedule::Skipped(
                automatic_recovery::AutoRecoveryIneligibleReason::WorkRunNotActive,
            ));
        }
        let plan = automatic_recovery::build_plan(&decision, &work)
            .expect("eligible auto recovery decision must build a plan");
        let child_id = self
            .product_submit_with_work(
                super::product::Action::Continue {
                    execution_id: parent_execution_id.clone(),
                    request_key: plan.request_key,
                    prompt: plan.prompt,
                },
                None,
                Some(super::store::transactions::product::WorkExecutionContext {
                    work_run_id: work.id,
                    parent_execution_id: Some(parent_execution_id.clone()),
                    delegation_context_json: Some(plan.delegation_context_json),
                }),
            )
            .await
            .map_err(|error| error.code)?;
        Ok(automatic_recovery::AutoRecoverySchedule::Scheduled {
            execution_id: child_id,
        })
    }

    /// 交接后的 dispatch 继续由 Host 拥有，调用方丢弃等待不会取消已创建的 Execution。
    async fn handoff_created_outcome(
        &self,
        outcome: CreateOutcome,
        continuation: bool,
    ) -> Result<String, super::product::ProductError> {
        let manager = self.clone();
        tokio::spawn(async move {
            if outcome.created {
                #[cfg(test)]
                if let Some(hook) = &manager.test_handoff {
                    hook.0.notify_one();
                    hook.1.notified().await;
                }
                let (tx, rx) = tokio::sync::oneshot::channel();
                let id = outcome.execution_id.clone();
                tokio::spawn(async move {
                    manager
                        .dispatch_with_receipt(&id, Some(tx), continuation)
                        .await
                });
                rx.await
                    .map_err(|error| {
                        super::product::ProductError::accepted(
                            error.to_string(),
                            outcome.execution_id.clone(),
                        )
                    })?
                    .map_err(|error| {
                        super::product::ProductError::accepted(error, outcome.execution_id.clone())
                    })?;
            }
            Ok(outcome.execution_id)
        })
        .await
        .map_err(|error| error.to_string())?
    }
    async fn dispatch_pending_execution(
        &self,
        execution_id: &str,
    ) -> Result<super::store::ExecutionRecord, ProviderExecutionFailure> {
        self.dispatch_with_receipt(execution_id, None, false).await
    }
    async fn dispatch_with_receipt(
        &self,
        execution_id: &str,
        receipt: Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
        _continuation: bool,
    ) -> Result<super::store::ExecutionRecord, ProviderExecutionFailure> {
        let manager = self.clone();
        let id = execution_id.to_owned();
        let backend_error = self.backend_error.clone();
        let acceptance = Arc::new(HostAcceptanceSink::new(receipt));
        let worker_acceptance = acceptance.clone();
        // The owned worker retains the permit even if its caller stops waiting.
        // Provider/ManagedClient continue to own Runtime and Job convergence.
        let joined = tokio::spawn(async move {
            let admission = async {
                let row = manager
                    .store
                    .execution(id.clone())
                    .await?
                    .ok_or_else(|| "EXECUTION_NOT_FOUND".to_string())?;
                let provider_id = provider_id(row.provider.clone())?;
                let provider = manager
                    .admit_provider(&provider_id, ProviderAdmissionCapability::Execute)
                    .map_err(provider_failure)?;
                let permit = manager.store.guard_pending_dispatch(id.clone()).await?;
                Ok::<_, ProviderExecutionFailure>((permit, provider))
            }
            .await?;
            let (_permit, provider) = admission;
            let telemetry = Arc::new(ExecutionTelemetryProjector::new(manager.store.clone(), id.clone()));

            #[cfg(all(test, any(windows, target_os = "macos")))]
            if let Some((client, database)) = manager.test_client.clone() {
                // Test-only Runtime creation boundary；Windows/macOS 共用 Fake wire Provider 契约。
                rusqlite::Connection::open(database).unwrap().execute(
                    "INSERT INTO runtime_instances(id,owner_host_instance_id,state,created_at,updated_at) VALUES (?1,'fixture','running',1,1)",
                    [client.runtime_id()],
                ).unwrap();
                client
                    .initialize()
                    .await
                    .map_err(|e| ProviderExecutionFailure::State(e.to_string()))?;
                let provider = CodexProvider {
                    store: manager.store.clone(),
                    executable: manager.executable.clone(),
                    backend_error: manager.backend_error.clone(),
                    owner: manager.owner.clone(),
                    runtime_pool: manager.runtime_pool.clone(),
                };
                return provider
                    .run_client_with_acceptance_and_telemetry(
                        &id,
                        &client,
                        worker_acceptance.as_ref(),
                        telemetry.as_ref(),
                    )
                    .await
                    .map_err(ProviderExecutionFailure::State);
            }

            let run = provider
                .execute(
                    ProviderExecutionContext {
                        execution_id: id.clone(),
                    },
                    worker_acceptance,
                    telemetry,
                )
                .await?;
            if run.execution_id != id {
                return Err(ProviderExecutionFailure::State(
                    "AGENT_PROVIDER_CONTRACT_ERROR".into(),
                ));
            }
            manager
                .store
                .execution(id)
                .await?
                .ok_or_else(|| "EXECUTION_NOT_FOUND".to_string().into())
        })
        .await;
        let result = match joined {
            Ok(result) => result,
            Err(_) => {
                acceptance.reject("AGENT_PROVIDER_CONTRACT_ERROR".into());
                return Err(ProviderExecutionFailure::State(
                    "AGENT_PROVIDER_CONTRACT_ERROR".into(),
                ));
            }
        };

        // Provider worker 已结束后重新读取 Store；只有已提交的业务终态可产生副作用。
        self.notify_persisted_terminal(execution_id).await;

        match result {
            Ok(row) if acceptance.is_accepted() => Ok(row),
            Ok(_) => {
                acceptance.reject("AGENT_PROVIDER_CONTRACT_ERROR".into());
                Err(ProviderExecutionFailure::State(
                    "AGENT_PROVIDER_CONTRACT_ERROR".into(),
                ))
            }
            Err(error) => {
                let terminal_failed = matches!(
                    &error,
                    ProviderExecutionFailure::State(code) if code == "PROVIDER_TERMINAL_failed"
                );
                let receipt_error = match &error {
                    ProviderExecutionFailure::State(error)
                        if error == "AGENT_PROVIDER_UNAVAILABLE" =>
                    {
                        backend_error.unwrap_or_else(|| error.clone())
                    }
                    ProviderExecutionFailure::State(error) => error.clone(),
                    ProviderExecutionFailure::Runtime { code, message } => {
                        format!("{code}: {message}")
                    }
                };
                acceptance.reject(receipt_error);
                if terminal_failed {
                    self.notify_auto_recovery(execution_id);
                }
                Err(error)
            }
        }
    }

    /// 读取最终持久化状态后通知产品层；读取或副作用失败都不影响既有结果。
    pub(crate) async fn notify_persisted_terminal(&self, execution_id: &str) {
        let Ok(mut snapshots) = self
            .store
            .product_read(Some(execution_id.to_owned()), None, None, 1)
            .await
        else {
            return;
        };
        let Some(snapshot) = snapshots.pop() else {
            return;
        };
        let Some(status) = AgentTerminalStatus::from_persisted_status(&snapshot.execution.status)
        else {
            return;
        };
        let title = task_title(snapshot.thread_name.as_deref(), &snapshot.execution.prompt);
        self.notify_terminal(status, &snapshot.execution.id, &title);
    }

    /// 固定安全码仅用于诊断，终态副作用永远不可回流到生命周期。
    fn notify_terminal(&self, status: AgentTerminalStatus, execution_id: &str, task_title: &str) {
        if self
            .terminal_notifier
            .notify(status, execution_id, task_title)
            .is_err()
        {
            eprintln!("AGENT_TERMINAL_NOTIFICATION_FAILED");
        }
    }
}

#[cfg(test)]
mod tests;
