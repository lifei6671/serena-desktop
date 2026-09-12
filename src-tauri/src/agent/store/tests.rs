use super::super::execution::{CreateExecutionInput, canonicalize_request};
use super::*;
use serde_json::json;

mod work_runs;

fn open(directory: &std::path::Path) -> StateStore {
    tauri::async_runtime::block_on(StateStore::open(directory.to_path_buf())).unwrap()
}
fn request(agent: &str, root: &str) -> CanonicalRequest {
    let input: CreateExecutionInput =
        serde_json::from_value(json!({"agent_id":agent,"request_key":"key",
        "prompt":"中文 ' ; --", "execution_profile":{"z":2,"a":1},"workspace_id":"w",
        "canonical_workspace_root":root,"mode":"workspace_write"}))
        .unwrap();
    canonicalize_request(input).unwrap()
}
fn insert(c: &mut Connection, id: &str, agent: &str, root: &str) {
    let tx = c.transaction().unwrap();
    insert_execution(&tx, id, 123, &request(agent, root)).unwrap();
    tx.commit().unwrap();
}
fn runtime(c: &Connection, id: &str) {
    c.execute("INSERT INTO runtime_instances (id,owner_host_instance_id,state,created_at,updated_at) VALUES (?1,'host','unknown',1,1)", [id]).unwrap();
}

#[test]
fn fresh_and_reopened_database_has_schema_and_every_connection_policy() {
    let dir = tempfile::tempdir().unwrap();
    for _ in 0..2 {
        let store = open(dir.path());
        let c = store.connection.lock().unwrap();
        for (pragma, expected) in [
            ("user_version", 5),
            ("foreign_keys", 1),
            ("synchronous", 2),
            ("busy_timeout", 5000),
        ] {
            assert_eq!(
                c.pragma_query_value(None, pragma, |r| r.get::<_, i64>(0))
                    .unwrap(),
                expected,
                "{pragma}"
            );
        }
        assert_eq!(
            c.pragma_query_value(None, "journal_mode", |r| r.get::<_, String>(0))
                .unwrap(),
            "wal"
        );
        for name in [
            "runtime_instances",
            "executions",
            "workspace_claims",
            "work_runs",
            "work_execution_links",
            "prevent_execution_runtime_rebind",
            "executions_runtime_state",
            "executions_one_unresolved_per_agent",
        ] {
            assert_eq!(
                c.query_row(
                    "SELECT count(*) FROM sqlite_schema WHERE name=?1",
                    [name],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
                1
            );
        }
    }
    assert!(dir.path().join("agent-state.db").is_file());
}

#[test]
fn migration_failure_rolls_back_all_ddl_and_version() {
    let mut c = Connection::open_in_memory().unwrap();
    {
        let tx = c.transaction().unwrap();
        let invalid = format!("{SCHEMA_V1}\nCREATE TABLE broken (");
        assert!(apply_migration(&tx, 1, &invalid).is_err());
    }
    assert_eq!(
        c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        c.query_row("SELECT count(*) FROM sqlite_schema", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    migrate(&mut c).unwrap();
    // A later migration that changes data and version also rolls back on failure.
    runtime(&c, "old");
    {
        let tx = c.transaction().unwrap();
        apply_migration(
            &tx,
            2,
            "CREATE TABLE provisional (id TEXT); UPDATE runtime_instances SET state='terminated';",
        )
        .unwrap();
        assert!(
            tx.execute_batch("INSERT INTO nonexistent VALUES (1);")
                .is_err()
        );
    }
    assert_eq!(
        c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        5
    );
    assert_eq!(
        c.query_row(
            "SELECT state FROM runtime_instances WHERE id='old'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "unknown"
    );
    assert_eq!(
        c.query_row(
            "SELECT count(*) FROM sqlite_schema WHERE name='provisional'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
}

#[test]
fn v1_migration_preserves_executions_and_adds_nullable_thread_names() {
    let mut c = Connection::open_in_memory().unwrap();
    c.execute_batch(SCHEMA_V1).unwrap();
    c.pragma_update(None, "user_version", 1).unwrap();
    insert(&mut c, "old", "agent", "root");
    let before: (String, String, i64) = c
        .query_row(
            "SELECT id,prompt,revision FROM executions WHERE id='old'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    migrate(&mut c).unwrap();
    let after: (String, String, i64) = c
        .query_row(
            "SELECT id,prompt,revision FROM executions WHERE id='old'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(after, before);
    let old = execution_record(&c, "old").unwrap().unwrap();
    assert_eq!(old.last_activity_at, None);
    assert_eq!(old.activity_phase, None);
    assert_eq!(old.tool_category, None);
    assert_eq!(
        c.query_row("SELECT count(*) FROM thread_names", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    migrate(&mut c).unwrap();
    assert_eq!(
        c.query_row(
            "SELECT id,prompt,revision FROM executions WHERE id='old'",
            [],
            |row| Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            )),
        )
        .unwrap(),
        before
    );
}

#[test]
fn v2_migration_preserves_history_and_adds_nullable_activity() {
    let mut c = Connection::open_in_memory().unwrap();
    c.execute_batch(SCHEMA_V1).unwrap();
    c.execute_batch(SCHEMA_V2).unwrap();
    c.pragma_update(None, "user_version", 2).unwrap();
    insert(&mut c, "old", "agent", "root");
    migrate(&mut c).unwrap();
    assert_eq!(
        c.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .unwrap(),
        5
    );
    let old = execution_record(&c, "old").unwrap().unwrap();
    assert_eq!(old.last_activity_at, None);
    assert_eq!(old.activity_phase, None);
    assert_eq!(old.tool_category, None);
}

#[test]
fn unsupported_or_unversioned_history_is_not_guessed_or_rewritten() {
    let mut c = Connection::open_in_memory().unwrap();
    c.execute_batch(
        "CREATE TABLE historical (evidence TEXT); INSERT INTO historical VALUES(NULL);",
    )
    .unwrap();
    assert!(
        migrate(&mut c)
            .unwrap_err()
            .contains("historical migration")
    );
    assert_eq!(
        c.query_row("SELECT evidence FROM historical", [], |r| r
            .get::<_, Option<String>>(0))
            .unwrap(),
        None
    );
    assert_eq!(
        c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    c.pragma_update(None, "user_version", 6).unwrap();
    assert!(migrate(&mut c).unwrap_err().contains("unsupported"));
    assert_eq!(
        c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        6
    );
}

#[test]
fn foreign_keys_and_runtime_immutability_are_enforced_by_sqlite() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    let mut c = store.connection.lock().unwrap();
    insert(&mut c, "e", "a", "root");
    assert!(
        c.execute(
            "UPDATE executions SET runtime_instance_id='missing' WHERE id='e'",
            []
        )
        .is_err()
    );
    c.execute(
        "UPDATE executions SET runtime_instance_id=NULL WHERE id='e'",
        [],
    )
    .unwrap();
    runtime(&c, "r1");
    runtime(&c, "r2");
    c.execute(
        "UPDATE executions SET runtime_instance_id='r1' WHERE id='e'",
        [],
    )
    .unwrap();
    c.execute(
        "UPDATE executions SET runtime_instance_id='r1' WHERE id='e'",
        [],
    )
    .unwrap();
    for sql in [
        "UPDATE executions SET runtime_instance_id='r2' WHERE id='e'",
        "UPDATE executions SET runtime_instance_id=NULL WHERE id='e'",
    ] {
        assert!(
            c.execute(sql, [])
                .unwrap_err()
                .to_string()
                .contains("immutable")
        );
    }
    assert!(
        c.execute("DELETE FROM runtime_instances WHERE id='r1'", [])
            .is_err()
    );
    assert!(
        c.execute(
            "UPDATE executions SET provider_terminal_evidence_runtime_instance_id='missing'",
            []
        )
        .is_err()
    );
    assert!(
        c.execute(
            "UPDATE executions SET background_cleanup_runtime_instance_id='missing'",
            []
        )
        .is_err()
    );
    assert!(
        c.execute(
            "UPDATE executions SET runtime_termination_evidence_runtime_instance_id='missing'",
            []
        )
        .is_err()
    );
}

#[test]
fn claim_schema_conflicts_and_transaction_rollback_without_lifecycle_api() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    let mut c = store.connection.lock().unwrap();
    insert(&mut c, "e1", "a1", "root");
    insert(&mut c, "e2", "a2", "root");
    assert!(
        c.execute(
            "INSERT INTO workspace_claims VALUES ('wrong','e1','exclusive_execution',1)",
            []
        )
        .is_err()
    );
    c.execute(
        "INSERT INTO workspace_claims VALUES ('root','e1','exclusive_execution',1)",
        [],
    )
    .unwrap();
    assert!(
        c.execute(
            "INSERT INTO workspace_claims VALUES ('root','e2','exclusive_execution',1)",
            []
        )
        .is_err()
    );
    assert!(
        c.execute("DELETE FROM executions WHERE id='e1'", [])
            .is_err()
    );
    {
        let tx = c.transaction().unwrap();
        tx.execute("UPDATE executions SET status='completed' WHERE id='e1'", [])
            .unwrap();
        tx.execute("DELETE FROM workspace_claims WHERE execution_id='e1'", [])
            .unwrap();
        assert!(
            tx.execute(
                "INSERT INTO workspace_claims VALUES ('wrong','missing','exclusive_execution',1)",
                []
            )
            .is_err()
        );
    }
    assert_eq!(
        c.query_row("SELECT status FROM executions WHERE id='e1'", [], |r| {
            r.get::<_, String>(0)
        })
        .unwrap(),
        "dispatch_pending"
    );
    assert_eq!(
        c.query_row("SELECT execution_id FROM workspace_claims", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "e1"
    );
}

#[test]
fn historical_null_evidence_and_claim_survive_reopen_and_read_api() {
    let dir = tempfile::tempdir().unwrap();
    {
        let store = open(dir.path());
        let mut c = store.connection.lock().unwrap();
        runtime(&c, "old");
        insert(&mut c, "e", "a", "root");
        c.execute(
            "UPDATE executions SET status='unknown', runtime_instance_id='old'",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO workspace_claims VALUES ('root','e','exclusive_execution',1)",
            [],
        )
        .unwrap();
    }
    let store = open(dir.path());
    tauri::async_runtime::block_on(async {
        let r = store.runtime("old".into()).await.unwrap().unwrap();
        assert_eq!(r.state, "unknown");
        assert_eq!(r.job_session_id, None);
        assert_eq!(r.job_creation_mode, None);
        assert_eq!(r.job_handle_inheritable, None);
        assert_eq!(r.job_kill_on_close, None);
        assert_eq!(r.job_breakaway_allowed, None);
        assert_eq!(r.job_policy_verified_at, None);
        assert_eq!(r.termination_evidence_state, "unknown");
        assert_eq!(r.termination_evidence_type, None);
        assert_eq!(r.termination_evidence_at, None);
        let e = store.execution("e".into()).await.unwrap().unwrap();
        assert_eq!(e.status, "unknown");
        assert_eq!(e.background_cleanup_state, "unknown");
        assert_eq!(e.release_evidence_state, "incomplete");
        assert_eq!(e.release_evidence_kind, None);
        assert_eq!(e.release_evidence_json, None);
        assert_eq!(e.result_completeness, "unknown");
        assert_eq!(
            store
                .workspace_claim("root".into())
                .await
                .unwrap()
                .unwrap()
                .execution_id,
            "e"
        );
    });
}

#[test]
fn repository_insert_is_plain_insert_and_duplicate_cannot_replace_record() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    {
        let mut c = store.connection.lock().unwrap();
        insert(&mut c, "e", "a", "root");
        runtime(&c, "r");
        c.execute("UPDATE executions SET runtime_instance_id='r'", [])
            .unwrap();
        let tx = c.transaction().unwrap();
        assert!(insert_execution(&tx, "e", 999, &request("other", "other-root")).is_err());
    }
    tauri::async_runtime::block_on(async {
        let e = store.execution("e".into()).await.unwrap().unwrap();
        let canonical = request("a", "root");
        assert_eq!(e.request_hash, canonical.request_hash());
        assert_eq!(e.prompt, canonical.input().prompt);
        assert_eq!(e.execution_profile_json, canonical.execution_profile_json());
        assert_eq!(e.runtime_instance_id.as_deref(), Some("r"));
        assert_eq!(e.agent_id, "a");
        assert_eq!(e.dispatch_state, "not_dispatched");
        assert_eq!(e.revision, 0);
        assert!(store.execution("missing".into()).await.unwrap().is_none());
        assert!(store.runtime("missing".into()).await.unwrap().is_none());
        assert!(
            store
                .workspace_claim("missing".into())
                .await
                .unwrap()
                .is_none()
        );
    });
    let source = include_str!("../store.rs").to_ascii_uppercase();
    assert!(!source.contains("INSERT OR REPLACE"));
    assert!(!source.contains("REPLACE INTO"));
}

#[test]
fn schema_matches_approved_design_sql_exactly() {
    let design = include_str!("../../../../docs/codex-agent-runtime.md");
    let blocks: Vec<_> = design
        .split("```sql")
        .skip(1)
        .map(|part| {
            part.split("```")
                .next()
                .unwrap()
                .trim()
                .replace("\r\n", "\n")
        })
        .collect();
    assert_eq!(SCHEMA_V1.trim().replace("\r\n", "\n"), blocks.join("\n\n"));
}

#[test]
fn repository_wait_does_not_block_single_thread_async_executor() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    let guard = store.connection.lock().unwrap();
    let executor = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    executor.block_on(async {
        // Holding the connection guarantees the repository operation cannot finish.
        // Its future must nevertheless yield, allowing this same-thread timer to fire.
        assert!(
            tokio::time::timeout(Duration::from_millis(50), store.execution("missing".into()))
                .await
                .is_err()
        );
    });
    drop(guard);
}

#[test]
fn v3_migration_adds_attempt_reservations_without_changing_legacy_rows() {
    let mut c=Connection::open_in_memory().unwrap();
    c.pragma_update(None,"foreign_keys",true).unwrap();
    c.execute_batch(SCHEMA_V1).unwrap(); c.execute_batch(SCHEMA_V2).unwrap(); c.execute_batch(SCHEMA_V3).unwrap();
    c.pragma_update(None,"user_version",3).unwrap();
    insert(&mut c,"E1","A","W"); runtime(&c,"runtime-E1");
    let before=execution_record(&c,"E1").unwrap().unwrap();
    migrate(&mut c).unwrap(); migrate(&mut c).unwrap();
    assert_eq!(execution_record(&c,"E1").unwrap().unwrap(),before);
    assert!(runtime_attempts::runtime_attempt_exists(&c,"E1").unwrap());
    c.execute("INSERT INTO execution_runtime_attempts(execution_id,runtime_instance_id,created_at) VALUES ('E1','R123',1)",[]).unwrap();
    assert!(runtime_attempts::runtime_attempt_exists(&c,"E1").unwrap());
    assert!(c.execute("UPDATE execution_runtime_attempts SET runtime_instance_id='R2'",[]).is_err());
    assert!(c.execute("DELETE FROM execution_runtime_attempts",[]).is_err());
    assert!(c.execute("INSERT INTO execution_runtime_attempts(execution_id,runtime_instance_id,created_at) VALUES ('missing','R2',1)",[]).is_err());
}
