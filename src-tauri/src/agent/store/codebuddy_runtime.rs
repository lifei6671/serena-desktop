//! CodeBuddy typed Runtime 持久化；SQL、事务和 provider 固定值不离开 Store。
use super::*;
use crate::agent::codebuddy::recovery::{TerminationEvidence, valid_identity};

/// CB7 使用的 typed lifecycle；不接受任意 state 字符串或 evidence。
#[allow(dead_code, reason = "CB6-005 冻结持久化 primitive，CB7 才接启动")]
pub(crate) enum CodeBuddyRuntimeUpdate {
    PolicyVerified,
    ProcessStarted {
        pid: u32,
        start_token: String,
    },
    #[cfg(target_os = "macos")]
    MacosProcessStarted {
        pid: u32,
        start_token: String,
    },
    Initialized,
    Terminating,
    Unknown,
}

/// 原 Runtime 所有已有 generic/private binding 必须一致；缺失 private 行不伪造身份。
fn binding_valid(c: &Connection, id: &str) -> Result<bool, String> {
    c.query_row("SELECT NOT EXISTS(SELECT 1 FROM executions e LEFT JOIN codebuddy_execution_state p ON p.execution_id=e.id WHERE (e.runtime_instance_id=?1 OR p.runtime_instance_id=?1) AND (e.provider!='codebuddy' OR e.runtime_instance_id IS NOT ?1 OR (p.execution_id IS NOT NULL AND p.runtime_instance_id IS NOT ?1))) AND NOT EXISTS(SELECT 1 FROM codebuddy_execution_state p LEFT JOIN executions e ON e.id=p.execution_id WHERE p.recovery_runtime_instance_id=?1 AND (e.id IS NULL OR e.provider!='codebuddy' OR e.runtime_instance_id IS NOT p.runtime_instance_id OR NOT EXISTS(SELECT 1 FROM execution_runtime_attempts a WHERE a.execution_id=e.id AND a.runtime_instance_id=?1)))",[id],|r|r.get(0)).map_err(|e|e.to_string())
}

impl StateStore {
    /// Fresh preparation 仅绑定已预留的 R1；不能借用 Dispatch 转换提前声明 prompt side effect。
    #[cfg(any(windows, target_os = "macos"))]
    pub(crate) async fn bind_codebuddy_prepared_runtime(
        &self,
        execution_id: String,
        expected_revision: i64,
        runtime_id: String,
        now: i64,
    ) -> Result<(), String> {
        self.write(move |tx| {
            let row = execution_record(tx, &execution_id)
                .map_err(|e| e.to_string())?.ok_or("EXECUTION_NOT_FOUND")?;
            if row.provider != "codebuddy" || row.revision != expected_revision
                || row.runtime_instance_id.is_some() || row.status != "dispatch_pending"
                || row.dispatch_state != "not_dispatched" {
                return Err("CODEBUDDY_PREPARATION_BINDING_CONFLICT".into());
            }
            transactions::owns_claim(tx, &execution_id)?;
            let reserved: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM execution_runtime_attempts a JOIN runtime_instances r ON r.id=a.runtime_instance_id WHERE a.execution_id=?1 AND r.id=?2 AND r.provider='codebuddy' AND r.state='preparing')",
                params![execution_id,runtime_id], |r| r.get(0)).map_err(|e|e.to_string())?;
            if !reserved { return Err("CODEBUDDY_RUNTIME_RESERVATION_REQUIRED".into()); }
            tx.execute("UPDATE executions SET runtime_instance_id=?2,revision=revision+1,updated_at=?3 WHERE id=?1 AND revision=?4",
                params![execution_id,runtime_id,now,expected_revision]).map_err(|e|e.to_string())?;
            Ok(())
        }).await
    }

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

    /// macOS preparing 只保存本平台身份类型，不写入任何 Windows Job 字段。
    #[cfg(target_os = "macos")]
    pub(crate) async fn prepare_codebuddy_macos_runtime(
        &self,
        id: String,
        owner: String,
        executable: String,
        now: i64,
    ) -> Result<(), String> {
        if id.is_empty() || id.contains(['\\', '/', '\0']) || owner.is_empty() {
            return Err("CODEBUDDY_RUNTIME_IDENTITY_INVALID".into());
        }
        self.write(move |tx| {
            tx.execute("INSERT INTO runtime_instances(id,provider,owner_host_instance_id,executable_path,state,created_at,updated_at,runtime_platform,containment_type,process_identity_scheme) VALUES (?1,'codebuddy',?2,?3,'preparing',?4,?4,'macos','macos_process_group','darwin_proc_bsd_start_v1')",params![id,owner,executable,now]).map_err(|e|e.to_string())?;
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
                #[cfg(target_os = "macos")]
                CodeBuddyRuntimeUpdate::MacosProcessStarted{pid,start_token} => {
                    if pid == 0 || pid > i32::MAX as u32 || crate::agent::codex::macos_launcher::ProcessStartToken::decode(&start_token).is_err() {
                        return Err("CODEBUDDY_RUNTIME_IDENTITY_INVALID".into());
                    }
                    tx.execute("UPDATE runtime_instances SET state='starting',process_id=?2,process_start_token=?3,containment_process_group_id=?2,containment_session_id=?2,containment_verified_at=?4,started_at=?4,updated_at=?4 WHERE id=?1 AND provider='codebuddy' AND state='preparing' AND runtime_platform='macos' AND containment_type='macos_process_group' AND process_identity_scheme='darwin_proc_bsd_start_v1' AND job_name IS NULL AND job_session_id IS NULL AND job_creation_mode IS NULL AND job_handle_inheritable IS NULL AND job_kill_on_close IS NULL AND job_breakaway_allowed IS NULL AND job_policy_verified_at IS NULL",params![id,pid,start_token,now])
                },
                CodeBuddyRuntimeUpdate::Initialized => tx.execute("UPDATE runtime_instances SET state='running',updated_at=?2 WHERE id=?1 AND provider='codebuddy' AND state='starting' AND ((runtime_platform='windows' AND job_policy_verified_at IS NOT NULL) OR (runtime_platform='macos' AND containment_verified_at IS NOT NULL AND process_id=containment_process_group_id AND process_id=containment_session_id))",params![id,now]),
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
            #[cfg(target_os = "macos")]
            if r.runtime_platform == "macos" {
                crate::agent::codebuddy::macos_recovery::valid_identity(r)?;
                let changed = tx.execute("UPDATE runtime_instances SET state='terminated',stopped_at=?2,updated_at=?2,termination_evidence_state='complete',termination_evidence_type=?3,termination_evidence_at=?2,last_error_code=NULL,last_error_message=NULL WHERE id=?1 AND provider='codebuddy' AND owner_host_instance_id=?4 AND runtime_platform='macos' AND containment_type='macos_process_group' AND process_identity_scheme='darwin_proc_bsd_start_v1' AND process_id=?5 AND process_start_token=?6 AND containment_process_group_id=?7 AND containment_session_id=?8 AND containment_verified_at=?9 AND job_name IS NULL AND job_session_id IS NULL AND job_creation_mode IS NULL AND job_handle_inheritable IS NULL AND job_kill_on_close IS NULL AND job_breakaway_allowed IS NULL AND job_policy_verified_at IS NULL AND termination_evidence_state!='complete'",params![r.id,proof.at(),proof.kind(),r.owner_host_instance_id,r.codex_pid,r.codex_process_start_token,r.containment_process_group_id,r.containment_session_id,r.containment_verified_at]).map_err(|e|e.to_string())?;
                if changed != 1 { return Err("CODEBUDDY_RUNTIME_EVIDENCE_CONFLICT".into()); }
                return Ok(());
            }
            valid_identity(r, r.job_session_id.ok_or("CODEBUDDY_SESSION_REQUIRED")? as u32)?;
            let changed = tx.execute("UPDATE runtime_instances SET state='terminated',stopped_at=?2,updated_at=?2,termination_evidence_state='complete',termination_evidence_type=?3,termination_evidence_at=?2,last_error_code=NULL,last_error_message=NULL WHERE id=?1 AND provider='codebuddy' AND owner_host_instance_id=?4 AND job_name=?5 AND job_session_id=?6 AND job_creation_mode=?7 AND job_handle_inheritable=0 AND job_kill_on_close=1 AND job_breakaway_allowed=0 AND job_policy_verified_at=?8 AND runtime_platform=?9 AND containment_type=?10 AND process_identity_scheme=?11 AND containment_process_group_id IS NULL AND containment_session_id IS NULL AND containment_verified_at IS NULL AND termination_evidence_state!='complete'",params![r.id,proof.at(),proof.kind(),r.owner_host_instance_id,r.job_name,r.job_session_id,r.job_creation_mode,r.job_policy_verified_at,r.runtime_platform,r.containment_type,r.process_identity_scheme]).map_err(|e|e.to_string())?;
            if changed != 1 { return Err("CODEBUDDY_RUNTIME_EVIDENCE_CONFLICT".into()); }
            Ok(())
        }).await
    }
}

/// 在 R1/R2 检查事务内确认 macOS 原身份与 group-empty evidence；不接受混合 Job 字段。
#[cfg(target_os = "macos")]
pub(super) fn complete_macos_runtime(c: &Connection, id: &str) -> Result<bool, String> {
    let token: Option<String> = c.query_row(
        "SELECT process_start_token FROM runtime_instances WHERE id=?1 AND provider='codebuddy'
         AND owner_host_instance_id!='' AND state='terminated' AND termination_evidence_state='complete'
         AND termination_evidence_type IN ('macos_live_process_group_empty','macos_recovered_process_group_empty')
         AND termination_evidence_at IS NOT NULL AND runtime_platform='macos'
         AND containment_type='macos_process_group' AND process_identity_scheme='darwin_proc_bsd_start_v1'
         AND process_id>0 AND process_id<=2147483647 AND containment_process_group_id=process_id
         AND containment_session_id=process_id AND containment_verified_at IS NOT NULL
         AND job_name IS NULL AND job_session_id IS NULL AND job_creation_mode IS NULL
         AND job_handle_inheritable IS NULL AND job_kill_on_close IS NULL
         AND job_breakaway_allowed IS NULL AND job_policy_verified_at IS NULL",
        [id], |row| row.get(0)).optional().map_err(|error|error.to_string())?.flatten();
    Ok(token.is_some_and(|token| {
        crate::agent::codex::macos_launcher::ProcessStartToken::decode(&token).is_ok()
    }) && binding_valid(c, id)?)
}
