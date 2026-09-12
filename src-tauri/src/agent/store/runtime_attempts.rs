use super::*;

// One authority: explicit reservations plus the historical origin-ID contract.
pub(super) fn runtime_attempt_exists(c: &Connection, id: &str) -> rusqlite::Result<bool> {
    c.query_row(
        "SELECT EXISTS(SELECT 1 FROM execution_runtime_attempts WHERE execution_id=?1)
        OR EXISTS(SELECT 1 FROM runtime_instances WHERE id='runtime-' || ?1)",
        [id],
        |r| r.get(0),
    )
}

impl StateStore {
    pub(crate) async fn has_runtime_attempt(&self, id: String) -> Result<bool, String> {
        self.read(move |c| runtime_attempt_exists(c, &id)).await
    }
    pub(crate) async fn reserve_runtime_attempt(
        &self,
        id: String,
        runtime_id: String,
        now: i64,
    ) -> Result<(), String> {
        self.write(move |tx| {
            let row = execution_record(tx, &id).map_err(|e| e.to_string())?.ok_or("EXECUTION_NOT_FOUND")?;
            if row.status != "dispatch_pending" || row.dispatch_state != "not_dispatched"
                || row.runtime_instance_id.is_some() || runtime_attempt_exists(tx, &id).map_err(|e| e.to_string())? {
                return Err("PENDING_RESUME_REJECTED".into());
            }
            transactions::owns_claim(tx, &id)?;
            tx.execute("INSERT INTO execution_runtime_attempts(execution_id,runtime_instance_id,created_at) VALUES (?1,?2,?3)",params![id,runtime_id,now]).map_err(|e| e.to_string())?;
            Ok(())
        }).await
    }
    pub(crate) async fn reserve_recovery_attempt(
        &self,
        id: String,
        runtime_id: String,
        now: i64,
    ) -> Result<(), String> {
        self.write(move |tx| {
            let row = execution_record(tx, &id).map_err(|e| e.to_string())?.ok_or("EXECUTION_NOT_FOUND")?;
            if row.status != "reconciling" { return Err("RECOVERY_STATE_REQUIRED".into()); }
            tx.execute("INSERT INTO execution_runtime_attempts(execution_id,runtime_instance_id,created_at) VALUES (?1,?2,?3)",params![id,runtime_id,now]).map_err(|e| e.to_string())?;
            Ok(())
        }).await
    }
    pub(crate) async fn runtime_workspace(&self, id: String) -> Result<Option<String>, String> {
        self.read(move |c| c.query_row("SELECT canonical_workspace_root FROM executions e WHERE
            e.runtime_instance_id=?1 OR 'runtime-' || e.id=?1 OR EXISTS(
                SELECT 1 FROM execution_runtime_attempts a WHERE a.execution_id=e.id AND a.runtime_instance_id=?1)
            LIMIT 1", [id], |r| r.get(0)).optional()).await
    }
}
