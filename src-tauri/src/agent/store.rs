//! Owns SQL and migrations. No raw connection or SQL closure escapes this module.
use super::execution::CanonicalRequest;
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

const SCHEMA_V1: &str = include_str!("schema_v1.sql");
const SCHEMA_V2: &str =
    "CREATE TABLE thread_names (thread_id TEXT PRIMARY KEY NOT NULL, name TEXT);";
const SCHEMA_V4: &str = include_str!("schema_v4.sql");
const SCHEMA_V3: &str = include_str!("schema_v3.sql");
const SCHEMA_V5: &str = include_str!("schema_v5.sql");
const SCHEMA_V6: &str = include_str!("schema_v6.sql");
const SCHEMA_V7: &str = include_str!("schema_v7.sql");
const SCHEMA_V8: &str = include_str!("schema_v8.sql");
const SCHEMA_V9: &str = include_str!("schema_v9.sql");
const SCHEMA_V10: &str = include_str!("schema_v10.sql");
const SCHEMA_V11: &str = include_str!("schema_v11.sql");

mod usage;
#[cfg(test)]
mod usage_tests;
mod command_runs;
mod work_runs;
#[cfg(any(windows, target_os = "macos", test))]
pub(crate) use usage::CodexUsageBaselineIntent;
#[cfg(any(windows, target_os = "macos"))]
pub(crate) use usage::USAGE_TERMINAL_GRACE_MS;
pub use command_runs::{
    CommandRunReceipt, CommandRunRecord, CreateCommandRunInput, CreateCommandRunOutcome,
    WorkCommandLinkRecord,
};
pub use work_runs::{WorkExecutionLinkRecord, WorkRunRecord};

#[derive(Clone)]
pub struct StateStore {
    connection: Arc<Mutex<Connection>>,
    database_identity: PathBuf,
    #[cfg(test)]
    observability_faults: Arc<Mutex<std::collections::HashSet<ObservabilityFault>>>,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ObservabilityFault {
    Activity,
    PermissionDiagnostic,
    UsageBaseline,
    UsageTerminalGrace,
    UsageFreeze,
    UsageProjection,
    UsageInvalidation,
}

/// Read projection; evidence is returned as stored, never inferred from policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionRecord {
    pub id: String,
    pub agent_id: String,
    pub request_key: String,
    pub request_hash: String,
    pub prompt: String,
    pub execution_profile_json: String,
    pub workspace_id: String,
    pub canonical_workspace_root: String,
    pub workspace_generation: u64,
    pub provider: String,
    pub mode: String,
    pub parent_execution_id: Option<String>,
    pub thread_id: Option<String>,
    pub turn_id: Option<String>,
    pub provider_terminal_status: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub provider_terminal_evidence_runtime_instance_id: Option<String>,
    pub provider_terminal_evidence_at: Option<i64>,
    pub runtime_instance_id: Option<String>,
    pub status: String,
    pub dispatch_state: String,
    pub revision: i64,
    pub background_cleanup_state: String,
    pub release_evidence_state: String,
    pub release_evidence_kind: Option<String>,
    pub release_evidence_json: Option<String>,
    pub result_completeness: String,
    pub final_result_json: Option<String>,
    pub interrupt_requested_at: Option<i64>,
    pub interrupt_ack_at: Option<i64>,
    pub interrupt_timeout_at: Option<i64>,
    pub interrupt_diagnostic: Option<String>,
    pub last_activity_at: Option<i64>,
    pub activity_phase: Option<String>,
    pub tool_category: Option<String>,
    pub activity_summary_code: Option<String>,
    pub activity_sequence: i64,
}

#[derive(Debug, PartialEq, Eq)]
pub struct RuntimeRecord {
    pub id: String,
    pub owner_host_instance_id: String,
    pub state: String,
    pub job_name: Option<String>,
    pub codex_pid: Option<u32>,
    pub codex_process_start_token: Option<String>,
    pub job_session_id: Option<i64>,
    pub job_creation_mode: Option<String>,
    pub job_handle_inheritable: Option<bool>,
    pub job_kill_on_close: Option<bool>,
    pub job_breakaway_allowed: Option<bool>,
    pub job_policy_verified_at: Option<i64>,
    pub termination_evidence_state: String,
    pub termination_evidence_type: Option<String>,
    pub termination_evidence_at: Option<i64>,
    pub runtime_platform: String,
    pub containment_type: String,
    pub process_identity_scheme: String,
    pub containment_process_group_id: Option<i64>,
    pub containment_session_id: Option<i64>,
    pub containment_verified_at: Option<i64>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct WorkspaceClaimRecord {
    pub canonical_workspace_root: String,
    pub execution_id: String,
    pub claim_type: String,
    pub acquired_at: i64,
}

/// 统一 Claim 查询 SQL，供异步读取与受 Supervisor 排他保护的同步读取复用。
fn workspace_claim_at(
    connection: &Connection,
    root: &str,
) -> rusqlite::Result<Option<WorkspaceClaimRecord>> {
    connection
        .query_row(
            "SELECT canonical_workspace_root, execution_id, claim_type, acquired_at
             FROM workspace_claims WHERE canonical_workspace_root = ?1",
            [root],
            |row| {
                Ok(WorkspaceClaimRecord {
                    canonical_workspace_root: row.get(0)?,
                    execution_id: row.get(1)?,
                    claim_type: row.get(2)?,
                    acquired_at: row.get(3)?,
                })
            },
        )
        .optional()
}

impl StateStore {
    /// Resolve exactly the application's data directory, not its workspace/config directory.
    pub async fn open_for_app(app: &tauri::AppHandle) -> Result<Self, String> {
        use tauri::Manager;
        let directory = app.path().app_data_dir().map_err(|e| e.to_string())?;
        Self::open(directory).await
    }

    /// `app_data_directory` is supplied by the host path resolver (or an isolated test).
    pub async fn open(app_data_directory: PathBuf) -> Result<Self, String> {
        tauri::async_runtime::spawn_blocking(move || {
            std::fs::create_dir_all(&app_data_directory).map_err(|e| e.to_string())?;
            let mut connection = Connection::open(app_data_directory.join("agent-state.db"))
                .map_err(|e| e.to_string())?;
            configure(&connection)?;
            migrate(&mut connection)?;
            Ok(Self {
                connection: Arc::new(Mutex::new(connection)),
                database_identity: std::fs::canonicalize(app_data_directory.join("agent-state.db"))
                    .map_err(|e| e.to_string())?,
                #[cfg(test)]
                observability_faults: Arc::new(Mutex::new(std::collections::HashSet::new())),
            })
        })
        .await
        .map_err(|e| e.to_string())?
    }

    // Private: SQL access is confined to this module. Both the mutex wait and the
    // SQLite busy wait happen on a blocking worker, never a Tokio async worker.
    async fn read<T: Send + 'static>(
        &self,
        operation: impl FnOnce(&Connection) -> rusqlite::Result<T> + Send + 'static,
    ) -> Result<T, String> {
        let connection = self.connection.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let connection = connection.lock().map_err(|e| e.to_string())?;
            operation(&connection).map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| e.to_string())?
    }

    pub async fn execution(&self, id: String) -> Result<Option<ExecutionRecord>, String> {
        self.read(move |c| execution_record(c, &id)).await
    }

    /// 读取已持久化的公共 Usage；无行保持为 unknown/null，不构造历史默认值。
    pub async fn execution_usage(
        &self,
        id: String,
    ) -> Result<Option<super::usage::UsageSnapshot>, String> {
        self.read(move |c| usage::execution_usage_snapshot(c, &id))
            .await
    }

    pub async fn runtime(&self, id: String) -> Result<Option<RuntimeRecord>, String> {
        self.read(move |c| {
            c.query_row(
                "SELECT id, owner_host_instance_id, state, job_session_id, job_creation_mode,
             job_handle_inheritable, job_kill_on_close, job_breakaway_allowed,
             job_policy_verified_at, termination_evidence_state, termination_evidence_type,
             termination_evidence_at, job_name, codex_pid, codex_process_start_token,
             runtime_platform, containment_type, process_identity_scheme,
             containment_process_group_id, containment_session_id, containment_verified_at
             FROM runtime_instances WHERE id = ?1",
                [&id],
                |r| {
                    Ok(RuntimeRecord {
                        id: r.get(0)?,
                        owner_host_instance_id: r.get(1)?,
                        state: r.get(2)?,
                        job_name: r.get(12)?,
                        codex_pid: r.get(13)?,
                        codex_process_start_token: r.get(14)?,
                        job_session_id: r.get(3)?,
                        job_creation_mode: r.get(4)?,
                        job_handle_inheritable: r.get(5)?,
                        job_kill_on_close: r.get(6)?,
                        job_breakaway_allowed: r.get(7)?,
                        job_policy_verified_at: r.get(8)?,
                        termination_evidence_state: r.get(9)?,
                        termination_evidence_type: r.get(10)?,
                        termination_evidence_at: r.get(11)?,
                        runtime_platform: r.get(15)?,
                        containment_type: r.get(16)?,
                        process_identity_scheme: r.get(17)?,
                        containment_process_group_id: r.get(18)?,
                        containment_session_id: r.get(19)?,
                        containment_verified_at: r.get(20)?,
                    })
                },
            )
            .optional()
        })
        .await
    }

    pub(crate) async fn orphan_runtimes(&self, owner: String) -> Result<Vec<String>, String> {
        self.read(move |c| {
            let mut statement = c.prepare("SELECT r.id FROM runtime_instances r
                WHERE r.owner_host_instance_id != ?1 AND r.state != 'terminated'
                AND NOT EXISTS (SELECT 1 FROM executions e JOIN workspace_claims w ON w.execution_id=e.id
                    WHERE e.runtime_instance_id=r.id)")?;
            statement.query_map([owner], |r| r.get(0))?.collect()
        }).await
    }

    pub async fn workspace_claim(
        &self,
        root: String,
    ) -> Result<Option<WorkspaceClaimRecord>, String> {
        self.read(move |connection| workspace_claim_at(connection, &root))
            .await
    }

    /// 仅供持有 Supervisor operation mutex 的短管理临界区读取 Claim，期间不得 await。
    pub(crate) fn workspace_claim_blocking(
        &self,
        root: &str,
    ) -> Result<Option<WorkspaceClaimRecord>, String> {
        let connection = self.connection.lock().map_err(|error| error.to_string())?;
        workspace_claim_at(&connection, root).map_err(|error| error.to_string())
    }

    #[cfg(test)]
    pub(crate) fn inject_observability_failure(&self, fault: ObservabilityFault) {
        self.observability_faults.lock().unwrap().insert(fault);
    }

    #[cfg(test)]
    pub(crate) fn observability_failure_pending(&self, fault: ObservabilityFault) -> bool {
        self.observability_faults.lock().unwrap().contains(&fault)
    }

    #[cfg(test)]
    pub(crate) fn clear_observability_failure(&self, fault: ObservabilityFault) {
        self.observability_faults.lock().unwrap().remove(&fault);
    }

    #[cfg(test)]
    fn take_observability_failure(&self, fault: ObservabilityFault) -> bool {
        self.observability_faults.lock().unwrap().remove(&fault)
    }
}

#[cfg(windows)]
mod runtime_store;

fn configure(connection: &Connection) -> Result<(), String> {
    connection
        .busy_timeout(Duration::from_millis(5000))
        .map_err(|e| e.to_string())?;
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .map_err(|e| e.to_string())?;
    let journal: String = connection
        .query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    if !journal.eq_ignore_ascii_case("wal") {
        return Err(format!("WAL unavailable: {journal}"));
    }
    connection
        .pragma_update(None, "synchronous", "FULL")
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn migrate(connection: &mut Connection) -> Result<(), String> {
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    let version: i64 = transaction
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(|e| e.to_string())?;
    match version {
        0 => {
            // v0 is only an empty database. No historical format/ownership can be
            // inferred safely; future historical migrations need their own contract.
            let count: i64 = transaction
                .query_row(
                    "SELECT count(*) FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%'",
                    [],
                    |r| r.get(0),
                )
                .map_err(|e| e.to_string())?;
            if count != 0 {
                return Err(
                    "unversioned nonempty database requires an explicit historical migration"
                        .into(),
                );
            }
            apply_migration(&transaction, 1, SCHEMA_V1).map_err(|e| e.to_string())?;
        }
        1..=11 => {}
        _ => return Err(format!("unsupported agent state schema version: {version}")),
    }
    if version < 2 {
        apply_migration(&transaction, 2, SCHEMA_V2).map_err(|e| e.to_string())?;
    }
    if version < 3 {
        apply_migration(&transaction, 3, SCHEMA_V3).map_err(|e| e.to_string())?;
    }
    if version < 4 {
        apply_migration(&transaction, 4, SCHEMA_V4).map_err(|e| e.to_string())?;
    }
    if version < 5 {
        apply_migration(&transaction, 5, SCHEMA_V5).map_err(|e| e.to_string())?;
    }
    if version < 6 {
        apply_migration(&transaction, 6, SCHEMA_V6).map_err(|e| e.to_string())?;
    }
    if version < 7 {
        apply_migration(&transaction, 7, SCHEMA_V7).map_err(|e| e.to_string())?;
    }
    if version < 8 {
        apply_migration(&transaction, 8, SCHEMA_V8).map_err(|e| e.to_string())?;
    }
    if version < 9 {
        apply_migration(&transaction, 9, SCHEMA_V9).map_err(|e| e.to_string())?;
    }
    if version < 10 {
        apply_migration(&transaction, 10, SCHEMA_V10).map_err(|e| e.to_string())?;
    }
    if version < 11 {
        apply_migration(&transaction, 11, SCHEMA_V11).map_err(|e| e.to_string())?;
    }
    transaction.commit().map_err(|e| e.to_string())
}

fn apply_migration(transaction: &Transaction<'_>, version: i64, sql: &str) -> rusqlite::Result<()> {
    transaction.execute_batch(sql)?;
    transaction.pragma_update(None, "user_version", version)
}

// Private insertion primitive, called inside the atomic creation transaction.
fn insert_execution(
    transaction: &Transaction<'_>,
    id: &str,
    created_at: i64,
    request: &CanonicalRequest,
) -> rusqlite::Result<()> {
    use super::execution::ExecutionMode;
    let input = request.input();
    let workspace_generation = i64::try_from(input.workspace_generation)
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    transaction.execute(
        "INSERT INTO executions (id, agent_id, request_key, request_hash, prompt,
         execution_profile_json, workspace_id, canonical_workspace_root, workspace_generation, provider, mode,
         parent_execution_id, thread_id, status, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'codex', ?10, ?11, ?12, 'dispatch_pending', ?13, ?13)",
        params![
            id,
            input.agent_id,
            input.request_key,
            request.request_hash(),
            input.prompt,
            request.execution_profile_json(),
            input.workspace_id,
            input.canonical_workspace_root,
            workspace_generation,
            match input.mode {
                ExecutionMode::ReadOnly => "read_only",
                ExecutionMode::WorkspaceWrite => "workspace_write",
            },
            input.parent_execution_id,
            input.thread_id,
            created_at
        ],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests;

mod runtime_attempts;
pub mod transactions;

fn execution_record(c: &Connection, id: &str) -> rusqlite::Result<Option<ExecutionRecord>> {
    c.query_row(
            "SELECT id, agent_id, request_key, request_hash, prompt, execution_profile_json,
             workspace_id, canonical_workspace_root, workspace_generation, provider, mode, parent_execution_id, thread_id,
             runtime_instance_id, status, dispatch_state, revision, background_cleanup_state,
             release_evidence_state, release_evidence_kind, release_evidence_json, result_completeness, turn_id, provider_terminal_status, provider_terminal_evidence_runtime_instance_id, final_result_json
             , interrupt_requested_at, interrupt_ack_at, interrupt_timeout_at, interrupt_diagnostic, provider_terminal_evidence_at, error_code, error_message,
             last_activity_at, activity_phase, tool_category, activity_summary_code, activity_sequence
             FROM executions WHERE id = ?1", [&id], |r| Ok(ExecutionRecord {
                id: r.get(0)?, agent_id: r.get(1)?, request_key: r.get(2)?, request_hash: r.get(3)?,
                prompt: r.get(4)?, execution_profile_json: r.get(5)?, workspace_id: r.get(6)?,
                canonical_workspace_root: r.get(7)?, workspace_generation: r.get::<_, i64>(8)?.try_into().map_err(|_| rusqlite::Error::IntegralValueOutOfRange(8, -1))?, provider: r.get(9)?, mode: r.get(10)?,
                parent_execution_id: r.get(11)?, thread_id: r.get(12)?, runtime_instance_id: r.get(13)?, status: r.get(14)?,
                dispatch_state: r.get(15)?, revision: r.get(16)?, background_cleanup_state: r.get(17)?,
                release_evidence_state: r.get(18)?, release_evidence_kind: r.get(19)?,
                release_evidence_json: r.get(20)?, result_completeness: r.get(21)?, turn_id: r.get(22)?, provider_terminal_status: r.get(23)?, provider_terminal_evidence_runtime_instance_id: r.get(24)?, final_result_json: r.get(25)?,
                interrupt_requested_at: r.get(26)?, interrupt_ack_at: r.get(27)?,
                interrupt_timeout_at: r.get(28)?, interrupt_diagnostic: r.get(29)?,
                 provider_terminal_evidence_at: r.get(30)?,
                 error_code: r.get(31)?, error_message: r.get(32)?,
                 last_activity_at: r.get(33)?, activity_phase: r.get(34)?, tool_category: r.get(35)?,
                 activity_summary_code: r.get(36)?, activity_sequence: r.get(37)?,
             })).optional()
}
