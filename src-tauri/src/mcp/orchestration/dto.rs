use rmcp::schemars::{self, JsonSchema};
use serde::{Deserialize, Serialize};
use serde_json::Value;

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
