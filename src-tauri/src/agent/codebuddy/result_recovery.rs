//! 独立 Result-Recovery Runtime R2；只允许 initialize + exact session/load 历史检查。

use super::{
    discovery::ResolvedLaunchSpec,
    protocol::{Failure, Limits, SessionFrame},
    runtime::Runtime,
    store::{
        CodeBuddyStore, InspectionOutcome, Ownership, PrivateState, PromptState, RecoveryState,
        new_conversation_id,
    },
    windows_launcher::{LaunchRequest, UncCurrentDirectoryPolicy},
};
use crate::agent::store::{ExecutionRecord, StateStore};
use agent_client_protocol::schema::v1::{ContentBlock, LoadSessionRequest};
use serde_json::{Value, json};
use std::path::Path;

const CONVERSATION: &str = "codebuddy.ai/conversationRequestId";
const PROVIDER_REQUEST: &str = "codebuddy.ai/requestId";

/// Recovery replay 只产生 partial 或 unknown；不存在 recovered complete。
pub(super) enum RecoveredResult {
    Unknown,
    Partial(Value),
}

/// R1 proof 已由调用方完成；本函数只管理 R2 inspection 与它自己的 durable termination proof。
pub(super) async fn inspect(
    store: &StateStore,
    owner: &str,
    row: &ExecutionRecord,
    private: PrivateState,
    resolved: Option<&ResolvedLaunchSpec>,
) -> Result<RecoveredResult, String> {
    let continuation_state = private.recovery_runtime_instance_id == private.runtime_instance_id;
    match private.recovery_state {
        RecoveryState::Inspecting if !continuation_state => {
            let runtime_id = private
                .recovery_runtime_instance_id
                .clone()
                .ok_or("CODEBUDDY_RECOVERY_RUNTIME_REQUIRED")?;
            super::recovery::recover(
                store,
                runtime_id.clone(),
                std::time::Duration::from_secs(10),
            )
            .await?;
            if !super::recovery::approved_runtime(store, &runtime_id).await? {
                return Err("CODEBUDDY_RECOVERY_RUNTIME_EVIDENCE_REQUIRED".into());
            }
            finish(store, row, private, InspectionOutcome::Unknown, None).await
        }
        RecoveryState::Partial if !continuation_state => {
            persisted_result(store, row, &private).await
        }
        RecoveryState::Unknown | RecoveryState::MaterialDifference => {
            require_finished_runtime(store, &private).await?;
            Ok(RecoveredResult::Unknown)
        }
        RecoveryState::NotAttempted | RecoveryState::Inspecting | RecoveryState::Partial => {
            let Some(resolved) = resolved else {
                return Ok(RecoveredResult::Unknown);
            };
            if !eligible(&private) {
                return Ok(RecoveredResult::Unknown);
            }
            launch_and_inspect(store, owner, row, private, resolved).await
        }
    }
}

/// Prepared 表示 Prompt 尚未发送；此时无需创建读取历史的 R2。
fn eligible(private: &PrivateState) -> bool {
    private.acp_protocol_version == Some(1)
        && private
            .session_id
            .as_ref()
            .is_some_and(|session| !session.is_empty())
        && private.runtime_instance_id.is_some()
        && !matches!(private.prompt_state, PromptState::Prepared)
}

/// 新建 R2 前先构造所有本地参数，避免无效 cwd/LaunchSpec 留下无法证明的伪 Runtime。
async fn launch_and_inspect(
    store: &StateStore,
    owner: &str,
    row: &ExecutionRecord,
    private: PrivateState,
    resolved: &ResolvedLaunchSpec,
) -> Result<RecoveredResult, String> {
    let runtime_id = format!("codebuddy-recovery-{}", new_conversation_id()?);
    let request = LaunchRequest::from_resolved(
        resolved,
        Path::new(&row.canonical_workspace_root),
        UncCurrentDirectoryPolicy::Unsupported,
        runtime_id.clone(),
    )
    .map_err(|_| "CODEBUDDY_RECOVERY_LAUNCH_INVALID")?;
    let cwd = request.projected_cwd().as_path().to_owned();
    let session = super::recovery::current_session()?;
    let private_store = CodeBuddyStore(store.clone());
    let ownership = Ownership {
        execution_revision: row.revision,
        runtime_instance_id: row.runtime_instance_id.clone(),
    };
    let inspecting = private_store
        .begin_result_inspection(
            private,
            ownership.clone(),
            runtime_id.clone(),
            owner.to_owned(),
            session,
            resolved.executable.to_string_lossy().into_owned(),
        )
        .await?;
    let limits = Limits::default();
    let started =
        Runtime::start_persisted(request, limits, store.clone(), runtime_id.clone()).await;
    let inspected = match started {
        Ok((runtime, handshake)) => {
            let inspected = inspect_wire(&runtime, &handshake, &inspecting, cwd, limits).await;
            // shutdown 返回值不构成 authority；下方必须重读 durable R2 Job evidence。
            let _ = runtime.shutdown().await;
            inspected
        }
        Err(_) => Err(Failure::Launch),
    };
    if !super::recovery::approved_runtime(store, &runtime_id).await? {
        return Err("CODEBUDDY_RECOVERY_RUNTIME_EVIDENCE_REQUIRED".into());
    }
    let (outcome, result) = match inspected {
        Ok(result @ Some(_)) => (InspectionOutcome::Partial, result),
        Ok(None) => (InspectionOutcome::Unknown, None),
        Err(Failure::Continuation | Failure::Malformed) => {
            (InspectionOutcome::MaterialDifference, None)
        }
        Err(_) => (InspectionOutcome::Unknown, None),
    };
    finish(store, row, inspecting, outcome, result).await
}

/// 唯一 wire 序列为 initialize（Runtime 内）→ session/load；不提供 prompt、resume 或 new 入口。
async fn inspect_wire(
    runtime: &Runtime,
    handshake: &super::client::Handshake,
    private: &PrivateState,
    cwd: std::path::PathBuf,
    limits: Limits,
) -> Result<Option<Value>, Failure> {
    if !handshake.response.agent_capabilities.load_session {
        return Err(Failure::Continuation);
    }
    let session_id = private.session_id.as_ref().ok_or(Failure::State)?;
    let requests = &runtime.client.as_ref().ok_or(Failure::Closed)?.requests;
    requests.shared.register_route(session_id)?;
    requests
        .request(LoadSessionRequest::new(session_id.clone(), cwd))
        .await?;
    requests.shared.take_session_load_extensions(session_id)?;
    let replay = requests.shared.take_result_recovery_replay(session_id)?;
    extract_exact_text(
        replay,
        session_id,
        &private.conversation_request_id,
        private.provider_request_id.as_deref(),
        limits.frame_bytes,
    )
}

/// foreign/missing identity 与 malformed/超预算正文一律不公开；只拼接 exact assistant Text。
fn extract_exact_text(
    frames: Vec<SessionFrame>,
    session_id: &str,
    conversation_id: &str,
    provider_request_id: Option<&str>,
    max_bytes: usize,
) -> Result<Option<Value>, Failure> {
    let mut text = String::new();
    let mut tainted = false;
    for frame in frames {
        if frame.session_id != session_id || frame.method != "session/update" {
            tainted = true;
            continue;
        }
        let update = &frame.params["update"];
        if update["sessionUpdate"] != "agent_message_chunk" {
            continue;
        }
        let meta = &update["_meta"];
        if meta[CONVERSATION].as_str() != Some(conversation_id)
            || provider_request_id.is_some_and(|id| meta[PROVIDER_REQUEST].as_str() != Some(id))
        {
            tainted = true;
            continue;
        }
        match serde_json::from_value::<ContentBlock>(update["content"].clone()) {
            Ok(ContentBlock::Text(content)) if content.text.is_empty() => {}
            Ok(ContentBlock::Text(content))
                if content.text.len() <= max_bytes.saturating_sub(text.len()) =>
            {
                text.push_str(&content.text);
            }
            _ => tainted = true,
        }
    }
    if tainted {
        return Err(Failure::Continuation);
    }
    Ok((!text.is_empty()).then(|| json!({"text": text})))
}

/// R2 evidence 先于 inspection result 持久化；事务同时冻结 private outcome 与 generic partial。
async fn finish(
    store: &StateStore,
    row: &ExecutionRecord,
    private: PrivateState,
    outcome: InspectionOutcome,
    result: Option<Value>,
) -> Result<RecoveredResult, String> {
    let private = CodeBuddyStore(store.clone())
        .finish_result_inspection(
            private,
            Ownership {
                execution_revision: row.revision,
                runtime_instance_id: row.runtime_instance_id.clone(),
            },
            outcome,
            result.clone(),
        )
        .await?;
    require_finished_runtime(store, &private).await?;
    Ok(match (outcome, result) {
        (InspectionOutcome::Partial, Some(result)) => RecoveredResult::Partial(result),
        _ => RecoveredResult::Unknown,
    })
}

/// restart 复用已完成 inspection 时仍重新读取 R2 proof，不能把 private state 当作 Job authority。
async fn persisted_result(
    store: &StateStore,
    row: &ExecutionRecord,
    private: &PrivateState,
) -> Result<RecoveredResult, String> {
    require_finished_runtime(store, private).await?;
    if row.result_completeness != "partial" {
        return Err("CODEBUDDY_RECOVERY_RESULT_INVALID".into());
    }
    let result = serde_json::from_str(
        row.final_result_json
            .as_deref()
            .ok_or("CODEBUDDY_RECOVERY_RESULT_REQUIRED")?,
    )
    .map_err(|_| "CODEBUDDY_RECOVERY_RESULT_INVALID")?;
    Ok(RecoveredResult::Partial(result))
}

/// 所有已开始的 R2 都必须有独立 complete durable Job evidence。
async fn require_finished_runtime(
    store: &StateStore,
    private: &PrivateState,
) -> Result<(), String> {
    let runtime_id = private
        .recovery_runtime_instance_id
        .as_deref()
        .ok_or("CODEBUDDY_RECOVERY_RUNTIME_REQUIRED")?;
    if super::recovery::approved_runtime(store, runtime_id).await? {
        Ok(())
    } else {
        Err("CODEBUDDY_RECOVERY_RUNTIME_EVIDENCE_REQUIRED".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// exact conversation 可得 partial；foreign/missing identity 与超预算正文均不泄漏。
    #[test]
    fn exact_text_filter_is_bounded_and_fail_closed() {
        let frame = |conversation: Option<&str>, request: Option<&str>, text: &str| {
            let mut meta = json!({});
            if let Some(conversation) = conversation {
                meta[CONVERSATION] = json!(conversation);
            }
            if let Some(request) = request {
                meta[PROVIDER_REQUEST] = json!(request);
            }
            SessionFrame {
                sequence: 1,
                session_id: "s".into(),
                method: "session/update".into(),
                params: json!({"sessionId":"s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":text},"_meta":meta}}),
                bytes: 1,
                expires: tokio::time::Instant::now() + std::time::Duration::from_secs(1),
            }
        };
        assert_eq!(
            extract_exact_text(vec![frame(Some("c"), None, "safe")], "s", "c", None, 8).unwrap(),
            Some(json!({"text":"safe"}))
        );
        assert!(
            extract_exact_text(
                vec![frame(Some("foreign"), None, "secret")],
                "s",
                "c",
                None,
                8
            )
            .is_err()
        );
        assert!(extract_exact_text(vec![frame(None, None, "secret")], "s", "c", None, 8).is_err());
        assert!(
            extract_exact_text(vec![frame(Some("c"), None, "too-long")], "s", "c", None, 2)
                .is_err()
        );
        assert_eq!(
            extract_exact_text(
                vec![frame(Some("c"), Some("provider"), "safe")],
                "s",
                "c",
                Some("provider"),
                8
            )
            .unwrap(),
            Some(json!({"text":"safe"}))
        );
        assert!(
            extract_exact_text(
                vec![frame(Some("c"), Some("foreign"), "secret")],
                "s",
                "c",
                Some("provider"),
                8
            )
            .is_err()
        );
    }
}
