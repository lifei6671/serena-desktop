use super::{Connection, OptionalExtension, StateStore, params};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandRunRecord {
    pub id: String,
    pub request_key: String,
    pub request_hash: String,
    pub workspace_id: String,
    pub canonical_workspace_root: String,
    pub workspace_generation: u64,
    pub mode: String,
    pub relative_cwd: String,
    pub execution_mode: String,
    pub timeout_ms: u64,
    pub status: String,
    pub revision: i64,
    pub runtime_platform: String,
    pub containment_type: String,
    pub pid: Option<u32>,
    pub started_at: Option<i64>,
    pub completed_at: Option<i64>,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub termination_reason: Option<String>,
    pub stdout_total_bytes: u64,
    pub stderr_total_bytes: u64,
    pub stdout_sha256: Option<String>,
    pub stderr_sha256: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct CreateCommandRunInput {
    pub request_key: String,
    pub request_hash: String,
    pub workspace_id: String,
    pub canonical_workspace_root: String,
    pub workspace_generation: u64,
    pub work_run_id: Option<String>,
    pub mode: String,
    pub relative_cwd: String,
    pub execution_mode: String,
    pub timeout_ms: u64,
    pub runtime_platform: String,
    pub containment_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateCommandRunOutcome {
    pub record: CommandRunRecord,
    pub created: bool,
}

#[derive(Debug, Clone, Default)]
pub struct CommandRunReceipt {
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub termination_reason: Option<String>,
    pub stdout_total_bytes: u64,
    pub stderr_total_bytes: u64,
    pub stdout_sha256: Option<String>,
    pub stderr_sha256: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkCommandLinkRecord {
    pub work_run_id: String,
    pub command_run_id: String,
    pub created_at: i64,
}

impl StateStore {
    pub async fn create_command_run(
        &self,
        id: String,
        input: CreateCommandRunInput,
        now: i64,
    ) -> Result<CreateCommandRunOutcome, String> {
        self.write(move |tx| {
            if input.request_key.trim().is_empty() || input.request_hash.trim().is_empty() {
                return Err("COMMAND_INVALID_ARGUMENT".into());
            }
            if let Some(existing) =
                command_run_by_request_key(tx, &input.workspace_id, &input.request_key)
                    .map_err(|error| error.to_string())?
            {
                if existing.request_hash != input.request_hash {
                    return Err("COMMAND_REQUEST_KEY_CONFLICT".into());
                }
                return Ok(CreateCommandRunOutcome {
                    record: existing,
                    created: false,
                });
            }
            if let Some(work_run_id) = input.work_run_id.as_deref() {
                let work = super::work_runs::work_run_record(tx, work_run_id)
                    .map_err(|error| error.to_string())?
                    .ok_or("WORK_NOT_FOUND")?;
                if work.status != "active" {
                    return Err("WORK_NOT_ACTIVE".into());
                }
                if work.workspace_id != input.workspace_id
                    || work.canonical_workspace_root != input.canonical_workspace_root
                    || work.workspace_generation != input.workspace_generation
                {
                    return Err("WORKSPACE_CONTEXT_MISMATCH".into());
                }
            }
            let generation = i64::try_from(input.workspace_generation)
                .map_err(|_| "COMMAND_INVALID_ARGUMENT")?;
            let timeout_ms =
                i64::try_from(input.timeout_ms).map_err(|_| "COMMAND_INVALID_ARGUMENT")?;
            tx.execute(
                "INSERT INTO command_runs (
                    id,request_key,request_hash,workspace_id,canonical_workspace_root,
                    workspace_generation,mode,relative_cwd,execution_mode,timeout_ms,
                    status,runtime_platform,containment_type,created_at,updated_at
                 ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,'starting',?11,?12,?13,?13)",
                params![
                    id,
                    input.request_key,
                    input.request_hash,
                    input.workspace_id,
                    input.canonical_workspace_root,
                    generation,
                    input.mode,
                    input.relative_cwd,
                    input.execution_mode,
                    timeout_ms,
                    input.runtime_platform,
                    input.containment_type,
                    now,
                ],
            )
            .map_err(|error| error.to_string())?;
            if let Some(work_run_id) = input.work_run_id {
                tx.execute(
                    "INSERT INTO work_command_links(work_run_id,command_run_id,created_at)
                     VALUES (?1,?2,?3)",
                    params![work_run_id, id, now],
                )
                .map_err(|error| error.to_string())?;
            }
            let record = command_run_record(tx, &id)
                .map_err(|error| error.to_string())?
                .ok_or("COMMAND_RUN_NOT_FOUND")?;
            Ok(CreateCommandRunOutcome {
                record,
                created: true,
            })
        })
        .await
    }

    pub async fn command_run(&self, id: String) -> Result<Option<CommandRunRecord>, String> {
        self.read(move |connection| command_run_record(connection, &id))
            .await
    }

    pub async fn command_run_by_request_key(
        &self,
        workspace_id: String,
        request_key: String,
    ) -> Result<Option<CommandRunRecord>, String> {
        self.read(move |connection| {
            command_run_by_request_key(connection, &workspace_id, &request_key)
        })
        .await
    }

    pub async fn list_command_runs(
        &self,
        workspace_id: Option<String>,
        work_run_id: Option<String>,
        limit: usize,
    ) -> Result<Vec<CommandRunRecord>, String> {
        self.read(move |connection| {
            let mut statement = connection.prepare(
                "SELECT c.id,c.request_key,c.request_hash,c.workspace_id,c.canonical_workspace_root,
                        c.workspace_generation,c.mode,c.relative_cwd,c.execution_mode,c.timeout_ms,
                        c.status,c.revision,c.runtime_platform,c.containment_type,c.pid,
                        c.started_at,c.completed_at,c.exit_code,c.timed_out,c.termination_reason,
                        c.stdout_total_bytes,c.stderr_total_bytes,c.stdout_sha256,c.stderr_sha256,
                        c.error_code,c.error_message,c.created_at,c.updated_at
                 FROM command_runs c
                 LEFT JOIN work_command_links l ON l.command_run_id=c.id
                 WHERE (?1 IS NULL OR c.workspace_id=?1)
                   AND (?2 IS NULL OR l.work_run_id=?2)
                 ORDER BY c.created_at DESC,c.id ASC LIMIT ?3",
            )?;
            statement
                .query_map(
                    params![workspace_id, work_run_id, limit.clamp(1, 100) as i64],
                    command_run_row,
                )?
                .collect()
        })
        .await
    }

    pub async fn command_mark_running(
        &self,
        id: String,
        pid: u32,
        now: i64,
    ) -> Result<CommandRunRecord, String> {
        self.write(move |tx| {
            let changed = tx
                .execute(
                    "UPDATE command_runs SET status='running',pid=?2,
                     started_at=COALESCE(started_at,?3),revision=revision+1,updated_at=?3
                     WHERE id=?1 AND status='starting'",
                    params![id, i64::from(pid), now],
                )
                .map_err(|error| error.to_string())?;
            if changed != 1 {
                return Err("COMMAND_INVALID_STATE".into());
            }
            command_run_record(tx, &id)
                .map_err(|error| error.to_string())?
                .ok_or_else(|| "COMMAND_RUN_NOT_FOUND".into())
        })
        .await
    }

    pub async fn command_mark_cancelling(
        &self,
        id: String,
        now: i64,
    ) -> Result<CommandRunRecord, String> {
        self.write(move |tx| {
            tx.execute(
                "UPDATE command_runs SET status='cancelling',revision=revision+1,updated_at=?2
                 WHERE id=?1 AND status IN ('starting','running')",
                params![id, now],
            )
            .map_err(|error| error.to_string())?;
            command_run_record(tx, &id)
                .map_err(|error| error.to_string())?
                .ok_or_else(|| "COMMAND_RUN_NOT_FOUND".into())
        })
        .await
    }

    pub async fn command_mark_terminal(
        &self,
        id: String,
        status: String,
        receipt: CommandRunReceipt,
        now: i64,
    ) -> Result<CommandRunRecord, String> {
        self.write(move |tx| {
            if !matches!(
                status.as_str(),
                "completed" | "failed" | "cancelled" | "interrupted" | "unknown"
            ) {
                return Err("COMMAND_INVALID_STATE".into());
            }
            let stdout_total = i64::try_from(receipt.stdout_total_bytes)
                .map_err(|_| "COMMAND_OUTPUT_LIMIT_EXCEEDED")?;
            let stderr_total = i64::try_from(receipt.stderr_total_bytes)
                .map_err(|_| "COMMAND_OUTPUT_LIMIT_EXCEEDED")?;
            let changed = tx
                .execute(
                    "UPDATE command_runs SET
                     status=?2,revision=revision+1,completed_at=?3,updated_at=?3,
                     exit_code=?4,timed_out=?5,termination_reason=?6,
                     stdout_total_bytes=?7,stderr_total_bytes=?8,
                     stdout_sha256=?9,stderr_sha256=?10,error_code=?11,error_message=?12
                     WHERE id=?1 AND status IN ('starting','running','cancelling')",
                    params![
                        id,
                        status,
                        now,
                        receipt.exit_code,
                        if receipt.timed_out { 1 } else { 0 },
                        receipt.termination_reason,
                        stdout_total,
                        stderr_total,
                        receipt.stdout_sha256,
                        receipt.stderr_sha256,
                        receipt.error_code,
                        receipt.error_message,
                    ],
                )
                .map_err(|error| error.to_string())?;
            if changed == 0 {
                let current = command_run_record(tx, &id)
                    .map_err(|error| error.to_string())?
                    .ok_or("COMMAND_RUN_NOT_FOUND")?;
                if current.status != status {
                    return Err("COMMAND_INVALID_STATE".into());
                }
                return Ok(current);
            }
            command_run_record(tx, &id)
                .map_err(|error| error.to_string())?
                .ok_or_else(|| "COMMAND_RUN_NOT_FOUND".into())
        })
        .await
    }

    pub async fn recover_command_runs(
        &self,
        windows_job_at_creation: bool,
        now: i64,
    ) -> Result<usize, String> {
        self.write(move |tx| {
            // 只有当前 Windows Host 的 Windows Job-at-Creation 记录有重启收口证据。
            tx.execute(
                "UPDATE command_runs SET
                 status=CASE WHEN ?1=1 AND runtime_platform='windows'
                                  AND containment_type='job_at_creation'
                             THEN 'interrupted' ELSE 'unknown' END,
                 revision=revision+1,completed_at=?2,updated_at=?2,
                 termination_reason=CASE WHEN ?1=1 AND runtime_platform='windows'
                                              AND containment_type='job_at_creation'
                                         THEN 'host_restart' ELSE 'host_restart_unverified' END,
                 error_code=CASE WHEN ?1=1 AND runtime_platform='windows'
                                      AND containment_type='job_at_creation'
                                 THEN 'COMMAND_HOST_RESTARTED'
                                 ELSE 'COMMAND_RECOVERY_EVIDENCE_INCOMPLETE' END
                 WHERE status IN ('starting','running','cancelling')",
                params![windows_job_at_creation, now],
            )
            .map_err(|error| error.to_string())
        })
        .await
    }

    pub async fn work_command_link(
        &self,
        command_run_id: String,
    ) -> Result<Option<WorkCommandLinkRecord>, String> {
        self.read(move |connection| work_command_link_record(connection, &command_run_id))
            .await
    }
}

pub(super) fn command_run_record(
    connection: &Connection,
    id: &str,
) -> rusqlite::Result<Option<CommandRunRecord>> {
    connection
        .query_row(
            "SELECT id,request_key,request_hash,workspace_id,canonical_workspace_root,
             workspace_generation,mode,relative_cwd,execution_mode,timeout_ms,
             status,revision,runtime_platform,containment_type,pid,started_at,completed_at,
             exit_code,timed_out,termination_reason,stdout_total_bytes,stderr_total_bytes,
             stdout_sha256,stderr_sha256,error_code,error_message,created_at,updated_at
             FROM command_runs WHERE id=?1",
            [id],
            command_run_row,
        )
        .optional()
}

fn command_run_by_request_key(
    connection: &Connection,
    workspace_id: &str,
    request_key: &str,
) -> rusqlite::Result<Option<CommandRunRecord>> {
    connection
        .query_row(
            "SELECT id,request_key,request_hash,workspace_id,canonical_workspace_root,
             workspace_generation,mode,relative_cwd,execution_mode,timeout_ms,
             status,revision,runtime_platform,containment_type,pid,started_at,completed_at,
             exit_code,timed_out,termination_reason,stdout_total_bytes,stderr_total_bytes,
             stdout_sha256,stderr_sha256,error_code,error_message,created_at,updated_at
             FROM command_runs WHERE workspace_id=?1 AND request_key=?2",
            params![workspace_id, request_key],
            command_run_row,
        )
        .optional()
}

pub(super) fn work_command_link_record(
    connection: &Connection,
    command_run_id: &str,
) -> rusqlite::Result<Option<WorkCommandLinkRecord>> {
    connection
        .query_row(
            "SELECT work_run_id,command_run_id,created_at
             FROM work_command_links WHERE command_run_id=?1",
            [command_run_id],
            |row| {
                Ok(WorkCommandLinkRecord {
                    work_run_id: row.get(0)?,
                    command_run_id: row.get(1)?,
                    created_at: row.get(2)?,
                })
            },
        )
        .optional()
}

fn command_run_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<CommandRunRecord> {
    let workspace_generation = row
        .get::<_, i64>(5)?
        .try_into()
        .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(5, -1))?;
    let timeout_ms = row
        .get::<_, i64>(9)?
        .try_into()
        .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(9, -1))?;
    let pid = row
        .get::<_, Option<i64>>(14)?
        .map(u32::try_from)
        .transpose()
        .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(14, -1))?;
    let stdout_total_bytes = row
        .get::<_, i64>(20)?
        .try_into()
        .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(20, -1))?;
    let stderr_total_bytes = row
        .get::<_, i64>(21)?
        .try_into()
        .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(21, -1))?;
    Ok(CommandRunRecord {
        id: row.get(0)?,
        request_key: row.get(1)?,
        request_hash: row.get(2)?,
        workspace_id: row.get(3)?,
        canonical_workspace_root: row.get(4)?,
        workspace_generation,
        mode: row.get(6)?,
        relative_cwd: row.get(7)?,
        execution_mode: row.get(8)?,
        timeout_ms,
        status: row.get(10)?,
        revision: row.get(11)?,
        runtime_platform: row.get(12)?,
        containment_type: row.get(13)?,
        pid,
        started_at: row.get(15)?,
        completed_at: row.get(16)?,
        exit_code: row.get(17)?,
        timed_out: row.get::<_, i64>(18)? != 0,
        termination_reason: row.get(19)?,
        stdout_total_bytes,
        stderr_total_bytes,
        stdout_sha256: row.get(22)?,
        stderr_sha256: row.get(23)?,
        error_code: row.get(24)?,
        error_message: row.get(25)?,
        created_at: row.get(26)?,
        updated_at: row.get(27)?,
    })
}
