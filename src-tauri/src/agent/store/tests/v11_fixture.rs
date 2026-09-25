use super::*;
use rusqlite::types::Value;
use std::path::Path;

/// 仅应用冻结的历史 schema 到 v11，再载入固定数据；后续 migrate() 增加 v12 不影响此输入。
pub(super) fn frozen_v11_connection(database: &Path) -> Connection {
    let mut connection = Connection::open(database).unwrap();
    configure(&connection).unwrap();
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    for (version, schema) in [
        (1, SCHEMA_V1),
        (2, SCHEMA_V2),
        (3, SCHEMA_V3),
        (4, SCHEMA_V4),
        (5, SCHEMA_V5),
        (6, SCHEMA_V6),
        (7, SCHEMA_V7),
        (8, SCHEMA_V8),
        (9, SCHEMA_V9),
        (10, SCHEMA_V10),
        (11, SCHEMA_V11),
    ] {
        apply_migration(&transaction, version, schema).unwrap();
    }
    transaction.commit().unwrap();
    connection
        .execute_batch(include_str!(
            "../../../../tests/fixtures/agent_state_v11.sql"
        ))
        .unwrap();
    connection
}

/// 读取固定表的全部原值，供本轮 restart 测试及后续 v12 逐字段对比复用。
fn table_rows(connection: &Connection, table: &str) -> Vec<Vec<Value>> {
    let mut statement = connection
        .prepare(&format!("SELECT * FROM {table} ORDER BY 1"))
        .unwrap();
    let columns = statement.column_count();
    let rows = statement
        .query_map([], |row| (0..columns).map(|index| row.get(index)).collect())
        .unwrap();
    rows.collect::<rusqlite::Result<_>>().unwrap()
}

/// 固定 v11 表、触发器和核心字段，并确认真实 SQLite 外键检查通过。
#[test]
fn frozen_v11_fixture_has_required_shape_and_key_fields() {
    let directory = tempfile::tempdir().unwrap();
    let connection = frozen_v11_connection(&directory.path().join("agent-state.db"));
    assert_eq!(
        connection
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .unwrap(),
        11
    );
    for (table, count) in [
        ("runtime_instances", 2),
        ("executions", 3),
        ("workspace_claims", 1),
        ("work_runs", 1),
        ("work_execution_links", 1),
        ("execution_usage", 1),
        ("codex_thread_usage_epochs", 1),
        ("codex_execution_usage_state", 1),
        ("command_runs", 1),
        ("work_command_links", 1),
    ] {
        assert_eq!(table_rows(&connection, table).len(), count, "{table}");
    }
    for column in [
        "runtime_platform",
        "containment_type",
        "process_identity_scheme",
        "containment_process_group_id",
        "containment_session_id",
        "containment_verified_at",
    ] {
        assert_eq!(
            connection
                .query_row(
                    "SELECT count(*) FROM pragma_table_info('runtime_instances') WHERE name=?1",
                    [column],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1,
            "{column}"
        );
    }
    for name in [
        "runtime_instances_v10_validate_insert",
        "runtime_instances_v10_validate_update",
        "prevent_execution_runtime_rebind",
        "executions_one_unresolved_per_agent",
        "command_runs_workspace_created",
    ] {
        assert_eq!(
            connection
                .query_row(
                    "SELECT count(*) FROM sqlite_schema WHERE name=?1",
                    [name],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1,
            "{name}"
        );
    }
    let execution_schema: String = connection
        .query_row(
            "SELECT sql FROM sqlite_schema WHERE name='executions'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(execution_schema.contains("CHECK(provider = 'codex')"));
    for (table, column) in [
        ("executions", "task_role"),
        ("runtime_instances", "provider"),
    ] {
        assert_eq!(
            connection
                .query_row(
                    "SELECT count(*) FROM pragma_table_info(?1) WHERE name=?2",
                    [table, column],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            0,
            "{table}.{column}"
        );
    }
    assert_eq!(
        connection
            .query_row(
                "SELECT request_hash, provider, status, dispatch_state, revision, \
                 release_evidence_kind, result_completeness FROM executions \
                 WHERE id='execution-completed-v11'",
                [],
                |row| Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, String>(6)?
                )),
            )
            .unwrap(),
        (
            "a".repeat(64),
            "codex".into(),
            "completed".into(),
            "dispatched".into(),
            7,
            Some("runtime_terminated".into()),
            "complete".into()
        )
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT status, dispatch_state, runtime_instance_id FROM executions \
                 WHERE id='execution-pending-v11'",
                [],
                |row| Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?
                )),
            )
            .unwrap(),
        ("dispatch_pending".into(), "not_dispatched".into(), None)
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT status, dispatch_state, runtime_instance_id, activity_summary_code \
                 FROM executions WHERE id='execution-unknown-v11'",
                [],
                |row| Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?
                )),
            )
            .unwrap(),
        (
            "unknown".into(),
            "uncertain".into(),
            "runtime-macos-v11".into(),
            "execution.reconciling".into()
        )
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT runtime_platform, containment_type, process_identity_scheme, \
                 containment_process_group_id, containment_session_id, containment_verified_at \
                 FROM runtime_instances WHERE id='runtime-macos-v11'",
                [],
                |row| Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?
                )),
            )
            .unwrap(),
        (
            "macos".into(),
            "macos_process_group".into(),
            "darwin_proc_bsd_start_v1".into(),
            510,
            510,
            121
        )
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT runtime_platform, containment_type, process_identity_scheme, \
                 containment_process_group_id, containment_session_id, containment_verified_at, \
                 termination_evidence_type, termination_evidence_at, termination_evidence_state \
                 FROM runtime_instances WHERE id='runtime-windows-v11'",
                [],
                |row| Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                    row.get::<_, Option<i64>>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, String>(8)?,
                )),
            )
            .unwrap(),
        (
            "windows".into(),
            "windows_job".into(),
            "windows_filetime_v1".into(),
            None,
            None,
            None,
            "managed_job_destroyed".into(),
            241,
            "complete".into(),
        )
    );
    assert!(
        connection
            .execute(
                "INSERT INTO runtime_instances (
                id,owner_host_instance_id,state,created_at,updated_at,
                runtime_platform,containment_type,process_identity_scheme
             ) VALUES (
                'invalid-platform-v11','host-v11','unknown',1,1,
                'macos','windows_job','windows_filetime_v1'
             )",
                [],
            )
            .unwrap_err()
            .to_string()
            .contains("RUNTIME_PLATFORM_EVIDENCE_INVALID")
    );
    assert!(
        connection
            .execute(
                "UPDATE runtime_instances SET containment_session_id=999 \
             WHERE id='runtime-macos-v11'",
                [],
            )
            .unwrap_err()
            .to_string()
            .contains("RUNTIME_PLATFORM_EVIDENCE_INVALID")
    );
    let mut foreign_key_check = connection.prepare("PRAGMA foreign_key_check").unwrap();
    assert!(
        foreign_key_check
            .query([])
            .unwrap()
            .next()
            .unwrap()
            .is_none()
    );
}

/// 磁盘 v11 fixture 重开与 restart 后，所有表值和公开/私有读模型保持原样。
#[test]
fn frozen_v11_fixture_reopens_and_reads_without_rewriting_values() {
    let directory = tempfile::tempdir().unwrap();
    let tables = [
        "runtime_instances",
        "executions",
        "workspace_claims",
        "work_runs",
        "work_execution_links",
        "execution_usage",
        "codex_thread_usage_epochs",
        "codex_execution_usage_state",
        "command_runs",
        "work_command_links",
    ];
    let connection = frozen_v11_connection(&directory.path().join("agent-state.db"));
    let before: Vec<_> = tables
        .iter()
        .map(|table| table_rows(&connection, table))
        .collect();
    drop(connection);

    for _ in 0..2 {
        let store = open(directory.path());
        tauri::async_runtime::block_on(async {
            let completed = store
                .execution("execution-completed-v11".into())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(completed.request_hash, "a".repeat(64));
            assert_eq!(completed.provider, "codex");
            assert_eq!(completed.status, "completed");
            assert_eq!(completed.thread_id.as_deref(), Some("thread-v11"));
            assert_eq!(
                completed.final_result_json.as_deref(),
                Some("{\"result\":\"done\"}")
            );
            let pending = store
                .execution("execution-pending-v11".into())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                (pending.status.as_str(), pending.dispatch_state.as_str()),
                ("dispatch_pending", "not_dispatched")
            );
            let unknown = store
                .execution("execution-unknown-v11".into())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(unknown.status, "unknown");
            assert_eq!(
                unknown.runtime_instance_id.as_deref(),
                Some("runtime-macos-v11")
            );
            let runtime = store
                .runtime("runtime-macos-v11".into())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(runtime.provider, "codex");
            assert_eq!(
                (
                    runtime.runtime_platform.as_str(),
                    runtime.containment_type.as_str()
                ),
                ("macos", "macos_process_group")
            );
            assert_eq!(runtime.containment_process_group_id, Some(510));
            let claim = store
                .workspace_claim("/fixture-v11".into())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(claim.execution_id, "execution-unknown-v11");
            let work = store.work_run("work-v11".into()).await.unwrap().unwrap();
            assert_eq!(
                (work.workspace_generation, work.status.as_str()),
                (2, "completed")
            );
            let usage = store
                .execution_usage("execution-completed-v11".into())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(usage.provider_id.as_str(), "codex");
            assert_eq!((usage.total_tokens, usage.revision), (Some(17), 4));
            let command = store
                .command_run("command-v11".into())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                (command.status.as_str(), command.stdout_total_bytes),
                ("completed", 5)
            );
            assert_eq!(command.request_hash, "d".repeat(64));
        });
        let connection = store.connection.lock().unwrap();
        assert_eq!(
            connection
                .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            12
        );
        for (table, expected) in tables.iter().zip(&before) {
            let actual = table_rows(&connection, table);
            if ["runtime_instances", "executions"].contains(table) {
                assert_eq!(actual.len(), expected.len(), "{table}");
                for (actual_row, expected_row) in actual.iter().zip(expected) {
                    assert_eq!(&actual_row[..expected_row.len()], expected_row, "{table}");
                    assert_eq!(
                        actual_row.last(),
                        Some(&Value::Text(if *table == "executions" {
                            "general".into()
                        } else {
                            "codex".into()
                        })),
                        "{table}"
                    );
                }
            } else {
                assert_eq!(&actual, expected, "{table}");
            }
        }
        assert_eq!(
            connection
                .query_row(
                    "SELECT work_run_id, execution_id FROM work_execution_links",
                    [],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                )
                .unwrap(),
            ("work-v11".into(), "execution-completed-v11".into())
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT work_run_id, command_run_id FROM work_command_links",
                    [],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                )
                .unwrap(),
            ("work-v11".into(), "command-v11".into())
        );
        let epoch = usage::codex_thread_usage_epoch_record(
            &connection,
            "execution-completed-v11",
            "runtime-windows-v11",
            "thread-v11",
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            (epoch.latest_turn_id.as_deref(), epoch.captured_at),
            (Some("turn-v11"), 244)
        );
        let private =
            usage::codex_execution_usage_state_record(&connection, "execution-completed-v11")
                .unwrap()
                .unwrap();
        assert_eq!(
            (
                private.baseline_kind.as_str(),
                private.telemetry_state.as_str()
            ),
            ("observed_same_epoch", "frozen")
        );
        assert_eq!(private.freeze_at, Some(251));
        let mut foreign_key_check = connection.prepare("PRAGMA foreign_key_check").unwrap();
        assert!(
            foreign_key_check
                .query([])
                .unwrap()
                .next()
                .unwrap()
                .is_none()
        );
    }
}
