//! CodeBuddy typed Runtime 持久化；SQL、事务和 provider 固定值不离开 Store。
use super::*;
use crate::agent::codebuddy::recovery::{TerminationEvidence, valid_identity};

/// CB7 使用的 typed lifecycle；不接受任意 state 字符串或 evidence。
#[allow(dead_code, reason = "CB6-005 冻结持久化 primitive，CB7 才接启动")]
pub(crate) enum CodeBuddyRuntimeUpdate {
    PolicyVerified,
    ProcessStarted { pid: u32, start_token: String },
    Initialized,
    Terminating,
    Unknown,
}

/// 原 Runtime 所有已有 generic/private binding 必须一致；缺失 private 行不伪造身份。
fn binding_valid(c: &Connection, id: &str) -> Result<bool, String> {
    c.query_row("SELECT NOT EXISTS(SELECT 1 FROM executions e LEFT JOIN codebuddy_execution_state p ON p.execution_id=e.id WHERE (e.runtime_instance_id=?1 OR p.runtime_instance_id=?1) AND (e.provider!='codebuddy' OR e.runtime_instance_id IS NOT ?1 OR (p.execution_id IS NOT NULL AND p.runtime_instance_id IS NOT ?1)))",[id],|r|r.get(0)).map_err(|e|e.to_string())
}

impl StateStore {
    /// OS mutation 前检查原 execution/provider/R1 binding，orphan 同样适用。
    pub(crate) async fn codebuddy_runtime_binding_valid(&self, id: String) -> Result<bool, String> {
        let store = self.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let connection = store.connection.lock().map_err(|e| e.to_string())?;
            binding_valid(&connection, &id)
        })
        .await
        .map_err(|e| e.to_string())?
    }

    /// 固定 CodeBuddy provider、Windows containment 及 CB6-002 policy，拒绝 Job 名注入。
    #[allow(dead_code, reason = "CB7 接启动持久化")]
    pub(crate) async fn prepare_codebuddy_runtime(
        &self,
        id: String,
        owner: String,
        session: u32,
        executable: String,
        now: i64,
    ) -> Result<(), String> {
        if id.is_empty() || id.contains(['\\', '/', '\0']) || owner.is_empty() {
            return Err("CODEBUDDY_RUNTIME_IDENTITY_INVALID".into());
        }
        self.write(move |tx| {
            tx.execute("INSERT INTO runtime_instances(id,provider,owner_host_instance_id,job_name,job_session_id,job_creation_mode,job_handle_inheritable,job_kill_on_close,job_breakaway_allowed,executable_path,state,created_at,updated_at) VALUES (?1,'codebuddy',?2,?3,?4,'proc_thread_attribute_job_list',0,1,0,?5,'preparing',?6,?6)",params![id,owner,format!("Local\\SerenaDesktop.CodeBuddy.{id}"),session,executable,now]).map_err(|e|e.to_string())?;
            Ok(())
        }).await
    }

    /// 每种生命周期操作具有固定前置条件，不能覆盖另一 Provider 或已完成证据。
    pub(crate) async fn update_codebuddy_runtime(
        &self,
        id: String,
        update: CodeBuddyRuntimeUpdate,
        now: i64,
    ) -> Result<(), String> {
        self.write(move |tx| {
            let changed = match update {
                CodeBuddyRuntimeUpdate::PolicyVerified => tx.execute("UPDATE runtime_instances SET job_policy_verified_at=?2,updated_at=?2 WHERE id=?1 AND provider='codebuddy' AND state='preparing' AND job_policy_verified_at IS NULL AND job_creation_mode='proc_thread_attribute_job_list' AND job_handle_inheritable=0 AND job_kill_on_close=1 AND job_breakaway_allowed=0",params![id,now]),
                CodeBuddyRuntimeUpdate::ProcessStarted{pid,start_token} => tx.execute("UPDATE runtime_instances SET state='starting',process_id=?2,process_start_token=?3,started_at=?4,updated_at=?4 WHERE id=?1 AND provider='codebuddy' AND state='preparing' AND job_policy_verified_at IS NOT NULL",params![id,pid,start_token,now]),
                CodeBuddyRuntimeUpdate::Initialized => tx.execute("UPDATE runtime_instances SET state='running',updated_at=?2 WHERE id=?1 AND provider='codebuddy' AND state='starting' AND job_policy_verified_at IS NOT NULL",params![id,now]),
                CodeBuddyRuntimeUpdate::Terminating => tx.execute("UPDATE runtime_instances SET state='terminating',updated_at=?2 WHERE id=?1 AND provider='codebuddy' AND state!='terminated' AND termination_evidence_state!='complete'",params![id,now]),
                CodeBuddyRuntimeUpdate::Unknown => tx.execute("UPDATE runtime_instances SET state='unknown',updated_at=?2 WHERE id=?1 AND provider='codebuddy' AND termination_evidence_state!='complete'",params![id,now]),
            }.map_err(|e|e.to_string())?;
            if changed != 1 { return Err("CODEBUDDY_RUNTIME_STATE_CONFLICT".into()); }
            Ok(())
        }).await
    }

    /// 只消费 recovery sealed observation；在同一写事务重验原 ownership/policy 快照。
    pub(crate) async fn complete_codebuddy_runtime(
        &self,
        proof: TerminationEvidence,
    ) -> Result<(), String> {
        self.write(move |tx| {
            let r = proof.original();
            if !binding_valid(tx,&r.id)? {return Err("CODEBUDDY_RUNTIME_BINDING_CONFLICT".into());}
            valid_identity(r, r.job_session_id.ok_or("CODEBUDDY_SESSION_REQUIRED")? as u32)?;
            let changed = tx.execute("UPDATE runtime_instances SET state='terminated',stopped_at=?2,updated_at=?2,termination_evidence_state='complete',termination_evidence_type=?3,termination_evidence_at=?2,last_error_code=NULL,last_error_message=NULL WHERE id=?1 AND provider='codebuddy' AND owner_host_instance_id=?4 AND job_name=?5 AND job_session_id=?6 AND job_creation_mode=?7 AND job_handle_inheritable=0 AND job_kill_on_close=1 AND job_breakaway_allowed=0 AND job_policy_verified_at=?8 AND runtime_platform=?9 AND containment_type=?10 AND process_identity_scheme=?11 AND containment_process_group_id IS NULL AND containment_session_id IS NULL AND containment_verified_at IS NULL AND termination_evidence_state!='complete'",params![r.id,proof.at(),proof.kind(),r.owner_host_instance_id,r.job_name,r.job_session_id,r.job_creation_mode,r.job_policy_verified_at,r.runtime_platform,r.containment_type,r.process_identity_scheme]).map_err(|e|e.to_string())?;
            if changed != 1 { return Err("CODEBUDDY_RUNTIME_EVIDENCE_CONFLICT".into()); }
            Ok(())
        }).await
    }
}
