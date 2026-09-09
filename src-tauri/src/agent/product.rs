//! One product boundary for MCP and Tauri. Runtime owns all mutation workers.
use super::{
    store::{
        StateStore,
        transactions::product::{WorkspaceSnapshot, continuation_eligible},
    },
    task_manager::AgentTaskManager,
};
use rmcp::schemars::{self, JsonSchema};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(
    tag = "action",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum Action {
    Start {
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
pub struct ExecutionView {
    pub prompt: String,
    pub canonical_workspace_root: String,
    pub execution_id: String,
    pub agent_id: String,
    pub workspace_id: String,
    pub status: String,
    pub dispatch_state: String,
    pub thread_id: Option<String>,
    pub turn_id: Option<String>,
    pub provider_terminal_status: Option<String>,
    pub result_completeness: String,
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
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum ProductData {
    Execution(Box<ExecutionView>),
    List { executions: Vec<ExecutionView> },
}
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum Envelope {
    Success { ok: bool, data: ProductData },
    Failure { ok: bool, error: ProductError },
}
// JSON boolean discriminator (serde internally tagged enums only support strings).
pub fn success(data: ProductData) -> Value {
    json!({"ok":true,"data":data})
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
            "AGENT_INVALID_ARGUMENT",
            "AGENT_NO_ACTIVE_WORKSPACE",
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
        }
    }
}
impl From<String> for ProductError {
    fn from(message: String) -> Self {
        Self::new(message, None)
    }
}
pub fn failure(message: String, execution_id: Option<String>) -> Value {
    serde_json::to_value(Envelope::Failure {
        ok: false,
        error: ProductError::new(message, execution_id),
    })
    .unwrap()
}
pub fn parse(value: Value) -> Result<Action, String> {
    let a: Action =
        serde_json::from_value(value).map_err(|e| format!("AGENT_INVALID_ARGUMENT: {e}"))?;
    let valid = match &a {
        Action::Start {
            agent_id,
            request_key,
            prompt,
        } => !agent_id.is_empty() && !request_key.is_empty() && !prompt.trim().is_empty(),
        Action::Continue {
            execution_id,
            request_key,
            prompt,
        } => !execution_id.is_empty() && !request_key.is_empty() && !prompt.trim().is_empty(),
        Action::Observe { execution_id }
        | Action::Cancel { execution_id }
        | Action::ResumePending { execution_id } => !execution_id.is_empty(),
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
                let diagnostic = if error.starts_with("CODEX_APP_SERVER_INCOMPATIBLE") || error.starts_with("BACKEND_UNAVAILABLE") { error } else { format!("BACKEND_UNAVAILABLE: {error}") };
                (std::path::PathBuf::new(), Some(diagnostic))
            },
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
            | Action::Observe { execution_id }
            | Action::Cancel { execution_id }
            | Action::ResumePending { execution_id } => Some(execution_id.clone()),
            _ => None,
        };
        match self.perform(action, workspace).await {
            Ok(d) => success(d),
            Err(mut e) => {
                if e.execution_id.is_none() {
                    e.execution_id = id;
                }
                serde_json::to_value(Envelope::Failure {
                    ok: false,
                    error: e,
                })
                .unwrap()
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
                    .views(None, agent_id, workspace_id, limit.unwrap_or(20))
                    .await?,
            }),
            Action::Observe { execution_id } => Ok(ProductData::Execution(Box::new(
                self.observe(execution_id).await?,
            ))),
            Action::Cancel { execution_id } => {
                let row = self.manager.cancel(&execution_id).await?;
                if row.status == "unknown" {
                    return Err("AGENT_MANUAL_RESOLUTION_REQUIRED".to_string().into());
                }
                Ok(ProductData::Execution(Box::new(
                    self.observe(execution_id).await?,
                )))
            }
            a => {
                let id = self.manager.product_submit(a, workspace).await?;
                Ok(ProductData::Execution(Box::new(self.observe(id).await?)))
            }
        }
    }
    async fn observe(&self, id: String) -> Result<ExecutionView, String> {
        self.views(Some(id), None, None, 1)
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
                    .map(|v| {
                        serde_json::from_str(v)
                            .map_err(|e| format!("Invalid persisted result: {e}"))
                    })
                    .transpose()?;
                Ok(ExecutionView {
                    prompt: r.prompt.clone(),
                    canonical_workspace_root: r.canonical_workspace_root.clone(),
                    execution_id: r.id.clone(),
                    agent_id: r.agent_id.clone(),
                    workspace_id: r.workspace_id.clone(),
                    status: r.status.clone(),
                    dispatch_state: r.dispatch_state.clone(),
                    thread_id: r.thread_id.clone(),
                    turn_id: r.turn_id.clone(),
                    provider_terminal_status: r.provider_terminal_status.clone(),
                    result_completeness: r.result_completeness.clone(),
                    final_result,
                    interrupt_requested: r.interrupt_requested_at.is_some(),
                    interrupt_acknowledged: r.interrupt_ack_at.is_some(),
                    interrupt_timed_out: r.interrupt_timeout_at.is_some(),
                    attention: if r.status == "unknown" {
                        "manual_resolution_required"
                    } else if pending {
                        "pending_explicit_resume"
                    } else {
                        "none"
                    }
                    .into(),
                    available_actions: actions,
                    created_at: s.created_at,
                    updated_at: s.updated_at,
                    completed_at: s.completed_at,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests;
