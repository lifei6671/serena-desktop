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
    command_run_ids: &'a [String],
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
                    let unresolved_commands: bool = tx.query_row(
                        "SELECT EXISTS(SELECT 1 FROM work_command_links l
                         JOIN command_runs c ON c.id=l.command_run_id
                         WHERE l.work_run_id=?1 AND c.status NOT IN
                         ('completed','failed','cancelled','interrupted'))",
                        [&id], |row| row.get(0),
                    ).map_err(|e| e.to_string())?;
                    if unresolved || unresolved_commands {
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
                            let mut seen_commands = std::collections::HashSet::new();
                            for command_run_id in &acceptance.command_run_ids {
                                validate_id(command_run_id)?;
                                if !seen_commands.insert(command_run_id) {
                                    return Err("WORK_INVALID_ARGUMENT".into());
                                }
                                let link = super::super::command_runs::work_command_link_record(tx, command_run_id)
                                    .map_err(|e| e.to_string())?;
                                if link.as_ref().is_none_or(|link| link.work_run_id != id) {
                                    return Err("COMMAND_RUN_NOT_IN_WORK".into());
                                }
                            }
                            let json = serde_json::to_string(&AcceptedWork {
                                decision: "accepted",
                                summary,
                                execution_ids: &acceptance.execution_ids,
                                command_run_ids: &acceptance.command_run_ids,
                                accepted_at: now,
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


#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{
        store::{CommandRunReceipt, CreateCommandRunInput},
        work::HostAcceptance,
    };

    #[tokio::test]
    async fn command_run_blocks_work_finish_until_durable_terminal_receipt_exists() {
        let directory = tempfile::tempdir().unwrap();
        let store = StateStore::open(directory.path().join("state")).await.unwrap();
        store
            .create_work_run(
                "work-command".into(),
                "workspace".into(),
                "root".into(),
                7,
                "verify command".into(),
                None,
                1,
            )
            .await
            .unwrap();
        store
            .create_command_run(
                "command-1".into(),
                CreateCommandRunInput {
                    request_key: "request-1".into(),
                    request_hash: "hash-1".into(),
                    workspace_id: "workspace".into(),
                    canonical_workspace_root: "root".into(),
                    workspace_generation: 7,
                    work_run_id: Some("work-command".into()),
                    mode: "process".into(),
                    relative_cwd: ".".into(),
                    execution_mode: "auto".into(),
                    timeout_ms: 30_000,
                    runtime_platform: "windows".into(),
                    containment_type: "job_at_creation".into(),
                },
                2,
            )
            .await
            .unwrap();

        let acceptance = || HostAcceptance {
            summary: "host verified".into(),
            execution_ids: Vec::new(),
            command_run_ids: vec!["command-1".into()],
        };
        assert_eq!(
            store
                .work_terminal(
                    "work-command".into(),
                    TerminalAction::Finish {
                        outcome: FinishOutcome::Completed,
                        acceptance: Some(acceptance()),
                    },
                    3,
                )
                .await,
            Err("WORK_HAS_ACTIVE_EXECUTIONS".into())
        );

        store
            .command_mark_running("command-1".into(), 123, 4)
            .await
            .unwrap();
        store
            .command_mark_terminal(
                "command-1".into(),
                "completed".into(),
                CommandRunReceipt {
                    exit_code: Some(0),
                    termination_reason: Some("exited".into()),
                    stdout_total_bytes: 12,
                    stderr_total_bytes: 0,
                    stdout_sha256: Some("stdout-digest".into()),
                    stderr_sha256: Some("stderr-digest".into()),
                    ..CommandRunReceipt::default()
                },
                5,
            )
            .await
            .unwrap();

        let finished = store
            .work_terminal(
                "work-command".into(),
                TerminalAction::Finish {
                    outcome: FinishOutcome::Completed,
                    acceptance: Some(acceptance()),
                },
                6,
            )
            .await
            .unwrap();
        assert_eq!(finished.status, "completed");
        let acceptance: serde_json::Value =
            serde_json::from_str(finished.acceptance_json.as_deref().unwrap()).unwrap();
        assert_eq!(acceptance["commandRunIds"], serde_json::json!(["command-1"]));
        assert_eq!(acceptance["executionIds"], serde_json::json!([]));
    }
}
