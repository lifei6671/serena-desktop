use super::*;
use rusqlite::types::Value;

fn context(work: &str) -> WorkExecutionContext {
    WorkExecutionContext {
        work_run_id: work.into(),
        parent_execution_id: None,
        delegation_context_json: Some(" opaque ' context: not JSON \n".into()),
    }
}

async fn work(store: &StateStore, id: &str) {
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

async fn fresh(
    store: &StateStore,
    id: &str,
    agent: &str,
    key: &str,
    work: Option<WorkExecutionContext>,
    now: i64,
) -> Result<CreateOutcome, String> {
    store
        .product_create_fresh_with_work(
            id.into(),
            agent.into(),
            key.into(),
            "prompt".into(),
            "W".into(),
            Some(WorkspaceSnapshot {
                id: "W".into(),
                root: "root".into(),
            }),
            work,
            now,
        )
        .await
}

async fn continuation(
    store: &StateStore,
    id: &str,
    source: &str,
    key: &str,
    work: Option<WorkExecutionContext>,
) -> Result<CreateOutcome, String> {
    store
        .product_create_continuation_with_work(
            id.into(),
            source.into(),
            key.into(),
            "next".into(),
            work,
            None,
            20,
        )
        .await
}

fn snapshot(store: &StateStore) -> Vec<Vec<Vec<Value>>> {
    let c = store.connection.lock().unwrap();
    [
        "SELECT * FROM executions ORDER BY id",
        "SELECT * FROM workspace_claims ORDER BY execution_id",
        "SELECT * FROM work_execution_links ORDER BY execution_id",
        "SELECT * FROM work_runs ORDER BY id",
    ]
    .iter()
    .map(|sql| {
        let mut q = c.prepare(sql).unwrap();
        let n = q.column_count();
        q.query_map([], |r| (0..n).map(|i| r.get(i)).collect())
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    })
    .collect()
}

fn sql(store: &StateStore, sql: &str) {
    store.connection.lock().unwrap().execute_batch(sql).unwrap();
}

#[tokio::test]
async fn work_preflight_is_read_only_and_matches_creation_request_identity() {
    use crate::agent::product::Action;
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::open(dir.path().into()).await.unwrap();
    work(&store, "work").await;
    let start = Action::Start {
        agent_id: "A".into(),
        workspace_id: "W".into(),
        request_key: "key".into(),
        prompt: "prompt".into(),
    };
    let before = snapshot(&store);
    assert_eq!(
        store
            .product_work_preflight(start.clone(), context("work"))
            .await
            .unwrap(),
        None
    );
    assert_eq!(snapshot(&store), before);
    let created = fresh(&store, "E1", "A", "key", Some(context("work")), 10)
        .await
        .unwrap();
    let before = snapshot(&store);
    assert_eq!(
        store
            .product_work_preflight(start.clone(), context("work"))
            .await
            .unwrap(),
        Some(created.execution_id)
    );
    assert_eq!(snapshot(&store), before);
    let mut changed = context("work");
    changed.delegation_context_json = None;
    assert_eq!(
        store
            .product_work_preflight(start, changed)
            .await
            .unwrap_err(),
        "EXECUTION_REQUEST_KEY_CONFLICT"
    );
    assert_eq!(snapshot(&store), before);

    terminal_parent(&store, "E1").await;
    let action = Action::Continue {
        execution_id: "E1".into(),
        request_key: "next".into(),
        prompt: "next".into(),
    };
    let mut ctx = context("work");
    ctx.parent_execution_id = Some("E1".into());
    let before = snapshot(&store);
    assert_eq!(
        store
            .product_work_preflight(action.clone(), ctx.clone())
            .await
            .unwrap(),
        None
    );
    assert_eq!(snapshot(&store), before);
    let created = continuation(&store, "E2", "E1", "next", Some(ctx.clone()))
        .await
        .unwrap();
    sql(
        &store,
        "UPDATE work_runs SET status='completed' WHERE id='work'",
    );
    let before = snapshot(&store);
    assert_eq!(
        store
            .product_work_preflight(action, ctx.clone())
            .await
            .unwrap(),
        Some(created.execution_id)
    );
    let changed = Action::Continue {
        execution_id: "E1".into(),
        request_key: "next".into(),
        prompt: "changed task".into(),
    };
    assert_eq!(
        store
            .product_work_preflight(changed, ctx)
            .await
            .unwrap_err(),
        "EXECUTION_REQUEST_KEY_CONFLICT"
    );
    assert_eq!(snapshot(&store), before);
}

async fn terminal_parent(store: &StateStore, id: &str) -> ExecutionRecord {
    // Release the Claim through the existing lifecycle API, then supply persisted
    // managed provenance as a fixture. No Provider is launched by these tests.
    store.request_cancel(id.into(), 11).await.unwrap();
    {
        let c = store.connection.lock().unwrap();
        let runtime = format!("runtime-{id}");
        c.execute("INSERT INTO runtime_instances(id,owner_host_instance_id,state,created_at,updated_at) VALUES (?1,'host','running',1,1)", [&runtime]).unwrap();
        let result = json!({"historyMode":"paginated", "executionId":id, "threadId":"T", "turnId":format!("turn-{id}"), "sourceRuntimeId":runtime}).to_string();
        c.execute("UPDATE executions SET runtime_instance_id=?2, thread_id='T', turn_id=?3, final_result_json=?4 WHERE id=?1", params![id, runtime, format!("turn-{id}"), result]).unwrap();
    }
    let row = store.execution(id.into()).await.unwrap().unwrap();
    assert!(continuation_core_eligible(&row));
    row
}

#[tokio::test]
async fn continuation_creation_rechecks_provider_opaque_source_revision() {
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::open(dir.path().into()).await.unwrap();
    fresh(&store, "E1", "A", "source", None, 10).await.unwrap();
    let source = terminal_parent(&store, "E1").await;
    let candidate = store
        .product_continuation_preflight("E1".into(), "next".into(), "next".into(), None)
        .await
        .unwrap();
    let ContinuationPreflight::Candidate(candidate) = candidate else {
        panic!("unexpected retry");
    };
    assert_eq!(candidate.source_revision, source.revision);
    sql(
        &store,
        "UPDATE executions SET revision=revision+1 WHERE id='E1'",
    );
    assert_eq!(
        store
            .product_create_continuation_with_work(
                "E2".into(),
                "E1".into(),
                "next".into(),
                "next".into(),
                None,
                Some(candidate.source_revision),
                20,
            )
            .await
            .unwrap_err(),
        "AGENT_CONTINUE_NOT_ALLOWED"
    );
    assert!(store.execution("E2".into()).await.unwrap().is_none());
    assert!(
        store
            .workspace_claim("root".into())
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn fresh_commits_execution_claim_and_link_and_reopens_exactly() {
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::open(dir.path().into()).await.unwrap();
    work(&store, "work").await;
    let ctx = context("work");
    let created = fresh(&store, "E1", "A", "key", Some(ctx.clone()), 10)
        .await
        .unwrap();
    assert!(created.created);
    assert_eq!(created.execution.status, "dispatch_pending");
    assert_eq!(created.execution.dispatch_state, "not_dispatched");
    assert_eq!(created.execution.runtime_instance_id, None);
    let claim = store.workspace_claim("root".into()).await.unwrap().unwrap();
    assert_eq!(claim.execution_id, "E1");
    assert_eq!(claim.claim_type, "exclusive_execution");
    let links = store.work_execution_links("work".into()).await.unwrap();
    assert_eq!(
        links,
        vec![WorkExecutionLinkRecord {
            work_run_id: "work".into(),
            execution_id: "E1".into(),
            parent_execution_id: None,
            delegation_context_json: ctx.delegation_context_json,
            created_at: 10
        }]
    );
    drop(store);
    let store = StateStore::open(dir.path().into()).await.unwrap();
    assert_eq!(
        store.execution("E1".into()).await.unwrap(),
        Some(created.execution)
    );
    assert_eq!(
        store.work_execution_links("work".into()).await.unwrap(),
        links
    );
    assert_eq!(
        store
            .workspace_claim("root".into())
            .await
            .unwrap()
            .unwrap()
            .execution_id,
        "E1"
    );
}

#[tokio::test]
async fn link_insert_failure_rolls_back_fresh_and_continuation_without_claim_or_key_residue() {
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::open(dir.path().into()).await.unwrap();
    work(&store, "work").await;
    let reject = "CREATE TRIGGER reject_work_link BEFORE INSERT ON work_execution_links BEGIN SELECT RAISE(ABORT,'TEST_LINK_FAILURE'); END;";
    sql(&store, reject);
    let before = snapshot(&store);
    assert!(
        fresh(&store, "E1", "A", "key", Some(context("work")), 10)
            .await
            .unwrap_err()
            .contains("TEST_LINK_FAILURE")
    );
    assert_eq!(snapshot(&store), before);
    assert!(store.execution("E1".into()).await.unwrap().is_none());
    assert!(
        store
            .workspace_claim("root".into())
            .await
            .unwrap()
            .is_none()
    );
    sql(&store, "DROP TRIGGER reject_work_link");
    assert!(
        fresh(&store, "E1", "A", "key", Some(context("work")), 10)
            .await
            .unwrap()
            .created
    );
    let parent = terminal_parent(&store, "E1").await;
    sql(&store, reject);
    let before = snapshot(&store);
    assert!(
        continuation(&store, "E2", "E1", "next", Some(context("work")))
            .await
            .unwrap_err()
            .contains("TEST_LINK_FAILURE")
    );
    assert_eq!(snapshot(&store), before);
    assert_eq!(store.execution("E1".into()).await.unwrap(), Some(parent));
    assert!(store.execution("E2".into()).await.unwrap().is_none());
    assert!(
        store
            .workspace_claim("root".into())
            .await
            .unwrap()
            .is_none()
    );
    sql(&store, "DROP TRIGGER reject_work_link");
    assert!(
        continuation(&store, "E2", "E1", "next", Some(context("work")))
            .await
            .unwrap()
            .created
    );
}

#[tokio::test]
async fn fresh_work_guards_have_no_side_effects() {
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::open(dir.path().into()).await.unwrap();
    for id in [
        "work",
        "completed",
        "failed",
        "cancelled",
        "wrong-id",
        "wrong-root",
    ] {
        work(&store, id).await;
    }
    sql(
        &store,
        "UPDATE work_runs SET status=id WHERE id IN ('completed','failed','cancelled'); UPDATE work_runs SET workspace_id='other' WHERE id='wrong-id'; UPDATE work_runs SET canonical_workspace_root='ROOT' WHERE id='wrong-root';",
    );
    for (id, error) in [
        ("missing", "WORK_NOT_FOUND"),
        ("completed", "WORK_NOT_ACTIVE"),
        ("failed", "WORK_NOT_ACTIVE"),
        ("cancelled", "WORK_NOT_ACTIVE"),
        ("wrong-id", "WORKSPACE_CONTEXT_MISMATCH"),
        ("wrong-root", "WORKSPACE_CONTEXT_MISMATCH"),
    ] {
        let before = snapshot(&store);
        assert_eq!(
            fresh(&store, "E", "A", "key", Some(context(id)), 2)
                .await
                .unwrap_err(),
            error
        );
        assert_eq!(snapshot(&store), before);
    }
    for current in [
        None,
        Some(WorkspaceSnapshot {
            id: "other".into(),
            root: "root".into(),
        }),
        Some(WorkspaceSnapshot {
            id: "W".into(),
            root: "elsewhere".into(),
        }),
    ] {
        let before = snapshot(&store);
        assert_eq!(
            store
                .product_create_fresh_with_work(
                    "E".into(),
                    "A".into(),
                    "key".into(),
                    "prompt".into(),
                    "W".into(),
                    current,
                    Some(context("work")),
                    2
                )
                .await
                .unwrap_err(),
            "WORKSPACE_CONTEXT_MISMATCH"
        );
        assert_eq!(snapshot(&store), before);
    }
    let mut ctx = context("work");
    ctx.parent_execution_id = Some("E0".into());
    let before = snapshot(&store);
    assert_eq!(
        fresh(&store, "E", "A", "key", Some(ctx), 2)
            .await
            .unwrap_err(),
        "WORK_INVALID_ARGUMENT"
    );
    assert_eq!(snapshot(&store), before);
}

#[tokio::test]
async fn fresh_retry_requires_existing_matching_link_before_work_state_or_current_workspace() {
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::open(dir.path().into()).await.unwrap();
    work(&store, "work").await;
    work(&store, "other").await;
    fresh(&store, "E1", "A", "key", Some(context("work")), 10)
        .await
        .unwrap();
    sql(
        &store,
        "UPDATE work_runs SET status='completed' WHERE id='work'",
    );
    let before = snapshot(&store);
    let retry = store
        .product_create_fresh_with_work(
            "E2".into(),
            "A".into(),
            "key".into(),
            "prompt".into(),
            "W".into(),
            None,
            Some(context("work")),
            99,
        )
        .await
        .unwrap();
    assert!(!retry.created);
    assert_eq!(retry.execution_id, "E1");
    assert_eq!(snapshot(&store), before);
    for (agent, key, ctx, error) in [
        ("A", "key", context("other"), "EXECUTION_NOT_IN_WORK"),
        ("A", "new", context("work"), "WORK_NOT_ACTIVE"),
        ("new-agent", "key", context("work"), "WORK_NOT_ACTIVE"),
    ] {
        assert_eq!(
            fresh(&store, "E2", agent, key, Some(ctx), 99)
                .await
                .unwrap_err(),
            error
        );
        assert_eq!(snapshot(&store), before);
    }
    let mut changed = context("work");
    changed.delegation_context_json = None;
    assert_eq!(
        fresh(&store, "E2", "A", "key", Some(changed), 99)
            .await
            .unwrap_err(),
        "EXECUTION_REQUEST_KEY_CONFLICT"
    );
    assert_eq!(
        store
            .product_create_fresh_with_work(
                "E2".into(),
                "A".into(),
                "key".into(),
                "different".into(),
                "W".into(),
                None,
                Some(context("work")),
                99
            )
            .await
            .unwrap_err(),
        "EXECUTION_REQUEST_KEY_CONFLICT"
    );
    assert_eq!(snapshot(&store), before);
}

#[tokio::test]
async fn historical_unlinked_execution_is_never_attached_on_retry() {
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::open(dir.path().into()).await.unwrap();
    work(&store, "work").await;
    fresh(&store, "E1", "A", "key", None, 10).await.unwrap();
    let before = snapshot(&store);
    assert_eq!(
        fresh(&store, "E2", "A", "key", Some(context("work")), 20)
            .await
            .unwrap_err(),
        "EXECUTION_NOT_IN_WORK"
    );
    assert_eq!(snapshot(&store), before);
    let parent = terminal_parent(&store, "E1").await;
    let before = snapshot(&store);
    assert_eq!(
        continuation(&store, "E2", "E1", "next", Some(context("work")))
            .await
            .unwrap_err(),
        "EXECUTION_NOT_IN_WORK"
    );
    assert_eq!(snapshot(&store), before);
    assert_eq!(store.execution("E1".into()).await.unwrap(), Some(parent));
}

#[tokio::test]
async fn continuation_preserves_terminal_parent_and_retry_after_work_completion() {
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::open(dir.path().into()).await.unwrap();
    work(&store, "work").await;
    work(&store, "other").await;
    fresh(&store, "E1", "A", "key", Some(context("work")), 10)
        .await
        .unwrap();
    let parent = terminal_parent(&store, "E1").await;
    let child = continuation(&store, "E2", "E1", "next", Some(context("work")))
        .await
        .unwrap();
    assert!(child.created);
    assert_eq!(child.execution.status, "dispatch_pending");
    assert_eq!(child.execution.parent_execution_id.as_deref(), Some("E1"));
    // C2B leaves current child runtime identity to the Provider adapter.
    assert_eq!(child.execution.thread_id, None);
    assert_eq!(store.execution("E1".into()).await.unwrap(), Some(parent));
    assert_eq!(
        store
            .workspace_claim("root".into())
            .await
            .unwrap()
            .unwrap()
            .execution_id,
        "E2"
    );
    let links = store.work_execution_links("work".into()).await.unwrap();
    assert_eq!(links.len(), 2);
    assert_eq!(links[1].parent_execution_id.as_deref(), Some("E1"));
    assert_eq!(
        links[1].delegation_context_json,
        context("work").delegation_context_json
    );
    sql(
        &store,
        "UPDATE work_runs SET status='completed' WHERE id='work'",
    );
    let before = snapshot(&store);
    let mut ctx = context("work");
    ctx.parent_execution_id = Some("E1".into());
    let retry = continuation(&store, "E3", "E1", "next", Some(ctx))
        .await
        .unwrap();
    assert!(!retry.created);
    assert_eq!(retry.execution_id, "E2");
    assert_eq!(snapshot(&store), before);
    assert_eq!(
        continuation(&store, "E3", "E1", "new-key", Some(context("work")))
            .await
            .unwrap_err(),
        "WORK_NOT_ACTIVE"
    );
    assert_eq!(
        continuation(&store, "E3", "E1", "next", Some(context("other")))
            .await
            .unwrap_err(),
        "EXECUTION_NOT_IN_WORK"
    );
    // Same canonical request cannot borrow a sibling's key with a different parent.
    terminal_parent(&store, "E2").await;
    let before = snapshot(&store);
    assert_eq!(
        continuation(&store, "E3", "E2", "next", Some(context("work")))
            .await
            .unwrap_err(),
        "EXECUTION_REQUEST_KEY_CONFLICT"
    );
    assert_eq!(snapshot(&store), before);
}

#[tokio::test]
async fn continuation_guards_keep_eligibility_claim_and_workspace_contracts() {
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::open(dir.path().into()).await.unwrap();
    work(&store, "work").await;
    fresh(&store, "E1", "A", "key", Some(context("work")), 10)
        .await
        .unwrap();
    let before = snapshot(&store);
    assert_eq!(
        continuation(&store, "E2", "E1", "next", Some(context("work")))
            .await
            .unwrap_err(),
        "AGENT_CONTINUE_NOT_ALLOWED"
    );
    assert_eq!(snapshot(&store), before);
    terminal_parent(&store, "E1").await;
    let mut ctx = context("work");
    ctx.parent_execution_id = Some("wrong-parent".into());
    let before = snapshot(&store);
    assert_eq!(
        continuation(&store, "E2", "E1", "next", Some(ctx))
            .await
            .unwrap_err(),
        "WORK_INVALID_ARGUMENT"
    );
    assert_eq!(snapshot(&store), before);
    for change in [
        "UPDATE work_runs SET workspace_id='other'",
        "UPDATE work_runs SET workspace_id='W',canonical_workspace_root='ROOT'",
    ] {
        sql(&store, change);
        let before = snapshot(&store);
        assert_eq!(
            continuation(&store, "E2", "E1", "next", Some(context("work")))
                .await
                .unwrap_err(),
            "WORKSPACE_CONTEXT_MISMATCH"
        );
        assert_eq!(snapshot(&store), before);
    }
}

#[tokio::test]
async fn original_no_work_entries_still_create_retry_and_continue_without_links() {
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::open(dir.path().into()).await.unwrap();
    let first = store
        .product_create_fresh(
            "E1".into(),
            "A".into(),
            "key".into(),
            "prompt".into(),
            "W".into(),
            Some(WorkspaceSnapshot {
                id: "W".into(),
                root: "root".into(),
            }),
            10,
        )
        .await
        .unwrap();
    let retry = store
        .product_create_fresh(
            "unused".into(),
            "A".into(),
            "key".into(),
            "prompt".into(),
            "W".into(),
            None,
            11,
        )
        .await
        .unwrap();
    assert!(!retry.created);
    assert_eq!(retry.execution_id, first.execution_id);
    let parent = terminal_parent(&store, "E1").await;
    let child = store
        .product_create_continuation("E2".into(), "E1".into(), "next".into(), "next".into(), 20)
        .await
        .unwrap();
    let retry = store
        .product_create_continuation(
            "unused".into(),
            "E1".into(),
            "next".into(),
            "next".into(),
            21,
        )
        .await
        .unwrap();
    assert!(child.created);
    assert!(!retry.created);
    assert_eq!(retry.execution_id, "E2");
    assert_eq!(store.execution("E1".into()).await.unwrap(), Some(parent));
    assert!(snapshot(&store)[2].is_empty());
}

#[tokio::test]
async fn continuation_request_identity_is_parent_execution_not_shared_codex_thread() {
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::open(dir.path().into()).await.unwrap();
    fresh(&store, "E1", "A", "source-one", None, 10)
        .await
        .unwrap();
    terminal_parent(&store, "E1").await;
    store
        .connection
        .lock()
        .unwrap()
        .execute(
            "INSERT INTO executions (id,agent_id,request_key,request_hash,prompt,execution_profile_json,workspace_id,canonical_workspace_root,provider,mode,parent_execution_id,thread_id,status,release_evidence_state,release_evidence_kind,release_evidence_json,created_at,updated_at,completed_at) VALUES ('E2','A','source-two','source-two','prompt','{}','W','root','codex','workspace_write',NULL,'T','completed','complete','not_dispatched','{}',11,11,11)",
            [],
        )
        .unwrap();

    let first = continuation(&store, "E3", "E1", "next", None)
        .await
        .unwrap();
    assert_eq!(first.execution.parent_execution_id.as_deref(), Some("E1"));
    assert_eq!(first.execution.thread_id, None);
    assert_eq!(
        continuation(&store, "unused", "E1", "next", None)
            .await
            .unwrap()
            .execution_id,
        "E3"
    );
    assert_eq!(
        continuation(&store, "E4", "E2", "next", None)
            .await
            .unwrap_err(),
        "EXECUTION_REQUEST_KEY_CONFLICT"
    );
}

#[tokio::test]
async fn pre_c2_null_parent_row_has_only_exact_legacy_retry_compatibility() {
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::open(dir.path().into()).await.unwrap();
    fresh(&store, "E1", "A", "source", None, 10).await.unwrap();
    let source = terminal_parent(&store, "E1").await;
    let child = continuation(&store, "E2", "E1", "next", None)
        .await
        .unwrap();
    let request = canonicalize_request(
        input(
            &source,
            "next".into(),
            "next".into(),
            Some(source.id.clone()),
            source.thread_id.clone(),
        )
        .unwrap(),
    )
    .unwrap();
    let legacy = legacy_pre_c2_continuation_hash(&request.input(), &source.thread_id).unwrap();
    store
        .connection
        .lock()
        .unwrap()
        .execute(
            "UPDATE executions SET parent_execution_id=NULL,request_hash=?2 WHERE id=?1",
            params![child.execution_id, legacy],
        )
        .unwrap();

    let retry = continuation(&store, "unused", "E1", "next", None)
        .await
        .unwrap();
    assert!(!retry.created);
    assert_eq!(retry.execution_id, "E2");
    assert_eq!(
        store
            .product_create_continuation(
                "unused".into(),
                "E1".into(),
                "next".into(),
                "changed".into(),
                21,
            )
            .await
            .unwrap_err(),
        "EXECUTION_REQUEST_KEY_CONFLICT"
    );
}

#[test]
fn product_and_task_manager_continuation_routing_do_not_read_private_thread_identity() {
    let product = include_str!("../../../../product.rs");
    let product_continue = product
        .split("async fn perform")
        .nth(1)
        .unwrap()
        .split("async fn observe_wait")
        .next()
        .unwrap();
    assert!(!product_continue.contains("thread_id"));

    let task_manager = include_str!("../../../../task_manager.rs");
    let continuation = task_manager
        .split("pub(crate) async fn product_submit_with_work")
        .nth(1)
        .unwrap()
        .split("async fn dispatch_pending_execution")
        .next()
        .unwrap();
    assert!(!continuation.contains("thread_id"));
}

#[test]
fn current_continuation_creation_reads_source_thread_only_in_sealed_legacy_hash_compatibility() {
    let source = include_str!("../../product.rs");
    assert_eq!(source.matches("source.thread_id").count(), 1);
    let legacy = source
        .split("fn continuation_prior_outcome")
        .nth(1)
        .unwrap()
        .split("impl StateStore")
        .next()
        .unwrap();
    assert!(legacy.contains("legacy_pre_c2_continuation_hash"));
}

#[tokio::test]
async fn concurrent_identical_work_requests_have_one_execution_claim_and_link() {
    let dir = tempfile::tempdir().unwrap();
    let one = StateStore::open(dir.path().into()).await.unwrap();
    let two = StateStore::open(dir.path().into()).await.unwrap();
    work(&one, "work").await;
    let (a, b) = tokio::join!(
        fresh(&one, "E1", "A", "key", Some(context("work")), 10),
        fresh(&two, "E2", "A", "key", Some(context("work")), 10)
    );
    let (a, b) = (a.unwrap(), b.unwrap());
    assert_ne!(a.created, b.created);
    assert_eq!(a.execution_id, b.execution_id);
    let rows = snapshot(&one);
    assert_eq!(rows[0].len(), 1);
    assert_eq!(rows[1].len(), 1);
    assert_eq!(rows[2].len(), 1);
}

#[tokio::test]
async fn link_list_is_exact_stable_and_read_only() {
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::open(dir.path().into()).await.unwrap();
    work(&store, "work").await;
    work(&store, "other").await;
    for (id, time, work_id) in [
        ("b", 20, "work"),
        ("a", 20, "work"),
        ("old", 10, "work"),
        ("other", 1, "other"),
    ] {
        fresh(&store, id, id, "key", Some(context(work_id)), time)
            .await
            .unwrap();
        store.request_cancel(id.into(), 30).await.unwrap();
    }
    let before = snapshot(&store);
    for _ in 0..2 {
        let links = store.work_execution_links("work".into()).await.unwrap();
        assert_eq!(
            links
                .iter()
                .map(|l| l.execution_id.as_str())
                .collect::<Vec<_>>(),
            ["old", "a", "b"]
        );
        assert!(
            store
                .work_execution_links("work' OR 1=1 --".into())
                .await
                .unwrap()
                .is_empty()
        );
    }
    assert_eq!(snapshot(&store), before);
}
