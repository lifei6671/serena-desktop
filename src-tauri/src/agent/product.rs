//! One product boundary for MCP and Tauri. Runtime owns all mutation workers.
pub use super::activity::ProgressPhase;
use super::{
    activity::{
        AGENT_ACTIVITY_CONTRACT_ERROR, ActivityPhase, ActivitySilence, ToolCategory,
        derive_activity_revision, derive_summary_code,
    },
    coordinator::now,
    execution::AgentTaskRole,
    provider::{ProviderDescriptor, ProviderId, port::ProviderReconcileItem},
    store::{
        StateStore,
        transactions::product::{ProductSnapshot, WorkspaceSnapshot, continuation_core_eligible},
    },
    task_manager::AgentTaskManager,
    usage::{UsageCompleteness, UsageSnapshot},
};
use rmcp::schemars::{self, JsonSchema};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::time::Duration;
use tokio::time::Instant;
mod control;
pub use control::ControlReceipt;
pub(crate) use control::adapter_rejection;
mod work_adapter;
mod work_context;
pub use work_adapter::{AgentExecuteAction, AgentQueryAction};

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(
    tag = "action",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum Action {
    Start {
        workspace_id: String,
        agent_id: String,
        request_key: String,
        prompt: String,
    },
    Continue {
        execution_id: String,
        request_key: String,
        prompt: String,
    },
    ResumePending {
        execution_id: String,
    },
    Observe {
        execution_id: String,
        known_revision: Option<String>,
        known_control_revision: Option<String>,
        known_activity_revision: Option<String>,
        wait_ms: Option<u32>,
        include_result: Option<bool>,
        wake_on: Option<WakeOn>,
    },
    Cancel {
        execution_id: String,
    },
    List {
        agent_id: Option<String>,
        workspace_id: Option<String>,
        limit: Option<u32>,
    },
}

/// Local Tauri IPC 专用的人类收口决议；不属于 MCP `agent_execute` 契约。
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalManualResolution {
    InterruptAndRelease,
}
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProductError {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_id: Option<String>,
    #[serde(skip)]
    pub(crate) accepted_execution_id: Option<String>,
}
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AvailableActions {
    pub can_cancel: bool,
    pub can_continue: bool,
    pub can_resume_pending: bool,
}
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProviderProduct {
    pub id: String,
    pub display_name: String,
    pub version: Option<String>,
}
impl ProviderProduct {
    /// 持久化 ID 是唯一身份；descriptor 只补充展示信息，缺失或不一致时安全回退。
    fn from_execution_provider(provider_id: &str, descriptor: Option<ProviderDescriptor>) -> Self {
        let descriptor = descriptor.filter(|value| value.id.as_str() == provider_id);
        Self {
            id: provider_id.into(),
            display_name: descriptor
                .as_ref()
                .map_or_else(|| provider_id.into(), |value| value.display_name.clone()),
            version: descriptor.and_then(|value| value.version),
        }
    }
}
/// Product schema 的 strict completeness enum；仅从 P4-001 generic enum 映射。
#[derive(Clone, Copy, Debug, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UsageCompletenessProduct {
    Unknown,
    Partial,
    Complete,
}
impl From<UsageCompleteness> for UsageCompletenessProduct {
    /// 保留 P4-001 已验证 completeness，不按 token 字段自行推导。
    fn from(value: UsageCompleteness) -> Self {
        match value {
            UsageCompleteness::Unknown => Self::Unknown,
            UsageCompleteness::Partial => Self::Partial,
            UsageCompleteness::Complete => Self::Complete,
        }
    }
}
/// Product 层的公共 Usage shape；不含 runtime、thread、turn 或 Provider-private state。
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UsageProduct {
    #[schemars(with = "Option<i64>", required)]
    pub input_tokens: Option<i64>,
    #[schemars(with = "Option<i64>", required)]
    pub cached_input_tokens: Option<i64>,
    #[schemars(with = "Option<i64>", required)]
    pub cache_write_input_tokens: Option<i64>,
    #[schemars(with = "Option<i64>", required)]
    pub output_tokens: Option<i64>,
    #[schemars(with = "Option<i64>", required)]
    pub reasoning_tokens: Option<i64>,
    #[schemars(with = "Option<i64>", required)]
    pub total_tokens: Option<i64>,
    #[schemars(with = "Option<i64>", required)]
    pub model_context_window: Option<i64>,
    pub completeness: UsageCompletenessProduct,
    pub usage_revision: u64,
    #[schemars(with = "Option<i64>", required)]
    pub updated_at: Option<i64>,
}
impl UsageProduct {
    /// 历史 Execution 无 Usage 行时的稳定公共默认值；空值不代表真实零。
    fn unknown() -> Self {
        Self {
            input_tokens: None,
            cached_input_tokens: None,
            cache_write_input_tokens: None,
            output_tokens: None,
            reasoning_tokens: None,
            total_tokens: None,
            model_context_window: None,
            completeness: UsageCompletenessProduct::Unknown,
            usage_revision: 0,
            updated_at: None,
        }
    }
    /// 只逐字段映射已验证快照；total 与 completeness 绝不在 Product 层推导。
    fn project(snapshot: Option<&UsageSnapshot>) -> Self {
        let Some(snapshot) = snapshot else {
            return Self::unknown();
        };
        Self {
            input_tokens: snapshot.input_tokens,
            cached_input_tokens: snapshot.cached_input_tokens,
            cache_write_input_tokens: snapshot.cache_write_input_tokens,
            output_tokens: snapshot.output_tokens,
            reasoning_tokens: snapshot.reasoning_tokens,
            total_tokens: snapshot.total_tokens,
            model_context_window: snapshot.model_context_window,
            completeness: snapshot.completeness.into(),
            usage_revision: snapshot.revision,
            updated_at: Some(snapshot.updated_at),
        }
    }
}
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub phase: ProgressPhase,
    pub activity_phase: Option<ActivityPhase>,
    pub tool_category: Option<ToolCategory>,
    pub last_activity_at: Option<i64>,
    pub activity_age_ms: Option<i64>,
    pub silence_level: Option<ActivitySilence>,
    pub summary_code: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryPage {
    pub executions: Vec<ExecutionView>,
    pub next_cursor: Option<String>,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WakeOn {
    Control,
    Activity,
}
/// 表示 Observe 返回当前快照的确定性唤醒原因。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WakeReason {
    InitialMismatch,
    Control,
    Activity,
    Terminal,
    Result,
    Timeout,
}
/// 表示首次 stale token 与当前快照不一致的唯一类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MismatchKind {
    Control,
    Activity,
}
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(
    tag = "action",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum NextAction {
    Observe { wait_ms: u32 },
    ReviewResult { include_result: bool },
    Continue,
    ResumePending,
    ManualResolution,
    CorrectInput,
    ActivateWorkspace,
    List,
}
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionView {
    #[serde(skip)]
    pub(crate) control: ControlReceipt,
    pub prompt: String,
    pub canonical_workspace_root: String,
    pub execution_id: String,
    pub agent_id: String,
    pub workspace_id: String,
    pub provider: ProviderProduct,
    /// 创建时持久化的冻结角色；不读取当前 Provider 路由策略。
    pub task_role: String,
    /// 始终存在的公共 Usage 投影；无持久化行时保持 unknown/null。
    pub usage: UsageProduct,
    pub status: String,
    pub dispatch_state: String,
    pub thread_id: Option<String>,
    pub thread_name: Option<String>,
    pub turn_id: Option<String>,
    pub provider_session_label: Option<String>,
    pub provider_terminal_status: Option<String>,
    /// Last diagnostic only; status and Provider terminal remain lifecycle authorities.
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub result_completeness: String,
    /// Legacy alias of control_revision; never changes for Activity alone.
    pub revision: String,
    pub control_revision: String,
    pub activity_revision: String,
    #[serde(skip)]
    runtime_instance_id: Option<String>,
    #[serde(skip)]
    owns_claim: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unchanged: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wake_reason: Option<WakeReason>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mismatch_kind: Option<MismatchKind>,
    pub result_available: bool,
    pub progress: Progress,
    pub next_action: Option<NextAction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_result: Option<Value>,
    pub interrupt_requested: bool,
    pub interrupt_acknowledged: bool,
    pub interrupt_timed_out: bool,
    pub attention: String,
    pub available_actions: AvailableActions,
    pub created_at: i64,
    pub updated_at: i64,
    pub completed_at: Option<i64>,
}

/// The sole Product projection of persisted provider-opaque compatibility fields.
///
/// These fields remain display-only and never inform Product control decisions.
#[derive(Debug)]
struct ProviderOpaqueCompatibility {
    thread_id: Option<String>,
    thread_name: Option<String>,
    turn_id: Option<String>,
    provider_session_label: Option<String>,
}

impl ProviderOpaqueCompatibility {
    fn project(snapshot: &ProductSnapshot) -> Self {
        let thread_name = snapshot.thread_name.clone();
        Self {
            thread_id: snapshot.execution.thread_id.clone(),
            thread_name: thread_name.clone(),
            turn_id: snapshot.execution.turn_id.clone(),
            // `thread_name` is the persisted safe Thread Title, never raw Provider payload.
            provider_session_label: thread_name,
        }
    }
}

impl ExecutionView {
    fn control_revision(&self) -> String {
        // Product control token is independent of the store's durable CAS revision.
        let input = json!([
            "agent-control-v1",
            self.execution_id,
            self.status,
            self.dispatch_state,
            self.control.provider_invoked,
            self.control.dispatch_certainty,
            self.owns_claim,
            self.provider_terminal_status,
            self.error_code,
            self.error_message,
            self.result_completeness,
            self.result_available,
            self.interrupt_requested,
            self.interrupt_acknowledged,
            self.interrupt_timed_out,
            self.attention,
            self.available_actions,
            self.progress.phase,
            self.completed_at
        ]);
        Self::hash_revision(input)
    }
    fn activity_revision(&self) -> String {
        derive_activity_revision(
            &self.execution_id,
            self.progress.activity_phase,
            self.progress.tool_category,
            self.progress.summary_code.as_deref(),
        )
        .expect("Activity Revision inputs must always serialize")
    }
    fn hash_revision(input: Value) -> String {
        Sha256::digest(input.to_string().as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }
}
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum ProductData {
    Execution(Box<ExecutionView>),
    List { executions: Vec<ExecutionView> },
}
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum Envelope {
    Success {
        ok: bool,
        data: ProductData,
        control: Option<ControlReceipt>,
    },
    Failure {
        ok: bool,
        error: ProductError,
        control: Option<ControlReceipt>,
    },
}
// JSON boolean discriminator (serde internally tagged enums only support strings).
pub fn success(data: ProductData) -> Value {
    let control = match &data {
        ProductData::Execution(view) => Some(view.control.clone()),
        ProductData::List { .. } => None,
    };
    json!({"ok":true,"data":data,"control":control})
}
impl ProductError {
    pub(crate) fn new(message: String, execution_id: Option<String>) -> Self {
        let codes = [
            (
                "EXECUTION_REQUEST_KEY_CONFLICT",
                "AGENT_REQUEST_KEY_CONFLICT",
            ),
            ("AGENT_SNAPSHOT_CONFLICT", "AGENT_LINEAGE_CONFLICT"),
            ("EXECUTION_NOT_FOUND", "AGENT_EXECUTION_NOT_FOUND"),
            ("PENDING_RESUME_REJECTED", "AGENT_RESUME_NOT_ALLOWED"),
            ("WORKSPACE_CLAIM_NOT_OWNED", "AGENT_RESUME_NOT_ALLOWED"),
        ];
        let stable = [
            "WORK_NOT_FOUND",
            "WORK_NOT_ACTIVE",
            "WORK_HAS_ACTIVE_EXECUTIONS",
            "WORK_ACCEPTANCE_REQUIRED",
            "EXECUTION_NOT_IN_WORK",
            "WORK_INVALID_ARGUMENT",
            "WORKSPACE_CONTEXT_REQUIRED",
            "INVALID_PARAMS",
            "CONTEXT_STALE",
            "WORKSPACE_CONTEXT_MISMATCH",
            "WORKSPACE_NOT_FOUND",
            "WORKSPACE_ROOT_NOT_FOUND",
            "WORKSPACE_ROOT_NOT_DIRECTORY",
            "AGENT_DISABLED",
            "AGENT_INVALID_ARGUMENT",
            "AGENT_NO_ACTIVE_WORKSPACE",
            "AGENT_WORKSPACE_CHANGED",
            "AGENT_LINEAGE_CONFLICT",
            "AGENT_BUSY",
            "WORKSPACE_CLAIM_CONFLICT",
            "AGENT_CONTINUE_NOT_ALLOWED",
            "AGENT_RESUME_NOT_ALLOWED",
            "AGENT_MANUAL_RESOLUTION_REQUIRED",
            "AGENT_RUNTIME_QUARANTINED",
            AGENT_ACTIVITY_CONTRACT_ERROR,
            "AGENT_TASK_ROLE_CONTRACT_ERROR",
            "AGENT_OBSERVE_INVALID_ARGUMENT",
            "BACKEND_UNAVAILABLE",
            "CODEX_APP_SERVER_INCOMPATIBLE",
            "CODEX_THREAD_WRITER_CONFLICT",
            "CODEX_HOST_ARCH_UNSUPPORTED",
            "CODEX_ARCH_UNSUPPORTED",
            "CODEX_EXECUTABLE_FORMAT_UNSUPPORTED",
            "CODEX_EXECUTABLE_NOT_RUNNABLE",
            "CODEX_COMPATIBILITY_BLOCKED",
            // Phase 1 的非 Windows Runtime 请求保留明确的 Provider 不可用诊断。
            #[cfg(not(windows))]
            "AGENT_PROVIDER_UNAVAILABLE",
        ];
        let code = codes
            .iter()
            .find(|(from, _)| message.starts_with(from))
            .map(|(_, to)| *to)
            .or_else(|| stable.into_iter().find(|c| message.starts_with(c)))
            .unwrap_or("AGENT_OPERATION_FAILED");
        Self {
            code: code.into(),
            message,
            execution_id,
            accepted_execution_id: None,
        }
    }
    pub(crate) fn accepted(message: String, execution_id: String) -> Self {
        let mut error = Self::new(message, Some(execution_id.clone()));
        error.accepted_execution_id = Some(execution_id);
        error
    }
}
impl From<String> for ProductError {
    fn from(message: String) -> Self {
        Self::new(message, None)
    }
}
pub fn failure(message: String, execution_id: Option<String>) -> Value {
    let error = ProductError::new(message, execution_id);
    let control = Some(ControlReceipt::rejected(
        match error.code.as_str() {
            "AGENT_INVALID_ARGUMENT" | "WORKSPACE_CONTEXT_REQUIRED" | "INVALID_PARAMS" => {
                Some(NextAction::CorrectInput)
            }
            "AGENT_NO_ACTIVE_WORKSPACE" | "WORKSPACE_NOT_FOUND" => {
                Some(NextAction::ActivateWorkspace)
            }
            "AGENT_RUNTIME_QUARANTINED" => Some(NextAction::ManualResolution),
            _ => None,
        },
        None,
    ));
    serde_json::to_value(Envelope::Failure {
        ok: false,
        error,
        control,
    })
    .unwrap()
}
pub fn parse(value: Value) -> Result<Action, String> {
    let a: Action =
        serde_json::from_value(value).map_err(|e| format!("AGENT_INVALID_ARGUMENT: {e}"))?;
    let valid = match &a {
        Action::Start {
            workspace_id,
            agent_id,
            request_key,
            prompt,
        } => {
            !workspace_id.is_empty()
                && !agent_id.is_empty()
                && !request_key.is_empty()
                && !prompt.trim().is_empty()
        }
        Action::Continue {
            execution_id,
            request_key,
            prompt,
        } => !execution_id.is_empty() && !request_key.is_empty() && !prompt.trim().is_empty(),
        Action::Observe {
            execution_id,
            wait_ms,
            known_revision,
            known_control_revision,
            known_activity_revision,
            ..
        } => {
            !execution_id.is_empty()
                && wait_ms.is_none_or(|n| n <= 25_000)
                && known_revision
                    .as_ref()
                    .is_none_or(|token| !token.is_empty())
                && known_control_revision
                    .as_ref()
                    .is_none_or(|token| !token.is_empty())
                && known_activity_revision
                    .as_ref()
                    .is_none_or(|token| !token.is_empty())
        }
        Action::Cancel { execution_id } | Action::ResumePending { execution_id } => {
            !execution_id.is_empty()
        }
        Action::List {
            agent_id,
            workspace_id,
            limit,
        } => {
            agent_id.as_ref().is_none_or(|s| !s.is_empty())
                && workspace_id.as_ref().is_none_or(|s| !s.is_empty())
                && limit.is_none_or(|n| (1..=100).contains(&n))
        }
    };
    if !valid {
        return Err("AGENT_INVALID_ARGUMENT".into());
    }
    Ok(a)
}
#[cfg(test)]
tokio::task_local! { static TEST_DISCOVERY: Result<std::path::PathBuf, String>; }
pub struct AgentProductService {
    store: StateStore,
    manager: AgentTaskManager,
}
impl AgentProductService {
    pub(crate) fn work_product(&self) -> super::work::WorkProductService {
        super::work::WorkProductService::new(self.store.clone())
    }
    pub(crate) async fn workspace_claim_exists(&self, root: String) -> Result<bool, String> {
        Ok(self.store.workspace_claim(root).await?.is_some())
    }

    /// 返回 StateStore 中所有未终态 Execution 数量，不读取 Runtime 私有状态。
    pub(crate) async fn nonterminal_execution_count(&self) -> Result<usize, String> {
        self.store.product_nonterminal_count().await
    }

    /// 仅供 Supervisor operation mutex 内的 Workspace Remove typed check 同步读取。
    pub(crate) fn workspace_claim_exists_blocking(&self, root: &str) -> Result<bool, String> {
        Ok(self.store.workspace_claim_blocking(root)?.is_some())
    }

    pub async fn shutdown(&self) -> Result<(), String> {
        let result = self.manager.runtime_pool.shutdown().await;
        self.manager.wait_auto_recovery_worker().await;
        result
    }

    /// 只供 Local Desktop Human Authority 调用；reason 从不写入持久化状态或诊断。
    pub async fn manual_resolve(
        &self,
        execution_id: String,
        resolution: LocalManualResolution,
        reason: Option<String>,
    ) -> Result<ExecutionView, String> {
        let reason_provided = reason.is_some_and(|value| !value.trim().is_empty());
        match resolution {
            LocalManualResolution::InterruptAndRelease => {
                self.store
                    .manual_resolve_and_release(execution_id.clone(), reason_provided, now())
                    .await?
            }
        }
        // 原子收口已成功返回后才触发产品副作用，绝不进入事务本身。
        self.manager.notify_persisted_terminal(&execution_id).await;
        self.observe(execution_id, false).await
    }
    /// Desktop-only read pagination; MCP Action/list stays unchanged.
    pub async fn history_page(
        &self,
        before: Option<String>,
        workspace: Option<String>,
    ) -> Result<HistoryPage, String> {
        let mut ids = self.store.product_history_ids(before, workspace).await?;
        let has_more = ids.len() > 5;
        ids.truncate(5);
        let next_cursor = if has_more { ids.last().cloned() } else { None };
        let mut executions = Vec::with_capacity(ids.len());
        for id in ids {
            executions.push(self.observe(id, false).await?);
        }
        Ok(HistoryPage {
            executions,
            next_cursor,
        })
    }

    pub async fn initialize(
        store: StateStore,
    ) -> Result<(Self, Vec<ProviderReconcileItem>), String> {
        Self::initialize_with_terminal_notifier(
            store,
            super::notification::noop_agent_terminal_notifier(),
        )
        .await
    }

    /// Desktop 启动时注入产品层终态副作用，保持 Agent core 不依赖桌面实现。
    pub(crate) async fn initialize_with_terminal_notifier(
        store: StateStore,
        terminal_notifier: std::sync::Arc<dyn super::notification::AgentTerminalNotifier>,
    ) -> Result<(Self, Vec<ProviderReconcileItem>), String> {
        // 先创建唯一 Manager shell，discovery probe 才能共用其 Store/owner/Pool。
        let mut manager = AgentTaskManager::new_with_terminal_notifier(
            store.clone(),
            std::path::PathBuf::new(),
            terminal_notifier,
        );
        #[cfg(test)]
        let resolution = match TEST_DISCOVERY.try_with(Clone::clone) {
            Ok(result) => result,
            Err(_) => manager.discover_backend().await,
        };
        #[cfg(not(test))]
        let resolution = manager.discover_backend().await;
        let resolution = resolution.map_err(|error| {
            // Discovery 已输出稳定平台/架构类别时直接保留，避免产品边界降级成一般不可用。
            if [
                "BACKEND_UNAVAILABLE",
                "CODEX_APP_SERVER_INCOMPATIBLE",
                "CODEX_HOST_ARCH_UNSUPPORTED",
                "CODEX_ARCH_UNSUPPORTED",
                "CODEX_EXECUTABLE_FORMAT_UNSUPPORTED",
                "CODEX_EXECUTABLE_NOT_RUNNABLE",
                "CODEX_COMPATIBILITY_BLOCKED",
            ]
            .iter()
            .any(|code| error.starts_with(code))
            {
                error
            } else {
                format!("BACKEND_UNAVAILABLE: {error}")
            }
        });
        manager.install_backend_resolution(resolution);
        Self::recover_before_publish(store, manager).await
    }

    /// macOS Desktop 只延后 Codex backend resolution；recovery 仍在发布前完整执行。
    #[cfg(target_os = "macos")]
    pub(crate) async fn initialize_desktop_deferred(
        store: StateStore,
        terminal_notifier: std::sync::Arc<dyn super::notification::AgentTerminalNotifier>,
    ) -> Result<(Self, Vec<ProviderReconcileItem>), String> {
        let mut manager = AgentTaskManager::new_with_terminal_notifier(
            store.clone(),
            std::path::PathBuf::new(),
            terminal_notifier,
        );
        manager.defer_backend_resolution();
        Self::recover_before_publish(store, manager).await
    }
    pub(crate) fn backend_diagnostic(&self) -> Option<&str> {
        self.manager.backend_error.as_deref()
    }

    /// macOS 状态页复用正式 Manager authority，重新执行只读 discovery/compatibility probe。
    #[cfg(target_os = "macos")]
    pub(crate) async fn discover_backend(&self) -> Result<std::path::PathBuf, String> {
        self.manager.discover_backend().await
    }
    async fn recover_before_publish(
        store: StateStore,
        mut manager: AgentTaskManager,
    ) -> Result<(Self, Vec<ProviderReconcileItem>), String> {
        let report = manager.reconcile_startup().await?;
        manager.start_auto_recovery_worker();
        Ok((Self { store, manager }, report))
    }
    #[cfg(test)]
    pub fn new(store: StateStore) -> Self {
        let manager = AgentTaskManager::new(store.clone(), std::path::PathBuf::new());
        #[cfg(not(any(windows, target_os = "macos")))]
        let manager = {
            let mut manager = manager;
            // 测试构造器同步未支持平台事实，避免绕过 unavailable 错误投影。
            manager.backend_error =
                Some("BACKEND_UNAVAILABLE: Codex runtime is unavailable on this platform".into());
            manager
        };
        Self { manager, store }
    }
    /// 构造在 Provider 接受前确定性拒绝派发的测试专用服务。
    #[cfg(test)]
    pub(crate) fn new_with_rejected_dispatch_for_test(store: StateStore) -> Self {
        let mut manager = AgentTaskManager::new(store.clone(), std::path::PathBuf::new());
        // 固定 Registry 为不可用，避免测试发现或启动用户安装的 Codex。
        manager.backend_error = Some("TEST_DISPATCH_REJECTED".into());
        Self { store, manager }
    }
    pub async fn operation(&self, value: Value, workspace: Option<WorkspaceSnapshot>) -> Value {
        let action = match parse(value) {
            Ok(a) => a,
            Err(e) => return failure(e, None),
        };
        let id = match &action {
            Action::Continue { execution_id, .. }
            | Action::Observe { execution_id, .. }
            | Action::Cancel { execution_id }
            | Action::ResumePending { execution_id } => Some(execution_id.clone()),
            _ => None,
        };
        match self.perform(action.clone(), workspace.clone()).await {
            Ok(d) => success(d),
            Err(mut e) => {
                if e.execution_id.is_none() {
                    e.execution_id = id;
                }
                self.error_response(action, workspace, e).await
            }
        }
    }

    /// Local Start 复用 Remote 的 Supervisor 线性化创建路径，禁止在兼容入口预先冻结快照。
    pub(crate) async fn operation_resolved_workspace_start(
        &self,
        supervisor: &crate::serena::SupervisorState,
        value: Value,
    ) -> Value {
        let action = match parse(value) {
            Ok(action @ Action::Start { .. }) => action,
            Ok(_) => return failure("AGENT_INVALID_ARGUMENT".into(), None),
            Err(error) => return failure(error, None),
        };
        match self
            .manager
            .product_submit_resolved_workspace_start(supervisor, action.clone(), None)
            .await
        {
            Ok(id) => match self.observe(id.clone(), false).await {
                Ok(execution) => success(ProductData::Execution(Box::new(execution))),
                Err(error) => {
                    self.error_response(action, None, ProductError::accepted(error, id))
                        .await
                }
            },
            Err(error) => self.error_response(action, None, error).await,
        }
    }
    async fn perform(
        &self,
        action: Action,
        workspace: Option<WorkspaceSnapshot>,
    ) -> Result<ProductData, ProductError> {
        match action {
            Action::List {
                agent_id,
                workspace_id,
                limit,
            } => Ok(ProductData::List {
                executions: self
                    .views(None, agent_id, workspace_id, limit.unwrap_or(20), false)
                    .await?,
            }),
            Action::Observe {
                execution_id,
                known_revision,
                known_control_revision,
                known_activity_revision,
                wait_ms,
                include_result,
                wake_on,
            } => Ok(ProductData::Execution(Box::new(
                self.observe_wait(
                    execution_id,
                    known_control_revision.or(known_revision),
                    known_activity_revision,
                    wait_ms.unwrap_or(20_000),
                    include_result.unwrap_or(false),
                    wake_on.unwrap_or(WakeOn::Control),
                )
                .await?,
            ))),
            Action::Cancel { execution_id } => {
                let row = self.manager.cancel(&execution_id).await?;
                if row.status == "unknown" {
                    return Err("AGENT_MANUAL_RESOLUTION_REQUIRED".to_string().into());
                }
                Ok(ProductData::Execution(Box::new(
                    self.observe(execution_id, false).await?,
                )))
            }
            a => {
                let id = self.manager.product_submit(a, workspace).await?;
                Ok(ProductData::Execution(Box::new(
                    self.observe(id.clone(), false)
                        .await
                        .map_err(|e| ProductError::accepted(e, id))?,
                )))
            }
        }
    }
    async fn observe_wait(
        &self,
        id: String,
        known_control_revision: Option<String>,
        known_activity_revision: Option<String>,
        wait_ms: u32,
        include_result: bool,
        wake_on: WakeOn,
    ) -> Result<ExecutionView, String> {
        let deadline = Instant::now() + Duration::from_millis(u64::from(wait_ms));
        let mut view = self.observe(id.clone(), include_result).await?;
        view.unchanged = Some(known_control_revision.as_ref() == Some(&view.control_revision));
        if known_control_revision
            .as_ref()
            .is_some_and(|known| known != &view.control_revision)
        {
            view.wake_reason = Some(WakeReason::InitialMismatch);
            view.mismatch_kind = Some(MismatchKind::Control);
            return Ok(view);
        }
        if known_activity_revision
            .as_ref()
            .is_some_and(|known| known != &view.activity_revision)
        {
            view.wake_reason = Some(WakeReason::InitialMismatch);
            view.mismatch_kind = Some(MismatchKind::Activity);
            return Ok(view);
        }
        let initial_control_revision = view.control_revision.clone();
        let initial_activity_revision = view.activity_revision.clone();
        loop {
            // Each read finishes its transaction before sleeping. Dropping this future has no side effects.
            view.unchanged = Some(known_control_revision.as_ref() == Some(&view.control_revision));
            let reason = if include_result && view.result_available {
                Some(WakeReason::Result)
            } else if view.progress.phase == ProgressPhase::Terminal {
                Some(WakeReason::Terminal)
            } else if view.control_revision != initial_control_revision {
                Some(WakeReason::Control)
            } else if matches!(wake_on, WakeOn::Activity)
                && view.activity_revision != initial_activity_revision
            {
                Some(WakeReason::Activity)
            } else if Instant::now() >= deadline {
                Some(WakeReason::Timeout)
            } else {
                None
            };
            if let Some(reason) = reason {
                view.wake_reason = Some(reason);
                return Ok(view);
            }
            tokio::time::sleep(
                Duration::from_millis(500).min(deadline.saturating_duration_since(Instant::now())),
            )
            .await;
            view = self.observe(id.clone(), include_result).await?;
        }
    }
    async fn observe(&self, id: String, include_result: bool) -> Result<ExecutionView, String> {
        self.views(Some(id), None, None, 1, include_result)
            .await?
            .pop()
            .ok_or_else(|| "EXECUTION_NOT_FOUND".into())
    }
    async fn views(
        &self,
        id: Option<String>,
        agent: Option<String>,
        workspace: Option<String>,
        limit: u32,
        include_result: bool,
    ) -> Result<Vec<ExecutionView>, String> {
        let snapshots = self.store.product_read(id, agent, workspace, limit).await?;
        // Registry 仅供可选展示元数据读取；失败不能阻断持久化 Execution 的查询。
        let registry = self.manager.registry().ok();
        let mut views = Vec::with_capacity(snapshots.len());
        for s in snapshots {
            let compatibility = ProviderOpaqueCompatibility::project(&s);
            let r = &s.execution;
            let task_role: AgentTaskRole =
                serde_json::from_value(Value::String(s.task_role.clone()))
                    .map_err(|_| "AGENT_TASK_ROLE_CONTRACT_ERROR".to_string())?;
            let pending = r.status == "dispatch_pending"
                && r.dispatch_state == "not_dispatched"
                && r.runtime_instance_id.is_none()
                && r.provider_terminal_status.is_none()
                && s.owns_claim
                && !s.runtime_attempt_exists
                && !self.store.product_worker_owned(&r.id);
            let quarantined_pending = pending
                && self
                    .manager
                    .runtime_pool
                    .check_workspace(&r.canonical_workspace_root)
                    .is_err();
            let actions = AvailableActions {
                can_cancel: matches!(
                    r.status.as_str(),
                    "dispatch_pending" | "running" | "cancel_requested" | "cancelling"
                ) && r.provider_terminal_status.is_none(),
                can_continue: if continuation_core_eligible(r) && s.claim_free && s.agent_free {
                    self.manager
                        .can_continue(r.id.clone(), r.provider.clone())
                        .await
                } else {
                    false
                },
                can_resume_pending: pending && !quarantined_pending,
            };
            let final_result = r
                .final_result_json
                .as_ref()
                .filter(|_| include_result)
                .map(|v| {
                    serde_json::from_str(v).map_err(|e| format!("Invalid persisted result: {e}"))
                })
                .transpose()?;
            let attention = if r.status == "unknown" || quarantined_pending {
                "manual_resolution_required"
            } else if pending {
                "pending_explicit_resume"
            } else {
                "none"
            };
            let phase = match r.status.as_str() {
                "dispatch_pending" => match r.dispatch_state.as_str() {
                    "not_dispatched" => ProgressPhase::Pending,
                    "dispatching" => ProgressPhase::Dispatching,
                    "dispatched" => ProgressPhase::Running,
                    "uncertain" => ProgressPhase::Reconciling,
                    _ => {
                        return Err(format!(
                            "Invalid persisted dispatch state: {}",
                            r.dispatch_state
                        ));
                    }
                },
                "running" | "cancel_requested" | "cancelling" => ProgressPhase::Running,
                "finalizing" => ProgressPhase::Finalizing,
                "reconciling" | "unknown" => ProgressPhase::Reconciling,
                "completed" | "failed" | "cancelled" | "interrupted" => ProgressPhase::Terminal,
                _ => return Err(format!("Invalid persisted execution status: {}", r.status)),
            };
            let result_available = r.final_result_json.is_some();
            let activity_phase = r
                .activity_phase
                .as_deref()
                .map(ActivityPhase::try_from)
                .transpose()?;
            let tool_category = r
                .tool_category
                .as_deref()
                .map(ToolCategory::try_from)
                .transpose()?;
            // Activity 的时间戳仅用于展示；持久化契约只要求 phase/category 配对合法。
            match (activity_phase, tool_category) {
                (None, None)
                | (Some(ActivityPhase::Provider), None)
                | (Some(ActivityPhase::Tool), Some(_)) => {}
                _ => return Err("Invalid persisted execution activity".into()),
            }
            // Product 只投影已由 Store 写入的 Activity 摘要；不一致时拒绝伪造新语义。
            let expected_summary_code =
                derive_summary_code(phase, activity_phase, tool_category).map_err(str::to_owned)?;
            if r.activity_summary_code.as_deref() != expected_summary_code {
                return Err(AGENT_ACTIVITY_CONTRACT_ERROR.into());
            }
            let activity_age_ms = r
                .last_activity_at
                .map(|last| now().saturating_sub(last).max(0));
            let silence_level = ActivitySilence::from_activity_age_ms(activity_age_ms);
            let next_action = match attention {
                "manual_resolution_required" => Some(NextAction::ManualResolution),
                "pending_explicit_resume" => Some(NextAction::ResumePending),
                _ if phase == ProgressPhase::Terminal
                    && result_available
                    && final_result.is_none() =>
                {
                    Some(NextAction::ReviewResult {
                        include_result: true,
                    })
                }
                _ if phase != ProgressPhase::Terminal => {
                    Some(NextAction::Observe { wait_ms: 20_000 })
                }
                _ => None,
            };
            let mut view = ExecutionView {
                control: ControlReceipt::accepted(r, next_action.clone()),
                prompt: r.prompt.clone(),
                canonical_workspace_root: r.canonical_workspace_root.clone(),
                execution_id: r.id.clone(),
                agent_id: r.agent_id.clone(),
                workspace_id: r.workspace_id.clone(),
                provider: ProviderProduct::from_execution_provider(
                    &r.provider,
                    registry.as_ref().and_then(|registry| {
                        ProviderId::new(r.provider.clone())
                            .ok()
                            .and_then(|id| registry.get_registered(&id).ok())
                            .map(|provider| provider.descriptor())
                    }),
                ),
                task_role: task_role.as_str().into(),
                usage: UsageProduct::project(s.usage.as_ref()),
                status: r.status.clone(),
                dispatch_state: r.dispatch_state.clone(),
                thread_id: compatibility.thread_id,
                thread_name: compatibility.thread_name,
                turn_id: compatibility.turn_id,
                provider_session_label: compatibility.provider_session_label,
                provider_terminal_status: r.provider_terminal_status.clone(),
                error_code: r.error_code.as_ref().map(|code| match code.as_str() {
                    "CODEX_TURN_ERROR" => code.clone(),
                    "CODEX_PROVIDER_FAILURE" => code.clone(),
                    "CODEX_PERMISSION_DENIED" => code.clone(),
                    _ => "EXECUTION_DIAGNOSTIC".into(),
                }),
                error_message: execution_diagnostic_message(r),
                result_completeness: r.result_completeness.clone(),
                revision: String::new(),
                control_revision: String::new(),
                activity_revision: String::new(),
                runtime_instance_id: r.runtime_instance_id.clone(),
                owns_claim: s.owns_claim,
                unchanged: None,
                wake_reason: None,
                mismatch_kind: None,
                result_available,
                progress: Progress {
                    phase,
                    activity_phase,
                    tool_category,
                    last_activity_at: r.last_activity_at,
                    activity_age_ms,
                    silence_level,
                    summary_code: r.activity_summary_code.clone(),
                },
                next_action,
                final_result,
                interrupt_requested: r.interrupt_requested_at.is_some(),
                interrupt_acknowledged: r.interrupt_ack_at.is_some(),
                interrupt_timed_out: r.interrupt_timeout_at.is_some(),
                attention: attention.into(),
                available_actions: actions,
                created_at: s.created_at,
                updated_at: s.updated_at,
                completed_at: s.completed_at,
            };
            view.control_revision = view.control_revision();
            view.revision = view.control_revision.clone();
            view.activity_revision = view.activity_revision();
            views.push(view);
        }
        Ok(views)
    }
}

// Project only typed categories and fixed text. Raw Provider messages may contain
// credentials even when short; truncating or keyword redaction is not sufficient.
fn execution_diagnostic_message(row: &super::store::ExecutionRecord) -> Option<String> {
    let code = row.error_code.as_deref()?;
    let raw = row.error_message.as_deref().unwrap_or("");
    let message = match code {
        "CODEX_TURN_ERROR" => {
            let category = safe_turn_error_category(raw).unwrap_or("other");
            let summary = match category {
                "badRequest" => safe_unsupported_model_message(raw)
                    .unwrap_or_else(|| "Codex reported a turn diagnostic.".into()),
                "usageLimitExceeded" => "Codex usage limit reached.".into(),
                "rateLimitExceeded" => "Codex rate limit reached.".into(),
                "serverOverloaded" => "Codex service is overloaded.".into(),
                "unauthorized" => "Codex authentication was rejected.".into(),
                "contextWindowExceeded" => "Codex context window was exceeded.".into(),
                _ => "Codex reported a turn diagnostic.".into(),
            };
            format!("{category}: {summary}")
        }
        "CODEX_PROVIDER_FAILURE" => match raw.split(':').next().unwrap_or("") {
            "CODEX_PROTOCOL_QUEUE_FULL" => "CODEX_PROTOCOL_QUEUE_FULL: Protocol event queue exhausted.".into(),
            "CODEX_STDIO_EOF" => "CODEX_STDIO_EOF: App Server stdout closed.".into(),
            "CODEX_PROTOCOL_INVALID_MESSAGE" => "CODEX_PROTOCOL_INVALID_MESSAGE: Invalid App Server message.".into(),
            "CODEX_APP_SERVER_INCOMPATIBLE" => "CODEX_APP_SERVER_INCOMPATIBLE: App Server protocol mismatch.".into(),
            "CODEX_TURN_ERROR_TERMINAL_TIMEOUT" => "CODEX_TURN_ERROR_TERMINAL_TIMEOUT: No authoritative terminal after non-retry error.".into(),
            "PROVIDER_THREAD_MISMATCH" => "PROVIDER_THREAD_MISMATCH: Notification does not belong to the Execution thread.".into(),
            // 持久化原文可能含 Provider 上下文；仅按稳定子码投影固定安全诊断。
            "CODEX_THREAD_WRITER_CONFLICT" => "CODEX_THREAD_WRITER_CONFLICT: Codex thread is currently being written by another client; release it and retry the continuation.".into(),
            "CODEX_RPC_TIMEOUT" => "CODEX_RPC_TIMEOUT: App Server request timed out.".into(),
            _ => "Codex Provider failed; raw details withheld.".into(),
        },
        "CODEX_PERMISSION_DENIED" => {
            let category = match raw {
                "command" => "command",
                "file_change" => "file change",
                "permissions" => "permissions",
                _ => "unknown",
            };
            format!("CODEX_PERMISSION_DENIED: Unexpected {category} approval request was denied.")
        }
        _ => "Execution diagnostic recorded; raw details withheld.".into(),
    };
    Some(message.chars().take(256).collect())
}

/// 只匹配 Codex 的完整固定错误句式；模型标识必须是短 ASCII ID，且不能像凭据字段。
fn safe_unsupported_model_message(raw: &str) -> Option<String> {
    const PREFIX: &str = "The '";
    const SUFFIX: &str = "' model is not supported when using Codex with a ChatGPT account.";
    const REASON: &str = "is not supported when using Codex with a ChatGPT account.";
    let value = serde_json::from_str::<Value>(raw).ok()?;
    let message = value.pointer("/error/message")?.as_str()?;
    let model = message.strip_prefix(PREFIX)?.strip_suffix(SUFFIX)?;
    let lower = model.to_ascii_lowercase();
    let looks_sensitive = [
        "private",
        "token",
        "authorization",
        "secret",
        "password",
        "bearer",
    ]
    .iter()
    .any(|word| lower.contains(word));
    if model.is_empty()
        || model.len() > 64
        || !model
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(&byte))
        || looks_sensitive
    {
        return Some(format!("Codex model {REASON}"));
    }
    Some(format!("Codex model '{model}' {REASON}"))
}

fn safe_turn_error_category(raw: &str) -> Option<&'static str> {
    const SAFE_CATEGORIES: &[&str] = &[
        "contextWindowExceeded",
        "sessionBudgetExceeded",
        "usageLimitExceeded",
        "rateLimitExceeded",
        "serverOverloaded",
        "cyberPolicy",
        "misalignmentPolicyViolation",
        "internalServerError",
        "unauthorized",
        "badRequest",
        "threadRollbackFailed",
        "sandboxError",
        "other",
        "httpConnectionFailed",
        "responseStreamConnectionFailed",
        "responseStreamDisconnected",
        "responseTooManyFailedAttempts",
        "activeTurnNotSteerable",
    ];
    let value = serde_json::from_str::<Value>(raw).ok()?;
    let category = match value.pointer("/error/codexErrorInfo")? {
        Value::String(category) => category.as_str(),
        Value::Object(fields) if fields.len() == 1 => fields.keys().next()?.as_str(),
        _ => return None,
    };
    SAFE_CATEGORIES
        .iter()
        .copied()
        .find(|safe| *safe == category)
}

#[cfg(test)]
mod tests;
