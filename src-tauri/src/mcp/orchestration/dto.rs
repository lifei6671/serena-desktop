use crate::agent::{
    activity::{ActivitySilence, ToolCategory},
    execution::AgentTaskRole,
    product::{
        ExecutionView, MismatchKind, NextAction, ProductData, ProductError, ProgressPhase,
        ProviderCatalogSnapshot, ProviderProduct, UsageProduct, WakeOn, WakeReason,
    },
    provider::ProviderId,
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
    /// 直接复用 Product 目录，MCP 不建立第二套 Provider Authority。
    Providers(ProviderCatalogSnapshot),
    Detail(Box<ExecutionView>),
    List {
        executions: Vec<QuerySummary>,
    },
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
    /// 顶层 Provider identity 来自 Execution，Usage 不重复此字段。
    provider: ProviderProduct,
    /// 列表直接携带公共 Usage 摘要，调用方无需为此再读取 detail。
    usage: UsageProduct,
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
            provider: view.provider,
            usage: view.usage,
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
    /// Observe 与 detail 使用同一完整公共 Usage 投影。
    usage: UsageProduct,
    status: String,
    /// Current controlRevision, reusable as the next knownRevision opaque token.
    revision: String,
    /// Opaque Activity Revision for the next activity-mode Observe request.
    activity_revision: String,
    unchanged: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "WakeReason")]
    wake_reason: Option<WakeReason>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "MismatchKind")]
    mismatch_kind: Option<MismatchKind>,
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
    #[schemars(with = "Option<String>")]
    summary_code: Option<String>,
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
            usage: view.usage,
            status: view.status,
            revision: view.control_revision,
            activity_revision: view.activity_revision,
            unchanged: view.unchanged.expect("observe_wait always sets unchanged"),
            wake_reason: view.wake_reason,
            mismatch_kind: view.mismatch_kind,
            result_available: view.result_available,
            result_completeness: view.result_completeness,
            progress: QueryProgress {
                phase: view.progress.phase,
                summary_code: view.progress.summary_code,
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
    #[serde(default)]
    pub command_run_ids: Vec<String>,
}
#[derive(Deserialize, JsonSchema)]
#[serde(
    tag = "action",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(super) enum AgentQuery {
    /// 空结构体保留 deny_unknown_fields，目录查询只接受 action。
    Providers {},
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
        known_control_revision: Option<String>,
        known_activity_revision: Option<String>,
        wake_on: Option<WakeOn>,
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
#[schemars(transform = start_routing_schema)]
pub(super) enum AgentExecute {
    Start {
        work_run_id: String,
        workspace_id: String,
        request_key: String,
        prompt: String,
        context: Option<Context>,
        /// 调用方显式声明角色；不根据 Prompt 猜测。
        #[schemars(with = "String")]
        task_role: AgentTaskRole,
        /// 必须匹配 Local Human 的 roleRouting。
        provider_id: ProviderId,
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
/// DTO 的必填字段直接进入公开 Start branch；补充 ProviderId 的字符串边界。
fn start_routing_schema(schema: &mut schemars::Schema) {
    let branches = schema
        .as_object_mut()
        .expect("action schema is an object")
        .get_mut("oneOf")
        .expect("action branches")
        .as_array_mut()
        .expect("action branches array");
    let start = branches
        .iter_mut()
        .find(|branch| branch["properties"]["action"]["const"] == "start")
        .expect("public Start branch");
    start["properties"]["taskRole"] = serde_json::json!({
        "type":"string",
        "enum":[AgentTaskRole::Development, AgentTaskRole::Testing, AgentTaskRole::Review,
                AgentTaskRole::Analysis, AgentTaskRole::General]
    });
    // 与 ProviderId 的 char::is_whitespace/is_control 闭集相同，允许未知但合法的身份。
    start["properties"]["providerId"] = serde_json::json!({
        "type":"string", "minLength":1,
        "not":{"pattern":r"[\u0000-\u0020\u007f-\u009f\u00a0\u1680\u2000-\u200a\u2028\u2029\u202f\u205f\u3000]"}
    });
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
    // Start 的 Workspace 由请求显式授权；先保留缺失、空值和类型的稳定错误语义。
    if name == "agent_execute"
        && matches!(args.get("action").and_then(Value::as_str), Some("start"))
    {
        crate::mcp::registry::parse_workspace_id(&args)?;
        if args.get("taskRole").is_none_or(Value::is_null)
            || args.get("providerId").is_none_or(Value::is_null)
        {
            return Err("INVALID_PARAMS".into());
        }
    }
    // 顶层新增/未知参数属于 MCP 参数错误；嵌套旧 context 仍保留历史错误语义。
    let has_unknown_start_field = name == "agent_execute"
        && args.get("action").and_then(Value::as_str) == Some("start")
        && args.as_object().is_some_and(|object| {
            object.keys().any(|key| {
                !matches!(
                    key.as_str(),
                    "action"
                        | "workRunId"
                        | "workspaceId"
                        | "requestKey"
                        | "prompt"
                        | "context"
                        | "taskRole"
                        | "providerId"
                )
            })
        });
    let has_execute_routing = name == "agent_execute"
        && (args.get("taskRole").is_some() || args.get("providerId").is_some());
    let is_agent_observe = name == "agent_query"
        && matches!(args.get("action").and_then(Value::as_str), Some("observe"));
    let is_provider_query = name == "agent_query"
        && matches!(
            args.get("action").and_then(Value::as_str),
            Some("providers")
        );
    let result = match name {
        "work_query" => serde_json::from_value(args).map(Request::WorkQuery),
        "work_update" => serde_json::from_value(args).map(Request::WorkUpdate),
        "agent_query" => serde_json::from_value(args).map(Request::AgentQuery),
        "agent_execute" => serde_json::from_value(args).map(Request::AgentExecute),
        _ => return Err("UNKNOWN_TOOL".into()),
    }
    .map_err(|_| {
        if is_provider_query || has_execute_routing || has_unknown_start_field {
            "INVALID_PARAMS".to_string()
        } else if is_agent_observe {
            "AGENT_OBSERVE_INVALID_ARGUMENT".to_string()
        } else {
            "WORK_INVALID_ARGUMENT".to_string()
        }
    })?;
    match &result {
        Request::WorkQuery(WorkQuery::List { limit: Some(n), .. })
        | Request::AgentQuery(AgentQuery::List { limit: Some(n), .. })
            if !(1..=100).contains(n) =>
        {
            return Err("WORK_INVALID_ARGUMENT".into());
        }
        Request::AgentQuery(AgentQuery::Observe {
            execution_id,
            known_revision,
            known_control_revision,
            known_activity_revision,
            wait_ms,
            ..
        // Observe 继承既有 executionId 语法，但统一映射至 Observe 专用错误码。
        }) if execution_id.is_empty()
            || execution_id
                .chars()
                .any(|character| character.is_whitespace() || character.is_control())
            || wait_ms.is_some_and(|n| n > 20_000)
            || known_revision
                .as_ref()
                .is_some_and(|token| token.trim().is_empty())
            || known_control_revision
                .as_ref()
                .is_some_and(|token| token.trim().is_empty())
            || known_activity_revision
                .as_ref()
                .is_some_and(|token| token.trim().is_empty()) =>
        {
            return Err("AGENT_OBSERVE_INVALID_ARGUMENT".into());
        }
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
    #[serde(default)]
    command_run_ids: Vec<String>,
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
                    generation: 1,
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
            // 紧凑 Observe 投影不得回流 Product detail 中的私有定位与 Provider 标识。
            view.prompt = "P3_PRIVATE_PROMPT_MARKER".into();
            view.canonical_workspace_root = "P3_PRIVATE_ROOT_MARKER".into();
            view.thread_id = Some("P3_PRIVATE_THREAD_MARKER".into());
            view.turn_id = Some("P3_PRIVATE_TURN_MARKER".into());
            view.provider_session_label = Some("P3_PRIVATE_PROVIDER_MARKER".into());
            view.next_action = None;
            view.attention = "none".into();
            view.error_code = code.map(str::to_owned);
            view.error_message = message.map(str::to_owned);
            let activity_revision = view.activity_revision.clone();
            let result =
                serde_json::to_value(QueryData::project(ProductData::Execution(view), true))
                    .unwrap();
            assert_eq!(result.get("error"), expected.as_ref());
            assert_eq!(result["revision"], "current-control-token");
            assert_eq!(result["unchanged"], true);
            assert_eq!(result["activityRevision"], activity_revision);
            assert_eq!(
                result["progress"],
                json!({"phase":"pending","summaryCode":null})
            );
            for field in ["nextAction", "attention", "finalResult"] {
                assert!(result.get(field).is_none());
            }
            for marker in [
                "P3_PRIVATE_PROMPT_MARKER",
                "P3_PRIVATE_ROOT_MARKER",
                "P3_PRIVATE_THREAD_MARKER",
                "P3_PRIVATE_TURN_MARKER",
                "P3_PRIVATE_PROVIDER_MARKER",
            ] {
                assert!(
                    !result.to_string().contains(marker),
                    "compact projection leaked {marker}"
                );
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

#[cfg(test)]
mod start_compatibility_tests {
    use super::*;
    use serde_json::json;
    use std::{
        io::Write,
        process::{Command, Stdio},
    };

    /// 完整矩阵同时走 serde、无业务依赖的 parse、Registry 和公开 JSON Schema。
    #[test]
    fn start_routing_serde_parser_and_schema_matrix() {
        let legacy = json!({"action":"start","workRunId":"w","workspaceId":"W","requestKey":"k","prompt":"p"});
        let mut cases = vec![(legacy.clone(), false)];
        for role in ["development", "testing", "review", "analysis", "general"] {
            for provider in [
                "codex",
                "codebuddy",
                "unregistered-provider",
                "提供者",
                "a\u{feff}b",
            ] {
                let mut args = legacy.clone();
                args["taskRole"] = json!(role);
                args["providerId"] = json!(provider);
                cases.push((args, true));
            }
        }
        let explicit = cases[1].0.clone();
        for field in ["taskRole", "providerId"] {
            let mut half = explicit.clone();
            half.as_object_mut().unwrap().remove(field);
            assert_eq!(
                parse("agent_execute", half.clone()).err().unwrap(),
                "INVALID_PARAMS"
            );
            cases.push((half, false));
            for value in [
                Value::Null,
                json!(1),
                json!(true),
                json!([]),
                json!({}),
                json!(""),
            ] {
                let mut args = explicit.clone();
                args[field] = value;
                cases.push((args, false));
            }
        }
        for role in ["Testing", "unknown", " testing"] {
            let mut args = explicit.clone();
            args["taskRole"] = json!(role);
            cases.push((args, false));
        }
        // 覆盖 Unicode 空白、C0/C1 控制；不是只测 ASCII 空格。
        for provider in [
            " ",
            "a b",
            "a\tb",
            "a\nb",
            "a\n",
            "a\r",
            "a\r\n",
            "a\u{2028}",
            "a\u{0}b",
            "a\u{7f}b",
            "a\u{85}b",
            "a\u{a0}b",
            "a\u{1680}b",
            "a\u{2007}b",
            "a\u{2028}b",
            "a\u{202f}b",
            "a\u{205f}b",
            "a\u{3000}b",
        ] {
            let mut args = explicit.clone();
            args["providerId"] = json!(provider);
            cases.push((args, false));
        }
        for base in [&legacy, &explicit] {
            let mut unknown = base.clone();
            unknown["extra"] = json!(true);
            assert_eq!(
                parse("agent_execute", unknown.clone()).err().unwrap(),
                "INVALID_PARAMS"
            );
            cases.push((unknown, false));
            let mut private = base.clone();
            private["routing"] = json!("LegacyGeneral");
            cases.push((private, false));
            let mut context = base.clone();
            context["context"] =
                json!({"summary":"host summary","files":[{"path":"../escape","sha256":"bad"}]});
            cases.push((context, base.get("taskRole").is_some())); // 保留 transport 形态校验，路径/hash Authority 不在 parser。
            for invalid in [
                json!({"summary":null}),
                json!({"unknown":true}),
                json!({"files":[{"path":"p"}]}),
            ] {
                let mut args = base.clone();
                args["context"] = invalid;
                cases.push((args, false));
            }
        }
        for action in ["continue", "cancel", "resume_pending"] {
            let base = if action == "continue" {
                json!({"action":action,"workRunId":"w","parentExecutionId":"e","requestKey":"k","prompt":"p"})
            } else {
                json!({"action":action,"workRunId":"w","executionId":"e"})
            };
            cases.push((base.clone(), true));
            for pair in [
                json!({"taskRole":"testing"}),
                json!({"providerId":"codex"}),
                json!({"taskRole":"testing","providerId":"codex"}),
                json!({"taskRole":null}),
                json!({"providerId":null}),
                json!({"taskRole":null,"providerId":null}),
            ] {
                let mut args = base.clone();
                args.as_object_mut()
                    .unwrap()
                    .extend(pair.as_object().unwrap().clone());
                cases.push((args, false));
            }
        }
        for (args, valid) in &cases {
            assert_eq!(
                serde_json::from_value::<AgentExecute>(args.clone()).is_ok(),
                *valid,
                "serde: {args}"
            );
            assert_eq!(
                parse("agent_execute", args.clone()).is_ok(),
                *valid,
                "parse: {args}"
            );
            assert_eq!(
                crate::mcp::registry::validate("agent_execute", args).is_ok(),
                *valid,
                "registry: {args}"
            );
            if !valid && (args.get("taskRole").is_some() || args.get("providerId").is_some()) {
                assert_eq!(
                    parse("agent_execute", args.clone()).err().unwrap(),
                    "INVALID_PARAMS"
                );
            }
        }
        let tool = super::super::descriptors()
            .into_iter()
            .find(|tool| tool.name == "agent_execute")
            .unwrap();
        // 直接检查真正公开的 generated Start branch，防止字段只存在于 parser。
        let public_schema = serde_json::to_value(&tool.input_schema).unwrap();
        let branches = public_schema["oneOf"].as_array().unwrap();
        let start = branches
            .iter()
            .find(|branch| branch["properties"]["action"]["const"] == "start")
            .unwrap();
        for field in ["taskRole", "providerId"] {
            assert!(
                start["properties"].get(field).is_some(),
                "missing exposed {field}"
            );
            assert!(
                start["required"]
                    .as_array()
                    .unwrap()
                    .contains(&json!(field)),
                "not required {field}"
            );
        }
        assert_eq!(start["additionalProperties"], false);
        for branch in branches
            .iter()
            .filter(|branch| branch["properties"]["action"]["const"] != "start")
        {
            assert!(branch["properties"].get("taskRole").is_none());
            assert!(branch["properties"].get("providerId").is_none());
        }
        println!(
            "CB3_EXECUTE_DESCRIPTOR={}",
            serde_json::to_string(&tool).unwrap()
        );
        println!("CB3_START_MATRIX={}", json!(cases));
        let mut child = Command::new("node").current_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap())
            .args(["-e", r#"
const Ajv = require('ajv'); let text='';
process.stdin.on('data', c => text += c);
process.stdin.on('end', () => {
 const {schema,cases}=JSON.parse(text); delete schema.$schema;
 const check=new Ajv().compile(schema);
 for(const [args,valid] of cases) if(check(args)!==valid) throw Error(JSON.stringify({args,valid,errors:check.errors}));
});
"#]).stdin(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(
                json!({"schema":tool.input_schema,"cases":cases})
                    .to_string()
                    .as_bytes(),
            )
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        println!("CB3_START_MATRIX_COUNT={}", cases.len());
    }

    /// Workspace 仍优先校验；完整 pair 沿用显式 Start 的 context 参数错误码。
    #[test]
    fn explicit_start_workspace_and_context_validation_regression() {
        let base = json!({"action":"start","workRunId":"w","workspaceId":"W","requestKey":"k","prompt":"p","taskRole":"general","providerId":"codex"});
        let mut missing = base.clone();
        missing.as_object_mut().unwrap().remove("workspaceId");
        assert_eq!(
            parse("agent_execute", missing).err().unwrap(),
            "WORKSPACE_CONTEXT_REQUIRED"
        );
        let mut null_workspace = base.clone();
        null_workspace["workspaceId"] = Value::Null;
        assert_eq!(
            parse("agent_execute", null_workspace).err().unwrap(),
            "WORKSPACE_CONTEXT_REQUIRED"
        );
        for workspace in [json!(""), json!(" "), json!(false), json!(1)] {
            let mut args = base.clone();
            args["workspaceId"] = workspace;
            assert!(
                parse("agent_execute", args)
                    .err()
                    .unwrap()
                    .starts_with("INVALID_PARAMS")
            );
        }
        for context in [json!({"summary":null}), json!({"unknown":true})] {
            let mut args = base.clone();
            args["context"] = context;
            assert_eq!(
                parse("agent_execute", args).err().unwrap(),
                "INVALID_PARAMS"
            );
        }
    }

    /// 无业务 Authority 时也能解析显式 general 和未知但合法的 Provider 身份。
    #[test]
    fn start_routing_intent_preserves_input_without_business_authority() {
        for role in [AgentTaskRole::General, AgentTaskRole::Testing] {
            let args = json!({"action":"start","workRunId":"nonexistent","workspaceId":"nonexistent","requestKey":"k","prompt":"p","taskRole":role,"providerId":"not-registered"});
            let Request::AgentExecute(AgentExecute::Start {
                task_role,
                provider_id,
                ..
            }) = parse("agent_execute", args).unwrap()
            else {
                panic!("expected Start")
            };
            assert_eq!(task_role, role);
            assert_eq!(provider_id.as_str(), "not-registered");
        }
    }
}
