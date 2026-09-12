use super::*;
use crate::agent::store::transactions::product::WorkExecutionContext;
use rusqlite::{Connection, types::Value};

fn snapshot(root: &std::path::Path, include_work: bool) -> Vec<Vec<Vec<Value>>> {
    let db = Connection::open(root.join("agent-state.db")).unwrap();
    let mut tables = vec![
        "executions",
        "workspace_claims",
        "work_execution_links",
        "runtime_instances",
        "execution_runtime_attempts",
    ];
    if include_work {
        tables.push("work_runs");
    }
    tables
        .into_iter()
        .map(|table| {
            let mut q = db
                .prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))
                .unwrap();
            let n = q.column_count();
            q.query_map([], |r| (0..n).map(|i| r.get(i)).collect())
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap()
        })
        .collect()
}

async fn create(store: &StateStore, id: &str) {
    store
        .create_work_run(
            id.into(),
            "W".into(),
            "root".into(),
            "title".into(),
            None,
            1,
        )
        .await
        .unwrap();
}

async fn linked(store: &StateStore, root: &std::path::Path, work: &str, id: &str, status: &str) {
    store
        .product_create_fresh_with_work(
            id.into(),
            id.into(),
            "key".into(),
            "prompt".into(),
            "W".into(),
            workspace("W", "root"),
            Some(WorkExecutionContext {
                work_run_id: work.into(),
                parent_execution_id: None,
                delegation_context_json: None,
            }),
            2,
        )
        .await
        .unwrap();
    // Only a fixture: use the existing lifecycle API to release the Claim before
    // setting up a mixture of resolved persisted states. No production transition changes.
    if matches!(status, "completed" | "failed" | "cancelled" | "interrupted") {
        store.request_cancel(id.into(), 3).await.unwrap();
    }
    Connection::open(root.join("agent-state.db"))
        .unwrap()
        .execute("UPDATE executions SET status=?2 WHERE id=?1", [id, status])
        .unwrap();
}

fn acceptance(ids: &[&str]) -> HostAcceptance {
    HostAcceptance {
        summary: "  Host reviewed\n ".into(),
        execution_ids: ids.iter().map(|s| (*s).into()).collect(),
    }
}

fn finish(id: &str, outcome: FinishOutcome, acceptance: Option<HostAcceptance>) -> UpdateAction {
    UpdateAction::Finish {
        work_run_id: id.into(),
        outcome,
        acceptance,
    }
}

#[tokio::test]
async fn every_unresolved_execution_blocks_both_outcomes_without_mutating_any_row() {
    for status in [
        "dispatch_pending",
        "running",
        "cancel_requested",
        "cancelling",
        "finalizing",
        "reconciling",
        "unknown",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let store = StateStore::open(dir.path().into()).await.unwrap();
        create(&store, "work").await;
        linked(&store, dir.path(), "work", "E", status).await;
        let service = WorkProductService::new(store.clone());
        let before = snapshot(dir.path(), true);
        for outcome in [FinishOutcome::Completed, FinishOutcome::Failed] {
            assert_eq!(
                service
                    .update(finish("work", outcome, None), None)
                    .await
                    .unwrap_err(),
                "WORK_HAS_ACTIVE_EXECUTIONS",
                "{status}"
            );
            assert_eq!(snapshot(dir.path(), true), before);
            assert_eq!(
                store
                    .workspace_claim("root".into())
                    .await
                    .unwrap()
                    .unwrap()
                    .execution_id,
                "E"
            );
        }
    }
}

#[tokio::test]
async fn completed_acceptance_is_canonical_server_timed_and_reopens_exactly() {
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::open(dir.path().into()).await.unwrap();
    create(&store, "work").await;
    for (id, status) in [
        ("E1", "completed"),
        ("E2", "failed"),
        ("E3", "cancelled"),
        ("E4", "interrupted"),
    ] {
        linked(&store, dir.path(), "work", id, status).await;
    }
    let service = WorkProductService::new(store.clone());
    let before = snapshot(dir.path(), false);
    let started = now();
    // A proper subset is allowed: acceptance names what the Host actually reviewed.
    let result = service
        .update(
            finish(
                "work",
                FinishOutcome::Completed,
                Some(acceptance(&["E4", "E2"])),
            ),
            None,
        )
        .await
        .unwrap();
    assert_eq!(result.status, "completed");
    assert_eq!(result.revision, 1);
    assert_eq!(result.created_at, 1);
    assert_eq!(result.completed_at, Some(result.updated_at));
    assert!((started..=now()).contains(&result.updated_at));
    assert_eq!(
        result.acceptance_json,
        Some(format!(
            r#"{{"decision":"accepted","summary":"Host reviewed","executionIds":["E4","E2"],"acceptedAt":{}}}"#,
            result.updated_at
        ))
    );
    assert_eq!(snapshot(dir.path(), false), before);
    drop(service);
    drop(store);
    let store = StateStore::open(dir.path().into()).await.unwrap();
    let service = WorkProductService::new(store.clone());
    assert_eq!(
        service
            .query(QueryAction::Get {
                work_run_id: "work".into()
            })
            .await
            .unwrap(),
        QueryData::WorkRun(result.clone())
    );
    assert_eq!(
        service
            .query(QueryAction::List {
                workspace_id: None,
                limit: None
            })
            .await
            .unwrap(),
        QueryData::List {
            work_runs: vec![result]
        }
    );
    assert_eq!(snapshot(dir.path(), false), before);
}

#[tokio::test]
async fn acceptance_validation_and_terminal_work_errors_have_zero_side_effects() {
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::open(dir.path().into()).await.unwrap();
    create(&store, "work").await;
    create(&store, "foreign").await;
    linked(&store, dir.path(), "work", "E", "completed").await;
    linked(&store, dir.path(), "foreign", "F", "completed").await;
    let service = WorkProductService::new(store.clone());
    let before = snapshot(dir.path(), true);
    let mut cases = vec![
        (None, "WORK_ACCEPTANCE_REQUIRED"),
        (
            Some(HostAcceptance {
                summary: " \n\t".into(),
                execution_ids: vec![],
            }),
            "WORK_INVALID_ARGUMENT",
        ),
        (Some(acceptance(&["E", "E"])), "WORK_INVALID_ARGUMENT"),
        (Some(acceptance(&["F"])), "EXECUTION_NOT_IN_WORK"),
        (Some(acceptance(&["missing"])), "EXECUTION_NOT_IN_WORK"),
    ];
    for id in ["", "white space", "\n", "\0", " leading"] {
        cases.push((Some(acceptance(&[id])), "WORK_INVALID_ARGUMENT"));
    }
    for (acceptance, error) in cases {
        assert_eq!(
            service
                .update(finish("work", FinishOutcome::Completed, acceptance), None)
                .await
                .unwrap_err(),
            error
        );
        assert_eq!(snapshot(dir.path(), true), before);
    }
    assert_eq!(
        service
            .update(
                finish("work", FinishOutcome::Failed, Some(acceptance(&[]))),
                None
            )
            .await
            .unwrap_err(),
        "WORK_INVALID_ARGUMENT"
    );
    assert_eq!(snapshot(dir.path(), true), before);
    // Empty references are also allowed even though this Work has a linked execution.
    service
        .update(
            finish("work", FinishOutcome::Completed, Some(acceptance(&[]))),
            None,
        )
        .await
        .unwrap();
    for status in ["completed", "failed", "cancelled"] {
        Connection::open(dir.path().join("agent-state.db"))
            .unwrap()
            .execute("UPDATE work_runs SET status=?1 WHERE id='work'", [status])
            .unwrap();
        let before = snapshot(dir.path(), true);
        for action in [
            finish("work", FinishOutcome::Completed, None),
            finish("work", FinishOutcome::Failed, None),
            UpdateAction::Cancel {
                work_run_id: "work".into(),
            },
        ] {
            assert_eq!(
                service.update(action, None).await.unwrap_err(),
                "WORK_NOT_ACTIVE"
            );
            assert_eq!(snapshot(dir.path(), true), before);
        }
        let agent = crate::agent::product::AgentProductService::new(store.clone());
        assert_eq!(
            agent
                .agent_execute(
                    crate::agent::product::AgentExecuteAction::Start {
                        work_run_id: "work".into(),
                        request_key: "new".into(),
                        prompt: "task".into(),
                        delegation_context_json: None
                    },
                    workspace("W", "root")
                )
                .await
                .unwrap_err()
                .code,
            "WORK_NOT_ACTIVE"
        );
        assert_eq!(snapshot(dir.path(), true), before);
    }
    let before = snapshot(dir.path(), true);
    for (id, error) in [
        ("missing", "WORK_NOT_FOUND"),
        (" ", "WORK_INVALID_ARGUMENT"),
    ] {
        for action in [
            finish(id, FinishOutcome::Completed, None),
            UpdateAction::Cancel {
                work_run_id: id.into(),
            },
        ] {
            assert_eq!(service.update(action, None).await.unwrap_err(), error);
            assert_eq!(snapshot(dir.path(), true), before);
        }
    }
}

#[tokio::test]
async fn failed_and_cancelled_work_only_change_the_container_and_block_new_execution() {
    use crate::agent::product::{AgentExecuteAction, AgentProductService};
    for status in ["failed", "cancelled"] {
        let dir = tempfile::tempdir().unwrap();
        let store = StateStore::open(dir.path().into()).await.unwrap();
        create(&store, "work").await;
        let service = WorkProductService::new(store.clone());
        let before = snapshot(dir.path(), false);
        let started = now();
        let action = if status == "failed" {
            finish("work", FinishOutcome::Failed, None)
        } else {
            UpdateAction::Cancel {
                work_run_id: "work".into(),
            }
        };
        let row = service.update(action, None).await.unwrap();
        assert_eq!(row.status, status);
        assert_eq!(row.revision, 1);
        assert_eq!(row.acceptance_json, None);
        assert_eq!(row.completed_at, Some(row.updated_at));
        assert!((started..=now()).contains(&row.updated_at));
        assert_eq!(snapshot(dir.path(), false), before);
        let agent = AgentProductService::new(store.clone());
        assert_eq!(
            agent
                .agent_execute(
                    AgentExecuteAction::Start {
                        work_run_id: "work".into(),
                        request_key: "new".into(),
                        prompt: "task".into(),
                        delegation_context_json: None
                    },
                    workspace("W", "root")
                )
                .await
                .unwrap_err()
                .code,
            "WORK_NOT_ACTIVE"
        );
        assert_eq!(snapshot(dir.path(), false), before);
    }
}

#[tokio::test]
async fn terminal_update_faults_and_zero_affected_rows_roll_back_the_transaction() {
    for trigger in [
        "CREATE TRIGGER reject_terminal AFTER UPDATE ON work_runs BEGIN SELECT RAISE(ABORT,'TEST_TERMINAL_FAILURE'); END;",
        "CREATE TRIGGER reject_terminal BEFORE UPDATE ON work_runs BEGIN UPDATE work_runs SET title='must rollback' WHERE id=NEW.id; SELECT RAISE(IGNORE); END;",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let store = StateStore::open(dir.path().into()).await.unwrap();
        create(&store, "work").await;
        let db = Connection::open(dir.path().join("agent-state.db")).unwrap();
        db.execute_batch(trigger).unwrap();
        let service = WorkProductService::new(store);
        let before = snapshot(dir.path(), true);
        for action in [
            finish("work", FinishOutcome::Completed, Some(acceptance(&[]))),
            finish("work", FinishOutcome::Failed, None),
            UpdateAction::Cancel {
                work_run_id: "work".into(),
            },
        ] {
            let error = service.update(action, None).await.unwrap_err();
            assert!(
                error.contains("TEST_TERMINAL_FAILURE") || error == "WORK_TERMINAL_UPDATE_FAILED",
                "{error}"
            );
            assert_eq!(snapshot(dir.path(), true), before);
        }
        db.execute_batch("DROP TRIGGER reject_terminal").unwrap();
        assert_eq!(
            service
                .update(finish("work", FinishOutcome::Failed, None), None)
                .await
                .unwrap()
                .revision,
            1
        );
    }
}

#[tokio::test]
async fn concurrent_finish_and_cancel_commit_exactly_one_terminal_revision() {
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::open(dir.path().into()).await.unwrap();
    create(&store, "work").await;
    let other = StateStore::open(dir.path().into()).await.unwrap();
    let (finished, cancelled) = tokio::join!(
        store.work_terminal(
            "work".into(),
            TerminalAction::Finish {
                outcome: FinishOutcome::Completed,
                acceptance: Some(acceptance(&[]))
            },
            10
        ),
        other.work_terminal("work".into(), TerminalAction::Cancel, 20)
    );
    let (winner, loser) = if finished.is_ok() {
        (finished.unwrap(), cancelled.unwrap_err())
    } else {
        (cancelled.unwrap(), finished.unwrap_err())
    };
    assert_eq!(loser, "WORK_NOT_ACTIVE");
    assert_eq!(winner.revision, 1);
    assert_eq!(store.work_run("work".into()).await.unwrap(), Some(winner));
}

#[test]
fn work_terminal_errors_remain_stable_at_product_boundary() {
    for code in [
        "WORK_HAS_ACTIVE_EXECUTIONS",
        "WORK_ACCEPTANCE_REQUIRED",
        "WORK_NOT_FOUND",
        "WORK_NOT_ACTIVE",
        "EXECUTION_NOT_IN_WORK",
        "WORK_INVALID_ARGUMENT",
        "WORKSPACE_CONTEXT_MISMATCH",
        "CONTEXT_STALE",
    ] {
        assert_eq!(
            crate::agent::product::ProductError::from(code.to_string()).code,
            code
        );
    }
}

#[tokio::test]
async fn finish_and_execution_admission_share_the_same_atomic_work_guard() {
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::open(dir.path().into()).await.unwrap();
    create(&store, "work").await;
    let other = StateStore::open(dir.path().into()).await.unwrap();
    let (finished, submitted) = tokio::join!(
        store.work_terminal(
            "work".into(),
            TerminalAction::Finish {
                outcome: FinishOutcome::Completed,
                acceptance: Some(acceptance(&[]))
            },
            10
        ),
        other.product_create_fresh_with_work(
            "E".into(),
            "A".into(),
            "key".into(),
            "task".into(),
            "W".into(),
            workspace("W", "root"),
            Some(WorkExecutionContext {
                work_run_id: "work".into(),
                parent_execution_id: None,
                delegation_context_json: None
            }),
            20
        )
    );
    if let Ok(row) = finished {
        assert_eq!(row.status, "completed");
        assert_eq!(submitted.unwrap_err(), "WORK_NOT_ACTIVE");
        assert!(store.execution("E".into()).await.unwrap().is_none());
        assert!(
            store
                .work_execution_links("work".into())
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            store
                .workspace_claim("root".into())
                .await
                .unwrap()
                .is_none()
        );
    } else {
        assert_eq!(finished.unwrap_err(), "WORK_HAS_ACTIVE_EXECUTIONS");
        assert!(submitted.unwrap().created);
        let row = store.work_run("work".into()).await.unwrap().unwrap();
        assert_eq!(row.status, "active");
        assert_eq!(row.revision, 0);
        assert_eq!(row.acceptance_json, None);
        assert_eq!(
            store
                .work_execution_links("work".into())
                .await
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            store
                .workspace_claim("root".into())
                .await
                .unwrap()
                .unwrap()
                .execution_id,
            "E"
        );
    }
}
