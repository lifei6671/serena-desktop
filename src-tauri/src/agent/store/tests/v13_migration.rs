use super::*;
use rusqlite::types::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// 载入完整冻结 v12 dump，避免以当前迁移器伪造旧版本输入。
fn frozen_v12_connection(path: &std::path::Path) -> Connection {
    let connection = Connection::open(path).unwrap();
    configure(&connection).unwrap();
    // dump 的表顺序不保证父表先存在；载入后立即检查所有历史外键。
    connection
        .pragma_update(None, "foreign_keys", "OFF")
        .unwrap();
    connection
        .execute_batch(include_str!(
            "../../../../tests/fixtures/agent_state_v12.sql"
        ))
        .unwrap();
    check_foreign_keys(&connection).unwrap();
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .unwrap();
    connection
}

/// 保存所有命名列的原始 SQLite 值，包含 Codex 私有表及 usage 历史。
fn table_values(connection: &Connection, table: &str) -> BTreeMap<String, Vec<Value>> {
    let mut statement = connection
        .prepare(&format!("SELECT * FROM \"{table}\" ORDER BY 1"))
        .unwrap();
    let columns: Vec<String> = statement
        .column_names()
        .iter()
        .map(|name| name.to_string())
        .collect();
    let rows: Vec<Vec<Value>> = statement
        .query_map([], |row| {
            (0..columns.len()).map(|index| row.get(index)).collect()
        })
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    columns
        .into_iter()
        .enumerate()
        .map(|(index, name)| (name, rows.iter().map(|row| row[index].clone()).collect()))
        .collect()
}

/// 冻结对象 SQL，检查失败事务没有遗留新表、索引或重写历史对象。
fn schema_objects(connection: &Connection) -> Vec<(String, String, Option<String>)> {
    connection.prepare("SELECT type,name,sql FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%' ORDER BY type,name")
        .unwrap().query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap().collect::<rusqlite::Result<_>>().unwrap()
}

/// 当前版本与外键策略都必须在成功升级和失败回滚后正确保留。
fn assert_version(connection: &Connection, expected: i64) {
    assert_eq!(
        connection
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .unwrap(),
        expected
    );
    assert_eq!(
        connection
            .pragma_query_value(None, "foreign_keys", |row| row.get::<_, i64>(0))
            .unwrap(),
        1
    );
}

/// 冻结 v12 全表、全字段值跨 v13 升级与重开保持不变，且不回填私有状态。
#[test]
fn frozen_v12_to_v13_preserves_all_values_and_reopens_without_backfill() {
    let directory = tempfile::tempdir().unwrap();
    let mut connection = frozen_v12_connection(&directory.path().join("agent-state.db"));
    assert_version(&connection, 12);
    let objects = schema_objects(&connection);
    let before: BTreeMap<_, _> = objects
        .iter()
        .filter(|(kind, _, _)| kind == "table")
        .map(|(_, name, _)| (name.clone(), table_values(&connection, name)))
        .collect();
    for table in [
        "executions",
        "runtime_instances",
        "execution_usage",
        "codex_thread_usage_epochs",
        "codex_execution_usage_state",
    ] {
        assert!(
            before[table].values().any(|values| !values.is_empty()),
            "{table} fixture must contain evidence"
        );
    }
    migrate(&mut connection).unwrap();
    drop(connection);
    for _ in 0..2 {
        let store = open(directory.path());
        let connection = store.connection.lock().unwrap();
        assert_version(&connection, 13);
        for (table, values) in &before {
            assert_eq!(&table_values(&connection, table), values, "{table}");
        }
        let historical_objects: Vec<_> = schema_objects(&connection)
            .into_iter()
            .filter(|(_, name, _)| !name.starts_with("codebuddy_execution_"))
            .collect();
        assert_eq!(historical_objects, objects);
        assert_eq!(
            connection
                .query_row(
                    "SELECT count(*) FROM codebuddy_execution_state",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
        check_foreign_keys(&connection).unwrap();
    }
}

/// 固定 v12 文本哈希；只归一化 checkout 换行，原始字节另由交付 hash Gate 核对。
#[test]
fn frozen_schema_v12_content_hash_is_unchanged() {
    let normalized = include_str!("../../schema_v12.sql").replace("\r\n", "\n");
    assert_eq!(
        Sha256::digest(normalized.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>(),
        "edcea31a12b37244dc6a772c4c22d82ed00710a6288b6d945ecc14f8ca7f68b3"
    );
}

/// v0 与冻结 v9/v11 均到达 v13，且没有为历史任务创建 CodeBuddy 私有行。
#[test]
fn v0_and_historical_databases_reach_v13_without_private_rows() {
    let directory = tempfile::tempdir().unwrap();
    let fresh = Connection::open_in_memory().unwrap();
    fresh.pragma_update(None, "foreign_keys", true).unwrap();
    let historical_v9 = frozen_v9_connection();
    historical_v9
        .pragma_update(None, "foreign_keys", true)
        .unwrap();
    let historical_v11 =
        v11_fixture::frozen_v11_connection(&directory.path().join("historical.db"));
    for mut connection in [fresh, historical_v9, historical_v11] {
        migrate(&mut connection).unwrap();
        assert_version(&connection, 13);
        assert_eq!(
            connection
                .query_row(
                    "SELECT count(*) FROM codebuddy_execution_state",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
        check_foreign_keys(&connection).unwrap();
    }
}

/// 未来 v14 明确拒绝，连接重新打开也不得修改版本、对象和历史值。
#[tokio::test]
async fn future_v14_is_rejected_without_mutation() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("agent-state.db");
    let mut connection = frozen_v12_connection(&path);
    connection.pragma_update(None, "user_version", 14).unwrap();
    let before = schema_objects(&connection);
    let values = table_values(&connection, "executions");
    assert_eq!(
        migrate(&mut connection).unwrap_err(),
        "unsupported agent state schema version: 14"
    );
    assert_eq!(schema_objects(&connection), before);
    assert_version(&connection, 14);
    drop(connection);
    assert!(
        matches!(StateStore::open(directory.path().into()).await, Err(error) if error.contains("unsupported agent state schema version: 14"))
    );
    let connection = Connection::open(path).unwrap();
    assert_eq!(schema_objects(&connection), before);
    assert_eq!(table_values(&connection, "executions"), values);
    assert_eq!(
        connection
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .unwrap(),
        14
    );
}

/// 索引冲突发生在 CREATE TABLE 之后，必须连同新表与版本一起回滚。
#[test]
fn v13_late_migration_failure_rolls_back_schema_rows_and_version() {
    let directory = tempfile::tempdir().unwrap();
    let mut connection = frozen_v12_connection(&directory.path().join("agent-state.db"));
    connection
        .execute_batch("CREATE INDEX codebuddy_execution_target_prompt ON executions(id)")
        .unwrap();
    let objects = schema_objects(&connection);
    let before = table_values(&connection, "executions");
    assert!(
        migrate(&mut connection)
            .unwrap_err()
            .contains("already exists")
    );
    assert_version(&connection, 12);
    assert_eq!(schema_objects(&connection), objects);
    assert_eq!(table_values(&connection, "executions"), before);
    check_foreign_keys(&connection).unwrap();
}

/// 历史外键损坏也须回滚新建私有表，不把错误输入标为 v13。
#[test]
fn v13_foreign_key_failure_rolls_back_new_schema() {
    let directory = tempfile::tempdir().unwrap();
    let mut connection = frozen_v12_connection(&directory.path().join("agent-state.db"));
    connection
        .pragma_update(None, "foreign_keys", "OFF")
        .unwrap();
    connection
        .execute(
            "INSERT INTO workspace_claims VALUES ('/orphan-v13','missing','exclusive_execution',1)",
            [],
        )
        .unwrap();
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .unwrap();
    let before = schema_objects(&connection);
    assert!(
        migrate(&mut connection)
            .unwrap_err()
            .contains("foreign_key_check failed")
    );
    assert_version(&connection, 12);
    assert_eq!(schema_objects(&connection), before);
    assert_eq!(connection.query_row("SELECT execution_id FROM workspace_claims WHERE canonical_workspace_root='/orphan-v13'", [], |row| row.get::<_, String>(0)).unwrap(), "missing");
}

/// 构造纯 schema 约束测试；只在 Store 内部测试模块直接操作 SQLite。
fn schema_fixture() -> Connection {
    let mut connection = Connection::open_in_memory().unwrap();
    // 内存 schema fixture 只启用外键；WAL 策略由真实文件重开测试覆盖。
    connection
        .pragma_update(None, "foreign_keys", true)
        .unwrap();
    migrate(&mut connection).unwrap();
    for index in 1..=3 {
        insert(
            &mut connection,
            &format!("e{index}"),
            &format!("a{index}"),
            &format!("/w{index}"),
        );
        runtime(&connection, &format!("r{index}"));
    }
    connection.execute("INSERT INTO codebuddy_execution_state (execution_id,conversation_request_id,prompt_state,recovery_state,revision,created_at,updated_at) VALUES ('e1','01900000000070008000000000000001','prepared','not_attempted',0,1,1)", []).unwrap();
    connection
}

/// 三条外键均采用 RESTRICT，运行时和恢复来源不能被删除或替换为孤儿。
#[test]
fn v13_foreign_keys_restrict_execution_and_both_runtimes() {
    let connection = schema_fixture();
    let mut foreign_keys: Vec<(String, String, String)> = connection
        .prepare("PRAGMA foreign_key_list(codebuddy_execution_state)")
        .unwrap()
        .query_map([], |row| Ok((row.get(2)?, row.get(3)?, row.get(6)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    foreign_keys.sort();
    assert_eq!(
        foreign_keys,
        vec![
            (
                "executions".into(),
                "execution_id".into(),
                "RESTRICT".into()
            ),
            (
                "runtime_instances".into(),
                "recovery_runtime_instance_id".into(),
                "RESTRICT".into()
            ),
            (
                "runtime_instances".into(),
                "runtime_instance_id".into(),
                "RESTRICT".into()
            ),
        ]
    );
    connection.execute("UPDATE codebuddy_execution_state SET runtime_instance_id='r1',acp_protocol_version=1,session_id='s',prompt_state='sent',recovery_method='session/load',recovery_state='inspecting',recovery_runtime_instance_id='r2',recovery_started_at=2", []).unwrap();
    for sql in [
        "DELETE FROM executions WHERE id='e1'",
        "DELETE FROM runtime_instances WHERE id='r1'",
        "DELETE FROM runtime_instances WHERE id='r2'",
        "UPDATE codebuddy_execution_state SET execution_id='missing'",
        "UPDATE codebuddy_execution_state SET runtime_instance_id='missing'",
        "UPDATE codebuddy_execution_state SET recovery_runtime_instance_id='missing'",
    ] {
        assert!(
            connection
                .execute(sql, [])
                .unwrap_err()
                .to_string()
                .contains("FOREIGN KEY"),
            "{sql}"
        );
    }
    check_foreign_keys(&connection).unwrap();
}

/// 数据库本身拒绝缺失 identity、非法枚举/范围/JSON 和不完整状态组合。
#[test]
fn v13_checks_reject_invalid_private_values_without_writes() {
    let connection = schema_fixture();
    let before = table_values(&connection, "codebuddy_execution_state");
    for assignment in [
        "conversation_request_id=NULL",
        "conversation_request_id='bad'",
        "conversation_request_id='01900000000040008000000000000001'",
        "conversation_request_id='01900000000070007000000000000001'",
        "conversation_request_id='0190000000007000800000000000000A'",
        "acp_protocol_version=-1",
        "acp_protocol_version=65536",
        "acp_protocol_version=1.5",
        "session_id=''",
        "provider_request_id='p'",
        "provider_request_id_source='exact_provider_observation'",
        "provider_request_id='p',provider_request_id_source='guessed'",
        "prompt_state='invalid'",
        "prompt_state='sent'",
        "prompt_state='uncertain'",
        "prompt_state='terminal_observed'",
        "prompt_rpc_id='1'",
        "terminal_stop_reason='end_turn'",
        "terminal_observed_at=1",
        "revision=-1",
        "revision=1.5",
        "recovery_state='complete'",
        "recovery_method='session/load'",
        "recovery_state='inspecting'",
        "recovery_finished_at=2",
    ] {
        assert!(
            connection
                .execute(
                    &format!("UPDATE codebuddy_execution_state SET {assignment}"),
                    []
                )
                .is_err(),
            "{assignment}"
        );
        assert_eq!(
            table_values(&connection, "codebuddy_execution_state"),
            before,
            "{assignment}"
        );
    }
    connection.execute("UPDATE codebuddy_execution_state SET runtime_instance_id='r1',acp_protocol_version=1,session_id='s',prompt_state='sent'", []).unwrap();
    let sent = table_values(&connection, "codebuddy_execution_state");
    for rpc in [
        "null",
        "1.5",
        "true",
        "{}",
        "[]",
        "9223372036854775808",
        "-9223372036854775809",
        "invalid",
    ] {
        assert!(
            connection
                .execute(
                    "UPDATE codebuddy_execution_state SET prompt_rpc_id=?1",
                    [rpc]
                )
                .is_err(),
            "{rpc}"
        );
        assert_eq!(table_values(&connection, "codebuddy_execution_state"), sent);
    }
    for rpc in [
        "\"rpc-string\"",
        "-9223372036854775808",
        "9223372036854775807",
    ] {
        connection
            .execute(
                "UPDATE codebuddy_execution_state SET prompt_rpc_id=?1",
                [rpc],
            )
            .unwrap();
    }
}

/// 空 Session 尚未进入唯一索引；已知的相同 Session/Conversation pair 必须唯一。
#[test]
fn v13_partial_unique_index_uses_exact_session_conversation_pair() {
    let connection = schema_fixture();
    let index: (i64, i64) = connection.query_row("SELECT \"unique\",partial FROM pragma_index_list('codebuddy_execution_state') WHERE name='codebuddy_execution_target_prompt'", [], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
    assert_eq!(index, (1, 1));
    let columns: Vec<String> = connection.prepare("SELECT name FROM pragma_index_info('codebuddy_execution_target_prompt') ORDER BY seqno").unwrap()
        .query_map([], |row| row.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap();
    assert_eq!(columns, ["session_id", "conversation_request_id"]);
    connection.execute("INSERT INTO codebuddy_execution_state SELECT 'e2',runtime_instance_id,acp_protocol_version,session_id,conversation_request_id,provider_request_id,provider_request_id_source,prompt_rpc_id,prompt_state,terminal_stop_reason,terminal_observed_at,recovery_method,recovery_state,recovery_runtime_instance_id,recovery_started_at,recovery_finished_at,revision,created_at,updated_at FROM codebuddy_execution_state WHERE execution_id='e1'", []).unwrap();
    connection
        .execute(
            "UPDATE codebuddy_execution_state SET session_id='session-a' WHERE execution_id='e1'",
            [],
        )
        .unwrap();
    let before = table_values(&connection, "codebuddy_execution_state");
    assert!(connection.execute("UPDATE codebuddy_execution_state SET session_id='session-a' WHERE execution_id='e2'", []).unwrap_err().to_string().contains("UNIQUE"));
    assert_eq!(
        table_values(&connection, "codebuddy_execution_state"),
        before
    );
    connection
        .execute(
            "UPDATE codebuddy_execution_state SET session_id='session-b' WHERE execution_id='e2'",
            [],
        )
        .unwrap();
    connection.execute("UPDATE codebuddy_execution_state SET session_id='session-a',conversation_request_id='01900000000070008000000000000002' WHERE execution_id='e2'", []).unwrap();
}
