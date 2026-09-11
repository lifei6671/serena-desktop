//! All business mutations use BEGIN IMMEDIATE and the same transition core.
use super::*;
use crate::agent::activity::{ActivityPhase, ToolCategory};
use crate::agent::execution::state::*;
use serde_json::{Value, json};
pub mod product;

// One live Host owns dispatch. Covers separate StateStore connections as well as
// clones; DB facts remain authoritative. Never persisted as an Execution state.
static PENDING_DISPATCH: std::sync::LazyLock<Mutex<std::collections::HashSet<(String, String)>>> =
    std::sync::LazyLock::new(|| Mutex::new(std::collections::HashSet::new()));
pub(crate) struct PendingDispatchPermit((String, String));
impl Drop for PendingDispatchPermit {
    fn drop(&mut self) {
        PENDING_DISPATCH.lock().unwrap().remove(&self.0);
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct CreateOutcome {
    pub execution_id: String,
    pub execution: ExecutionRecord,
    pub created: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ClaimRecovery {
    PendingExplicitResume {
        execution_id: String,
    },
    Pending {
        execution_id: String,
    },
    Unknown {
        execution_id: String,
    },
    Released {
        execution_id: String,
    },
    Inconsistent {
        execution_id: String,
        code: &'static str,
    },
}

impl StateStore {
    pub(crate) async fn guard_pending_dispatch(
        &self,
        id: String,
    ) -> Result<PendingDispatchPermit, String> {
        let database = self.database_identity.to_string_lossy().to_lowercase();
        self.write(move |tx| {
            let row = execution_record(tx, &id)
                .map_err(|e| e.to_string())?
                .ok_or("EXECUTION_NOT_FOUND")?;
            if row.status != "dispatch_pending"
                || row.dispatch_state != "not_dispatched"
                || row.runtime_instance_id.is_some()
                || row.provider_terminal_status.is_some()
            {
                return Err("PENDING_RESUME_REJECTED".into());
            }
            owns_claim(tx, &id)?;
            let attempted: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM runtime_instances WHERE id=?1)",
                [format!("runtime-{id}")],
                |r| r.get(0),
            ).map_err(|e| e.to_string())?;
            if attempted {
                return Err("PENDING_RESUME_REJECTED".into());
            }
            let key = (database, id);
            if !PENDING_DISPATCH
                .lock()
                .map_err(|e| e.to_string())?
                .insert(key.clone())
            {
                return Err("PENDING_RESUME_REJECTED".into());
            }
            Ok(PendingDispatchPermit(key))
        })
        .await
    }
    /// Cancel and dispatch serialize on the same SQLite transaction boundary.
    pub(crate) async fn request_cancel(
        &self,
        id: String,
        now: i64,
    ) -> Result<ExecutionRecord, String> {
        self.write(move |tx| {
            let row = execution_record(tx, &id)
                .map_err(|e| e.to_string())?
                .ok_or("EXECUTION_NOT_FOUND")?;
            let mutation = if row.status == "dispatch_pending"
                && row.dispatch_state == "not_dispatched"
            {
                Some(Mutation::CancelBeforeDispatch)
            } else if row.provider_terminal_status.is_none()
                && (row.status == "running"
                    || (row.status == "dispatch_pending" && row.interrupt_requested_at.is_none()))
            {
                Some(Mutation::Event(Transition::RequestCancel))
            } else {
                None
            };
            if let Some(mutation) = mutation {
                transition_execution(tx, &id, row.revision, mutation, now)?;
            }
            execution_record(tx, &id)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "EXECUTION_NOT_FOUND".into())
        })
        .await
    }

    /// Single Provider event, serialized with concurrent cancellation intent.
    pub(crate) async fn provider_event(
        &self,
        id: String,
        event: Transition,
        now: i64,
    ) -> Result<(), String> {
        self.write(move |tx| {
            let row = load(tx, &id)?;
            transition_execution(tx, &id, row.revision, Mutation::Event(event), now)
        })
        .await
    }

    /// Latest safe Root Turn activity hint. It never changes lifecycle authority.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn execution_activity(
        &self,
        id: String,
        thread_id: String,
        turn_id: String,
        phase: ActivityPhase,
        tool_category: Option<ToolCategory>,
        observed_at: i64,
    ) -> Result<(), String> {
        if (phase == ActivityPhase::Provider) != tool_category.is_none() {
            return Err("INVALID_EXECUTION_ACTIVITY".into());
        }
        #[cfg(test)]
        if self.take_observability_failure(ObservabilityFault::Activity) {
            return Err("INJECTED_ACTIVITY_PERSISTENCE_FAILURE".into());
        }
        self.write(move |tx| {
            let row = execution_record(tx, &id)
                .map_err(|error| error.to_string())?
                .ok_or("EXECUTION_NOT_FOUND")?;
            if matches!(
                row.status.as_str(),
                "completed" | "failed" | "cancelled" | "interrupted"
            ) {
                return Ok(());
            }
            owns_claim(tx, &id)?;
            if row.thread_id.as_deref() != Some(thread_id.as_str())
                || row.turn_id.as_deref() != Some(turn_id.as_str())
            {
                return Err("EXECUTION_PROTOCOL_IDENTITY_MISMATCH".into());
            }
            let changed = tx
                .execute(
                    "UPDATE executions SET last_activity_at=?2,activity_phase=?3,tool_category=?4,revision=revision+1,updated_at=MAX(updated_at,?2) WHERE id=?1 AND revision=?5",
                    params![id, observed_at, phase.as_str(), tool_category.map(ToolCategory::as_str), row.revision],
                )
                .map_err(|error| error.to_string())?;
            if changed != 1 {
                return Err("EXECUTION_REVISION_CONFLICT".into());
            }
            Ok(())
        })
        .await
    }

    /// Diagnostic only: never creates Provider terminal or release evidence.
    pub(crate) async fn execution_diagnostic(&self, id: String, code: String, message: String, now: i64) -> Result<(), String> {
        #[cfg(test)]
        if code == "CODEX_PERMISSION_DENIED"
            && self.take_observability_failure(ObservabilityFault::PermissionDiagnostic)
        {
            return Err("INJECTED_PERMISSION_DIAGNOSTIC_FAILURE".into());
        }
        self.write(move |tx| {
            let row = load(tx, &id)?;
            if row.status.terminal() { return Ok(()); }
            owns_claim(tx, &id)?;
            tx.execute("UPDATE executions SET error_code=?2,error_message=?3,revision=revision+1,updated_at=?4 WHERE id=?1
                AND (?2 != 'CODEX_PROVIDER_FAILURE' OR error_code IS NULL OR error_code != 'CODEX_TURN_ERROR')
                AND (?2 != 'CODEX_PERMISSION_DENIED' OR error_code IS NULL OR error_code NOT IN ('CODEX_TURN_ERROR','CODEX_PROVIDER_FAILURE'))",
                params![id, code, message, now]).map_err(|e| e.to_string())?;
            Ok(())
        }).await
    }

    /// Fill identity from verified Provider responses without changing lifecycle or Runtime.
    pub(crate) async fn bind_protocol_identity(
        &self,
        id: String,
        revision: i64,
        runtime: String,
        thread: String,
        turn: Option<String>,
        now: i64,
    ) -> Result<(), String> {
        self.write(move |tx| {
            let row = execution_record(tx, &id).map_err(|e| e.to_string())?.ok_or("EXECUTION_NOT_FOUND")?;
            if row.revision != revision { return Err("EXECUTION_REVISION_CONFLICT".into()); }
            owns_claim(tx, &id)?;
            if row.runtime_instance_id.as_deref() != Some(&runtime) || thread.is_empty()
                || turn.as_ref().is_some_and(String::is_empty)
                || row.thread_id.as_ref().is_some_and(|v| v != &thread)
                || row.turn_id.as_ref().is_some_and(|v| Some(v) != turn.as_ref())
                || !matches!(row.status.as_str(), "dispatch_pending" | "running" | "cancel_requested" | "cancelling" | "finalizing" | "reconciling") {
                return Err("EXECUTION_PROTOCOL_IDENTITY_MISMATCH".into());
            }
            if row.thread_id.as_ref() == Some(&thread) && row.turn_id == turn { return Ok(()); }
            tx.execute("UPDATE executions SET thread_id=?2,turn_id=COALESCE(turn_id,?3),revision=revision+1,updated_at=?4 WHERE id=?1", params![id,thread,turn,now]).map_err(|e| e.to_string())?;
            Ok(())
        }).await
    }

    async fn write<T: Send + 'static>(
        &self,
        operation: impl FnOnce(&Transaction<'_>) -> Result<T, String> + Send + 'static,
    ) -> Result<T, String> {
        let connection = self.connection.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let mut connection = connection.lock().map_err(|e| e.to_string())?;
            let tx = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(|e| e.to_string())?;
            let result = operation(&tx)?;
            tx.commit().map_err(|e| e.to_string())?;
            #[cfg(test)]
            crash_checkpoint("after_commit");
            Ok(result)
        })
        .await
        .map_err(|e| e.to_string())?
    }

    pub async fn create_execution(
        &self,
        id: String,
        request: CanonicalRequest,
        now: i64,
    ) -> Result<CreateOutcome, String> {
        self.write(move |tx| create(tx, &id, &request, now)).await
    }

    pub async fn transition_execution(
        &self,
        id: String,
        revision: i64,
        event: Transition,
        now: i64,
    ) -> Result<(), String> {
        self.write(move |tx| transition_execution(tx, &id, revision, Mutation::Event(event), now))
            .await
    }

    pub async fn finalize_and_release_execution(
        &self,
        id: String,
        revision: i64,
        finalization: Finalization,
        now: i64,
    ) -> Result<(), String> {
        self.write(move |tx| {
            transition_execution(tx, &id, revision, Mutation::Finalize(finalization), now)
        })
        .await
    }

    pub async fn cancel_before_dispatch_and_release(
        &self,
        id: String,
        revision: i64,
        now: i64,
    ) -> Result<(), String> {
        self.write(move |tx| {
            transition_execution(tx, &id, revision, Mutation::CancelBeforeDispatch, now)
        })
        .await
    }

    /// Classify durable Claims before publishing the startup Product service.
    pub async fn recover_claims(&self, now: i64) -> Result<Vec<ClaimRecovery>, String> {
        self.write(move |tx| {
            // Claims are the authority for recovery, including legacy terminal rows.
            let mut statement = tx
                .prepare(
                    "SELECT execution_id FROM workspace_claims ORDER BY canonical_workspace_root",
                )
                .map_err(|e| e.to_string())?;
            let ids = statement
                .query_map([], |r| r.get::<_, String>(0))
                .map_err(|e| e.to_string())?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(|e| e.to_string())?;
            let mut recovered = Vec::new();
            for id in ids {
                let mut row = load(tx, &id)?;
                if row.dispatch == DispatchState::Dispatching {
                    transition_execution(
                        tx,
                        &id,
                        row.revision,
                        Mutation::Event(Transition::Dispatch {
                            to: DispatchState::Uncertain,
                            runtime_id: None,
                        }),
                        now,
                    )?;
                    row = load(tx, &id)?;
                }
                if row.status.terminal() {
                    if row.release_state == "complete"
                        && row.release_kind.is_some()
                        && row.release_json.is_some()
                    {
                        delete_claim(tx, &id)?;
                        recovered.push(ClaimRecovery::Released { execution_id: id });
                    } else {
                        recovered.push(ClaimRecovery::Inconsistent {
                            execution_id: id,
                            code: "WORKSPACE_CLAIM_INCONSISTENT",
                        });
                    }
                } else if row.status == Status::DispatchPending
                    && row.dispatch == DispatchState::NotDispatched
                    && row.runtime.is_none()
                    && row.terminal.is_none()
                    && !tx.query_row(
                        "SELECT EXISTS(SELECT 1 FROM runtime_instances WHERE id=?1)",
                        [format!("runtime-{id}")],
                        |r| r.get::<_, bool>(0),
                    ).map_err(|e| e.to_string())?
                {
                    if owns_claim(tx, &id).is_ok() {
                        recovered.push(ClaimRecovery::PendingExplicitResume { execution_id: id });
                    } else {
                        recovered.push(ClaimRecovery::Inconsistent {
                            execution_id: id,
                            code: "WORKSPACE_CLAIM_INCONSISTENT",
                        });
                    }
                } else if row.status == Status::Unknown {
                    recovered.push(ClaimRecovery::Unknown { execution_id: id });
                } else {
                    if matches!(
                        row.dispatch,
                        DispatchState::Uncertain | DispatchState::Dispatched
                    ) && row.status != Status::Reconciling
                    {
                        transition_execution(
                            tx,
                            &id,
                            row.revision,
                            Mutation::Event(Transition::Reconcile),
                            now,
                        )?;
                    }
                    recovered.push(ClaimRecovery::Pending { execution_id: id });
                }
            }
            Ok(recovered)
        })
        .await
    }
}

fn create(
    tx: &Transaction<'_>,
    id: &str,
    request: &CanonicalRequest,
    now: i64,
) -> Result<CreateOutcome, String> {
    let input = request.input();
    let prior: Option<(String, String)> = tx
        .query_row(
            "SELECT id, request_hash FROM executions WHERE agent_id=?1 AND request_key=?2",
            params![input.agent_id, input.request_key],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if let Some((execution_id, hash)) = prior {
        return if hash == request.request_hash() {
            Ok(CreateOutcome {
                execution: execution_record(tx, &execution_id)
                    .map_err(|e| e.to_string())?
                    .ok_or("EXECUTION_NOT_FOUND")?,
                execution_id,
                created: false,
            })
        } else {
            Err("EXECUTION_REQUEST_KEY_CONFLICT".into())
        };
    }
    let busy: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM executions WHERE agent_id=?1 AND status NOT IN ('completed','failed','cancelled','interrupted'))", [&input.agent_id], |r| r.get(0)).map_err(|e| e.to_string())?;
    if busy {
        return Err("AGENT_BUSY".into());
    }
    let snapshot_mismatch: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM executions WHERE agent_id=?1 AND
         (workspace_id IS NOT ?2 OR canonical_workspace_root IS NOT ?3 OR thread_id IS NOT ?4
          OR execution_profile_json IS NOT ?5 OR mode IS NOT ?6 OR provider != 'codex'))",
            params![
                input.agent_id,
                input.workspace_id,
                input.canonical_workspace_root,
                input.thread_id,
                request.execution_profile_json(),
                match input.mode {
                    crate::agent::execution::ExecutionMode::ReadOnly => "read_only",
                    crate::agent::execution::ExecutionMode::WorkspaceWrite => "workspace_write",
                }
            ],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if snapshot_mismatch {
        return Err("AGENT_SNAPSHOT_CONFLICT".into());
    }
    let claimed: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM workspace_claims WHERE canonical_workspace_root=?1)",
            [&input.canonical_workspace_root],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if claimed {
        return Err("WORKSPACE_CLAIM_CONFLICT".into());
    }
    insert_execution(tx, id, now, request).map_err(|e| e.to_string())?;
    tx.execute("INSERT INTO workspace_claims (canonical_workspace_root,execution_id,claim_type,acquired_at) VALUES (?1,?2,'exclusive_execution',?3)", params![input.canonical_workspace_root, id, now]).map_err(|e| e.to_string())?;
    Ok(CreateOutcome {
        execution: execution_record(tx, id)
            .map_err(|e| e.to_string())?
            .ok_or("EXECUTION_NOT_FOUND")?,
        execution_id: id.into(),
        created: true,
    })
}

struct Row {
    status: Status,
    dispatch: DispatchState,
    revision: i64,
    runtime: Option<String>,
    runtime_evidence_at: Option<i64>,
    terminal: Option<String>,
    terminal_runtime: Option<String>,
    terminal_at: Option<i64>,
    cleanup: String,
    cleanup_runtime: Option<String>,
    cleanup_at: Option<i64>,
    release_state: String,
    release_kind: Option<String>,
    release_json: Option<String>,
}
fn load(tx: &Transaction<'_>, id: &str) -> Result<Row, String> {
    tx.query_row("SELECT status,dispatch_state,revision,runtime_instance_id,runtime_termination_evidence_at,
        provider_terminal_status,provider_terminal_evidence_runtime_instance_id,provider_terminal_evidence_at,
        background_cleanup_state,background_cleanup_runtime_instance_id,background_cleanup_evidence_at,
        release_evidence_state,release_evidence_kind,release_evidence_json FROM executions WHERE id=?1", [id], |r| {
        let parse = |index| -> rusqlite::Result<serde_json::Value> { Ok(serde_json::Value::String(r.get(index)?)) };
        Ok(Row { status: serde_json::from_value(parse(0)?).map_err(|e| rusqlite::Error::FromSqlConversionFailure(0,rusqlite::types::Type::Text,Box::new(e)))?,
            dispatch: serde_json::from_value(parse(1)?).map_err(|e| rusqlite::Error::FromSqlConversionFailure(1,rusqlite::types::Type::Text,Box::new(e)))?,
            revision:r.get(2)?,runtime:r.get(3)?,runtime_evidence_at:r.get(4)?,terminal:r.get(5)?,terminal_runtime:r.get(6)?,terminal_at:r.get(7)?,
            cleanup:r.get(8)?,cleanup_runtime:r.get(9)?,cleanup_at:r.get(10)?,release_state:r.get(11)?,release_kind:r.get(12)?,release_json:r.get(13)? })
    }).map_err(|e| e.to_string())
}

enum Mutation {
    Event(Transition),
    Finalize(Finalization),
    CancelBeforeDispatch,
}

fn owns_claim(tx: &Transaction<'_>, id: &str) -> Result<(), String> {
    let owns: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM workspace_claims c JOIN executions e ON c.execution_id=e.id AND c.canonical_workspace_root=e.canonical_workspace_root WHERE e.id=?1)",[id],|r|r.get(0)).map_err(|e| e.to_string())?;
    if owns {
        Ok(())
    } else {
        Err("WORKSPACE_CLAIM_INCONSISTENT".into())
    }
}
fn delete_claim(tx: &Transaction<'_>, id: &str) -> Result<(), String> {
    if tx
        .execute("DELETE FROM workspace_claims WHERE execution_id=?1", [id])
        .map_err(|e| e.to_string())?
        != 1
    {
        return Err("WORKSPACE_CLAIM_INCONSISTENT".into());
    }
    Ok(())
}

/// Verify persisted evidence from the future Runtime collector, not live OS state.
fn terminated_runtime(tx: &Transaction<'_>, runtime: &str) -> Result<i64, String> {
    let at: Option<i64> = tx.query_row("SELECT termination_evidence_at FROM runtime_instances WHERE id=?1 AND state='terminated'
        AND termination_evidence_state='complete' AND termination_evidence_at IS NOT NULL AND
        (termination_evidence_type='job_active_processes_zero' OR
         (termination_evidence_type='managed_job_destroyed' AND job_session_id IS NOT NULL
          AND job_creation_mode='proc_thread_attribute_job_list' AND job_handle_inheritable=0
          AND job_kill_on_close=1 AND job_breakaway_allowed=0 AND job_policy_verified_at IS NOT NULL))",
        [runtime], |r| r.get(0)).optional().map_err(|e|e.to_string())?.flatten();
    at.ok_or_else(|| "RUNTIME_TERMINATION_EVIDENCE_REQUIRED".into())
}

fn transition_execution(
    tx: &Transaction<'_>,
    id: &str,
    revision: i64,
    mutation: Mutation,
    now: i64,
) -> Result<(), String> {
    let row = load(tx, id)?;
    if row.revision != revision {
        return Err("EXECUTION_REVISION_CONFLICT".into());
    }
    let mut next = row.status;
    let mut dispatch = row.dispatch;
    let mut runtime = row.runtime.clone();
    let mut release: Option<(&str, String)> = None;
    match mutation {
        Mutation::CancelBeforeDispatch => {
            if row.status != Status::DispatchPending
                || row.dispatch != DispatchState::NotDispatched
                || row.runtime.is_some()
                || row.terminal.is_some()
            {
                return Err("CANCEL_BEFORE_DISPATCH_REJECTED".into());
            }
            owns_claim(tx, id)?;
            next = Status::Cancelled;
            release = Some((
                "not_dispatched",
                json!({"kind":"not_dispatched","execution_id":id,"at":now}).to_string(),
            ));
        }
        Mutation::Finalize(finalization) => {
            if matches!(finalization.basis, ReleaseBasis::RuntimeTerminated)
                && (row.status != Status::Reconciling
                    || finalization.terminal != Status::Interrupted)
            {
                return Err("RUNTIME_TERMINATION_REQUIRES_RECONCILING_TO_INTERRUPTED".into());
            }
            if !matches!(row.status, Status::Finalizing | Status::Reconciling)
                || !finalization.terminal.terminal()
            {
                return Err("INVALID_TERMINAL_TRANSITION".into());
            }
            owns_claim(tx, id)?;
            let original = row.runtime.as_deref().ok_or("RUNTIME_EVIDENCE_REQUIRED")?;
            let (kind, at) = match finalization.basis {
                ReleaseBasis::SameRuntimeCleanup => {
                    if row.terminal.is_none()
                        || row.terminal_runtime.as_deref() != Some(original)
                        || row.terminal_at.is_none()
                        || row.cleanup != "empty"
                        || row.cleanup_runtime.as_deref() != Some(original)
                        || row.cleanup_at.is_none()
                    {
                        return Err("SAFE_RELEASE_EVIDENCE_INCOMPLETE".into());
                    }
                    ("same_runtime_cleanup", row.cleanup_at.unwrap())
                }
                ReleaseBasis::RuntimeTerminated => {
                    let at = terminated_runtime(tx, original)?;
                    tx.execute("UPDATE executions SET runtime_termination_evidence_runtime_instance_id=?2,runtime_termination_evidence_at=?3 WHERE id=?1",params![id,original,at]).map_err(|e|e.to_string())?;
                    ("runtime_terminated", at)
                }
            };
            next = finalization.terminal;
            release = Some((
                kind,
                json!({"kind":kind,"runtime_instance_id":original,"evidence_at":at,"at":now})
                    .to_string(),
            ));
            let completeness =
                serde_json::to_value(finalization.completeness).map_err(|e| e.to_string())?;
            if let Some(result) = &finalization.result
                && let Some(name) = result.get("threadName")
            {
                let thread_id = result.get("threadId").and_then(Value::as_str).ok_or("RESULT_THREAD_REQUIRED")?;
                let name: Option<String> = serde_json::from_value(name.clone()).map_err(|e| e.to_string())?;
                tx.execute("INSERT INTO thread_names(thread_id,name) VALUES (?1,?2) ON CONFLICT(thread_id) DO UPDATE SET name=excluded.name", params![thread_id,name]).map_err(|e|e.to_string())?;
            }
            tx.execute(
                "UPDATE executions SET final_result_json=?2,result_completeness=?3 WHERE id=?1",
                params![
                    id,
                    finalization.result.map(|v| v.to_string()),
                    completeness.as_str().unwrap()
                ],
            )
            .map_err(|e| e.to_string())?;
        }
        Mutation::Event(event) => {
            if matches!(
                (&event, row.status),
                (Transition::Running, Status::Running)
                    | (Transition::RequestCancel, Status::CancelRequested)
                    | (Transition::Reconcile, Status::Reconciling)
                    | (Transition::MarkUnknown, Status::Unknown)
            ) {
                return Err("INVALID_EXECUTION_TRANSITION".into());
            }
            match event {
                Transition::Running => next = Status::Running,
                Transition::RequestCancel => {
                    // A dispatched request with no Turn yet keeps its cancellation intent.
                    if row.status == Status::DispatchPending
                        && row.dispatch != DispatchState::NotDispatched
                    {
                    } else {
                        next = Status::CancelRequested;
                    }
                    tx.execute("UPDATE executions SET interrupt_requested_at=COALESCE(interrupt_requested_at,?2) WHERE id=?1",params![id,now]).map_err(|e|e.to_string())?;
                }
                Transition::InterruptAck => {
                    if row.terminal.is_none()
                        && row.status != Status::CancelRequested
                        && row.status != Status::Finalizing
                        && !row.status.terminal()
                    {
                        return Err("INVALID_INTERRUPT_ACK_CONTEXT".into());
                    }
                    tx.execute("UPDATE executions SET interrupt_ack_at=COALESCE(interrupt_ack_at,?2) WHERE id=?1",params![id,now]).map_err(|e|e.to_string())?;
                    if row.terminal.is_none() && row.status == Status::CancelRequested {
                        next = Status::Cancelling;
                    }
                }
                Transition::InterruptTimeout { diagnostic } => {
                    if row.terminal.is_none()
                        && !matches!(row.status, Status::CancelRequested | Status::Cancelling)
                        && !row.status.terminal()
                    {
                        return Err("INVALID_INTERRUPT_TIMEOUT_CONTEXT".into());
                    }
                    tx.execute("UPDATE executions SET interrupt_timeout_at=COALESCE(interrupt_timeout_at,?2),interrupt_diagnostic=?3 WHERE id=?1",params![id,now,diagnostic]).map_err(|e|e.to_string())?;
                    if row.terminal.is_none()
                        && matches!(row.status, Status::CancelRequested | Status::Cancelling)
                    {
                        next = Status::Reconciling;
                    }
                }
                Transition::ProviderTerminal { runtime_id, status } => {
                    if !status.terminal() || row.runtime.as_deref() != Some(&runtime_id) {
                        return Err("PROVIDER_EVIDENCE_RUNTIME_MISMATCH".into());
                    }
                    if row
                        .terminal
                        .as_deref()
                        .is_some_and(|old| old != status.as_str())
                        || row
                            .terminal_runtime
                            .as_deref()
                            .is_some_and(|old| old != runtime_id)
                    {
                        return Err("PROVIDER_TERMINAL_EVIDENCE_CONFLICT".into());
                    }
                    tx.execute("UPDATE executions SET provider_terminal_status=COALESCE(provider_terminal_status,?2),provider_terminal_evidence_runtime_instance_id=COALESCE(provider_terminal_evidence_runtime_instance_id,?3),provider_terminal_evidence_at=COALESCE(provider_terminal_evidence_at,?4) WHERE id=?1",params![id,status.as_str(),runtime_id,now]).map_err(|e|e.to_string())?;
                    if matches!(
                        row.status,
                        Status::DispatchPending
                            | Status::Running
                            | Status::CancelRequested
                            | Status::Cancelling
                    ) {
                        next = Status::Finalizing;
                    }
                }
                Transition::CleanupEmpty { runtime_id } => {
                    if row.runtime.as_deref() != Some(&runtime_id)
                        || row.terminal.is_none()
                        || row.terminal_runtime.as_deref() != Some(&runtime_id)
                    {
                        return Err("CLEANUP_EVIDENCE_RUNTIME_MISMATCH".into());
                    }
                    tx.execute("UPDATE executions SET background_cleanup_state='empty',background_cleanup_runtime_instance_id=?2,background_cleanup_evidence_at=?3 WHERE id=?1",params![id,runtime_id,now]).map_err(|e|e.to_string())?;
                }
                Transition::Reconcile => {
                    if row.status == Status::Unknown {
                        return Err("NEW_RECOVERY_EVIDENCE_REQUIRED".into());
                    }
                    next = Status::Reconciling;
                }
                Transition::MarkUnknown => next = Status::Unknown,
                Transition::ResumeRecovery(basis) => {
                    if row.status != Status::Unknown {
                        return Err("RECOVERY_REQUIRES_UNKNOWN".into());
                    }
                    match basis {
                        RecoveryBasis::RuntimeTermination {
                            runtime_id,
                            evidence_at,
                        } => {
                            if row.runtime.as_deref() != Some(&runtime_id)
                                || terminated_runtime(tx, &runtime_id)? != evidence_at
                                || row
                                    .runtime_evidence_at
                                    .is_some_and(|previous| evidence_at <= previous)
                            {
                                return Err("NEW_RECOVERY_EVIDENCE_REQUIRED".into());
                            }
                            tx.execute("UPDATE executions SET runtime_termination_evidence_runtime_instance_id=?2,runtime_termination_evidence_at=?3 WHERE id=?1",params![id,runtime_id,evidence_at]).map_err(|e|e.to_string())?;
                        }
                        RecoveryBasis::LocalResolve { diagnostic } => {
                            if diagnostic.trim().is_empty() {
                                return Err("LOCAL_RESOLVE_DIAGNOSTIC_REQUIRED".into());
                            }
                            tx.execute(
                                "UPDATE executions SET interrupt_diagnostic=?2 WHERE id=?1",
                                params![id, diagnostic],
                            )
                            .map_err(|e| e.to_string())?;
                        }
                    }
                    next = Status::Reconciling;
                }
                Transition::Dispatch { to, runtime_id } => {
                    if !row.dispatch.allows(to) {
                        return Err("INVALID_DISPATCH_TRANSITION".into());
                    }
                    if to == DispatchState::Dispatching {
                        if row.status != Status::DispatchPending || row.runtime.is_some() {
                            return Err("DISPATCH_REJECTED".into());
                        }
                        owns_claim(tx, id)?;
                        let selected = runtime_id.ok_or("RUNTIME_REQUIRED")?;
                        let running: bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM runtime_instances WHERE id=?1 AND state='running')",[&selected],|r|r.get(0)).map_err(|e|e.to_string())?;
                        if !running {
                            return Err("RUNTIME_NOT_RUNNING".into());
                        }
                        runtime = Some(selected);
                    } else if runtime_id.is_some() {
                        return Err("EXECUTION_RUNTIME_IMMUTABLE".into());
                    }
                    dispatch = to;
                }
            }
        }
    }
    if next != row.status {
        if !row.status.allows(next) {
            return Err("INVALID_EXECUTION_TRANSITION".into());
        }
        if row.terminal.is_some()
            && matches!(
                next,
                Status::Running | Status::CancelRequested | Status::Cancelling
            )
        {
            return Err("PROVIDER_TERMINAL_IS_MONOTONIC".into());
        }
    }
    if next == Status::Running && row.dispatch == DispatchState::NotDispatched {
        return Err("RUNNING_REQUIRES_DISPATCH".into());
    }
    if next.terminal() && next != row.status && release.is_none() {
        return Err("SAFE_RELEASE_EVIDENCE_REQUIRED".into());
    }
    #[cfg(test)]
    if release.is_some() {
        crash_checkpoint("before_terminal");
    }
    let changed=tx.execute("UPDATE executions SET status=?2,dispatch_state=?3,runtime_instance_id=?4,revision=revision+1,updated_at=?5 WHERE id=?1 AND revision=?6 AND status=?7 AND dispatch_state=?8",
        params![id,next.as_str(),dispatch.as_str(),runtime,now,revision,row.status.as_str(),row.dispatch.as_str()]).map_err(|e|e.to_string())?;
    if changed != 1 {
        return Err("EXECUTION_REVISION_CONFLICT".into());
    }
    if let Some((kind, evidence)) = release {
        #[cfg(test)]
        crash_checkpoint("after_terminal");
        tx.execute("UPDATE executions SET completed_at=?2,release_evidence_state='complete',release_evidence_kind=?3,release_evidence_json=?4 WHERE id=?1",params![id,now,kind,evidence]).map_err(|e|e.to_string())?;
        #[cfg(test)]
        crash_checkpoint("before_delete");
        delete_claim(tx, id)?;
        #[cfg(test)]
        crash_checkpoint("after_delete");
    }
    Ok(())
}

#[cfg(test)]
mod tests;

#[cfg(test)]
fn crash_checkpoint(point: &str) {
    if std::env::var("TASK002_CRASH_POINT").as_deref() == Ok(point) {
        std::process::exit(91);
    }
}
