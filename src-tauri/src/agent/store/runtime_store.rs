//! Called only on the Runtime blocking worker. SQL never leaves StateStore.
use super::*;
use crate::agent::codex::runtime::{RuntimeError, TerminationEvidence};

impl StateStore {
    pub(in crate::agent) fn runtime_initialized(&self, id: &str, version: &str, schema: &str, now: i64) -> Result<(), RuntimeError> {
        self.runtime_write(|tx| tx.execute("UPDATE runtime_instances SET state='running',codex_version=?2,protocol_schema_sha256=?3,updated_at=?4 WHERE id=?1 AND state='starting' AND job_policy_verified_at IS NOT NULL", params![id,version,schema,now]))
    }
    fn runtime_write(
        &self,
        op: impl FnOnce(&Transaction<'_>) -> Result<usize, rusqlite::Error>,
    ) -> Result<(), RuntimeError> {
        let mut c = self
            .connection
            .lock()
            .map_err(|e| RuntimeError::new("CODEX_RUNTIME_STORE_FAILED", e.to_string()))?;
        let tx = c
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| RuntimeError::new("CODEX_RUNTIME_STORE_FAILED", e.to_string()))?;
        if op(&tx).map_err(|e| RuntimeError::new("CODEX_RUNTIME_STORE_FAILED", e.to_string()))? != 1
        {
            return Err(RuntimeError::new(
                "CODEX_RUNTIME_STATE_CONFLICT",
                "Runtime update did not affect exactly one row",
            ));
        }
        tx.commit()
            .map_err(|e| RuntimeError::new("CODEX_RUNTIME_STORE_FAILED", e.to_string()))
    }

    pub(in crate::agent) fn prepare_runtime(
        &self,
        id: &str,
        owner: &str,
        job: &str,
        session: u32,
        exe: &str,
        now: i64,
    ) -> Result<(), RuntimeError> {
        self.runtime_write(|tx| {
            tx.execute(
                "INSERT INTO runtime_instances
            (id,owner_host_instance_id,job_name,job_session_id,job_creation_mode,
             job_handle_inheritable,job_kill_on_close,job_breakaway_allowed,
             codex_executable_path,state,created_at,updated_at)
            VALUES (?1,?2,?3,?4,'proc_thread_attribute_job_list',0,1,0,?5,'preparing',?6,?6)",
                params![id, owner, job, session, exe, now],
            )
        })
    }
    pub(in crate::agent) fn verify_runtime_policy(
        &self,
        id: &str,
        now: i64,
    ) -> Result<(), RuntimeError> {
        self.runtime_write(|tx| {
            tx.execute(
                "UPDATE runtime_instances SET job_policy_verified_at=?2,updated_at=?2
            WHERE id=?1 AND state='preparing' AND job_policy_verified_at IS NULL",
                params![id, now],
            )
        })
    }
    pub(in crate::agent) fn start_runtime(
        &self,
        id: &str,
        pid: u32,
        token: &str,
        now: i64,
    ) -> Result<(), RuntimeError> {
        self.runtime_write(|tx| tx.execute("UPDATE runtime_instances SET state='starting',codex_pid=?2,
            codex_process_start_token=?3,started_at=?4,updated_at=?4 WHERE id=?1 AND state='preparing'
            AND job_policy_verified_at IS NOT NULL",params![id,pid,token,now]))
    }
    pub(in crate::agent) fn runtime_terminating(
        &self,
        id: &str,
        now: i64,
    ) -> Result<(), RuntimeError> {
        self.runtime_write(|tx| {
            tx.execute(
                "UPDATE runtime_instances SET state='terminating',updated_at=?2
            WHERE id=?1 AND state != 'terminated'",
                params![id, now],
            )
        })
    }
    pub(in crate::agent) fn runtime_unknown(
        &self,
        id: &str,
        error: &RuntimeError,
        now: i64,
    ) -> Result<(), RuntimeError> {
        self.runtime_write(|tx| {
            tx.execute(
                "UPDATE runtime_instances SET state='unknown',last_error_code=?2,
            last_error_message=?3,updated_at=?4 WHERE id=?1 AND termination_evidence_state != 'complete'",
                params![id, error.code, error.message, now],
            )
        })
    }
    // A caller cannot construct this proof; only Runtime's Job observations do so.
    pub(in crate::agent) fn complete_runtime(
        &self,
        evidence: &TerminationEvidence,
    ) -> Result<(), RuntimeError> {
        self.runtime_write(|tx| tx.execute("UPDATE runtime_instances SET state='terminated',stopped_at=?3,
            termination_evidence_type=?2,termination_evidence_at=?3,termination_evidence_state='complete',
            last_error_code=NULL,last_error_message=NULL,updated_at=?3 WHERE id=?1 AND state != 'terminated'",params![evidence.id(),evidence.kind(),evidence.at()]))
    }
}
