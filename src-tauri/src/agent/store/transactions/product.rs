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
    pub owns_claim: bool,
    pub claim_free: bool,
    pub agent_free: bool,
    pub created_at: i64,
    pub updated_at: i64,
    pub completed_at: Option<i64>,
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
    pub async fn product_create_fresh(
        &self,
        id: String,
        agent: String,
        request_key: String,
        prompt: String,
        workspace: Option<WorkspaceSnapshot>,
        now: i64,
    ) -> Result<CreateOutcome, String> {
        self.write(move |tx| {
            if let Some(row) = key(tx, &agent, &request_key)? {
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
            let mut q=tx.prepare("SELECT id,created_at,updated_at,completed_at FROM executions WHERE (?1 IS NULL OR id=?1) AND (?2 IS NULL OR agent_id=?2) AND (?3 IS NULL OR workspace_id=?3) ORDER BY created_at DESC,id DESC LIMIT ?4")?;
            let rows=q.query_map(params![id,agent,workspace,limit],|r|Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?,r.get::<_,i64>(2)?,r.get::<_,Option<i64>>(3)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
            rows.into_iter().map(|(id,created_at,updated_at,completed_at)|{
                let row=execution_record(&tx,&id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
                let owns:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM workspace_claims WHERE execution_id=?1 AND canonical_workspace_root=?2)",params![id,row.canonical_workspace_root],|r|r.get(0))?;
                let claimed:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM workspace_claims WHERE execution_id=?1 OR canonical_workspace_root=?2)",params![id,row.canonical_workspace_root],|r|r.get(0))?;
                let busy:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM executions WHERE agent_id=?1 AND status NOT IN ('completed','failed','cancelled','interrupted'))",[&row.agent_id],|r|r.get(0))?;
                Ok(ProductSnapshot{execution:row,owns_claim:owns,claim_free:!claimed,agent_free:!busy,created_at,updated_at,completed_at})
            }).collect()
        }).await
    }
}
