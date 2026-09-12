//! Public orchestration transport, without Provider or lifecycle logic.
mod dto;
use super::Broker;
use crate::agent::{
    product::{self, AgentExecuteAction, AgentQueryAction, ProductData},
    work,
};
use dto::*;
use rmcp::{
    model::{Tool, ToolAnnotations},
    schemars::{self, JsonSchema},
};
use serde_json::{Value, json};

pub const NAMES: [&str; 4] = ["work_query", "work_update", "agent_query", "agent_execute"];
pub fn contains(name: &str) -> bool {
    NAMES.contains(&name)
}
pub fn validate(name: &str, args: &Value) -> Result<(), String> {
    parse(name, args.clone()).map(|_| ())
}
fn work_success(data: WorkData) -> Value {
    serde_json::to_value(WorkEnvelope::Success { ok: true, data }).expect("Work view serialization")
}
fn work_failure(error: &str) -> Value {
    let code = [
        "WORK_NOT_FOUND",
        "WORK_NOT_ACTIVE",
        "WORK_HAS_ACTIVE_EXECUTIONS",
        "WORK_ACCEPTANCE_REQUIRED",
        "EXECUTION_NOT_IN_WORK",
        "WORK_INVALID_ARGUMENT",
        "WORKSPACE_CONTEXT_MISMATCH",
        "AGENT_DISABLED",
        "BACKEND_UNAVAILABLE",
    ]
    .into_iter()
    .find(|code| error == *code || error.starts_with(&format!("{code}:")))
    .unwrap_or("WORK_OPERATION_FAILED");
    serde_json::to_value(WorkEnvelope::Failure {
        ok: false,
        error: WorkError {
            code: code.into(),
            message: code.into(),
        },
    })
    .expect("Work error serialization")
}
pub(super) fn failure(name: &str, code: &str) -> Value {
    if name.starts_with("work_") {
        work_failure(code)
    } else if name == "agent_query" {
        query_response(Err(code.to_string().into()), false)
    } else {
        product::adapter_rejection(code.to_string().into())
    }
}
fn query_response(result: Result<ProductData, product::ProductError>, observe: bool) -> Value {
    let envelope = match result {
        Ok(data) => QueryEnvelope::Success {
            ok: true,
            data: QueryData::project(data, observe),
        },
        Err(mut error) => {
            // Preserve the existing adapter's sanitized public error message.
            error.message = error.code.clone();
            QueryEnvelope::Failure { ok: false, error }
        }
    };
    serde_json::to_value(envelope).expect("Query view serialization")
}

fn query_output_schema() -> Value {
    let mut output = serde_json::to_value(
        schemars::generate::SchemaSettings::draft07()
            .for_serialize()
            .into_generator()
            .into_root_schema_for::<QueryEnvelope>(),
    )
    .unwrap();
    output["type"] = json!("object");
    output["anyOf"][0]["properties"]["ok"] = json!({"const":true});
    output["anyOf"][1]["properties"]["ok"] = json!({"const":false});
    output["definitions"]["ProductError"]["additionalProperties"] = json!(false);
    output["definitions"]["ProductError"]["properties"]["executionId"]["type"] = json!("string");
    output["definitions"]["QueryDiagnostic"]["minProperties"] = json!(1);
    output
}
pub fn descriptors() -> Vec<Tool> {
    fn schema<T: JsonSchema>() -> Value {
        let mut v = serde_json::to_value(schemars::schema_for!(T)).unwrap();
        v["type"] = json!("object");
        v
    }
    let mut work_output = serde_json::to_value(
        schemars::generate::SchemaSettings::default()
            .for_serialize()
            .into_generator()
            .into_root_schema_for::<WorkEnvelope>(),
    )
    .unwrap();
    work_output["type"] = json!("object");
    work_output["anyOf"][0]["properties"]["ok"] = json!({"const":true});
    work_output["anyOf"][1]["properties"]["ok"] = json!({"const":false});
    [
        ("work_query", "【做什么】\n只查询 Work 业务容器，不执行任务。\n\n【什么时候使用】\nget 查询单个 Work；list 查询列表。\n\n【关键约束】\n只读；不要求当前活动 Workspace。", schema::<WorkQuery>(), work_output.clone(), true, false),
        ("work_update", "【做什么】\nbegin 创建 Work，finish 提交 Host Acceptance，cancel 关闭 Work。\n\n【什么时候使用】\n管理业务任务容器。\n\n【关键约束】\nWork 不拥有 Workspace Claim；cancel 不等于取消 Execution。finish/cancel 不可逆。", schema::<WorkUpdate>(), work_output, false, false),
        ("agent_query", "【做什么】\n只读查询或 observe Work 内 Execution。\n\n【什么时候使用】\n耗时任务用 bounded observe；结果按需 includeResult，可重复读取。\n\n【关键约束】\nobserve 默认 15000ms、最大 20000ms，只等待 control 变化，不执行 Provider。", schema::<AgentQuery>(), query_output_schema(), true, false),
        ("agent_execute", "【做什么】\n执行已确定的修改或测试。\n\n【什么时候使用】\nChatGPT 负责 source/git/codegraph 分析与 Review；实际工程执行交给 Agent。\n\n【关键约束】\ncontinue 创建新 Execution，可复用 Thread；context 传递 Host 验证的 path+sha256 引用。start/continue 重试保留原 requestKey 和请求；cancel 取消指定 Execution，resume_pending 仅显式恢复允许首次派发的原 Execution。", schema::<AgentExecute>(), super::registry::agent_output_schema(), false, true),
    ].into_iter().map(|(name, description, input, output, read_only, open_world)| {
        let mut tool=Tool::new(name, description, input.as_object().unwrap().clone());
        tool.annotations=Some(ToolAnnotations::default().read_only(read_only).destructive(!read_only).idempotent(read_only).open_world(open_world));
        tool.output_schema=Some(output.as_object().unwrap().clone().into()); tool
    }).collect()
}

impl Broker {
    pub(super) async fn orchestration_operation(&self, name: &str, args: Value) -> Value {
        if !self.config().agent_enabled {
            return failure(name, "AGENT_DISABLED");
        }
        let Some(product) = self.product.get() else {
            return failure(name, "BACKEND_UNAVAILABLE");
        };
        let request = match parse(name, args) {
            Ok(request) => request,
            Err(error) => return failure(name, &error),
        };
        let needs_workspace = matches!(
            &request,
            Request::WorkUpdate(WorkUpdate::Begin { .. })
                | Request::AgentExecute(AgentExecute::Start { .. })
        );
        let workspace = if needs_workspace {
            self.workspace.read().await.as_ref().map(|active| {
                crate::agent::store::transactions::product::WorkspaceSnapshot {
                    id: active.workspace.id.clone(),
                    root: active.workspace.root.to_string_lossy().into_owned(),
                }
            })
        } else {
            None
        };
        let agent_result = match request {
            Request::WorkQuery(action) => {
                let action = match action {
                    WorkQuery::Get { work_run_id } => work::QueryAction::Get { work_run_id },
                    WorkQuery::List {
                        workspace_id,
                        limit,
                    } => work::QueryAction::List {
                        workspace_id,
                        limit,
                    },
                };
                let result =
                    product
                        .work_product()
                        .query(action)
                        .await
                        .and_then(|data| match data {
                            work::QueryData::WorkRun(row) => Ok(WorkData::One {
                                work_run: row.try_into()?,
                            }),
                            work::QueryData::List { work_runs } => Ok(WorkData::Many {
                                work_runs: work_runs
                                    .into_iter()
                                    .map(TryInto::try_into)
                                    .collect::<Result<_, String>>()?,
                            }),
                        });
                return match result {
                    Ok(data) => work_success(data),
                    Err(error) => work_failure(&error),
                };
            }
            Request::WorkUpdate(action) => {
                let action = match action {
                    WorkUpdate::Begin {
                        workspace_id,
                        title,
                        goal,
                    } => work::UpdateAction::Begin {
                        workspace_id,
                        title,
                        goal,
                    },
                    WorkUpdate::Finish {
                        work_run_id,
                        outcome,
                        acceptance,
                    } => work::UpdateAction::Finish {
                        work_run_id,
                        outcome: match outcome {
                            Outcome::Completed => work::FinishOutcome::Completed,
                            Outcome::Failed => work::FinishOutcome::Failed,
                        },
                        acceptance: acceptance.map(|a| work::HostAcceptance {
                            summary: a.summary,
                            execution_ids: a.execution_ids,
                        }),
                    },
                    WorkUpdate::Cancel { work_run_id } => {
                        work::UpdateAction::Cancel { work_run_id }
                    }
                };
                return match product
                    .work_product()
                    .update(action, workspace)
                    .await
                    .and_then(TryInto::try_into)
                {
                    Ok(work_run) => work_success(WorkData::One { work_run }),
                    Err(error) => work_failure(&error),
                };
            }
            Request::AgentQuery(action) => {
                let observe = matches!(&action, AgentQuery::Observe { .. });
                let result = product
                    .agent_query(match action {
                        AgentQuery::Get {
                            execution_id,
                            include_result,
                        } => AgentQueryAction::Get {
                            execution_id,
                            include_result,
                        },
                        AgentQuery::List { work_run_id, limit } => {
                            AgentQueryAction::List { work_run_id, limit }
                        }
                        AgentQuery::Observe {
                            execution_id,
                            known_revision,
                            wait_ms,
                            include_result,
                        } => AgentQueryAction::Observe {
                            execution_id,
                            known_revision,
                            wait_ms,
                            include_result,
                        },
                    })
                    .await;
                return query_response(result, observe);
            }
            Request::AgentExecute(action) => {
                let context_json = |context: Option<Context>| {
                    context.map(|c| serde_json::to_string(&c).expect("typed context serialization"))
                };
                let action = match action {
                    AgentExecute::Start {
                        work_run_id,
                        request_key,
                        prompt,
                        context,
                    } => AgentExecuteAction::Start {
                        work_run_id,
                        request_key,
                        prompt,
                        delegation_context_json: context_json(context),
                    },
                    AgentExecute::Continue {
                        work_run_id,
                        parent_execution_id,
                        request_key,
                        prompt,
                        context,
                    } => AgentExecuteAction::Continue {
                        work_run_id,
                        parent_execution_id,
                        request_key,
                        prompt,
                        delegation_context_json: context_json(context),
                    },
                    AgentExecute::Cancel {
                        work_run_id,
                        execution_id,
                    } => AgentExecuteAction::Cancel {
                        work_run_id,
                        execution_id,
                    },
                    AgentExecute::ResumePending {
                        work_run_id,
                        execution_id,
                    } => AgentExecuteAction::ResumePending {
                        work_run_id,
                        execution_id,
                    },
                };
                product
                    .agent_execute(action, workspace)
                    .await
                    .map(|view| ProductData::Execution(Box::new(view)))
            }
        };
        match agent_result {
            Ok(data) => product::success(data),
            Err(error) => product.adapter_error_response(error).await,
        }
    }
}
