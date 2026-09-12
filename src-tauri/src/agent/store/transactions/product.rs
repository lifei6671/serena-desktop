//! Product guards layered on the unchanged Foundation creation transaction.
use super::*;
use crate::agent::execution::{
    CreateExecutionInput, ExecutionMode, Provider, canonicalize_request,
};

#[derive(Clone, Debug)]
pub struct WorkspaceSnapshot {
    pub id: String,
    pub root: String,
}
#[derive(Debug)]
pub struct ProductSnapshot {
    pub execution: ExecutionRecord,
    pub thread_name: Option<String>,
    pub owns_claim: bool,
    pub runtime_attempt_exists: bool,
    pub claim_free: bool,
    pub agent_free: bool,
    pub created_at: i64,
    pub updated_at: i64,
    pub completed_at: Option<i64>,
}
pub struct ControlContext {
    pub accepted_id: Option<String>,
    pub related_id: Option<String>,
    pub related_unknown: bool,
    pub blocker_id: Option<String>,
}
pub fn continuation_eligible(row: &ExecutionRecord) -> bool {
    if !matches!(
        row.status.as_str(),
        "completed" | "failed" | "cancelled" | "interrupted"
    ) || row.release_evidence_state != "complete"
        || row.provider != "codex"
        || row.mode != "workspace_write"
        || row.execution_profile_json != "{}"
        || row.thread_id.as_deref().is_none_or(str::is_empty)
        || row.turn_id.as_deref().is_none_or(str::is_empty)
        || row.runtime_instance_id.as_deref().is_none_or(str::is_empty)
    {
        return false;
    }
    // Managed provenance comes from the sealed, persisted result, never caller input.
    row.final_result_json
        .as_ref()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
        .is_some_and(|v| {
            v["historyMode"] == "paginated"
                && v["executionId"].as_str() == Some(row.id.as_str())
                && v["turnId"].as_str() == row.turn_id.as_deref()
                && v["threadId"].as_str() == row.thread_id.as_deref()
                && v["sourceRuntimeId"].as_str() == row.runtime_instance_id.as_deref()
        })
}
fn key(c: &Connection, agent: &str, request: &str) -> Result<Option<ExecutionRecord>, String> {
    let id: Option<String> = c
        .query_row(
            "SELECT id FROM executions WHERE agent_id=?1 AND request_key=?2",
            params![agent, request],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    id.map(|id| {
        execution_record(c, &id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "EXECUTION_NOT_FOUND".into())
    })
    .transpose()
}
fn input(
    row: &ExecutionRecord,
    key: String,
    prompt: String,
    thread: Option<String>,
) -> Result<CreateExecutionInput, String> {
    Ok(CreateExecutionInput {
        agent_id: row.agent_id.clone(),
        request_key: key,
        prompt,
        execution_profile: serde_json::from_str(&row.execution_profile_json)
            .map_err(|e| e.to_string())?,
        workspace_id: row.workspace_id.clone(),
        canonical_workspace_root: row.canonical_workspace_root.clone(),
        provider: serde_json::from_value(serde_json::json!(row.provider))
            .map_err(|e| e.to_string())?,
        mode: serde_json::from_value(serde_json::json!(row.mode)).map_err(|e| e.to_string())?,
        thread_id: thread,
    })
}
fn prior_outcome(
    row: ExecutionRecord,
    request: &CanonicalRequest,
) -> Result<CreateOutcome, String> {
    if row.request_hash != request.request_hash() {
        return Err("EXECUTION_REQUEST_KEY_CONFLICT".into());
    }
    Ok(CreateOutcome {
        execution_id: row.id.clone(),
        execution: row,
        created: false,
    })
}
impl StateStore {
    /// Desktop history cursor: creation time plus exact identity, never OFFSET.
    pub async fn product_history_ids(
        &self,
        before: Option<String>,
        workspace: Option<String>,
    ) -> Result<Vec<String>, String> {
        self.read(move |c| {
            let cursor = before.map(|id| c.query_row("SELECT created_at,id FROM executions WHERE id=?1", [id], |r| Ok((r.get::<_,i64>(0)?, r.get::<_,String>(1)?)))).transpose()?;
            let (time, id) = cursor.map_or((None, None), |(t, id)| (Some(t), Some(id)));
            let mut q = c.prepare("SELECT id FROM executions WHERE (?1 IS NULL OR created_at<?1 OR (created_at=?1 AND id<?2)) AND (?3 IS NULL OR canonical_workspace_root=?3) ORDER BY created_at DESC,id DESC LIMIT 6")?;
            q.query_map(params![time,id,workspace], |r| r.get(0))?.collect()
        }).await
    }

    /// Read-only attribution after an operation error; reuse the frozen canonical request.
    pub async fn product_control_context(
        &self,
        action: crate::agent::product::Action,
        workspace: Option<WorkspaceSnapshot>,
        error_code: String,
    ) -> Result<ControlContext, String> {
        self.read(move |c| {
            let tx = c.unchecked_transaction()?;
            Ok((|| -> Result<ControlContext, String> {
                use crate::agent::product::Action;
                let mut context = ControlContext { accepted_id: None, related_id: None, related_unknown: false, blocker_id: None };
                let (agent, root) = match action {
                    Action::Start { agent_id, request_key, prompt, workspace_id } => {
                        if let Some(row) = key(&tx, &agent_id, &request_key)? {
                            let request = canonicalize_request(input(&row, request_key, prompt, None)?)?;
                            if row.workspace_id == workspace_id && row.request_hash == request.request_hash() { context.accepted_id = Some(row.id); }
                        }
                        if let Some((id, status)) = tx.query_row("SELECT id,status FROM executions WHERE agent_id=?1 ORDER BY created_at DESC,id DESC LIMIT 1", [&agent_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))).optional().map_err(|e| e.to_string())? {
                            context.related_id = Some(id);
                            context.related_unknown = status == "unknown";
                        }
                        (Some(agent_id), workspace.map(|w| w.root))
                    }
                    Action::Continue { execution_id, request_key, prompt } => {
                        if let Some(row) = execution_record(&tx, &execution_id).map_err(|e| e.to_string())? {
                            let request = canonicalize_request(input(&row, request_key.clone(), prompt, row.thread_id.clone())?)?;
                            if let Some(prior) = key(&tx, &row.agent_id, &request_key)?
                                && prior.request_hash == request.request_hash() { context.accepted_id = Some(prior.id); }
                            context.related_unknown = row.status == "unknown";
                            context.related_id = Some(row.id);
                            (Some(row.agent_id), Some(row.canonical_workspace_root))
                        } else { (None, None) }
                    }
                    Action::Observe { execution_id, .. } | Action::Cancel { execution_id } | Action::ResumePending { execution_id } => {
                        if let Some(row) = execution_record(&tx, &execution_id).map_err(|e| e.to_string())? {
                            context.accepted_id = Some(row.id.clone());
                            context.related_id = Some(row.id);
                            (Some(row.agent_id), Some(row.canonical_workspace_root))
                        } else { (None, None) }
                    }
                    Action::List { .. } => (None, None),
                };
                context.blocker_id = if error_code == "WORKSPACE_CLAIM_CONFLICT" {
                    tx.query_row("SELECT e.id FROM workspace_claims c JOIN executions e ON e.id=c.execution_id AND e.canonical_workspace_root=c.canonical_workspace_root WHERE c.canonical_workspace_root=?1", [root], |r| r.get(0)).optional().map_err(|e| e.to_string())?
                } else if error_code == "AGENT_BUSY" {
                    tx.query_row("SELECT id FROM executions WHERE agent_id=?1 AND status NOT IN ('completed','failed','cancelled','interrupted') ORDER BY created_at DESC,id DESC LIMIT 1", [agent], |r| r.get(0)).optional().map_err(|e| e.to_string())?
                } else { None };
                Ok(context)
            })())
        }).await?
    }
    #[allow(clippy::too_many_arguments)]
    pub async fn product_create_fresh(
        &self,
        id: String,
        agent: String,
        request_key: String,
        prompt: String,
        expected_workspace_id: String,
        workspace: Option<WorkspaceSnapshot>,
        now: i64,
    ) -> Result<CreateOutcome, String> {
        self.write(move |tx| {
            if let Some(row) = key(tx, &agent, &request_key)? {
                if row.workspace_id != expected_workspace_id {
                    return Err("EXECUTION_REQUEST_KEY_CONFLICT".into());
                }
                let request = canonicalize_request(input(&row, request_key, prompt, None)?)?;
                return prior_outcome(row, &request);
            }
            let history: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM executions WHERE agent_id=?1)",
                    [&agent],
                    |r| r.get(0),
                )
                .map_err(|e| e.to_string())?;
            if history {
                return Err("AGENT_LINEAGE_CONFLICT".into());
            }
            let w = workspace.ok_or("AGENT_NO_ACTIVE_WORKSPACE")?;
            if w.id != expected_workspace_id {
                return Err("AGENT_WORKSPACE_CHANGED".into());
            }
            let request = canonicalize_request(CreateExecutionInput {
                agent_id: agent,
                request_key,
                prompt,
                execution_profile: json!({}),
                workspace_id: w.id,
                canonical_workspace_root: w.root,
                provider: Provider::Codex,
                mode: ExecutionMode::WorkspaceWrite,
                thread_id: None,
            })?;
            create(tx, &id, &request, now)
        })
        .await
    }
    pub async fn product_create_continuation(
        &self,
        id: String,
        source: String,
        request_key: String,
        prompt: String,
        now: i64,
    ) -> Result<CreateOutcome, String> {
        self.write(move |tx| {
            let row = execution_record(tx, &source)
                .map_err(|e| e.to_string())?
                .ok_or("EXECUTION_NOT_FOUND")?;
            let request = canonicalize_request(input(
                &row,
                request_key.clone(),
                prompt,
                row.thread_id.clone(),
            )?)?;
            if let Some(prior) = key(tx, &row.agent_id, &request_key)? {
                return prior_outcome(prior, &request);
            }
            let claimed: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM workspace_claims WHERE execution_id=?1)",
                    [&source],
                    |r| r.get(0),
                )
                .map_err(|e| e.to_string())?;
            if !continuation_eligible(&row) || claimed {
                return Err("AGENT_CONTINUE_NOT_ALLOWED".into());
            }
            create(tx, &id, &request, now)
        })
        .await
    }
    pub fn product_worker_owned(&self, id: &str) -> bool {
        PENDING_DISPATCH.lock().unwrap().contains(&(
            self.database_identity.to_string_lossy().to_lowercase(),
            id.into(),
        ))
    }
    pub async fn product_read(
        &self,
        id: Option<String>,
        agent: Option<String>,
        workspace: Option<String>,
        limit: u32,
    ) -> Result<Vec<ProductSnapshot>, String> {
        self.read(move |c| {
            let tx=c.unchecked_transaction()?;
            let query = if id.is_some() {
                "SELECT id,created_at,updated_at,completed_at FROM executions WHERE id=?1 AND (?2 IS NULL OR agent_id=?2) AND (?3 IS NULL OR workspace_id=?3) LIMIT ?4"
            } else {
                "SELECT id,created_at,updated_at,completed_at FROM executions WHERE (?1 IS NULL OR id=?1) AND (?2 IS NULL OR agent_id=?2) AND (?3 IS NULL OR workspace_id=?3) ORDER BY created_at DESC,id DESC LIMIT ?4"
            };
            let mut q=tx.prepare(query)?;
            let rows=q.query_map(params![id,agent,workspace,limit],|r|Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?,r.get::<_,i64>(2)?,r.get::<_,Option<i64>>(3)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
            rows.into_iter().map(|(id,created_at,updated_at,completed_at)|{
                let row=execution_record(&tx,&id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
                let thread_name = tx.query_row("SELECT name FROM thread_names WHERE thread_id=?1", [&row.thread_id], |r| r.get::<_, Option<String>>(0)).optional()?.flatten();
                let owns:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM workspace_claims WHERE execution_id=?1 AND canonical_workspace_root=?2)",params![id,row.canonical_workspace_root],|r|r.get(0))?;
                let claimed:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM workspace_claims WHERE execution_id=?1 OR canonical_workspace_root=?2)",params![id,row.canonical_workspace_root],|r|r.get(0))?;
                let busy:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM executions WHERE agent_id=?1 AND status NOT IN ('completed','failed','cancelled','interrupted'))",[&row.agent_id],|r|r.get(0))?;
                let runtime_attempt_exists:bool=crate::agent::store::runtime_attempts::runtime_attempt_exists(&tx,&id)?;
                Ok(ProductSnapshot{execution:row,thread_name,owns_claim:owns,runtime_attempt_exists,claim_free:!claimed,agent_free:!busy,created_at,updated_at,completed_at})
            }).collect()
        }).await
    }
}

impl StateStore {
    /// Presentation metadata shared by all executions of the same official Thread.
    /// Does not modify execution identity, CAS revisions, timestamps, or claims.
    pub(crate) async fn save_thread_name(&self, thread_id: String, name: Option<String>) -> Result<(), String> {
        self.read(move |c| {
            c.execute("INSERT INTO thread_names(thread_id,name) VALUES (?1,?2) ON CONFLICT(thread_id) DO UPDATE SET name=excluded.name", params![thread_id,name])?;
            Ok(())
        }).await
    }
}
