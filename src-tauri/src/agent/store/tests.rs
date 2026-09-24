use super::super::execution::{
    CreateExecutionInput, canonicalize_request, legacy_pre_workspace_generation_hash,
};
use super::work_runs::work_run_record;
use super::*;
use serde_json::json;

mod work_runs;

fn open(directory: &std::path::Path) -> StateStore {
    tauri::async_runtime::block_on(StateStore::open(directory.to_path_buf())).unwrap()
}

/// 从冻结 SQL 构造真实 v9 数据库，避免测试依赖当前 schema 拼装旧版本。
fn frozen_v9_connection() -> Connection {
    let connection = Connection::open_in_memory().unwrap();
    connection
        .execute_batch(include_str!("../../../tests/fixtures/agent_state_v9.sql"))
        .unwrap();
    connection
}

/// v9 升级测试读取的 v10 平台证据投影。
#[derive(Debug, PartialEq, Eq)]
struct V10PlatformProjection {
    runtime_platform: String,
    containment_type: String,
    process_identity_scheme: String,
    process_group_id: Option<i64>,
    session_id: Option<i64>,
    verified_at: Option<i64>,
    evidence_type: String,
    evidence_at: i64,
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
fn insert_pre_v6(c: &Connection, id: &str, agent: &str, root: &str, thread_id: Option<&str>) {
    let request = request(agent, root);
    let request_hash = legacy_pre_workspace_generation_hash(request.input()).unwrap();
    c.execute(
        "INSERT INTO executions (id,agent_id,request_key,request_hash,prompt,execution_profile_json,workspace_id,canonical_workspace_root,provider,mode,thread_id,status,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,'codex','workspace_write',?9,'dispatch_pending',123,123)",
        params![id, agent, request.input().request_key, request_hash, request.input().prompt, request.execution_profile_json(), request.input().workspace_id, root, thread_id],
    ).unwrap();
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
            ("user_version", 11),
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
            "command_runs",
            "work_command_links",
            "execution_activity_events",
            "execution_usage",
            "codex_thread_usage_epochs",
            "codex_execution_usage_state",
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
        assert_eq!(
            c.query_row(
                "SELECT count(*) FROM sqlite_schema WHERE name='codex_thread_usage_checkpoints'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            0
        );
    }
    assert!(dir.path().join("agent-state.db").is_file());
}

/// 验证冻结的 Windows v9 Runtime/Execution/Claim 完整升级且不改写历史证据。
#[test]
fn migrates_frozen_v9_fixture_through_v11() {
    let mut connection = frozen_v9_connection();
    assert_eq!(
        connection
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .unwrap(),
        9
    );

    migrate(&mut connection).unwrap();

    assert_eq!(
        connection
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .unwrap(),
        11
    );
    let projected = connection
        .query_row(
            "SELECT runtime_platform, containment_type, process_identity_scheme,
                        containment_process_group_id, containment_session_id,
                        containment_verified_at, termination_evidence_type,
                        termination_evidence_at
                 FROM runtime_instances WHERE id='runtime-v9'",
            [],
            |row| {
                Ok(V10PlatformProjection {
                    runtime_platform: row.get(0)?,
                    containment_type: row.get(1)?,
                    process_identity_scheme: row.get(2)?,
                    process_group_id: row.get(3)?,
                    session_id: row.get(4)?,
                    verified_at: row.get(5)?,
                    evidence_type: row.get(6)?,
                    evidence_at: row.get(7)?,
                })
            },
        )
        .unwrap();
    assert_eq!(
        projected,
        V10PlatformProjection {
            runtime_platform: "windows".into(),
            containment_type: "windows_job".into(),
            process_identity_scheme: "windows_filetime_v1".into(),
            process_group_id: None,
            session_id: None,
            verified_at: None,
            evidence_type: "managed_job_destroyed".into(),
            evidence_at: 200,
        }
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT count(*) FROM workspace_claims c
                 JOIN executions e ON e.id=c.execution_id
                 JOIN runtime_instances r ON r.id=e.runtime_instance_id
                 WHERE c.execution_id='execution-v9' AND r.id='runtime-v9'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        1
    );
}

/// 验证 v10 最终历史行校验失败时新增列、触发器、数据和版本号全部回滚。
#[test]
fn v10_migration_failure_preserves_v9_database() {
    let mut connection = frozen_v9_connection();
    connection
        .execute_batch(
            "CREATE TRIGGER reject_v10_validation BEFORE UPDATE ON runtime_instances
             BEGIN SELECT RAISE(ABORT, 'fixture rejects validation update'); END;",
        )
        .unwrap();

    assert!(
        migrate(&mut connection)
            .unwrap_err()
            .contains("fixture rejects validation update")
    );
    assert_eq!(
        connection
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .unwrap(),
        9
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT count(*) FROM pragma_table_info('runtime_instances')
                 WHERE name='runtime_platform'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        0
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT count(*) FROM sqlite_schema
                 WHERE type='trigger' AND name LIKE 'runtime_instances_v10_%'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        0
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT count(*) FROM workspace_claims WHERE execution_id='execution-v9'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        1
    );
}

/// 验证 v10 触发器拒绝平台、身份与 complete evidence 的错误组合。
#[test]
fn v10_runtime_platform_constraints_reject_mismatched_evidence() {
    let mut connection = Connection::open_in_memory().unwrap();
    migrate(&mut connection).unwrap();
    let macos_insert = |connection: &Connection,
                        id: &str,
                        state: &str,
                        pid: Option<i64>,
                        pgid: Option<i64>,
                        sid: Option<i64>,
                        token: Option<&str>,
                        verified: Option<i64>| {
        connection.execute(
            "INSERT INTO runtime_instances(
                id,owner_host_instance_id,state,created_at,updated_at,
                runtime_platform,containment_type,process_identity_scheme,
                codex_pid,codex_process_start_token,containment_process_group_id,
                containment_session_id,containment_verified_at)
             VALUES(?1,'host',?2,1,1,'macos','macos_process_group',
                    'darwin_proc_bsd_start_v1',?3,?4,?5,?6,?7)",
            params![id, state, pid, token, pgid, sid, verified],
        )
    };

    macos_insert(
        &connection,
        "prepared",
        "preparing",
        None,
        None,
        None,
        None,
        None,
    )
    .unwrap();
    macos_insert(
        &connection,
        "running",
        "running",
        Some(42),
        Some(42),
        Some(42),
        Some("darwin_proc_bsd_start_v1:1:2"),
        Some(3),
    )
    .unwrap();
    assert!(
        macos_insert(
            &connection,
            "partial",
            "running",
            Some(43),
            Some(43),
            None,
            Some("darwin_proc_bsd_start_v1:1:2"),
            Some(3),
        )
        .is_err()
    );
    assert!(
        macos_insert(
            &connection,
            "mismatch",
            "running",
            Some(44),
            Some(45),
            Some(44),
            Some("darwin_proc_bsd_start_v1:1:2"),
            Some(3),
        )
        .is_err()
    );
    assert!(
        connection
            .execute(
                "UPDATE runtime_instances SET termination_evidence_state='complete',
                 termination_evidence_type='job_active_processes_zero',termination_evidence_at=9,
                 state='terminated' WHERE id='running'",
                [],
            )
            .is_err()
    );
    assert!(
        connection
            .execute(
                "INSERT INTO runtime_instances(
                    id,owner_host_instance_id,state,created_at,updated_at,
                    runtime_platform,containment_type,process_identity_scheme,
                    job_creation_mode,job_handle_inheritable,job_kill_on_close,
                    job_breakaway_allowed,containment_process_group_id)
                 VALUES('bad-windows','host','preparing',1,1,'windows','windows_job',
                        'windows_filetime_v1','proc_thread_attribute_job_list',0,1,0,99)",
                [],
            )
            .is_err()
    );
}

/// 验证 Runtime 读模型直接投影 v10 原始平台证据，不从旧 Job 字段推断。
#[test]
fn runtime_record_projects_v10_platform_evidence() {
    let directory = tempfile::tempdir().unwrap();
    let store = open(directory.path());
    {
        let connection = store.connection.lock().unwrap();
        connection
            .execute(
                "INSERT INTO runtime_instances(
                    id,owner_host_instance_id,state,created_at,updated_at,
                    runtime_platform,containment_type,process_identity_scheme,
                    codex_pid,codex_process_start_token,containment_process_group_id,
                    containment_session_id,containment_verified_at)
                 VALUES('mac-runtime','host','running',1,1,'macos','macos_process_group',
                        'darwin_proc_bsd_start_v1',77,'darwin_proc_bsd_start_v1:1:2',77,77,9)",
                [],
            )
            .unwrap();
    }

    let record = tauri::async_runtime::block_on(store.runtime("mac-runtime".into()))
        .unwrap()
        .unwrap();
    assert_eq!(record.runtime_platform, "macos");
    assert_eq!(record.containment_type, "macos_process_group");
    assert_eq!(record.process_identity_scheme, "darwin_proc_bsd_start_v1");
    assert_eq!(record.containment_process_group_id, Some(77));
    assert_eq!(record.containment_session_id, Some(77));
    assert_eq!(record.containment_verified_at, Some(9));
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
        11
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

    let mut legacy = Connection::open_in_memory().unwrap();
    legacy.execute_batch(SCHEMA_V1).unwrap();
    legacy.execute_batch(SCHEMA_V2).unwrap();
    legacy.execute_batch(SCHEMA_V3).unwrap();
    legacy.execute_batch(SCHEMA_V4).unwrap();
    legacy.execute_batch(SCHEMA_V5).unwrap();
    legacy.pragma_update(None, "user_version", 5).unwrap();
    insert_pre_v6(&legacy, "old", "agent", "root", Some("legacy-thread"));
    {
        let tx = legacy.transaction().unwrap();
        assert!(
            tx.execute_batch(&format!("{SCHEMA_V6}\nCREATE TABLE broken ("))
                .is_err()
        );
    }
    assert_eq!(
        legacy
            .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        5
    );
    assert_eq!(
        legacy
            .query_row(
                "SELECT count(*) FROM pragma_table_info('executions') WHERE name='parent_execution_id'",
                [],
                |r| r.get::<_, i64>(0),
            )
            .unwrap(),
        0
    );
}

/// 验证 v9 发生 SQL 故障时，版本号和三张 Usage 表都不会部分提交。
#[test]
fn v9_migration_failure_rolls_back_usage_schema_and_version() {
    let mut c = Connection::open_in_memory().unwrap();
    create_v8(&c);
    {
        let tx = c.transaction().unwrap();
        let invalid = format!("{SCHEMA_V9}\nCREATE TABLE v9_broken (");
        assert!(apply_migration(&tx, 9, &invalid).is_err());
    }
    assert_eq!(
        c.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .unwrap(),
        8
    );
    for table in [
        "execution_usage",
        "codex_thread_usage_epochs",
        "codex_execution_usage_state",
    ] {
        assert_eq!(
            c.query_row(
                "SELECT count(*) FROM sqlite_schema WHERE type='table' AND name=?1",
                [table],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            0,
            "{table}"
        );
    }
    migrate(&mut c).unwrap();
    assert_eq!(
        c.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .unwrap(),
        11
    );
}

#[test]
fn v1_migration_preserves_executions_and_adds_nullable_thread_names() {
    let mut c = Connection::open_in_memory().unwrap();
    c.execute_batch(SCHEMA_V1).unwrap();
    c.pragma_update(None, "user_version", 1).unwrap();
    insert_pre_v6(&c, "old", "agent", "root", Some("legacy-thread"));
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
    assert_eq!(old.parent_execution_id, None);
    assert_eq!(old.thread_id.as_deref(), Some("legacy-thread"));
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
    insert_pre_v6(&c, "old", "agent", "root", Some("legacy-thread"));
    migrate(&mut c).unwrap();
    assert_eq!(
        c.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .unwrap(),
        11
    );
    let old = execution_record(&c, "old").unwrap().unwrap();
    assert_eq!(old.last_activity_at, None);
    assert_eq!(old.activity_phase, None);
    assert_eq!(old.tool_category, None);
}

#[test]
fn every_pre_v6_schema_preserves_history_and_reopens_with_null_parent() {
    let schemas = [SCHEMA_V1, SCHEMA_V2, SCHEMA_V3, SCHEMA_V4, SCHEMA_V5];
    for version in 1..=5 {
        let mut c = Connection::open_in_memory().unwrap();
        for schema in schemas.iter().take(version) {
            c.execute_batch(schema).unwrap();
        }
        c.pragma_update(None, "user_version", version as i64)
            .unwrap();
        insert_pre_v6(&c, "old", "agent", "root", Some("shared-codex-thread"));
        let before: (String, String, Option<String>) = c
            .query_row(
                "SELECT request_hash,prompt,thread_id FROM executions WHERE id='old'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();

        migrate(&mut c).unwrap();
        migrate(&mut c).unwrap();
        assert_eq!(
            c.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            11
        );
        let after = execution_record(&c, "old").unwrap().unwrap();
        assert_eq!((after.request_hash, after.prompt, after.thread_id), before);
        assert_eq!(after.parent_execution_id, None);
    }
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
    c.pragma_update(None, "user_version", 12).unwrap();
    assert!(migrate(&mut c).unwrap_err().contains("unsupported"));
    assert_eq!(
        c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        12
    );
}

#[test]
fn v6_upgrade_preserves_rows_and_defines_generation_one_baseline() {
    let mut c = Connection::open_in_memory().unwrap();
    for schema in [
        SCHEMA_V1, SCHEMA_V2, SCHEMA_V3, SCHEMA_V4, SCHEMA_V5, SCHEMA_V6,
    ] {
        c.execute_batch(schema).unwrap();
    }
    c.pragma_update(None, "user_version", 6).unwrap();
    insert_pre_v6(&c, "old", "agent", "root", Some("legacy-thread"));
    c.execute(
        "INSERT INTO work_runs (id,workspace_id,canonical_workspace_root,title,status,created_at,updated_at)
         VALUES ('work','workspace','root','title','active',1,1)",
        [],
    )
    .unwrap();
    let executions = c
        .prepare("SELECT * FROM executions ORDER BY id")
        .unwrap()
        .query_map([], |row| {
            (0..row.as_ref().column_count())
                .map(|index| row.get(index))
                .collect::<rusqlite::Result<Vec<rusqlite::types::Value>>>()
        })
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    let work_runs = c
        .prepare("SELECT * FROM work_runs ORDER BY id")
        .unwrap()
        .query_map([], |row| {
            (0..row.as_ref().column_count())
                .map(|index| row.get(index))
                .collect::<rusqlite::Result<Vec<rusqlite::types::Value>>>()
        })
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();

    migrate(&mut c).unwrap();
    migrate(&mut c).unwrap();
    assert_eq!(
        c.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .unwrap(),
        11
    );
    let mut expected_executions = executions;
    expected_executions[0].push(rusqlite::types::Value::Integer(1));
    expected_executions[0].push(rusqlite::types::Value::Null);
    expected_executions[0].push(rusqlite::types::Value::Integer(0));
    let mut expected_work_runs = work_runs;
    expected_work_runs[0].push(rusqlite::types::Value::Integer(1));
    let actual_executions = c
        .prepare("SELECT * FROM executions ORDER BY id")
        .unwrap()
        .query_map([], |row| {
            (0..row.as_ref().column_count())
                .map(|index| row.get(index))
                .collect::<rusqlite::Result<Vec<rusqlite::types::Value>>>()
        })
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    let actual_work_runs = c
        .prepare("SELECT * FROM work_runs ORDER BY id")
        .unwrap()
        .query_map([], |row| {
            (0..row.as_ref().column_count())
                .map(|index| row.get(index))
                .collect::<rusqlite::Result<Vec<rusqlite::types::Value>>>()
        })
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(actual_executions, expected_executions);
    assert_eq!(actual_work_runs, expected_work_runs);
    assert_eq!(
        execution_record(&c, "old")
            .unwrap()
            .unwrap()
            .workspace_generation,
        1
    );
    assert_eq!(
        work_run_record(&c, "work")
            .unwrap()
            .unwrap()
            .workspace_generation,
        1
    );
}

#[test]
fn v7_generation_columns_default_to_one_and_reject_nonpositive_raw_sql() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    let mut c = store.connection.lock().unwrap();
    insert(&mut c, "execution", "agent", "root");
    c.execute(
        "INSERT INTO work_runs (id,workspace_id,canonical_workspace_root,title,status,created_at,updated_at)
         VALUES ('work','workspace','root','title','active',1,1)",
        [],
    )
    .unwrap();
    for table in ["executions", "work_runs"] {
        assert_eq!(
            c.query_row(
                &format!("SELECT workspace_generation FROM {table} LIMIT 1"),
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            1
        );
        for value in [0, -1] {
            assert!(
                c.execute(
                    &format!("UPDATE {table} SET workspace_generation=?1"),
                    [value],
                )
                .is_err()
            );
        }
    }
    for table in ["executions", "work_runs"] {
        let sql: String = c
            .query_row(
                "SELECT sql FROM sqlite_schema WHERE type='table' AND name=?1",
                [table],
                |row| row.get(0),
            )
            .unwrap();
        assert!(sql.contains(
            "workspace_generation INTEGER NOT NULL DEFAULT 1 CHECK(workspace_generation >= 1)"
        ));
    }
}

/// 构造真实 v7 数据库，以覆盖 v8 的独立升级路径。
fn create_v7(c: &Connection) {
    for schema in [
        SCHEMA_V1, SCHEMA_V2, SCHEMA_V3, SCHEMA_V4, SCHEMA_V5, SCHEMA_V6, SCHEMA_V7,
    ] {
        c.execute_batch(schema).unwrap();
    }
    c.pragma_update(None, "user_version", 7).unwrap();
}

/// 构造真实 v8 数据库，以覆盖 v9 的独立升级路径。
fn create_v8(c: &Connection) {
    for schema in [
        SCHEMA_V1, SCHEMA_V2, SCHEMA_V3, SCHEMA_V4, SCHEMA_V5, SCHEMA_V6, SCHEMA_V7, SCHEMA_V8,
    ] {
        c.execute_batch(schema).unwrap();
    }
    c.pragma_update(None, "user_version", 8).unwrap();
}

/// 验证新库包含 v8 Activity 结构及其全部关键约束。
#[test]
fn v8_fresh_database_has_activity_current_and_history_constraints() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    let mut c = store.connection.lock().unwrap();
    insert(&mut c, "parent", "agent", "root");

    for column in ["activity_summary_code", "activity_sequence"] {
        assert_eq!(
            c.query_row(
                "SELECT count(*) FROM pragma_table_info('executions') WHERE name=?1",
                [column],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            1,
            "{column}"
        );
    }
    assert_eq!(
        c.query_row(
            "SELECT on_delete FROM pragma_foreign_key_list('execution_activity_events')",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap(),
        "RESTRICT"
    );
    assert!(
        c.execute(
            "UPDATE executions SET activity_sequence=-1 WHERE id='parent'",
            []
        )
        .is_err()
    );

    c.execute(
        "INSERT INTO execution_activity_events
         (execution_id,sequence,activity_phase,tool_category,summary_code,activity_revision,observed_at)
         VALUES ('parent',0,'tool','read','tool.read','revision',1)",
        [],
    )
    .unwrap();
    c.execute(
        "INSERT INTO execution_activity_events
         (execution_id,sequence,summary_code,activity_revision,observed_at)
         VALUES ('parent',1,NULL,'revision',2)",
        [],
    )
    .unwrap();
    assert_eq!(
        c.query_row(
            "SELECT summary_code FROM execution_activity_events WHERE execution_id='parent' AND sequence=1",
            [],
            |row| row.get::<_, Option<String>>(0),
        )
        .unwrap(),
        None
    );
    assert!(
        c.execute(
            "INSERT INTO execution_activity_events
             (execution_id,sequence,summary_code,activity_revision,observed_at)
             VALUES ('parent',0,'provider.processing','revision',3)",
            [],
        )
        .is_err()
    );
    assert!(
        c.execute(
            "INSERT INTO execution_activity_events
             (execution_id,sequence,summary_code,activity_revision,observed_at)
             VALUES ('missing',0,'provider.processing','revision',1)",
            [],
        )
        .is_err()
    );
    assert!(
        c.execute("DELETE FROM executions WHERE id='parent'", [])
            .is_err()
    );
}

/// 验证 v7 历史行只回填当前摘要，并完整覆盖冻结的 summaryCode 映射。
#[test]
fn v8_backfills_current_summary_without_inventing_history() {
    let mut c = Connection::open_in_memory().unwrap();
    create_v7(&c);
    let cases = [
        (
            "finalizing",
            "not_dispatched",
            Some("provider"),
            Some("read"),
            Some("execution.finalizing"),
        ),
        (
            "reconciling",
            "not_dispatched",
            Some("tool"),
            Some("test"),
            Some("execution.reconciling"),
        ),
        (
            "dispatch_pending",
            "uncertain",
            Some("tool"),
            Some("command"),
            Some("execution.reconciling"),
        ),
        (
            "running",
            "dispatched",
            Some("provider"),
            None,
            Some("provider.processing"),
        ),
        (
            "running",
            "dispatched",
            Some("tool"),
            Some("read"),
            Some("tool.read"),
        ),
        (
            "running",
            "dispatched",
            Some("tool"),
            Some("edit"),
            Some("tool.edit"),
        ),
        (
            "running",
            "dispatched",
            Some("tool"),
            Some("command"),
            Some("tool.command"),
        ),
        (
            "running",
            "dispatched",
            Some("tool"),
            Some("build"),
            Some("tool.build"),
        ),
        (
            "running",
            "dispatched",
            Some("tool"),
            Some("test"),
            Some("tool.test"),
        ),
        (
            "running",
            "dispatched",
            Some("tool"),
            Some("tool"),
            Some("tool.other"),
        ),
        ("completed", "dispatched", None, None, None),
    ];
    for (index, (status, dispatch_state, activity_phase, tool_category, _)) in
        cases.iter().enumerate()
    {
        let id = format!("execution-{index}");
        let agent = format!("agent-{index}");
        insert_pre_v6(&c, &id, &agent, "root", None);
        c.execute(
            "UPDATE executions SET status=?1, dispatch_state=?2, activity_phase=?3, tool_category=?4 WHERE id=?5",
            params![status, dispatch_state, activity_phase, tool_category, id],
        )
        .unwrap();
    }

    migrate(&mut c).unwrap();
    assert_eq!(
        c.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .unwrap(),
        11
    );
    assert_eq!(
        c.query_row(
            "SELECT count(*) FROM execution_activity_events",
            [],
            |row| { row.get::<_, i64>(0) }
        )
        .unwrap(),
        0
    );
    for (index, (_, _, _, _, expected_summary)) in cases.iter().enumerate() {
        let id = format!("execution-{index}");
        let row = execution_record(&c, &id).unwrap().unwrap();
        assert_eq!(
            row.activity_summary_code.as_deref(),
            *expected_summary,
            "{id}"
        );
        assert_eq!(row.activity_sequence, 0, "{id}");
    }
    migrate(&mut c).unwrap();
}

/// 验证非法旧 Activity 组合会中止整个 v8 事务，不留下半升级结构。
#[test]
fn v8_invalid_legacy_activity_fails_closed_without_partial_schema() {
    for (activity_phase, tool_category) in [("provider", Some("read")), ("tool", None)] {
        let mut c = Connection::open_in_memory().unwrap();
        create_v7(&c);
        insert_pre_v6(&c, "invalid", "agent", "root", None);
        c.execute(
            "UPDATE executions SET status='running', dispatch_state='dispatched',
             activity_phase=?1, tool_category=?2 WHERE id='invalid'",
            params![activity_phase, tool_category],
        )
        .unwrap();

        assert!(
            migrate(&mut c)
                .unwrap_err()
                .contains("AGENT_ACTIVITY_CONTRACT_ERROR")
        );
        assert_eq!(
            c.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            7
        );
        for (kind, name) in [
            ("table", "execution_activity_events"),
            ("trigger", "validate_v8_activity_backfill"),
        ] {
            assert_eq!(
                c.query_row(
                    "SELECT count(*) FROM sqlite_schema WHERE type=?1 AND name=?2",
                    [kind, name],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
                0,
                "{name}"
            );
        }
        assert_eq!(
            c.query_row(
                "SELECT count(*) FROM pragma_table_info('executions') WHERE name='activity_summary_code'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            0
        );
    }
}

/// 验证磁盘上的真实 v8 fixture 原子升级到 v9，且不为历史 Execution 补 Usage。
#[test]
fn v9_migrates_real_v8_fixture_without_backfilling_usage() {
    let dir = tempfile::tempdir().unwrap();
    let database = dir.path().join("agent-state.db");
    let mut c = Connection::open(&database).unwrap();
    create_v8(&c);
    insert(&mut c, "legacy", "agent", "root");
    drop(c);

    let store = open(dir.path());
    {
        let c = store.connection.lock().unwrap();
        assert_eq!(
            c.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            11
        );
        assert_eq!(
            c.query_row(
                "SELECT count(*) FROM execution_usage WHERE execution_id='legacy'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            0
        );
        for table in ["codex_thread_usage_epochs", "codex_execution_usage_state"] {
            assert_eq!(
                c.query_row(
                    "SELECT count(*) FROM sqlite_schema WHERE type='table' AND name=?1",
                    [table],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
                1,
                "{table}"
            );
        }
    }
    assert!(
        tauri::async_runtime::block_on(store.execution_usage("legacy".into()))
            .unwrap()
            .is_none()
    );
}

/// 验证 v9 的 FK、CHECK 与 epoch 复合主键均由 SQLite 拒绝非法写入。
#[test]
fn v9_usage_schema_enforces_foreign_keys_keys_and_constraints() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    let mut c = store.connection.lock().unwrap();

    assert!(
        c.execute(
            "INSERT INTO execution_usage(execution_id,provider_id,completeness,updated_at)
             VALUES ('missing','codex','unknown',0)",
            [],
        )
        .is_err()
    );
    assert!(
        c.execute(
            "INSERT INTO codex_execution_usage_state(
                execution_id,runtime_instance_id,thread_id,baseline_kind,telemetry_state
             ) VALUES ('missing','runtime','thread','unknown','accepting')",
            [],
        )
        .is_err()
    );

    insert(&mut c, "completeness", "completeness-agent", "root");
    assert!(
        c.execute(
            "INSERT INTO execution_usage(execution_id,provider_id,completeness,updated_at)
             VALUES ('completeness','codex','invalid',0)",
            [],
        )
        .is_err()
    );
    assert!(
        c.execute(
            "INSERT INTO execution_usage(execution_id,provider_id,completeness,usage_revision,updated_at)
             VALUES ('completeness','codex','unknown',-1,0)",
            [],
        )
        .is_err()
    );

    for (index, column) in [
        "input_tokens",
        "cached_input_tokens",
        "cache_write_input_tokens",
        "output_tokens",
        "reasoning_tokens",
        "total_tokens",
        "model_context_window",
    ]
    .iter()
    .enumerate()
    {
        let negative_id = format!("negative-{index}");
        let nullable_id = format!("nullable-{index}");
        let negative_agent = format!("negative-agent-{index}");
        let nullable_agent = format!("nullable-agent-{index}");
        insert(&mut c, &negative_id, &negative_agent, "root");
        insert(&mut c, &nullable_id, &nullable_agent, "root");
        assert!(c
            .execute(
                &format!(
                    "INSERT INTO execution_usage(execution_id,provider_id,{column},completeness,updated_at)
                     VALUES (?1,'codex',-1,'unknown',0)"
                ),
                [&negative_id],
            )
            .is_err(),
            "{column}"
        );
        c.execute(
            &format!(
                "INSERT INTO execution_usage(execution_id,provider_id,{column},completeness,updated_at)
                 VALUES (?1,'codex',NULL,'unknown',0)"
            ),
            [&nullable_id],
        )
        .unwrap();
        c.execute(
            &format!("UPDATE execution_usage SET {column}=0 WHERE execution_id=?1"),
            [&nullable_id],
        )
        .unwrap();
    }

    c.execute(
        "INSERT INTO codex_thread_usage_epochs(
            runtime_instance_id,thread_id,latest_cumulative_json,latest_turn_id,captured_at
         ) VALUES ('runtime','thread','{}',NULL,1)",
        [],
    )
    .unwrap();
    assert!(
        c.execute(
            "INSERT INTO codex_thread_usage_epochs(
                runtime_instance_id,thread_id,latest_cumulative_json,latest_turn_id,captured_at
             ) VALUES ('runtime','thread','{}',NULL,2)",
            [],
        )
        .is_err()
    );
    c.execute(
        "INSERT INTO codex_thread_usage_epochs(
            runtime_instance_id,thread_id,latest_cumulative_json,latest_turn_id,captured_at
         ) VALUES ('runtime','other-thread','{}',NULL,2)",
        [],
    )
    .unwrap();

    insert(&mut c, "baseline", "baseline-agent", "root");
    assert!(
        c.execute(
            "INSERT INTO codex_execution_usage_state(
                execution_id,runtime_instance_id,thread_id,baseline_kind,telemetry_state
             ) VALUES ('baseline','runtime','thread','invalid','accepting')",
            [],
        )
        .is_err()
    );
    insert(&mut c, "telemetry", "telemetry-agent", "root");
    assert!(
        c.execute(
            "INSERT INTO codex_execution_usage_state(
                execution_id,runtime_instance_id,thread_id,baseline_kind,telemetry_state
             ) VALUES ('telemetry','runtime','thread','unknown','invalid')",
            [],
        )
        .is_err()
    );
}

/// 验证 v9 行重开后不丢失，并经由读模型保持公共与 Provider-private 边界。
#[test]
fn v9_usage_records_survive_restart_and_read_through_store_scaffolding() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    {
        let mut c = store.connection.lock().unwrap();
        insert(&mut c, "persisted", "agent", "root");
        c.execute(
            "INSERT INTO execution_usage(
                execution_id,provider_id,input_tokens,cached_input_tokens,cache_write_input_tokens,
                output_tokens,reasoning_tokens,total_tokens,model_context_window,completeness,
                usage_revision,updated_at
             ) VALUES ('persisted','codex',0,NULL,2,3,NULL,5,6,'partial',7,8)",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO codex_thread_usage_epochs(
                runtime_instance_id,thread_id,latest_cumulative_json,latest_turn_id,captured_at
             ) VALUES ('runtime','thread','{\"total\":5}','turn',9)",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO codex_execution_usage_state(
                execution_id,runtime_instance_id,thread_id,turn_id,baseline_kind,baseline_json,
                latest_cumulative_json,telemetry_state,terminal_at,freeze_at,last_event_at
             ) VALUES ('persisted','runtime','thread','turn','observed_same_epoch','{\"total\":1}',
                       '{\"total\":5}','terminal_grace',10,NULL,11)",
            [],
        )
        .unwrap();
    }
    drop(store);

    let reopened = open(dir.path());
    let snapshot = tauri::async_runtime::block_on(reopened.execution_usage("persisted".into()))
        .unwrap()
        .unwrap();
    assert_eq!(snapshot.provider_id.as_str(), "codex");
    assert_eq!(snapshot.input_tokens, Some(0));
    assert_eq!(snapshot.cached_input_tokens, None);
    assert_eq!(snapshot.total_tokens, Some(5));
    assert_eq!(snapshot.revision, 7);
    assert_eq!(snapshot.updated_at, 8);
    assert_eq!(
        snapshot.completeness,
        crate::agent::usage::UsageCompleteness::Partial
    );

    let c = reopened.connection.lock().unwrap();
    let epoch = usage::codex_thread_usage_epoch_record(&c, "runtime", "thread")
        .unwrap()
        .unwrap();
    assert_eq!(epoch.latest_turn_id.as_deref(), Some("turn"));
    assert_eq!(epoch.captured_at, 9);
    let state = usage::codex_execution_usage_state_record(&c, "persisted")
        .unwrap()
        .unwrap();
    assert_eq!(state.baseline_kind, "observed_same_epoch");
    assert_eq!(state.telemetry_state, "terminal_grace");
    assert_eq!(state.freeze_at, None);
    assert_eq!(state.last_event_at, Some(11));
}

/// 验证公共存储行复用 P4-001 校验，不让损坏 identity 伪装为快照。
#[test]
fn v9_usage_record_mapping_reuses_public_usage_validation() {
    let record = usage::ExecutionUsageRecord {
        execution_id: "invalid execution id".into(),
        provider_id: "codex".into(),
        input_tokens: Some(0),
        cached_input_tokens: None,
        cache_write_input_tokens: None,
        output_tokens: None,
        reasoning_tokens: None,
        total_tokens: None,
        model_context_window: None,
        completeness: "unknown".into(),
        usage_revision: 0,
        updated_at: 0,
    };
    let mapped: Result<crate::agent::usage::UsageSnapshot, String> = record.try_into();
    assert_eq!(
        mapped.unwrap_err(),
        crate::agent::usage::USAGE_EVENT_INVALID
    );
}

#[test]
fn migrated_v6_legacy_hash_retries_only_at_generation_one_without_rewrite() {
    let dir = tempfile::tempdir().unwrap();
    let database = dir.path().join("agent-state.db");
    let c = Connection::open(&database).unwrap();
    for schema in [
        SCHEMA_V1, SCHEMA_V2, SCHEMA_V3, SCHEMA_V4, SCHEMA_V5, SCHEMA_V6,
    ] {
        c.execute_batch(schema).unwrap();
    }
    c.pragma_update(None, "user_version", 6).unwrap();
    insert_pre_v6(&c, "legacy", "agent", "root", None);
    let before: String = c
        .query_row(
            "SELECT request_hash FROM executions WHERE id='legacy'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    drop(c);

    tauri::async_runtime::block_on(async {
        let store = StateStore::open(dir.path().into()).await.unwrap();
        let retry = store
            .product_create_fresh(
                "unused".into(),
                "agent".into(),
                "key".into(),
                "中文 ' ; --".into(),
                "w".into(),
                Some(
                    crate::agent::store::transactions::product::WorkspaceSnapshot {
                        id: "w".into(),
                        root: "root".into(),
                        generation: 1,
                    },
                ),
                2,
            )
            .await
            .unwrap();
        assert!(!retry.created);
        assert_eq!(retry.execution_id, "legacy");
        assert_eq!(retry.execution.workspace_generation, 1);
        assert_eq!(retry.execution.request_hash, before);
        assert_eq!(
            store
                .product_create_fresh(
                    "unused".into(),
                    "agent".into(),
                    "key".into(),
                    "中文 ' ; --".into(),
                    "w".into(),
                    Some(
                        crate::agent::store::transactions::product::WorkspaceSnapshot {
                            id: "w".into(),
                            root: "root".into(),
                            generation: 2,
                        }
                    ),
                    3,
                )
                .await
                .unwrap_err(),
            "EXECUTION_REQUEST_KEY_CONFLICT"
        );
    });
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
    let mut c = Connection::open_in_memory().unwrap();
    c.pragma_update(None, "foreign_keys", true).unwrap();
    c.execute_batch(SCHEMA_V1).unwrap();
    c.execute_batch(SCHEMA_V2).unwrap();
    c.execute_batch(SCHEMA_V3).unwrap();
    c.pragma_update(None, "user_version", 3).unwrap();
    insert_pre_v6(&c, "E1", "A", "W", Some("legacy-thread"));
    runtime(&c, "runtime-E1");
    let before: (String, String, Option<String>) = c
        .query_row(
            "SELECT request_hash,prompt,thread_id FROM executions WHERE id='E1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    migrate(&mut c).unwrap();
    migrate(&mut c).unwrap();
    let after = execution_record(&c, "E1").unwrap().unwrap();
    assert_eq!((after.request_hash, after.prompt, after.thread_id), before);
    assert_eq!(after.parent_execution_id, None);
    assert!(runtime_attempts::runtime_attempt_exists(&c, "E1").unwrap());
    c.execute("INSERT INTO execution_runtime_attempts(execution_id,runtime_instance_id,created_at) VALUES ('E1','R123',1)",[]).unwrap();
    assert!(runtime_attempts::runtime_attempt_exists(&c, "E1").unwrap());
    assert!(
        c.execute(
            "UPDATE execution_runtime_attempts SET runtime_instance_id='R2'",
            []
        )
        .is_err()
    );
    assert!(
        c.execute("DELETE FROM execution_runtime_attempts", [])
            .is_err()
    );
    assert!(c.execute("INSERT INTO execution_runtime_attempts(execution_id,runtime_instance_id,created_at) VALUES ('missing','R2',1)",[]).is_err());
}
