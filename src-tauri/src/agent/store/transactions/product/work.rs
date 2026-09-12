use super::*;
use crate::agent::store::work_runs::{work_execution_link_record, work_run_record};

#[derive(Clone, Debug)]
pub struct WorkExecutionContext {
    pub work_run_id: String,
    pub parent_execution_id: Option<String>,
    pub delegation_context_json: Option<String>,
}

pub(super) fn require_membership(
    tx: &Transaction<'_>,
    execution_id: &str,
    work_run_id: &str,
) -> Result<WorkExecutionLinkRecord, String> {
    work_execution_link_record(tx, execution_id)
        .map_err(|e| e.to_string())?
        .filter(|link| link.work_run_id == work_run_id)
        .ok_or_else(|| "EXECUTION_NOT_IN_WORK".into())
}

pub(super) fn require_retry_context(
    tx: &Transaction<'_>,
    execution_id: &str,
    work: &WorkExecutionContext,
) -> Result<(), String> {
    let link = require_membership(tx, execution_id, &work.work_run_id)?;
    // Product supplies canonical context. Its frozen bytes and parent are request identity.
    if link.parent_execution_id != work.parent_execution_id
        || link.delegation_context_json != work.delegation_context_json
    {
        return Err("EXECUTION_REQUEST_KEY_CONFLICT".into());
    }
    Ok(())
}

impl StateStore {
    /// Read-only retry check before Host context verification. Creation still repeats
    /// these checks in its existing transaction to arbitrate concurrent submissions.
    pub(crate) async fn product_work_preflight(
        &self,
        action: crate::agent::product::Action,
        work: WorkExecutionContext,
    ) -> Result<Option<String>, String> {
        self.read(move |c| {
            let tx = c.unchecked_transaction()?;
            Ok((|| -> Result<Option<String>, String> {
                use crate::agent::product::Action;
                let (prior, request) = match action {
                    Action::Start {
                        agent_id,
                        workspace_id,
                        request_key,
                        prompt,
                    } => {
                        let Some(row) = key(&tx, &agent_id, &request_key)? else {
                            return Ok(None);
                        };
                        require_retry_context(&tx, &row.id, &work)?;
                        if row.workspace_id != workspace_id {
                            return Err("EXECUTION_REQUEST_KEY_CONFLICT".into());
                        }
                        let request =
                            canonicalize_request(input(&row, request_key, prompt, None)?)?;
                        (row, request)
                    }
                    Action::Continue {
                        execution_id,
                        request_key,
                        prompt,
                    } => {
                        require_membership(&tx, &execution_id, &work.work_run_id)?;
                        let parent = execution_record(&tx, &execution_id)
                            .map_err(|e| e.to_string())?
                            .ok_or("EXECUTION_NOT_FOUND")?;
                        let Some(row) = key(&tx, &parent.agent_id, &request_key)? else {
                            return Ok(None);
                        };
                        require_retry_context(&tx, &row.id, &work)?;
                        let request = canonicalize_request(input(
                            &parent,
                            request_key,
                            prompt,
                            parent.thread_id.clone(),
                        )?)?;
                        (row, request)
                    }
                    _ => return Err("WORK_INVALID_ARGUMENT".into()),
                };
                Ok(Some(prior_outcome(prior, &request)?.execution_id))
            })())
        })
        .await?
    }
}

pub(super) fn validate_new_work(
    tx: &Transaction<'_>,
    context: &WorkExecutionContext,
    workspace_id: &str,
    root: Option<&str>,
) -> Result<(), String> {
    let work = work_run_record(tx, &context.work_run_id)
        .map_err(|e| e.to_string())?
        .ok_or("WORK_NOT_FOUND")?;
    if work.status != "active" {
        return Err("WORK_NOT_ACTIVE".into());
    }
    if work.workspace_id != workspace_id || root != Some(work.canonical_workspace_root.as_str()) {
        return Err("WORKSPACE_CONTEXT_MISMATCH".into());
    }
    Ok(())
}

pub(super) fn create_with_work(
    tx: &Transaction<'_>,
    id: &str,
    request: &CanonicalRequest,
    work: Option<&WorkExecutionContext>,
    now: i64,
) -> Result<CreateOutcome, String> {
    let outcome = create(tx, id, request, now)?;
    if let Some(work) = work {
        if outcome.created {
            tx.execute(
                "INSERT INTO work_execution_links
                 (work_run_id, execution_id, parent_execution_id, delegation_context_json, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![work.work_run_id, outcome.execution_id, work.parent_execution_id, work.delegation_context_json, now],
            ).map_err(|e| e.to_string())?;
        } else {
            require_retry_context(tx, &outcome.execution_id, work)?;
        }
    }
    Ok(outcome)
}

#[cfg(test)]
mod tests;
