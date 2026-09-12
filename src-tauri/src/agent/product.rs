//! One product boundary for MCP and Tauri. Runtime owns all mutation workers.
use super::{
    activity::{ActivityPhase, ActivitySilence, ToolCategory},
    coordinator::now,
    store::{
        StateStore,
        transactions::product::{WorkspaceSnapshot, continuation_eligible},
    },
    task_manager::AgentTaskManager,
};
use rmcp::schemars::{self, JsonSchema};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::time::Duration;
use tokio::time::Instant;
mod control;
pub use control::ControlReceipt;

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
        wait_ms: Option<u32>,
        include_result: Option<bool>,
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
pub struct Progress {
    pub phase: ProgressPhase,
    pub activity_phase: Option<ActivityPhase>,
    pub tool_category: Option<ToolCategory>,
    pub last_activity_at: Option<i64>,
    pub activity_age_ms: Option<i64>,
    pub silence_level: Option<ActivitySilence>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryPage {
    pub executions: Vec<ExecutionView>,
    pub next_cursor: Option<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProgressPhase {
    Pending,
    Dispatching,
    Running,
    Finalizing,
    Reconciling,
    Terminal,
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
    pub status: String,
    pub dispatch_state: String,
    pub thread_id: Option<String>,
    pub thread_name: Option<String>,
    pub turn_id: Option<String>,
    pub provider_terminal_status: Option<String>,
    /// Last diagnostic only; status and Provider terminal remain lifecycle authorities.
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub result_completeness: String,
    pub revision: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unchanged: Option<bool>,
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
impl ExecutionView {
    fn observation_revision(&self) -> String {
        // Product semantics only: never expose the store's CAS revision or hash clocks/result text.
        let input = json!([
            "agent-observation-v2",
            self.execution_id,
            self.status,
            self.dispatch_state,
            self.thread_id,
            self.thread_name,
            self.turn_id,
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
            self.progress.activity_phase,
            self.progress.tool_category,
            self.progress.last_activity_at
        ]);
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
            "BACKEND_UNAVAILABLE",
            "CODEX_APP_SERVER_INCOMPATIBLE",
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
            "AGENT_INVALID_ARGUMENT" => Some(NextAction::CorrectInput),
            "AGENT_NO_ACTIVE_WORKSPACE" => Some(NextAction::ActivateWorkspace),
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
            ..
        } => !execution_id.is_empty() && wait_ms.is_none_or(|n| n <= 25_000),
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
    ) -> Result<(Self, Vec<super::task_manager::recovery::RecoveryOutcome>), String> {
        #[cfg(test)]
        let resolution = match TEST_DISCOVERY.try_with(Clone::clone) {
            Ok(result) => result,
            Err(_) => super::codex::discovery::discover().await,
        };
        #[cfg(not(test))]
        let resolution = super::codex::discovery::discover().await;
        let (executable, error) = match resolution {
            Ok(path) => (path, None),
            Err(error) => {
                let diagnostic = if error.starts_with("CODEX_APP_SERVER_INCOMPATIBLE")
                    || error.starts_with("BACKEND_UNAVAILABLE")
                {
                    error
                } else {
                    format!("BACKEND_UNAVAILABLE: {error}")
                };
                (std::path::PathBuf::new(), Some(diagnostic))
            }
        };
        let mut manager = AgentTaskManager::new(store.clone(), executable);
        manager.backend_error = error;
        Self::recover_before_publish(store, manager).await
    }
    pub(crate) fn backend_diagnostic(&self) -> Option<&str> {
        self.manager.backend_error.as_deref()
    }
    async fn recover_before_publish(
        store: StateStore,
        manager: AgentTaskManager,
    ) -> Result<(Self, Vec<super::task_manager::recovery::RecoveryOutcome>), String> {
        let outcomes = manager.recover_startup().await?;
        Ok((Self { store, manager }, outcomes))
    }
    #[cfg(test)]
    pub fn new(store: StateStore) -> Self {
        Self {
            manager: AgentTaskManager::new(store.clone(), std::path::PathBuf::new()),
            store,
        }
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
                wait_ms,
                include_result,
            } => Ok(ProductData::Execution(Box::new(
                self.observe_wait(
                    execution_id,
                    known_revision,
                    wait_ms.unwrap_or(20_000),
                    include_result.unwrap_or(false),
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
        known_revision: Option<String>,
        wait_ms: u32,
        include_result: bool,
    ) -> Result<ExecutionView, String> {
        let deadline = Instant::now() + Duration::from_millis(u64::from(wait_ms));
        loop {
            // Each read finishes its transaction before sleeping. Dropping this future has no side effects.
            let mut view = self.observe(id.clone(), include_result).await?;
            let unchanged = known_revision.as_ref() == Some(&view.revision);
            view.unchanged = Some(unchanged);
            if view.progress.phase == ProgressPhase::Terminal
                || (include_result && view.result_available)
                || (known_revision.is_some() && !unchanged)
                || Instant::now() >= deadline
            {
                return Ok(view);
            }
            tokio::time::sleep(
                Duration::from_millis(500).min(deadline.saturating_duration_since(Instant::now())),
            )
            .await;
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
        self.store
            .product_read(id, agent, workspace, limit)
            .await?
            .into_iter()
            .map(|s| {
                let r = &s.execution;
                let pending = r.status == "dispatch_pending"
                    && r.dispatch_state == "not_dispatched"
                    && r.runtime_instance_id.is_none()
                    && r.provider_terminal_status.is_none()
                    && s.owns_claim
                    && !s.runtime_attempt_exists
                    && !self.store.product_worker_owned(&r.id);
                let actions = AvailableActions {
                    can_cancel: matches!(
                        r.status.as_str(),
                        "dispatch_pending" | "running" | "cancel_requested" | "cancelling"
                    ) && r.provider_terminal_status.is_none(),
                    can_continue: continuation_eligible(r) && s.claim_free && s.agent_free,
                    can_resume_pending: pending,
                };
                let final_result = r
                    .final_result_json
                    .as_ref()
                    .filter(|_| include_result)
                    .map(|v| {
                        serde_json::from_str(v)
                            .map_err(|e| format!("Invalid persisted result: {e}"))
                    })
                    .transpose()?;
                let attention = if r.status == "unknown" {
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
                match (r.last_activity_at, activity_phase, tool_category) {
                    (None, None, None)
                    | (Some(_), Some(ActivityPhase::Provider), None)
                    | (Some(_), Some(ActivityPhase::Tool), Some(_)) => {}
                    _ => return Err("Invalid persisted execution activity".into()),
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
                    status: r.status.clone(),
                    dispatch_state: r.dispatch_state.clone(),
                    thread_id: r.thread_id.clone(),
                    thread_name: s.thread_name,
                    turn_id: r.turn_id.clone(),
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
                    unchanged: None,
                    result_available,
                    progress: Progress {
                        phase,
                        activity_phase,
                        tool_category,
                        last_activity_at: r.last_activity_at,
                        activity_age_ms,
                        silence_level,
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
                view.revision = view.observation_revision();
                Ok(view)
            })
            .collect()
    }
}

// Project only typed categories and fixed text. Raw Provider messages may contain
// credentials even when short; truncating or keyword redaction is not sufficient.
fn execution_diagnostic_message(row: &super::store::ExecutionRecord) -> Option<String> {
    let code = row.error_code.as_deref()?;
    let raw = row.error_message.as_deref().unwrap_or("");
    let message = match code {
        "CODEX_TURN_ERROR" => {
            let category = serde_json::from_str::<Value>(raw).ok()
                .and_then(|v| serde_json::from_value::<super::codex::protocol::CodexErrorInfo>(v["error"]["codexErrorInfo"].clone()).ok())
                .and_then(|v| serde_json::to_value(v).ok())
                .and_then(|v| match v { Value::String(s) => Some(s), Value::Object(o) => o.keys().next().cloned(), _ => None })
                .unwrap_or_else(|| "other".into());
            format!("{category}: Codex reported a turn diagnostic.")
        }
        "CODEX_PROVIDER_FAILURE" => match raw.split(':').next().unwrap_or("") {
            "CODEX_PROTOCOL_QUEUE_FULL" => "CODEX_PROTOCOL_QUEUE_FULL: Protocol event queue exhausted.".into(),
            "CODEX_STDIO_EOF" => "CODEX_STDIO_EOF: App Server stdout closed.".into(),
            "CODEX_PROTOCOL_INVALID_MESSAGE" => "CODEX_PROTOCOL_INVALID_MESSAGE: Invalid App Server message.".into(),
            "CODEX_APP_SERVER_INCOMPATIBLE" => "CODEX_APP_SERVER_INCOMPATIBLE: App Server protocol mismatch.".into(),
            "CODEX_TURN_ERROR_TERMINAL_TIMEOUT" => "CODEX_TURN_ERROR_TERMINAL_TIMEOUT: No authoritative terminal after non-retry error.".into(),
            "PROVIDER_THREAD_MISMATCH" => "PROVIDER_THREAD_MISMATCH: Notification does not belong to the Execution thread.".into(),
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

#[cfg(test)]
mod tests;
