use super::*;
use rusqlite::types::Value;
use std::collections::BTreeMap;

/// 冻结 v11 fixture 直接迁入 v12 后，Product 展示历史 General 而非当前策略推断。
#[tokio::test]
async fn migrated_v11_execution_product_projects_general_role() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("agent-state.db");
    drop(v11_fixture::frozen_v11_connection(&database));
    let store = StateStore::open(directory.path().into()).await.unwrap();
    let service = crate::agent::product::AgentProductService::new(store);
    for id in [
        "execution-completed-v11",
        "execution-pending-v11",
        "execution-unknown-v11",
    ] {
        let response = service
            .operation(
                json!({"action":"observe","executionId":id,"waitMs":0}),
                None,
            )
            .await;
        assert_eq!(response["ok"], true, "{id}: {response}");
        assert_eq!(response["data"]["provider"]["id"], "codex");
        assert_eq!(response["data"]["taskRole"], "general");
    }
}

/// 把迁移前后的表值按列名而非位置投影，验证每个历史值仍在对应列中。
fn rows_by_column(connection: &Connection, table: &str) -> BTreeMap<String, Vec<Value>> {
    let mut statement = connection
        .prepare(&format!("SELECT * FROM {table} ORDER BY 1"))
        .unwrap();
    let names: Vec<String> = statement
        .column_names()
        .iter()
        .map(|s| s.to_string())
        .collect();
    let rows: Vec<Vec<Value>> = statement
        .query_map([], |row| (0..names.len()).map(|i| row.get(i)).collect())
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    names
        .into_iter()
        .enumerate()
        .map(|(i, name)| (name, rows.iter().map(|row| row[i].clone()).collect()))
        .collect()
}

/// 冻结真实 v11 数据逐列迁入 v12；所有跨表记录与 Codex 私有证据原值不变。
#[test]
fn frozen_v11_to_v12_preserves_every_historical_field() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("agent-state.db");
    let mut connection = v11_fixture::frozen_v11_connection(&database);
    let tables = [
        "executions",
        "runtime_instances",
        "workspace_claims",
        "work_runs",
        "work_execution_links",
        "execution_activity_events",
        "execution_usage",
        "codex_thread_usage_epochs",
        "codex_execution_usage_state",
        "execution_runtime_attempts",
        "command_runs",
        "work_command_links",
    ];
    let before: Vec<_> = tables
        .iter()
        .map(|table| rows_by_column(&connection, table))
        .collect();

    migrate(&mut connection).unwrap();
    assert_eq!(
        connection
            .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        12
    );
    for (table, old) in tables.iter().zip(&before) {
        let after = rows_by_column(&connection, table);
        for (column, values) in old {
            let mapped = if *table == "runtime_instances" {
                match column.as_str() {
                    "codex_executable_path" => "executable_path",
                    "codex_version" => "executable_version",
                    "codex_pid" => "process_id",
                    "codex_process_start_token" => "process_start_token",
                    "protocol_schema_sha256" => "protocol_contract_sha256",
                    _ => column,
                }
            } else {
                column
            };
            assert_eq!(after.get(mapped), Some(values), "{table}.{column}");
        }
        if *table == "executions" {
            assert_eq!(after["task_role"], vec![Value::Text("general".into()); 3]);
        }
        if *table == "runtime_instances" {
            assert_eq!(after["provider"], vec![Value::Text("codex".into()); 2]);
            assert_eq!(after["protocol_contract_sha256"][0], Value::Null);
        }
    }
    check_foreign_keys(&connection).unwrap();
    assert_eq!(
        connection
            .pragma_query_value(None, "foreign_keys", |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
}

/// 真实 v11 输入迁入 v12 后，旧 v2 与新 v3 均可跨重启幂等重试且不改旧 hash。
#[tokio::test]
async fn migrated_v11_v2_and_fresh_v3_retry_survive_reopen_without_hash_rewrite() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("agent-state.db");
    let connection = v11_fixture::frozen_v11_connection(&database);
    let historical_input: CreateExecutionInput = serde_json::from_value(json!({
        "agent_id":"agent-pending-v11","request_key":"request-pending-v11",
        "prompt":"pending prompt","execution_profile":{},"workspace_id":"workspace-v11",
        "canonical_workspace_root":"C:/pending-v11","workspace_generation":2,
        "provider":"codex","mode":"read_only"
    }))
    .unwrap();
    let historical_hash =
        crate::agent::execution::legacy_v2_request_hash(&historical_input).unwrap();
    connection
        .execute(
            "UPDATE executions SET request_hash=?1 WHERE id='execution-pending-v11'",
            [&historical_hash],
        )
        .unwrap();
    drop(connection);

    let store = StateStore::open(directory.path().into()).await.unwrap();
    let historical_request = canonicalize_request(historical_input).unwrap();
    assert_eq!(
        store
            .connection
            .lock()
            .unwrap()
            .query_row(
                "SELECT task_role,request_hash FROM executions WHERE id='execution-pending-v11'",
                [],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            )
            .unwrap(),
        ("general".into(), historical_hash.clone())
    );
    let historical_retry = store
        .create_execution("unused".into(), historical_request.clone(), 300)
        .await
        .unwrap();
    assert!(!historical_retry.created);
    assert_eq!(historical_retry.execution_id, "execution-pending-v11");
    assert_eq!(historical_retry.execution.request_hash, historical_hash);

    let current_input: CreateExecutionInput = serde_json::from_value(json!({
        "agent_id":"agent-v3-gate","request_key":"request-v3-gate","prompt":"fresh",
        "execution_profile":{},"workspace_id":"workspace-v3-gate",
        "canonical_workspace_root":"C:/gate-v3","provider":"codex",
        "task_role":"testing","mode":"read_only"
    }))
    .unwrap();
    let current_request = canonicalize_request(current_input).unwrap();
    assert!(
        std::str::from_utf8(current_request.bytes())
            .unwrap()
            .starts_with("[\"execution-request-v3\"")
    );
    let created = store
        .create_execution("execution-v3-gate".into(), current_request.clone(), 301)
        .await
        .unwrap();
    assert!(created.created);
    assert_eq!(
        created.execution.request_hash,
        current_request.request_hash()
    );
    drop(store);

    let reopened = StateStore::open(directory.path().into()).await.unwrap();
    for (request, expected_id, expected_hash) in [
        (
            historical_request,
            "execution-pending-v11",
            historical_hash.as_str(),
        ),
        (
            current_request.clone(),
            "execution-v3-gate",
            current_request.request_hash(),
        ),
    ] {
        let retry = reopened
            .create_execution("unused".into(), request, 302)
            .await
            .unwrap();
        assert!(!retry.created);
        assert_eq!(retry.execution_id, expected_id);
        assert_eq!(retry.execution.request_hash, expected_hash);
    }
    assert_eq!(
        reopened
            .connection
            .lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM executions", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        4
    );
}

/// 所有原有 SQL 对象及子表外键留存；v10 containment 与 Runtime 绑定触发器仍拦截非法变更。
#[test]
fn v12_preserves_schema_objects_foreign_keys_and_safety_triggers() {
    let directory = tempfile::tempdir().unwrap();
    let mut connection =
        v11_fixture::frozen_v11_connection(&directory.path().join("agent-state.db"));
    let objects = |c: &Connection| -> Vec<(String, String)> {
        c.prepare(
            "SELECT type,name FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%' ORDER BY type,name",
        )
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
    };
    let before = objects(&connection);
    let child_tables = [
        "executions",
        "workspace_claims",
        "work_execution_links",
        "execution_activity_events",
        "execution_usage",
        "codex_execution_usage_state",
        "execution_runtime_attempts",
        "command_runs",
        "work_command_links",
    ];
    let foreign_keys = |c: &Connection, table: &str| -> Vec<Vec<Value>> {
        let mut stmt = c
            .prepare(&format!("PRAGMA foreign_key_list('{table}')"))
            .unwrap();
        stmt.query_map([], |row| (0..8).map(|i| row.get(i)).collect())
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    };
    let old_fks: Vec<_> = child_tables
        .iter()
        .map(|table| foreign_keys(&connection, table))
        .collect();

    migrate(&mut connection).unwrap();
    assert_eq!(objects(&connection), before);
    for (table, old) in child_tables.iter().zip(old_fks) {
        assert_eq!(foreign_keys(&connection, table), old, "{table}");
    }
    assert!(connection.execute(
        "UPDATE executions SET runtime_instance_id='runtime-macos-v11' WHERE id='execution-completed-v11'", []
    ).unwrap_err().to_string().contains("execution runtime instance is immutable"));
    assert!(connection.execute(
        "UPDATE runtime_instances SET containment_session_id=999 WHERE id='runtime-macos-v11'", []
    ).unwrap_err().to_string().contains("RUNTIME_PLATFORM_EVIDENCE_INVALID"));
    assert!(connection.execute(
        "INSERT INTO workspace_claims VALUES ('/missing','missing','exclusive_execution',1)", []
    ).is_err());
    check_foreign_keys(&connection).unwrap();
}

/// 中途 SQL 冲突须回滚已重命名的 Runtime 列，并恢复连接的外键策略。
#[test]
fn v12_failure_rolls_back_schema_rows_and_user_version() {
    let directory = tempfile::tempdir().unwrap();
    let mut connection =
        v11_fixture::frozen_v11_connection(&directory.path().join("agent-state.db"));
    connection
        .execute_batch("CREATE TABLE executions_v12 (sentinel TEXT);")
        .unwrap();
    let before = rows_by_column(&connection, "executions");
    assert!(
        migrate(&mut connection)
            .unwrap_err()
            .contains("already exists")
    );
    assert_eq!(
        connection
            .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        11
    );
    assert_eq!(
        connection
            .pragma_query_value(None, "foreign_keys", |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(rows_by_column(&connection, "executions"), before);
    assert_eq!(connection.query_row(
        "SELECT count(*) FROM pragma_table_info('runtime_instances') WHERE name='codex_pid'", [], |r| r.get::<_, i64>(0)
    ).unwrap(), 1);
    assert_eq!(connection.query_row(
        "SELECT count(*) FROM pragma_table_info('runtime_instances') WHERE name='process_id'", [], |r| r.get::<_, i64>(0)
    ).unwrap(), 0);
    check_foreign_keys(&connection).unwrap();
}

/// 已损坏的历史外键必须阻断升级，不能把 v12 或任何证据写成已迁移。
#[test]
fn v12_foreign_key_gate_failure_rolls_back() {
    let directory = tempfile::tempdir().unwrap();
    let mut connection =
        v11_fixture::frozen_v11_connection(&directory.path().join("agent-state.db"));
    connection
        .pragma_update(None, "foreign_keys", "OFF")
        .unwrap();
    connection.execute(
        "INSERT INTO workspace_claims VALUES ('/orphan','missing-execution','exclusive_execution',1)",
        [],
    ).unwrap();
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .unwrap();
    assert!(
        migrate(&mut connection)
            .unwrap_err()
            .contains("foreign_key_check failed")
    );
    assert_eq!(
        connection
            .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        11
    );
    assert_eq!(
        connection
            .pragma_query_value(None, "foreign_keys", |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(connection.query_row(
        "SELECT count(*) FROM pragma_table_info('runtime_instances') WHERE name='codex_pid'", [], |r| r.get::<_, i64>(0)
    ).unwrap(), 1);
    assert_eq!(
        connection
            .query_row(
                "SELECT count(*) FROM pragma_table_info('executions') WHERE name='task_role'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        0
    );
}

/// 仅验证 Store/schema 可表达非 Codex Provider；不通过 ProviderRegistry 执行或授权释放。
#[test]
fn state_store_persists_codebuddy_identity_and_explicit_task_role() {
    let mut connection = Connection::open_in_memory().unwrap();
    migrate(&mut connection).unwrap();
    connection.execute(
        "INSERT INTO runtime_instances (id,owner_host_instance_id,provider,state,created_at,updated_at)
         VALUES ('runtime-codebuddy','host','codebuddy','unknown',1,1)",
        [],
    ).unwrap();
    assert_eq!(connection.query_row(
        "SELECT provider,process_id,protocol_contract_sha256 FROM runtime_instances WHERE id='runtime-codebuddy'",
        [],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<i64>>(1)?, r.get::<_, Option<String>>(2)?))
    ).unwrap(), ("codebuddy".into(), None, None));
    let input: CreateExecutionInput = serde_json::from_value(json!({
        "agent_id":"agent-codebuddy", "request_key":"request-codebuddy",
        "prompt":"task", "execution_profile":{}, "workspace_id":"workspace",
        "canonical_workspace_root":"/codebuddy", "provider":"codebuddy",
        "task_role":"testing", "mode":"read_only"
    }))
    .unwrap();
    let codebuddy_request = canonicalize_request(input).unwrap();
    assert!(
        std::str::from_utf8(codebuddy_request.bytes())
            .unwrap()
            .contains("execution-request-v3")
    );
    let tx = connection.transaction().unwrap();
    insert_execution(&tx, "execution-codebuddy", 1, &codebuddy_request).unwrap();
    tx.commit().unwrap();
    assert_eq!(connection.query_row(
        "SELECT provider, task_role, request_hash FROM executions WHERE id='execution-codebuddy'", [],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
    ).unwrap(), ("codebuddy".into(), "testing".into(), codebuddy_request.request_hash().into()));
    let codex = request("agent-codex", "/codex");
    let tx = connection.transaction().unwrap();
    insert_execution(&tx, "execution-codex", 2, &codex).unwrap();
    tx.commit().unwrap();
    assert_eq!(
        connection
            .query_row(
                "SELECT provider, task_role FROM executions WHERE id='execution-codex'",
                [],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            )
            .unwrap(),
        ("codex".into(), "general".into())
    );
}
