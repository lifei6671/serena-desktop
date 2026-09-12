use crate::agent::{
    activity::{ActivitySilence, ToolCategory},
    product::{ExecutionView, NextAction, ProductData, ProductError, ProgressPhase},
};
use rmcp::schemars::{self, JsonSchema};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Serialize, JsonSchema)]
#[serde(untagged, deny_unknown_fields)]
pub(super) enum QueryEnvelope {
    Success { ok: bool, data: QueryData },
    Failure { ok: bool, error: ProductError },
}

#[derive(Serialize, JsonSchema)]
#[serde(untagged, deny_unknown_fields)]
pub(super) enum QueryData {
    Detail(Box<ExecutionView>),
    List { executions: Vec<QuerySummary> },
    Observation(Box<QueryObservation>),
}

impl QueryData {
    pub(super) fn project(data: ProductData, observe: bool) -> Self {
        match data {
            ProductData::Execution(view) if observe => Self::Observation(Box::new((*view).into())),
            ProductData::Execution(view) => Self::Detail(view),
            ProductData::List { executions } => Self::List {
                executions: executions.into_iter().map(Into::into).collect(),
            },
        }
    }
}

#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct QuerySummary {
    execution_id: String,
    status: String,
    dispatch_state: String,
    /// Opaque control revision; Activity alone does not change it.
    revision: String,
    result_available: bool,
    result_completeness: String,
    attention: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    thread_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "NextAction")]
    next_action: Option<NextAction>,
    created_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "i64")]
    completed_at: Option<i64>,
}

impl From<ExecutionView> for QuerySummary {
    fn from(view: ExecutionView) -> Self {
        Self {
            execution_id: view.execution_id,
            status: view.status,
            dispatch_state: view.dispatch_state,
            revision: view.control_revision,
            result_available: view.result_available,
            result_completeness: view.result_completeness,
            attention: view.attention,
            thread_name: view.thread_name,
            next_action: view.next_action,
            created_at: view.created_at,
            completed_at: view.completed_at,
        }
    }
}

#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct QueryObservation {
    execution_id: String,
    status: String,
    /// Current controlRevision, reusable as the next knownRevision opaque token.
    revision: String,
    unchanged: bool,
    result_available: bool,
    result_completeness: String,
    progress: QueryProgress,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "NextAction")]
    next_action: Option<NextAction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    attention: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "QueryDiagnostic")]
    error: Option<QueryDiagnostic>,
    /// Exact persisted result, present only when includeResult=true and a result exists.
    #[serde(skip_serializing_if = "Option::is_none")]
    final_result: Option<Value>,
}

#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct QueryProgress {
    phase: ProgressPhase,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "ToolCategory")]
    tool_category: Option<ToolCategory>,
    /// Display-only age bucket: fresh <30s, quiet 30s–<120s, prolonged >=120s.
    /// Absent when no Activity is observable. Quiet/prolonged do not mean stalled,
    /// timeout or failure and must never alone justify a control action.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "ActivitySilence")]
    silence_level: Option<ActivitySilence>,
}

#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct QueryDiagnostic {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    message: Option<String>,
}

impl From<ExecutionView> for QueryObservation {
    fn from(view: ExecutionView) -> Self {
        let error = (view.error_code.is_some() || view.error_message.is_some()).then_some(
            QueryDiagnostic {
                code: view.error_code,
                message: view.error_message,
            },
        );
        Self {
            execution_id: view.execution_id,
            status: view.status,
            revision: view.control_revision,
            unchanged: view.unchanged.expect("observe_wait always sets unchanged"),
            result_available: view.result_available,
            result_completeness: view.result_completeness,
            progress: QueryProgress {
                phase: view.progress.phase,
                tool_category: view.progress.tool_category,
                silence_level: view.progress.silence_level,
            },
            next_action: view.next_action,
            attention: (view.attention != "none").then_some(view.attention),
            error,
            final_result: view.final_result,
        }
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(
    tag = "action",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(super) enum WorkQuery {
    Get {
        work_run_id: String,
    },
    List {
        workspace_id: Option<String>,
        #[schemars(range(min = 1, max = 100))]
        limit: Option<u32>,
    },
}
#[derive(Deserialize, JsonSchema)]
#[serde(
    tag = "action",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(super) enum WorkUpdate {
    Begin {
        workspace_id: String,
        title: String,
        goal: Option<String>,
    },
    Finish {
        work_run_id: String,
        outcome: Outcome,
        acceptance: Option<AcceptanceInput>,
    },
    Cancel {
        work_run_id: String,
    },
}
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(super) enum Outcome {
    Completed,
    Failed,
}
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct AcceptanceInput {
    pub summary: String,
    pub execution_ids: Vec<String>,
}
#[derive(Deserialize, JsonSchema)]
#[serde(
    tag = "action",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(super) enum AgentQuery {
    Get {
        execution_id: String,
        include_result: Option<bool>,
    },
    List {
        work_run_id: String,
        #[schemars(range(min = 1, max = 100))]
        limit: Option<u32>,
    },
    Observe {
        execution_id: String,
        known_revision: Option<String>,
        #[schemars(range(min = 0, max = 20000))]
        wait_ms: Option<u32>,
        include_result: Option<bool>,
    },
}
#[derive(Deserialize, JsonSchema)]
#[serde(
    tag = "action",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(super) enum AgentExecute {
    Start {
        work_run_id: String,
        request_key: String,
        prompt: String,
        context: Option<Context>,
    },
    Continue {
        work_run_id: String,
        parent_execution_id: String,
        request_key: String,
        prompt: String,
        context: Option<Context>,
    },
    Cancel {
        work_run_id: String,
        execution_id: String,
    },
    ResumePending {
        work_run_id: String,
        execution_id: String,
    },
}
// Only transport types; Phase 6 remains the path/hash/canonicalization authority.
#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Context {
    #[serde(
        default,
        deserialize_with = "optional_string",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "String")]
    summary: Option<String>,
    #[serde(default)]
    files: Vec<FileReference>,
}
fn optional_string<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    String::deserialize(d).map(Some)
}
#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct FileReference {
    path: String,
    sha256: String,
}

pub(super) enum Request {
    WorkQuery(WorkQuery),
    WorkUpdate(WorkUpdate),
    AgentQuery(AgentQuery),
    AgentExecute(AgentExecute),
}
pub(super) fn parse(name: &str, args: Value) -> Result<Request, String> {
    let result = match name {
        "work_query" => serde_json::from_value(args).map(Request::WorkQuery),
        "work_update" => serde_json::from_value(args).map(Request::WorkUpdate),
        "agent_query" => serde_json::from_value(args).map(Request::AgentQuery),
        "agent_execute" => serde_json::from_value(args).map(Request::AgentExecute),
        _ => return Err("UNKNOWN_TOOL".into()),
    }
    .map_err(|_| "WORK_INVALID_ARGUMENT".to_string())?;
    match &result {
        Request::WorkQuery(WorkQuery::List { limit: Some(n), .. })
        | Request::AgentQuery(AgentQuery::List { limit: Some(n), .. })
            if !(1..=100).contains(n) =>
        {
            return Err("WORK_INVALID_ARGUMENT".into());
        }
        Request::AgentQuery(AgentQuery::Observe {
            wait_ms: Some(n), ..
        }) if *n > 20000 => return Err("WORK_INVALID_ARGUMENT".into()),
        _ => {}
    }
    Ok(result)
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum WorkStatus {
    Active,
    Completed,
    Failed,
    Cancelled,
}
#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Decision {
    Accepted,
}
#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AcceptanceView {
    decision: Decision,
    summary: String,
    execution_ids: Vec<String>,
    accepted_at: i64,
}
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct WorkRunView {
    work_run_id: String,
    workspace_id: String,
    title: String,
    goal: Option<String>,
    status: WorkStatus,
    revision: i64,
    acceptance: Option<AcceptanceView>,
    created_at: i64,
    updated_at: i64,
    completed_at: Option<i64>,
}
impl TryFrom<crate::agent::store::WorkRunRecord> for WorkRunView {
    type Error = String;
    fn try_from(row: crate::agent::store::WorkRunRecord) -> Result<Self, String> {
        let invalid = || "WORK_OPERATION_FAILED".to_string();
        let status = match row.status.as_str() {
            "active" => WorkStatus::Active,
            "completed" => WorkStatus::Completed,
            "failed" => WorkStatus::Failed,
            "cancelled" => WorkStatus::Cancelled,
            _ => return Err(invalid()),
        };
        let acceptance = row
            .acceptance_json
            .as_deref()
            .map(serde_json::from_str::<AcceptanceView>)
            .transpose()
            .map_err(|_| invalid())?;
        if acceptance
            .as_ref()
            .is_some_and(|a| a.summary.trim().is_empty())
        {
            return Err(invalid());
        }
        Ok(Self {
            work_run_id: row.id,
            workspace_id: row.workspace_id,
            title: row.title,
            goal: row.goal,
            status,
            revision: row.revision,
            acceptance,
            created_at: row.created_at,
            updated_at: row.updated_at,
            completed_at: row.completed_at,
        })
    }
}
#[derive(Serialize, JsonSchema)]
#[serde(untagged, rename_all_fields = "camelCase")]
pub(super) enum WorkData {
    One { work_run: WorkRunView },
    Many { work_runs: Vec<WorkRunView> },
}
#[derive(Serialize, JsonSchema)]
pub(super) struct WorkError {
    pub code: String,
    pub message: String,
}
#[derive(Serialize, JsonSchema)]
#[serde(untagged)]
pub(super) enum WorkEnvelope {
    Success { ok: bool, data: WorkData },
    Failure { ok: bool, error: WorkError },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{
        product::{AgentProductService, AgentQueryAction},
        store::{StateStore, transactions::product::WorkspaceSnapshot},
    };
    use serde_json::json;

    #[tokio::test]
    async fn compact_projection_omits_empty_options_and_preserves_sparse_diagnostics() {
        let dir = tempfile::tempdir().unwrap();
        let store = StateStore::open(dir.path().into()).await.unwrap();
        store
            .product_create_fresh(
                "E".into(),
                "A".into(),
                "key".into(),
                "prompt".into(),
                "W".into(),
                Some(WorkspaceSnapshot {
                    id: "W".into(),
                    root: dir.path().to_string_lossy().into(),
                }),
                1,
            )
            .await
            .unwrap();
        let product = AgentProductService::new(store);
        for (code, message, expected) in [
            (None, None, None),
            (Some("CODE"), None, Some(json!({"code":"CODE"}))),
            (None, Some("message"), Some(json!({"message":"message"}))),
            (
                Some("CODE"),
                Some("message"),
                Some(json!({"code":"CODE","message":"message"})),
            ),
        ] {
            let ProductData::Execution(mut view) = product
                .agent_query(AgentQueryAction::Get {
                    execution_id: "E".into(),
                    include_result: None,
                })
                .await
                .unwrap()
            else {
                panic!("expected detail")
            };
            view.unchanged = Some(true);
            view.revision = "legacy alias must not be used".into();
            view.control_revision = "current-control-token".into();
            view.next_action = None;
            view.attention = "none".into();
            view.error_code = code.map(str::to_owned);
            view.error_message = message.map(str::to_owned);
            let result =
                serde_json::to_value(QueryData::project(ProductData::Execution(view), true))
                    .unwrap();
            assert_eq!(result.get("error"), expected.as_ref());
            assert_eq!(result["revision"], "current-control-token");
            assert_eq!(result["unchanged"], true);
            assert_eq!(result["progress"], json!({"phase":"pending"}));
            for field in ["nextAction", "attention", "finalResult"] {
                assert!(result.get(field).is_none());
            }
        }
        let error = super::super::query_response(
            Err(ProductError::new(
                "SQL /private/path secret".into(),
                Some("E".into()),
            )),
            false,
        );
        assert_eq!(
            error,
            json!({"ok":false,"error":{"code":"AGENT_OPERATION_FAILED","message":"AGENT_OPERATION_FAILED","executionId":"E"}})
        );
    }
}
