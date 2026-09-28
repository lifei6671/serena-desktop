//! SQL ownership 留在 StateStore 子模块；对 Adapter 只暴露 typed 数据。

use super::*;
use crate::agent::codebuddy::store::{
    InspectionOutcome, Mutation, Ownership, PrivateState, PromptRpcId, PromptState, RecoveryState,
    new_conversation_id, valid_conversation_id,
};
use serde::{Serialize, de::DeserializeOwned};

/// 枚举按稳定字符串落盘；未知值视为损坏，不回退为默认状态。
fn decode<T: DeserializeOwned>(value: String) -> rusqlite::Result<T> {
    serde_json::from_value(serde_json::Value::String(value))
        .map_err(|_| rusqlite::Error::InvalidQuery)
}

/// 私有枚举都序列化为字符串，其他形状立即失败。
fn encode<T: Serialize>(value: &T) -> Result<String, String> {
    serde_json::to_value(value)
        .map_err(|e| e.to_string())?
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| "invalid private enum".into())
}

/// 读取原值，不由历史正文、时间或 generic lifecycle 补充字段。
fn load(c: &Connection, id: &str) -> rusqlite::Result<PrivateState> {
    c.query_row("SELECT execution_id,runtime_instance_id,acp_protocol_version,session_id,
        conversation_request_id,provider_request_id,provider_request_id_source,prompt_rpc_id,
        prompt_state,terminal_stop_reason,terminal_observed_at,recovery_method,recovery_state,
        recovery_runtime_instance_id,recovery_started_at,recovery_finished_at,revision,created_at,updated_at
        FROM codebuddy_execution_state WHERE execution_id=?1", [id], |r| {
        let rpc: Option<String> = r.get(7)?;
        let stop: Option<String> = r.get(9)?;
        Ok(PrivateState {
            execution_id:r.get(0)?, runtime_instance_id:r.get(1)?, acp_protocol_version:r.get(2)?,
            session_id:r.get(3)?, conversation_request_id:r.get(4)?, provider_request_id:r.get(5)?,
            provider_request_id_source:r.get(6)?,
            prompt_rpc_id:rpc.map(|s| PromptRpcId::from_json(&s).map_err(|_| rusqlite::Error::InvalidQuery)).transpose()?,
            prompt_state:decode(r.get(8)?)?, terminal_stop_reason:stop.map(decode).transpose()?,
            terminal_observed_at:r.get(10)?, recovery_method:r.get(11)?, recovery_state:decode(r.get(12)?)?,
            recovery_runtime_instance_id:r.get(13)?, recovery_started_at:r.get(14)?, recovery_finished_at:r.get(15)?,
            revision:r.get(16)?, created_at:r.get(17)?, updated_at:r.get(18)?,
        })
    })
}

/// 即便 DB 被外部绕过 CHECK 修改，读写也不得使用损坏的 private authority。
fn validate(s: &PrivateState) -> Result<(), String> {
    let terminal = s.prompt_state == PromptState::TerminalObserved;
    let source_ok = match (&s.provider_request_id, &s.provider_request_id_source) {
        (None, None) => true,
        (Some(id), Some(source)) => !id.is_empty() && source == "exact_provider_observation",
        _ => false,
    };
    let recovery_ok = match s.recovery_state {
        RecoveryState::NotAttempted => {
            s.recovery_method.is_none()
                && s.recovery_runtime_instance_id.is_none()
                && s.recovery_started_at.is_none()
                && s.recovery_finished_at.is_none()
        }
        state => {
            let continuation_load = s.recovery_runtime_instance_id == s.runtime_instance_id
                && matches!(state, RecoveryState::Inspecting | RecoveryState::Partial)
                && s.prompt_state != PromptState::Prepared
                && s.session_id.is_some();
            let result_inspection = s.recovery_runtime_instance_id != s.runtime_instance_id
                && s.prompt_state != PromptState::Prepared;
            s.recovery_method.as_deref() == Some("session/load")
                && s.recovery_runtime_instance_id.is_some()
                && s.recovery_started_at.is_some()
                && (continuation_load || result_inspection)
                && ((state == RecoveryState::Inspecting) == s.recovery_finished_at.is_none())
        }
    };
    if s.revision < 0
        || !source_ok
        || !recovery_ok
        || s.session_id.as_ref().is_some_and(|id| id.is_empty())
        || !valid_conversation_id(&s.conversation_request_id)
        || (s.prompt_state != PromptState::Prepared
            && (s.runtime_instance_id.is_none()
                || s.acp_protocol_version.is_none()
                || s.session_id.is_none()))
        || (s.prompt_state == PromptState::Prepared && s.prompt_rpc_id.is_some())
        || (terminal && (s.terminal_stop_reason.is_none() || s.terminal_observed_at.is_none()))
        || (!terminal && (s.terminal_stop_reason.is_some() || s.terminal_observed_at.is_some()))
    {
        return Err("corrupt CodeBuddy private state".into());
    }
    Ok(())
}

/// FK 只证明存在；每次仍检查 Runtime 属于 CodeBuddy。
fn runtime_owner(c: &Connection, id: &str) -> Result<(), String> {
    let provider: String = c
        .query_row(
            "SELECT provider FROM runtime_instances WHERE id=?1",
            [id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if provider != "codebuddy" {
        return Err("CodeBuddy runtime provider mismatch".into());
    }
    Ok(())
}

/// IMMEDIATE 事务内同时校验 generic revision/provider/binding，不信任 FK 代替 ownership。
fn ownership(
    c: &Connection,
    id: &str,
    expected: Option<&Ownership>,
) -> Result<Option<String>, String> {
    let (provider, revision, runtime): (String, i64, Option<String>) = c
        .query_row(
            "SELECT provider,revision,runtime_instance_id FROM executions WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(|e| e.to_string())?;
    if provider != "codebuddy" {
        return Err("CodeBuddy execution provider mismatch".into());
    }
    if expected
        .is_some_and(|e| e.execution_revision != revision || e.runtime_instance_id != runtime)
    {
        return Err("CodeBuddy generic ownership conflict; reread execution".into());
    }
    if let Some(id) = &runtime {
        runtime_owner(c, id)?;
    }
    Ok(runtime)
}

/// 私有 R1 不得被重绑；R2 只被校验，不写回 generic binding。
fn private_ownership(
    c: &Connection,
    s: &PrivateState,
    binding: &Option<String>,
    allow_bind: bool,
) -> Result<(), String> {
    if s.runtime_instance_id != *binding && !(allow_bind && s.runtime_instance_id.is_none()) {
        return Err("CodeBuddy immutable R1 mismatch".into());
    }
    if let Some(id) = &s.runtime_instance_id {
        runtime_owner(c, id)?;
    }
    if let Some(id) = &s.recovery_runtime_instance_id {
        runtime_owner(c, id)?;
        if Some(id) == s.runtime_instance_id.as_ref()
            && !matches!(
                s.recovery_state,
                RecoveryState::Inspecting | RecoveryState::Partial
            )
        {
            return Err("inspection requires separate R2".into());
        }
    }
    Ok(())
}

impl StateStore {
    /// 单一 IMMEDIATE 事务保证 R2 attempt/runtime/private provenance 不出现可重试的中间状态。
    #[allow(
        clippy::too_many_arguments,
        reason = "durable R1/R2 identity tuple is intentional"
    )]
    pub(crate) async fn begin_codebuddy_result_inspection(
        &self,
        expected: PrivateState,
        expected_ownership: Ownership,
        recovery_runtime_instance_id: String,
        owner: String,
        session: u32,
        executable: String,
    ) -> Result<PrivateState, String> {
        if recovery_runtime_instance_id.is_empty()
            || recovery_runtime_instance_id.contains(['\\', '/', '\0'])
            || owner.is_empty()
        {
            return Err("CODEBUDDY_RUNTIME_IDENTITY_INVALID".into());
        }
        let connection = self.connection.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let mut connection = connection.lock().map_err(|e| e.to_string())?;
            let tx = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(|e| e.to_string())?;
            let row = execution_record(&tx, &expected.execution_id)
                .map_err(|e| e.to_string())?
                .ok_or("EXECUTION_NOT_FOUND")?;
            let binding = ownership(&tx, &expected.execution_id, Some(&expected_ownership))?;
            let mut state = load(&tx, &expected.execution_id).map_err(|e| e.to_string())?;
            validate(&state)?;
            private_ownership(&tx, &state, &binding, false)?;
            if state != expected || row.status != "reconciling" {
                return Err("CodeBuddy inspection ownership conflict".into());
            }
            transactions::owns_claim(&tx, &expected.execution_id)?;
            let r1 = binding.as_deref().ok_or("RUNTIME_EVIDENCE_REQUIRED")?;
            let r1_approved: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM runtime_instances WHERE id=?1 AND provider='codebuddy'
                     AND state='terminated' AND termination_evidence_state='complete'
                     AND termination_evidence_type IN ('job_active_processes_zero','managed_job_destroyed')
                     AND termination_evidence_at IS NOT NULL AND job_name=?2 AND job_session_id=?3
                     AND job_creation_mode='proc_thread_attribute_job_list' AND job_handle_inheritable=0
                     AND job_kill_on_close=1 AND job_breakaway_allowed=0 AND job_policy_verified_at IS NOT NULL
                     AND runtime_platform='windows' AND containment_type='windows_job'
                     AND process_identity_scheme='windows_filetime_v1')",
                    params![r1, format!("Local\\SerenaDesktop.CodeBuddy.{r1}"), i64::from(session)],
                    |row| row.get(0),
                )
                .map_err(|e| e.to_string())?;
            if !r1_approved {
                return Err("RUNTIME_EVIDENCE_REQUIRED".into());
            }
            let now = chrono::Utc::now().timestamp_millis();
            tx.execute(
                "INSERT INTO execution_runtime_attempts(execution_id,runtime_instance_id,created_at)
                 VALUES (?1,?2,?3)",
                params![expected.execution_id, recovery_runtime_instance_id, now],
            )
            .map_err(|e| e.to_string())?;
            tx.execute(
                "INSERT INTO runtime_instances(id,provider,owner_host_instance_id,job_name,job_session_id,
                 job_creation_mode,job_handle_inheritable,job_kill_on_close,job_breakaway_allowed,
                 executable_path,state,created_at,updated_at)
                 VALUES (?1,'codebuddy',?2,?3,?4,'proc_thread_attribute_job_list',0,1,0,?5,'preparing',?6,?6)",
                params![
                    recovery_runtime_instance_id,
                    owner,
                    format!("Local\\SerenaDesktop.CodeBuddy.{recovery_runtime_instance_id}"),
                    i64::from(session),
                    executable,
                    now
                ],
            )
            .map_err(|e| e.to_string())?;
            apply(
                &mut state,
                Mutation::BeginInspection { recovery_runtime_instance_id },
                binding.clone(),
                now,
            )?;
            validate(&state)?;
            private_ownership(&tx, &state, &binding, false)?;
            let next_revision = state.revision.checked_add(1).ok_or("CodeBuddy revision overflow")?;
            let changed = tx.execute(
                "UPDATE codebuddy_execution_state SET recovery_method='session/load',
                 recovery_state='inspecting',recovery_runtime_instance_id=?2,recovery_started_at=?3,
                 recovery_finished_at=NULL,revision=?4,updated_at=?3
                 WHERE execution_id=?1 AND revision=?5",
                params![state.execution_id,state.recovery_runtime_instance_id,now,next_revision,state.revision]
            ).map_err(|e| e.to_string())?;
            if changed != 1 {
                return Err("CodeBuddy inspection revision conflict".into());
            }
            let result = load(&tx, &state.execution_id).map_err(|e| e.to_string())?;
            tx.commit().map_err(|e| e.to_string())?;
            Ok(result)
        })
        .await
        .map_err(|e| e.to_string())?
    }

    /// R2 complete evidence、private outcome 与 generic partial/unknown result 同一事务提交。
    pub(crate) async fn finish_codebuddy_result_inspection(
        &self,
        expected: PrivateState,
        expected_ownership: Ownership,
        outcome: InspectionOutcome,
        result: Option<serde_json::Value>,
    ) -> Result<PrivateState, String> {
        let connection = self.connection.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let mut connection = connection.lock().map_err(|e| e.to_string())?;
            let tx = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(|e| e.to_string())?;
            let row = execution_record(&tx, &expected.execution_id)
                .map_err(|e| e.to_string())?
                .ok_or("EXECUTION_NOT_FOUND")?;
            let binding = ownership(&tx, &expected.execution_id, Some(&expected_ownership))?;
            let mut state = load(&tx, &expected.execution_id).map_err(|e| e.to_string())?;
            validate(&state)?;
            private_ownership(&tx, &state, &binding, false)?;
            if state != expected || row.status != "reconciling" || row.provider_terminal_status.is_some() {
                return Err("CodeBuddy inspection finish ownership conflict".into());
            }
            transactions::owns_claim(&tx, &expected.execution_id)?;
            let r2 = state.recovery_runtime_instance_id.as_deref().ok_or("CODEBUDDY_RECOVERY_RUNTIME_REQUIRED")?;
            let r2_approved: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM runtime_instances WHERE id=?1 AND provider='codebuddy'
                 AND state='terminated' AND termination_evidence_state='complete'
                 AND termination_evidence_type IN ('job_active_processes_zero','managed_job_destroyed')
                 AND termination_evidence_at IS NOT NULL AND job_name=?2
                 AND job_creation_mode='proc_thread_attribute_job_list' AND job_handle_inheritable=0
                 AND job_kill_on_close=1 AND job_breakaway_allowed=0 AND job_policy_verified_at IS NOT NULL
                 AND runtime_platform='windows' AND containment_type='windows_job'
                 AND process_identity_scheme='windows_filetime_v1')",
                params![r2,format!("Local\\SerenaDesktop.CodeBuddy.{r2}")],|row|row.get(0)
            ).map_err(|e| e.to_string())?;
            if !r2_approved {
                return Err("CODEBUDDY_RECOVERY_RUNTIME_EVIDENCE_REQUIRED".into());
            }
            let (serialized, completeness) = match (&outcome, &result) {
                (InspectionOutcome::Partial, Some(value))
                    if value.get("text").and_then(serde_json::Value::as_str)
                        .is_some_and(|text| !text.is_empty() && text.len() <= 256 * 1024) =>
                {
                    (Some(value.to_string()), "partial")
                }
                (InspectionOutcome::Unknown | InspectionOutcome::MaterialDifference, None) => (None, "unknown"),
                _ => return Err("CODEBUDDY_RECOVERY_RESULT_INVALID".into()),
            };
            let now = chrono::Utc::now().timestamp_millis();
            apply(&mut state, Mutation::FinishInspection { outcome }, binding.clone(), now)?;
            validate(&state)?;
            private_ownership(&tx, &state, &binding, false)?;
            let next_private_revision = state.revision.checked_add(1).ok_or("CodeBuddy revision overflow")?;
            let private_changed = tx.execute(
                "UPDATE codebuddy_execution_state SET recovery_state=?2,recovery_finished_at=?3,
                 revision=?4,updated_at=?3 WHERE execution_id=?1 AND revision=?5",
                params![state.execution_id,encode(&state.recovery_state)?,now,next_private_revision,state.revision]
            ).map_err(|e| e.to_string())?;
            let execution_changed = tx.execute(
                "UPDATE executions SET final_result_json=?2,result_completeness=?3,
                 revision=revision+1,updated_at=?4 WHERE id=?1 AND revision=?5
                 AND status='reconciling' AND provider_terminal_status IS NULL",
                params![state.execution_id,serialized,completeness,now,expected_ownership.execution_revision]
            ).map_err(|e| e.to_string())?;
            if private_changed != 1 || execution_changed != 1 {
                return Err("CodeBuddy inspection finish revision conflict".into());
            }
            let result = load(&tx, &state.execution_id).map_err(|e| e.to_string())?;
            tx.commit().map_err(|e| e.to_string())?;
            Ok(result)
        })
        .await
        .map_err(|e| e.to_string())?
    }

    /// Provider validation 只认 exact terminal private identity；缺行或不合格不推造 Session。
    pub(crate) async fn read_codebuddy_continuation_source(
        &self,
        id: String,
    ) -> Result<Option<PrivateState>, String> {
        let connection = self.connection.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let mut connection = connection.lock().map_err(|e| e.to_string())?;
            let tx = connection.transaction().map_err(|e| e.to_string())?;
            let Some(row) = execution_record(&tx, &id).map_err(|e| e.to_string())? else {
                return Ok(None);
            };
            if row.provider != "codebuddy" {
                return Ok(None);
            }
            let Some(state) = load(&tx, &id).optional().map_err(|e| e.to_string())? else {
                return Ok(None);
            };
            validate(&state)?;
            let binding = ownership(&tx, &id, None)?;
            private_ownership(&tx, &state, &binding, false)?;
            let result = continuation_source_eligible(&state).then_some(state);
            tx.commit().map_err(|e| e.to_string())?;
            Ok(result)
        })
        .await
        .map_err(|e| e.to_string())?
    }

    /// child dispatch 前再次原子校验 generic lineage；任何漂移都不允许进入 session/load。
    pub(crate) async fn read_codebuddy_continuation_lineage(
        &self,
        child_id: String,
        source_id: String,
    ) -> Result<Option<(ExecutionRecord, PrivateState)>, String> {
        let connection = self.connection.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let mut connection = connection.lock().map_err(|e| e.to_string())?;
            let tx = connection.transaction().map_err(|e| e.to_string())?;
            let Some(child) = execution_record(&tx, &child_id).map_err(|e| e.to_string())? else {
                return Ok(None);
            };
            let Some(source) = execution_record(&tx, &source_id).map_err(|e| e.to_string())? else {
                return Ok(None);
            };
            let child_role: String = tx
                .query_row(
                    "SELECT task_role FROM executions WHERE id=?1",
                    [&child_id],
                    |row| row.get(0),
                )
                .map_err(|e| e.to_string())?;
            let source_role: String = tx
                .query_row(
                    "SELECT task_role FROM executions WHERE id=?1",
                    [&source_id],
                    |row| row.get(0),
                )
                .map_err(|e| e.to_string())?;
            if child.provider != "codebuddy"
                || source.provider != "codebuddy"
                || child.parent_execution_id.as_deref() != Some(source_id.as_str())
                || child.status != "dispatch_pending"
                || child.dispatch_state != "not_dispatched"
                || child.runtime_instance_id.is_some()
                || !super::transactions::product::continuation_core_eligible(&source)
                || child.agent_id != source.agent_id
                || child.workspace_id != source.workspace_id
                || child.canonical_workspace_root != source.canonical_workspace_root
                || child.workspace_generation != source.workspace_generation
                || child.mode != source.mode
                || child.execution_profile_json != source.execution_profile_json
                || child_role != source_role
            {
                return Ok(None);
            }
            let Some(state) = load(&tx, &source_id)
                .optional()
                .map_err(|e| e.to_string())?
            else {
                return Ok(None);
            };
            validate(&state)?;
            let binding = ownership(&tx, &source_id, None)?;
            private_ownership(&tx, &state, &binding, false)?;
            if !continuation_source_eligible(&state) {
                return Ok(None);
            }
            tx.commit().map_err(|e| e.to_string())?;
            Ok(Some((child, state)))
        })
        .await
        .map_err(|e| e.to_string())?
    }

    /// exact response 在同一事务内读取 generic ownership 并冻结 private terminal。
    /// 用户 cancel 只改变 generic revision，不能使已收到的可靠终态失效；private OCC 仍严格保留。
    pub(crate) async fn commit_codebuddy_prompt_response(
        &self,
        expected: PrivateState,
        provider_request_id: Option<String>,
        stop_reason: agent_client_protocol::schema::v1::StopReason,
        observed_at: i64,
    ) -> Result<PrivateState, String> {
        self.write(move |tx| {
            let id = &expected.execution_id;
            let row = execution_record(tx, id).map_err(|e| e.to_string())?.ok_or("EXECUTION_NOT_FOUND")?;
            let binding = ownership(tx, id, None)?;
            let mut s = load(tx, id).map_err(|e| e.to_string())?;
            validate(&s)?;
            private_ownership(tx, &s, &binding, false)?;
            if s != expected || s.prompt_state != PromptState::Sent
                || binding.is_none() || row.dispatch_state != "dispatched"
                || !matches!(row.status.as_str(), "running" | "cancel_requested" | "cancelling")
                || row.provider_terminal_status.is_some()
            {
                return Err("CodeBuddy exact response ownership conflict".into());
            }
            if let Some(id) = provider_request_id {
                apply(&mut s, Mutation::ExactProviderRequest(id), binding.clone(), observed_at)?;
            }
            apply(&mut s, Mutation::ObserveTerminal {
                session_id: expected.session_id.ok_or("CodeBuddy session required")?,
                conversation_request_id: expected.conversation_request_id,
                stop_reason, observed_at,
            }, binding, observed_at)?;
            validate(&s)?;
            let revision = s.revision.checked_add(1).ok_or("CodeBuddy revision overflow")?;
            // provider request identity 与 terminal 同一提交，无两次写入之间的 cancel/OCC 窗口。
            let changed = tx.execute("UPDATE codebuddy_execution_state SET provider_request_id=?2,
                provider_request_id_source=?3,prompt_state='terminal_observed',terminal_stop_reason=?4,
                terminal_observed_at=?5,revision=?6,updated_at=?5 WHERE execution_id=?1 AND revision=?7",
                params![id,s.provider_request_id,s.provider_request_id_source,encode(&stop_reason)?,observed_at,revision,s.revision]
            ).map_err(|e| e.to_string())?;
            if changed != 1 { return Err("CodeBuddy private response revision conflict".into()); }
            load(tx, id).map_err(|e| e.to_string())
        }).await
    }

    /// 区分真正缺失与已有私有行校验冲突；缺失绝不推造 Session。
    pub(crate) async fn codebuddy_state_exists(&self, id: String) -> Result<bool, String> {
        self.read(move |c| {
            c.query_row(
                "SELECT EXISTS(SELECT 1 FROM codebuddy_execution_state WHERE execution_id=?1)",
                [id],
                |r| r.get(0),
            )
        })
        .await
    }

    /// Adapter 创建入口；重复 create 明确 conflict，无 upsert。
    pub(crate) async fn create_codebuddy_state(
        &self,
        id: String,
        expected: Ownership,
    ) -> Result<PrivateState, String> {
        self.write_codebuddy(id, expected, None).await
    }

    /// Adapter mutation 入口；只接受 typed 操作，不泄漏 SQL closure。
    pub(crate) async fn mutate_codebuddy_state(
        &self,
        id: String,
        expected: Ownership,
        revision: i64,
        mutation: Mutation,
    ) -> Result<PrivateState, String> {
        self.write_codebuddy(id, expected, Some((revision, mutation)))
            .await
    }

    /// 单一快照读包含 generic/private/R2 校验，缺失与损坏都失败关闭。
    pub(crate) async fn read_codebuddy_state(&self, id: String) -> Result<PrivateState, String> {
        let connection = self.connection.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let mut connection = connection.lock().map_err(|e| e.to_string())?;
            let tx = connection.transaction().map_err(|e| e.to_string())?;
            let s = load(&tx, &id).map_err(|e| e.to_string())?;
            validate(&s)?;
            let binding = ownership(&tx, &id, None)?;
            private_ownership(&tx, &s, &binding, false)?;
            tx.commit().map_err(|e| e.to_string())?;
            Ok(s)
        })
        .await
        .map_err(|e| e.to_string())?
    }

    /// 所有写入共用最窄 IMMEDIATE 边界，先验证再写；失败依赖 RAII 整体回滚。
    async fn write_codebuddy(
        &self,
        id: String,
        expected: Ownership,
        change: Option<(i64, Mutation)>,
    ) -> Result<PrivateState, String> {
        let connection = self.connection.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let mut connection = connection.lock().map_err(|e| e.to_string())?;
            let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(|e| e.to_string())?;
            let binding = ownership(&tx, &id, Some(&expected))?;
            let now = chrono::Utc::now().timestamp_millis();
            if let Some((revision, mutation)) = change {
                let mut s = load(&tx,&id).map_err(|e| e.to_string())?;
                validate(&s)?;
                if revision != s.revision || revision < 0 { return Err("CodeBuddy private revision conflict; reread state".into()); }
                private_ownership(&tx,&s,&binding,matches!(mutation, Mutation::BindRuntime))?;
                apply(&mut s, mutation, binding, now)?;
                validate(&s)?;
                private_ownership(&tx,&s,&expected.runtime_instance_id,false)?;
                let next_revision = revision.checked_add(1).ok_or("CodeBuddy revision overflow")?;
                let changed = tx.execute("UPDATE codebuddy_execution_state SET runtime_instance_id=?2,
                    acp_protocol_version=?3,session_id=?4,conversation_request_id=?5,provider_request_id=?6,
                    provider_request_id_source=?7,prompt_rpc_id=?8,prompt_state=?9,terminal_stop_reason=?10,
                    terminal_observed_at=?11,recovery_method=?12,recovery_state=?13,recovery_runtime_instance_id=?14,
                    recovery_started_at=?15,recovery_finished_at=?16,revision=?17,updated_at=?18
                    WHERE execution_id=?1 AND revision=?19", params![id,s.runtime_instance_id,s.acp_protocol_version,
                    s.session_id,s.conversation_request_id,s.provider_request_id,s.provider_request_id_source,
                    s.prompt_rpc_id.as_ref().map(PromptRpcId::to_json),encode(&s.prompt_state)?,
                    s.terminal_stop_reason.as_ref().map(encode).transpose()?,s.terminal_observed_at,s.recovery_method,
                    encode(&s.recovery_state)?,s.recovery_runtime_instance_id,s.recovery_started_at,s.recovery_finished_at,
                    next_revision,now,revision]).map_err(|e| e.to_string())?;
                if changed != 1 { return Err("CodeBuddy update conflict; reread state".into()); }
            } else {
                let changed = tx.execute("INSERT INTO codebuddy_execution_state
                    (execution_id,runtime_instance_id,conversation_request_id,prompt_state,recovery_state,revision,created_at,updated_at)
                    VALUES (?1,?2,?4,'prepared','not_attempted',0,?3,?3)",params![id,binding,now,new_conversation_id()?]).map_err(|e| e.to_string())?;
                if changed != 1 { return Err("CodeBuddy create conflict; reread state".into()); }
            }
            let result = load(&tx,&id).map_err(|e| e.to_string())?;
            tx.commit().map_err(|e| e.to_string())?;
            Ok(result)
        }).await.map_err(|e| e.to_string())?
    }
}

/// 纯 typed 状态转换：不发送 Prompt，不执行 session/load，不改变其他表。
fn apply(
    s: &mut PrivateState,
    mutation: Mutation,
    binding: Option<String>,
    now: i64,
) -> Result<(), String> {
    match mutation {
        Mutation::BindRuntime => {
            if binding.is_none() || s.prompt_state != PromptState::Prepared {
                return Err("missing generic R1 binding".into());
            }
            s.runtime_instance_id = binding;
        }
        Mutation::NegotiatedProtocol(version) => {
            if s.runtime_instance_id.is_none()
                || s.prompt_state != PromptState::Prepared
                || s.acp_protocol_version.is_some_and(|old| old != version)
            {
                return Err("protocol identity conflict".into());
            }
            s.acp_protocol_version = Some(version);
        }
        Mutation::ExactSession(id) => {
            if id.is_empty()
                || s.acp_protocol_version.is_none()
                || s.runtime_instance_id.is_none()
                || s.prompt_state != PromptState::Prepared
                || s.session_id.as_ref().is_some_and(|old| old != &id)
            {
                return Err("session identity conflict".into());
            }
            s.session_id = Some(id);
        }
        Mutation::BeginContinuationLoad {
            session_id,
            recovery_runtime_instance_id,
        } => {
            if session_id.is_empty()
                || s.prompt_state != PromptState::Sent
                || s.acp_protocol_version != Some(1)
                || s.runtime_instance_id.as_deref() != Some(recovery_runtime_instance_id.as_str())
                || s.session_id.as_deref() != Some(session_id.as_str())
                || s.recovery_state != RecoveryState::NotAttempted
            {
                return Err("continuation load begin conflict".into());
            }
            s.recovery_method = Some("session/load".into());
            s.recovery_state = RecoveryState::Inspecting;
            s.recovery_runtime_instance_id = Some(recovery_runtime_instance_id);
            s.recovery_started_at = Some(now);
        }
        Mutation::FinishContinuationLoad => {
            if s.prompt_state != PromptState::Sent
                || s.recovery_state != RecoveryState::Inspecting
                || s.recovery_runtime_instance_id != s.runtime_instance_id
                || s.session_id.is_none()
            {
                return Err("continuation load finish conflict".into());
            }
            // history replay 只能支持 Session continuation 与 partial result recovery。
            s.recovery_state = RecoveryState::Partial;
            s.recovery_finished_at = Some(now);
        }
        Mutation::ExactProviderRequest(id) => {
            if s.prompt_state == PromptState::TerminalObserved
                || id.is_empty()
                || s.provider_request_id.as_ref().is_some_and(|old| old != &id)
            {
                return Err("provider request identity conflict".into());
            }
            s.provider_request_id_source = Some("exact_provider_observation".into());
            s.provider_request_id = Some(id);
        }
        Mutation::MarkSent { rpc_id } => {
            if s.prompt_state != PromptState::Prepared
                || s.runtime_instance_id.is_none()
                || s.acp_protocol_version.is_none()
                || s.session_id.is_none()
                || !valid_conversation_id(&s.conversation_request_id)
            {
                return Err("illegal sent transition".into());
            }
            s.prompt_state = PromptState::Sent;
            s.prompt_rpc_id = rpc_id;
        }
        Mutation::MarkUncertain => {
            if s.prompt_state != PromptState::Sent {
                return Err("illegal uncertain transition".into());
            }
            s.prompt_state = PromptState::Uncertain;
        }
        Mutation::ObserveTerminal {
            session_id,
            conversation_request_id,
            stop_reason,
            observed_at,
        } => {
            if s.session_id.as_ref() != Some(&session_id)
                || s.conversation_request_id != conversation_request_id
                || !matches!(
                    s.prompt_state,
                    PromptState::Sent | PromptState::Uncertain | PromptState::TerminalObserved
                )
                || (s.prompt_state == PromptState::TerminalObserved
                    && (s.terminal_stop_reason.as_ref() != Some(&stop_reason)
                        || s.terminal_observed_at != Some(observed_at)))
            {
                return Err("exact terminal observation conflict".into());
            }
            s.prompt_state = PromptState::TerminalObserved;
            s.terminal_stop_reason = Some(stop_reason);
            s.terminal_observed_at = Some(observed_at);
        }
        Mutation::BeginInspection {
            recovery_runtime_instance_id,
        } => {
            let continuation_state = s.recovery_runtime_instance_id == s.runtime_instance_id
                && matches!(
                    s.recovery_state,
                    RecoveryState::Inspecting | RecoveryState::Partial
                );
            if !matches!(
                s.prompt_state,
                PromptState::Sent | PromptState::Uncertain | PromptState::TerminalObserved
            ) || !(s.recovery_state == RecoveryState::NotAttempted || continuation_state)
                || s.runtime_instance_id.as_deref() == Some(recovery_runtime_instance_id.as_str())
            {
                return Err("inspection begin conflict".into());
            }
            s.recovery_method = Some("session/load".into());
            s.recovery_state = RecoveryState::Inspecting;
            s.recovery_runtime_instance_id = Some(recovery_runtime_instance_id);
            s.recovery_started_at = Some(now);
            s.recovery_finished_at = None;
        }
        Mutation::FinishInspection { outcome } => {
            if s.recovery_state != RecoveryState::Inspecting
                || s.recovery_runtime_instance_id == s.runtime_instance_id
            {
                return Err("inspection finish conflict".into());
            }
            s.recovery_state = match outcome {
                InspectionOutcome::Partial => RecoveryState::Partial,
                InspectionOutcome::Unknown => RecoveryState::Unknown,
                InspectionOutcome::MaterialDifference => RecoveryState::MaterialDifference,
            };
            s.recovery_finished_at = Some(now);
        }
    }
    Ok(())
}

/// Source 必须持有 exact terminal/session/protocol identity；provider request id 可缺失。
fn continuation_source_eligible(state: &PrivateState) -> bool {
    state.prompt_state == PromptState::TerminalObserved
        && state.acp_protocol_version == Some(1)
        && state
            .session_id
            .as_ref()
            .is_some_and(|session_id| !session_id.is_empty())
        && state.runtime_instance_id.is_some()
        && state.terminal_stop_reason.is_some()
        && state.terminal_observed_at.is_some()
        && state.recovery_state != RecoveryState::MaterialDifference
}
