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
            s.recovery_method.as_deref() == Some("session/load")
                && s.recovery_runtime_instance_id.is_some()
                && s.recovery_started_at.is_some()
                && s.prompt_state != PromptState::Prepared
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
        if Some(id) == s.runtime_instance_id.as_ref() {
            return Err("inspection requires separate R2".into());
        }
    }
    Ok(())
}

impl StateStore {
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
            if !matches!(s.prompt_state, PromptState::Sent | PromptState::Uncertain)
                || s.recovery_state != RecoveryState::NotAttempted
            {
                return Err("inspection begin conflict".into());
            }
            s.recovery_method = Some("session/load".into());
            s.recovery_state = RecoveryState::Inspecting;
            s.recovery_runtime_instance_id = Some(recovery_runtime_instance_id);
            s.recovery_started_at = Some(now);
        }
        Mutation::FinishInspection { outcome } => {
            if s.recovery_state != RecoveryState::Inspecting {
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
