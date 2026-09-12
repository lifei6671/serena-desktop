use super::*;
use rusqlite::types::Value;

fn snapshot(c: &Connection, sql: &str) -> Vec<Vec<Value>> {
    let mut statement = c.prepare(sql).unwrap();
    let columns = statement.column_count();
    statement
        .query_map([], |row| (0..columns).map(|i| row.get(i)).collect())
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

fn legacy_database(c: &mut Connection, version: i64) {
    c.pragma_update(None, "foreign_keys", true).unwrap();
    for (number, schema) in [
        (1, SCHEMA_V1),
        (2, SCHEMA_V2),
        (3, SCHEMA_V3),
        (4, SCHEMA_V4),
    ] {
        if number <= version {
            c.execute_batch(schema).unwrap();
        }
    }
    c.pragma_update(None, "user_version", version).unwrap();
    insert(c, "legacy", "agent", "root");
    runtime(c, "legacy-runtime");
    c.execute(
        "UPDATE executions SET runtime_instance_id='legacy-runtime', status='unknown',
         revision=7, error_code='legacy-error', updated_at=456 WHERE id='legacy'",
        [],
    )
    .unwrap();
    c.execute(
        "INSERT INTO workspace_claims VALUES ('root','legacy','exclusive_execution',789)",
        [],
    )
    .unwrap();
}

#[test]
fn v4_upgrade_preserves_complete_execution_claim_and_existing_schema() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = Connection::open(dir.path().join("agent-state.db")).unwrap();
    legacy_database(&mut c, 4);
    let queries = [
        "SELECT * FROM executions ORDER BY id",
        "SELECT * FROM workspace_claims ORDER BY canonical_workspace_root",
        "SELECT * FROM runtime_instances ORDER BY id",
        "SELECT * FROM sqlite_schema ORDER BY name",
    ];
    let before: Vec<_> = queries.iter().map(|sql| snapshot(&c, sql)).collect();
    drop(c);

    for _ in 0..2 {
        let store = open(dir.path());
        let c = store.connection.lock().unwrap();
        assert_eq!(
            c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            5
        );
        for (sql, expected) in queries[..3].iter().zip(&before[..3]) {
            assert_eq!(&snapshot(&c, sql), expected, "{sql}");
        }
        let schema = snapshot(&c, queries[3]);
        for row in &before[3] {
            assert!(schema.contains(row), "legacy schema changed: {row:?}");
        }
        assert!(snapshot(&c, "SELECT * FROM work_runs").is_empty());
        assert!(snapshot(&c, "SELECT * FROM work_execution_links").is_empty());
    }
}

#[test]
fn v5_failure_rolls_back_the_whole_upgrade_from_every_supported_version() {
    for version in 1..=4 {
        let mut c = Connection::open_in_memory().unwrap();
        legacy_database(&mut c, version);
        // Fail after the first v5 CREATE TABLE, including any earlier migrations.
        c.execute_batch(
            "CREATE TABLE work_execution_links (sentinel TEXT);
                         INSERT INTO work_execution_links VALUES ('preserve');",
        )
        .unwrap();
        let queries = [
            "SELECT * FROM sqlite_schema ORDER BY name",
            "SELECT * FROM executions",
            "SELECT * FROM workspace_claims",
            "SELECT * FROM work_execution_links",
        ];
        let before: Vec<_> = queries.iter().map(|sql| snapshot(&c, sql)).collect();
        assert!(migrate(&mut c).unwrap_err().contains("already exists"));
        assert_eq!(
            c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            version
        );
        for (sql, expected) in queries.iter().zip(before) {
            assert_eq!(snapshot(&c, sql), expected, "v{version}: {sql}");
        }
    }
}

#[test]
fn create_get_list_work_runs_survive_reopen_with_exact_values_and_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let title = "中文 ' ; --";
    let expected = WorkRunRecord {
        id: "b".into(),
        workspace_id: "workspace '".into(),
        canonical_workspace_root: "C:/中文/root".into(),
        title: title.into(),
        goal: Some("goal ' ; --".into()),
        status: "active".into(),
        revision: 0,
        acceptance_json: None,
        created_at: 20,
        updated_at: 20,
        completed_at: None,
    };
    {
        let store = open(dir.path());
        tauri::async_runtime::block_on(async {
            assert!(store.list_work_runs(None, 10).await.unwrap().is_empty());
            for (id, workspace, now, goal) in [
                ("b", "workspace '", 20, expected.goal.clone()),
                ("a", "workspace '", 20, None),
                ("newest", "other", 30, None),
                ("oldest", "workspace '", 10, None),
            ] {
                store
                    .create_work_run(
                        id.into(),
                        workspace.into(),
                        expected.canonical_workspace_root.clone(),
                        title.into(),
                        goal,
                        now,
                    )
                    .await
                    .unwrap();
            }
            assert!(
                store
                    .create_work_run(
                        "b".into(),
                        "replacement".into(),
                        "root".into(),
                        "replacement".into(),
                        None,
                        99
                    )
                    .await
                    .is_err()
            );
        });
    }
    let store = open(dir.path());
    tauri::async_runtime::block_on(async {
        assert_eq!(
            store.work_run("b".into()).await.unwrap(),
            Some(expected.clone())
        );
        assert_eq!(store.work_run("missing".into()).await.unwrap(), None);
        assert_eq!(store.work_run("b' OR 1=1 --".into()).await.unwrap(), None);
        let rows = store.list_work_runs(None, 10).await.unwrap();
        assert_eq!(
            rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            ["newest", "a", "b", "oldest"]
        );
        assert_eq!(rows[2], expected);
        assert_eq!(rows[1].goal, None);
        let filtered = store
            .list_work_runs(Some("workspace '".into()), 2)
            .await
            .unwrap();
        assert_eq!(filtered, rows[1..3]);
        assert!(
            store
                .list_work_runs(Some("missing".into()), 10)
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(store.list_work_runs(None, 0).await.unwrap(), rows[..1]);
    });
}

#[test]
fn work_run_list_caps_large_limits_and_projects_nullable_values_as_stored() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    tauri::async_runtime::block_on(async {
        for i in 0..101 {
            store
                .create_work_run(
                    format!("w{i:03}"),
                    "workspace".into(),
                    "root".into(),
                    "title".into(),
                    None,
                    i,
                )
                .await
                .unwrap();
        }
        {
            let c = store.connection.lock().unwrap();
            c.execute(
                r#"UPDATE work_runs SET status='completed', revision=9,
                       acceptance_json='{"status":"pass"}', updated_at=200, completed_at=199
                       WHERE id='w100'"#,
                [],
            )
            .unwrap();
        }
        let rows = store.list_work_runs(None, usize::MAX).await.unwrap();
        assert_eq!(rows.len(), 100);
        assert_eq!(rows[99].id, "w001");
        let row = store.work_run("w100".into()).await.unwrap().unwrap();
        assert_eq!(row, rows[0]);
        assert_eq!(row.status, "completed");
        assert_eq!(row.revision, 9);
        assert_eq!(row.acceptance_json.as_deref(), Some(r#"{"status":"pass"}"#));
        assert_eq!(row.created_at, 100);
        assert_eq!(row.updated_at, 200);
        assert_eq!(row.completed_at, Some(199));
    });
}

#[test]
fn work_run_schema_matches_design_and_enforces_check_not_null_and_primary_key() {
    let design =
        include_str!("../../../../../docs/core-work-orchestration.md").replace("\r\n", "\n");
    let ddl: String = design
        .split("```sql\n")
        .skip(1)
        .map(|block| block.split("```").next().unwrap())
        .filter(|sql| {
            sql.starts_with("CREATE TABLE work_runs (")
                || sql.starts_with("CREATE TABLE work_execution_links (")
        })
        .collect();
    let normalize = |sql: &str| sql.split_whitespace().collect::<String>();
    assert!(!ddl.is_empty());
    assert_eq!(normalize(SCHEMA_V5), normalize(&ddl));

    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    let c = store.connection.lock().unwrap();
    c.execute("INSERT INTO work_runs (id,workspace_id,canonical_workspace_root,title,status,created_at,updated_at)
               VALUES ('w','workspace','root','title','active',1,1)", []).unwrap();
    for status in ["active", "completed", "failed", "cancelled"] {
        c.execute("UPDATE work_runs SET status=?1 WHERE id='w'", [status])
            .unwrap();
    }
    let error = c
        .execute("UPDATE work_runs SET status='paused'", [])
        .unwrap_err();
    assert_eq!(
        error.sqlite_error().unwrap().extended_code,
        rusqlite::ffi::SQLITE_CONSTRAINT_CHECK
    );
    for column in [
        "id",
        "workspace_id",
        "canonical_workspace_root",
        "title",
        "status",
        "revision",
        "created_at",
        "updated_at",
    ] {
        let error = c
            .execute(&format!("UPDATE work_runs SET {column}=NULL"), [])
            .unwrap_err();
        assert_eq!(
            error.sqlite_error().unwrap().extended_code,
            rusqlite::ffi::SQLITE_CONSTRAINT_NOTNULL,
            "{column}"
        );
    }
    let error = c
        .execute("INSERT INTO work_runs SELECT * FROM work_runs", [])
        .unwrap_err();
    assert_eq!(
        error.sqlite_error().unwrap().extended_code,
        rusqlite::ffi::SQLITE_CONSTRAINT_PRIMARYKEY
    );
}

#[test]
fn work_execution_links_enforce_unique_foreign_keys_and_restrict_deletes() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    let mut c = store.connection.lock().unwrap();
    insert(&mut c, "e1", "a1", "root");
    insert(&mut c, "e2", "a2", "root");
    for id in ["w1", "w2"] {
        c.execute("INSERT INTO work_runs (id,workspace_id,canonical_workspace_root,title,status,created_at,updated_at)
                   VALUES (?1,'workspace','root','title','active',1,1)", [id]).unwrap();
    }
    let link =
        "INSERT INTO work_execution_links (work_run_id,execution_id,created_at) VALUES (?1,?2,1)";
    for pair in [["missing", "e1"], ["w1", "missing"]] {
        let error = c.execute(link, pair).unwrap_err();
        assert_eq!(
            error.sqlite_error().unwrap().extended_code,
            rusqlite::ffi::SQLITE_CONSTRAINT_FOREIGNKEY
        );
    }
    c.execute(link, ["w1", "e1"]).unwrap();
    let error = c.execute(link, ["w2", "e1"]).unwrap_err();
    assert_eq!(
        error.sqlite_error().unwrap().extended_code,
        rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE
    );
    assert!(c.execute(link, ["w1", "e1"]).is_err());
    assert_eq!(
        snapshot(
            &c,
            "SELECT parent_execution_id,delegation_context_json FROM work_execution_links"
        ),
        vec![vec![Value::Null, Value::Null]]
    );
    // Parent identity has no additional business validation in this unit.
    c.execute(
        "INSERT INTO work_execution_links VALUES ('w1','e2','unvalidated-parent','{}',2)",
        [],
    )
    .unwrap();
    for sql in [
        "DELETE FROM work_runs WHERE id='w1'",
        "DELETE FROM executions WHERE id='e1'",
    ] {
        assert!(c.execute(sql, []).is_err(), "{sql}");
    }
    assert_eq!(
        snapshot(
            &c,
            "SELECT execution_id FROM work_execution_links ORDER BY execution_id"
        )
        .len(),
        2
    );
    // With no other references, removal succeeds only after the link is removed.
    c.execute(
        "DELETE FROM work_execution_links WHERE execution_id='e1'",
        [],
    )
    .unwrap();
    assert_eq!(
        c.execute("DELETE FROM executions WHERE id='e1'", [])
            .unwrap(),
        1
    );
    c.execute(
        "DELETE FROM work_execution_links WHERE execution_id='e2'",
        [],
    )
    .unwrap();
    assert_eq!(
        c.execute("DELETE FROM work_runs WHERE id='w1'", [])
            .unwrap(),
        1
    );
}
