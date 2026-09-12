//! Business-container terminal updates; never mutate Execution, Claim or Runtime.
use super::*;
use crate::agent::work::{FinishOutcome, TerminalAction, validate_id};
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AcceptedWork<'a> {
    decision: &'static str,
    summary: &'a str,
    execution_ids: &'a [String],
    accepted_at: i64,
}

impl StateStore {
    pub(crate) async fn work_terminal(
        &self,
        id: String,
        action: TerminalAction,
        now: i64,
    ) -> Result<WorkRunRecord, String> {
        self.write(move |tx| {
            let work = work_run_record(tx, &id).map_err(|e| e.to_string())?.ok_or("WORK_NOT_FOUND")?;
            if work.status != "active" {
                return Err("WORK_NOT_ACTIVE".into());
            }
            let (status, acceptance_json) = match action {
                TerminalAction::Cancel => ("cancelled", None),
                TerminalAction::Finish { outcome, acceptance } => {
                    let unresolved: bool = tx.query_row(
                        "SELECT EXISTS(SELECT 1 FROM work_execution_links l
                         JOIN executions e ON e.id=l.execution_id
                         WHERE l.work_run_id=?1 AND e.status NOT IN ('completed','failed','cancelled','interrupted'))",
                        [&id], |row| row.get(0),
                    ).map_err(|e| e.to_string())?;
                    if unresolved {
                        return Err("WORK_HAS_ACTIVE_EXECUTIONS".into());
                    }
                    match outcome {
                        FinishOutcome::Failed => {
                            if acceptance.is_some() {
                                return Err("WORK_INVALID_ARGUMENT".into());
                            }
                            ("failed", None)
                        }
                        FinishOutcome::Completed => {
                            let acceptance = acceptance.ok_or("WORK_ACCEPTANCE_REQUIRED")?;
                            let summary = acceptance.summary.trim();
                            if summary.is_empty() {
                                return Err("WORK_INVALID_ARGUMENT".into());
                            }
                            let mut seen = std::collections::HashSet::new();
                            for execution_id in &acceptance.execution_ids {
                                validate_id(execution_id)?;
                                if !seen.insert(execution_id) {
                                    return Err("WORK_INVALID_ARGUMENT".into());
                                }
                            }
                            for execution_id in &acceptance.execution_ids {
                                let link = work_execution_link_record(tx, execution_id).map_err(|e| e.to_string())?;
                                if link.as_ref().is_none_or(|link| link.work_run_id != id) {
                                    return Err("EXECUTION_NOT_IN_WORK".into());
                                }
                            }
                            let json = serde_json::to_string(&AcceptedWork {
                                decision: "accepted", summary, execution_ids: &acceptance.execution_ids, accepted_at: now,
                            }).map_err(|e| e.to_string())?;
                            ("completed", Some(json))
                        }
                    }
                }
            };
            let changed = tx.execute(
                "UPDATE work_runs SET status=?2, revision=revision+1, acceptance_json=?3,
                 updated_at=?4, completed_at=?4 WHERE id=?1 AND status='active'",
                params![id, status, acceptance_json, now],
            ).map_err(|e| e.to_string())?;
            if changed != 1 {
                return Err("WORK_TERMINAL_UPDATE_FAILED".into());
            }
            work_run_record(tx, &id).map_err(|e| e.to_string())?.ok_or_else(|| "WORK_NOT_FOUND".into())
        }).await
    }
}
