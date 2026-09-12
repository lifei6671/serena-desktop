//! Read-only control-plane projection. Never grants dispatch/replay/release authority.
use super::*;
use crate::agent::store::ExecutionRecord;

#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DirectedNextAction {
    #[serde(flatten)]
    pub instruction: NextAction,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_id: Option<String>,
}
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DispatchCertainty {
    NotDispatched,
    Dispatched,
    Uncertain,
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ControlReceipt {
    pub request_accepted: bool,
    pub provider_invoked: Option<bool>,
    pub dispatch_certainty: DispatchCertainty,
    pub next_action: Option<DirectedNextAction>,
}
impl ControlReceipt {
    pub(super) fn rejected(next: Option<NextAction>, id: Option<String>) -> Self {
        Self {
            request_accepted: false,
            provider_invoked: Some(false),
            dispatch_certainty: DispatchCertainty::NotDispatched,
            next_action: next.map(|instruction| DirectedNextAction {
                instruction,
                execution_id: id,
            }),
        }
    }
    fn accepted_unreadable(id: String) -> Self {
        Self {
            request_accepted: true,
            provider_invoked: None,
            dispatch_certainty: DispatchCertainty::Uncertain,
            next_action: Some(DirectedNextAction {
                instruction: NextAction::Observe { wait_ms: 20_000 },
                execution_id: Some(id),
            }),
        }
    }
    pub(super) fn accepted(row: &ExecutionRecord, next: Option<NextAction>) -> Self {
        let terminal_evidence = matches!(
            row.provider_terminal_status.as_deref(),
            Some("completed" | "failed" | "interrupted")
        ) && row.provider_terminal_evidence_at.is_some()
            && row.runtime_instance_id.is_some()
            && row.provider_terminal_evidence_runtime_instance_id == row.runtime_instance_id;
        let (provider_invoked, dispatch_certainty) = if terminal_evidence
            || row.dispatch_state == "dispatched"
        {
            (Some(true), DispatchCertainty::Dispatched)
        } else if row.dispatch_state == "not_dispatched" && row.provider_terminal_status.is_none() {
            (Some(false), DispatchCertainty::NotDispatched)
        } else {
            (None, DispatchCertainty::Uncertain)
        };
        Self {
            request_accepted: true,
            provider_invoked,
            dispatch_certainty,
            next_action: next.map(|instruction| DirectedNextAction {
                instruction,
                execution_id: Some(row.id.clone()),
            }),
        }
    }
}

impl AgentProductService {
    /// Transport projection only; durable acceptance always comes from the existing view.
    pub(crate) async fn adapter_error_response(&self, mut error: ProductError) -> Value {
        if let Some(id) = error.accepted_execution_id.clone() {
            let control = self
                .observe(id.clone(), false)
                .await
                .map(|view| view.control)
                .unwrap_or_else(|_| ControlReceipt::accepted_unreadable(id.clone()));
            error.execution_id = Some(id);
            error.message = error.code.clone();
            return json!({"ok":false,"error":error,"control":control});
        }
        adapter_rejection(error)
    }
    pub(super) async fn error_response(
        &self,
        action: Action,
        workspace: Option<WorkspaceSnapshot>,
        mut error: ProductError,
    ) -> Value {
        let context = self
            .store
            .product_control_context(action, workspace, error.code.clone())
            .await;
        let rejected = error.accepted_execution_id.is_none()
            && matches!(
                error.code.as_str(),
                "AGENT_INVALID_ARGUMENT"
                    | "AGENT_NO_ACTIVE_WORKSPACE"
                    | "AGENT_WORKSPACE_CHANGED"
                    | "AGENT_LINEAGE_CONFLICT"
                    | "AGENT_REQUEST_KEY_CONFLICT"
                    | "AGENT_BUSY"
                    | "WORKSPACE_CLAIM_CONFLICT"
                    | "AGENT_CONTINUE_NOT_ALLOWED"
            );
        let accepted_id = if rejected {
            None
        } else {
            context
                .as_ref()
                .ok()
                .and_then(|c| c.accepted_id.clone())
                .or(error.accepted_execution_id.clone())
        };
        let control = if let Some(id) = accepted_id {
            error.execution_id = Some(id.clone());
            let mut receipt = self
                .observe(id.clone(), false)
                .await
                .map(|v| v.control)
                .unwrap_or_else(|_| ControlReceipt::accepted_unreadable(id.clone()));
            if matches!(
                error.code.as_str(),
                "AGENT_MANUAL_RESOLUTION_REQUIRED" | "AGENT_RUNTIME_QUARANTINED"
            ) {
                receipt.next_action = Some(DirectedNextAction {
                    instruction: NextAction::ManualResolution,
                    execution_id: Some(id),
                });
            }
            Some(receipt)
        } else {
            let related = context.as_ref().ok().and_then(|c| c.related_id.clone());
            let related_unknown = context.as_ref().is_ok_and(|c| c.related_unknown);
            let blocker = context.as_ref().ok().and_then(|c| c.blocker_id.clone());
            let (next, id) = match error.code.as_str() {
                "AGENT_INVALID_ARGUMENT" | "AGENT_REQUEST_KEY_CONFLICT" => {
                    (Some(NextAction::CorrectInput), None)
                }
                "AGENT_NO_ACTIVE_WORKSPACE" | "AGENT_WORKSPACE_CHANGED" => {
                    (Some(NextAction::ActivateWorkspace), None)
                }
                "AGENT_LINEAGE_CONFLICT" | "AGENT_CONTINUE_NOT_ALLOWED" => (
                    related.as_ref().map(|_| {
                        if related_unknown {
                            NextAction::ManualResolution
                        } else {
                            NextAction::Observe { wait_ms: 20_000 }
                        }
                    }),
                    related,
                ),
                "AGENT_BUSY" | "WORKSPACE_CLAIM_CONFLICT" => (
                    Some(if blocker.is_some() {
                        NextAction::Observe { wait_ms: 20_000 }
                    } else {
                        NextAction::List
                    }),
                    blocker,
                ),
                "AGENT_MANUAL_RESOLUTION_REQUIRED" | "AGENT_RUNTIME_QUARANTINED" => {
                    (Some(NextAction::ManualResolution), related)
                }
                _ => (None, None),
            };
            // If storage itself is unavailable, do not assert a false non-acceptance.
            if rejected || context.is_ok() {
                Some(ControlReceipt::rejected(next, id))
            } else {
                None
            }
        };
        serde_json::to_value(Envelope::Failure {
            ok: false,
            error,
            control,
        })
        .unwrap()
    }
}

pub(crate) fn adapter_rejection(mut error: ProductError) -> Value {
    let next = match error.code.as_str() {
        "WORK_INVALID_ARGUMENT" | "CONTEXT_STALE" | "EXECUTION_REQUEST_KEY_CONFLICT" => {
            Some(NextAction::CorrectInput)
        }
        "WORKSPACE_CONTEXT_MISMATCH" | "AGENT_NO_ACTIVE_WORKSPACE" | "AGENT_WORKSPACE_CHANGED" => {
            Some(NextAction::ActivateWorkspace)
        }
        _ => None,
    };
    error.message = error.code.clone();
    json!({"ok":false,"error":error,"control":ControlReceipt::rejected(next, None)})
}
