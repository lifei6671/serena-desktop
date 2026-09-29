use super::v13_migration::{assert_version, frozen_v12_connection, schema_objects, table_values};
use super::*;
use rusqlite::types::Value;

/// 用冻结 v12 加冻结 v13 DDL 构造真实旧版本输入，不经当前迁移器预造 v14 列。
fn frozen_v13_connection(path: &std::path::Path) -> Connection {
    let connection = frozen_v12_connection(path);
    connection.execute_batch(SCHEMA_V13).unwrap();
    connection.pragma_update(None, "user_version", 13).unwrap();
    check_foreign_keys(&connection).unwrap();
    connection
}

/// v13 -> v14 只增加 nullable evidence 列，不回填或改写历史请求快照。
#[test]
fn v13_to_v14_preserves_rows_and_leaves_effective_profile_null() {
    let directory = tempfile::tempdir().unwrap();
    let mut connection = frozen_v13_connection(&directory.path().join("agent-state.db"));
    assert_version(&connection, 13);
    let requested: Vec<Value> = connection
        .prepare("SELECT execution_profile_json FROM executions ORDER BY id")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    let row_count = requested.len();

    migrate(&mut connection).unwrap();

    assert_version(&connection, 14);
    assert_eq!(
        connection
            .prepare("SELECT execution_profile_json FROM executions ORDER BY id")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<Value>>>()
            .unwrap(),
        requested
    );
    assert_eq!(
        table_values(&connection, "executions")
            .remove("effective_execution_profile_json")
            .unwrap(),
        vec![Value::Null; row_count]
    );
    check_foreign_keys(&connection).unwrap();
}

/// v14 DDL 失败时不得提升版本或改写已有对象与历史值。
#[test]
fn v14_migration_failure_rolls_back_version_and_rows() {
    let directory = tempfile::tempdir().unwrap();
    let mut connection = frozen_v13_connection(&directory.path().join("agent-state.db"));
    connection.execute_batch(SCHEMA_V14).unwrap();
    let objects = schema_objects(&connection);
    let rows = table_values(&connection, "executions");

    assert!(
        migrate(&mut connection)
            .unwrap_err()
            .contains("duplicate column name")
    );
    assert_version(&connection, 13);
    assert_eq!(schema_objects(&connection), objects);
    assert_eq!(table_values(&connection, "executions"), rows);
}
