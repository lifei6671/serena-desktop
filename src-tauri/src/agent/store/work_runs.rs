use super::{Connection, OptionalExtension, StateStore, params};

mod terminal;

/// Read projection of a WorkRun; nullable values are returned exactly as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkRunRecord {
    pub id: String,
    pub workspace_id: String,
    pub canonical_workspace_root: String,
    pub title: String,
    pub goal: Option<String>,
    pub status: String,
    pub revision: i64,
    pub acceptance_json: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub completed_at: Option<i64>,
}

/// Read projection of the persisted Work membership and delegation context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkExecutionLinkRecord {
    pub work_run_id: String,
    pub execution_id: String,
    pub parent_execution_id: Option<String>,
    pub delegation_context_json: Option<String>,
    pub created_at: i64,
}

impl StateStore {
    pub async fn create_work_run(
        &self,
        id: String,
        workspace_id: String,
        canonical_workspace_root: String,
        title: String,
        goal: Option<String>,
        now: i64,
    ) -> Result<(), String> {
        self.write(move |tx| {
            tx.execute(
                "INSERT INTO work_runs
                 (id, workspace_id, canonical_workspace_root, title, goal, status, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'active', ?6, ?6)",
                params![id, workspace_id, canonical_workspace_root, title, goal, now],
            ).map_err(|e| e.to_string())?;
            Ok(())
        })
        .await
    }

    pub async fn work_run(&self, id: String) -> Result<Option<WorkRunRecord>, String> {
        self.read(move |c| work_run_record(c, &id)).await
    }

    pub async fn work_execution_link(
        &self,
        execution_id: String,
    ) -> Result<Option<WorkExecutionLinkRecord>, String> {
        self.read(move |c| work_execution_link_record(c, &execution_id))
            .await
    }

    /// All links for one Work, oldest first with execution id as the tie-breaker.
    pub async fn work_execution_links(
        &self,
        work_run_id: String,
    ) -> Result<Vec<WorkExecutionLinkRecord>, String> {
        self.read(move |c| {
            let mut statement = c.prepare(
                "SELECT work_run_id, execution_id, parent_execution_id,
                 delegation_context_json, created_at FROM work_execution_links
                 WHERE work_run_id=?1 ORDER BY created_at ASC, execution_id ASC",
            )?;
            statement
                .query_map([work_run_id], work_execution_link_row)?
                .collect()
        })
        .await
    }

    /// Newest creation first, with id ascending as a stable tie-breaker.
    /// The limit is clamped to 1..=100; None includes every workspace.
    pub async fn list_work_runs(
        &self,
        workspace_id: Option<String>,
        limit: usize,
    ) -> Result<Vec<WorkRunRecord>, String> {
        self.read(move |c| {
            let mut statement = c.prepare(
                "SELECT id, workspace_id, canonical_workspace_root, title, goal, status,
                 revision, acceptance_json, created_at, updated_at, completed_at
                 FROM work_runs WHERE (?1 IS NULL OR workspace_id = ?1)
                 ORDER BY created_at DESC, id ASC LIMIT ?2",
            )?;
            statement
                .query_map(
                    params![workspace_id, limit.clamp(1, 100) as i64],
                    work_run_row,
                )?
                .collect()
        })
        .await
    }
}

pub(super) fn work_run_record(c: &Connection, id: &str) -> rusqlite::Result<Option<WorkRunRecord>> {
    c.query_row(
        "SELECT id, workspace_id, canonical_workspace_root, title, goal, status,
         revision, acceptance_json, created_at, updated_at, completed_at
         FROM work_runs WHERE id = ?1",
        [id],
        work_run_row,
    )
    .optional()
}

pub(super) fn work_execution_link_record(
    c: &Connection,
    execution_id: &str,
) -> rusqlite::Result<Option<WorkExecutionLinkRecord>> {
    c.query_row(
        "SELECT work_run_id, execution_id, parent_execution_id,
         delegation_context_json, created_at FROM work_execution_links WHERE execution_id=?1",
        [execution_id],
        work_execution_link_row,
    )
    .optional()
}

fn work_execution_link_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<WorkExecutionLinkRecord> {
    Ok(WorkExecutionLinkRecord {
        work_run_id: row.get(0)?,
        execution_id: row.get(1)?,
        parent_execution_id: row.get(2)?,
        delegation_context_json: row.get(3)?,
        created_at: row.get(4)?,
    })
}

fn work_run_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<WorkRunRecord> {
    Ok(WorkRunRecord {
        id: row.get(0)?,
        workspace_id: row.get(1)?,
        canonical_workspace_root: row.get(2)?,
        title: row.get(3)?,
        goal: row.get(4)?,
        status: row.get(5)?,
        revision: row.get(6)?,
        acceptance_json: row.get(7)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
        completed_at: row.get(10)?,
    })
}
