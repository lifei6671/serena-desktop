//! 原 Runtime/Prompt 的单槽 permission permit；只决定拒绝，不拥有 terminal authority。
use super::*;
use crate::agent::store::StateStore;
use agent_client_protocol::schema::v1::{
    PermissionOptionKind, RequestPermissionOutcome, RequestPermissionRequest,
    RequestPermissionResponse, SelectedPermissionOutcome, SessionUpdate, ToolCallStatus,
};
use tokio::sync::mpsc;

/// 身份只由已经核验 durable ownership 的 Prompt owner 注册。
pub(crate) struct PermissionContext {
    runtime: String,
    execution: String,
    session: String,
    conversation: String,
    request: Option<String>,
    active: bool,
    tools: HashSet<String>,
    pending: Option<(Value, RequestPermissionResponse, Instant)>,
    events: mpsc::Sender<PermissionDenied>,
}

/// 私有 flush 决策通知只在原 CodeBuddy owner 内消费，不扩展通用 telemetry 契约。
pub(crate) struct PermissionDenied {
    pub(crate) execution: String,
    pub(crate) identity: (String, String, String),
    pub(crate) activity_sequence: u64,
    observed_at: i64,
}
impl PermissionDenied {
    /// owner 有界调用：exact 事务直接发布固定 Activity，身份失效时无投影。
    pub(crate) async fn project(self, store: &StateStore) {
        let _ = store
            .project_codebuddy_permission_denied(
                self.execution.clone(),
                self.identity,
                self.observed_at,
            )
            .await;
    }
}

/// Prompt 离开任意路径时撤销 permit；不会跨 Runtime 或 Prompt 重用。
pub(crate) struct PermissionLease(pub(crate) Arc<Shared>);
impl Drop for PermissionLease {
    /// owner drop 即撤销原 Prompt 的 permission authority。
    fn drop(&mut self) {
        self.0.state.lock().unwrap().permission = None;
    }
}

/// 唯一 typed RejectOnce 和全局唯一非空 optionId，绝不猜测 provider ID。
fn deny(request: &RequestPermissionRequest) -> Result<RequestPermissionResponse, Failure> {
    let mut ids = HashSet::new();
    let mut selected = None;
    for option in &request.options {
        if option.option_id.0.is_empty() || !ids.insert(&option.option_id) {
            return Err(Failure::Malformed);
        }
        if option.kind == PermissionOptionKind::RejectOnce {
            if selected.is_some() {
                return Err(Failure::PermissionOptions);
            }
            selected = Some(option.option_id.clone());
        }
    }
    Ok(RequestPermissionResponse::new(
        RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(
            selected.ok_or(Failure::PermissionOptions)?,
        )),
    ))
}

impl Shared {
    /// 由原 owner 在完整 Store identity 核验后、Prompt 入队之前注册。
    pub(crate) fn register_permission(
        self: &Arc<Self>,
        runtime: String,
        execution: String,
        session: String,
        conversation: String,
        request: Option<String>,
    ) -> Result<(PermissionLease, mpsc::Receiver<PermissionDenied>), Failure> {
        if self.failure().is_some()
            || [&runtime, &execution, &session, &conversation]
                .iter()
                .any(|v| v.is_empty())
        {
            return Err(Failure::State);
        }
        let mut state = self.state.lock().unwrap();
        if state.permission.is_some() || state.prompt_response_received {
            return Err(Failure::State);
        }
        let (events, receiver) = mpsc::channel(self.limits.queue_count.max(1));
        state.permission = Some(PermissionContext {
            runtime,
            execution,
            session,
            conversation,
            request,
            active: false,
            tools: HashSet::new(),
            pending: None,
            events,
        });
        Ok((PermissionLease(self.clone()), receiver))
    }

    /// 物理 Prompt flush 激活窗口；SDK enqueue 不能授权 permission。
    pub(crate) fn activate_permission(&self) {
        if let Some(context) = &mut self.state.lock().unwrap().permission {
            context.active = true;
        }
    }

    /// Dispatcher 同步验证 typed 请求与已知工具，不读数据库也不等待 telemetry。
    pub(crate) fn permission_response(
        &self,
        request: &RequestPermissionRequest,
        id: Value,
    ) -> Result<RequestPermissionResponse, Failure> {
        let mut state = self.state.lock().unwrap();
        let context = state.permission.as_mut().ok_or(Failure::State)?;
        if !context.active
            || context.session != request.session_id.0.as_ref()
            || !context
                .tools
                .contains(request.tool_call.tool_call_id.0.as_ref())
            || context.pending.is_some()
            || matches!(
                request.tool_call.fields.status,
                Some(ToolCallStatus::Completed | ToolCallStatus::Failed)
            )
        {
            return Err(Failure::State);
        }
        // wire 可不带 metadata；若显式携带则不允许与已知工具所属 Prompt 冲突。
        for meta in [request.meta.as_ref(), request.tool_call.meta.as_ref()]
            .into_iter()
            .flatten()
        {
            if let Some(conversation) = meta.get("codebuddy.ai/conversationRequestId")
                && conversation.as_str() != Some(&context.conversation)
            {
                return Err(Failure::State);
            }
            if let (Some(expected), Some(actual)) =
                (&context.request, meta.get("codebuddy.ai/requestId"))
                && actual.as_str() != Some(expected)
            {
                return Err(Failure::State);
            }
        }
        let response = deny(request)?;
        context.pending = Some((
            id,
            response.clone(),
            Instant::now() + self.limits.request_timeout,
        ));
        Ok(response)
    }

    /// server reply 真正 flush 后只交付固定 Activity；read window 仍由原 guard 管理。
    pub(crate) fn permission_flushed(&self) -> Result<(), Failure> {
        let mut state = self.state.lock().unwrap();
        let activity_sequence = state.notification_sequence;
        if let Some(context) = &mut state.permission
            && context.pending.take().is_some()
        {
            let event = PermissionDenied {
                activity_sequence,
                execution: context.execution.clone(),
                identity: (
                    context.runtime.clone(),
                    context.session.clone(),
                    context.conversation.clone(),
                ),
                observed_at: crate::agent::coordinator::now(),
            };
            context
                .events
                .try_send(event)
                .map_err(|_| Failure::QueueCount)?;
        }
        Ok(())
    }
}

impl PermissionContext {
    /// 只信任 active Prompt 的 typed 初始 ToolCall，未知更新不能注册身份。
    pub(super) fn notification(&mut self, params: &Value, limit: usize) -> Result<(), Failure> {
        if !self.active || params["sessionId"].as_str() != Some(&self.session) {
            return Ok(());
        }
        let Ok(update) = serde_json::from_value::<SessionUpdate>(params["update"].clone()) else {
            return Ok(());
        };
        if let SessionUpdate::ToolCallUpdate(call) = &update
            && matches!(
                call.fields.status,
                Some(ToolCallStatus::Completed | ToolCallStatus::Failed)
            )
        {
            self.tools.remove(call.tool_call_id.0.as_ref());
        }
        if let SessionUpdate::ToolCall(call) = update {
            if call
                .meta
                .as_ref()
                .and_then(|meta| meta.get("codebuddy.ai/conversationRequestId"))
                .and_then(Value::as_str)
                != Some(&self.conversation)
            {
                return Ok(());
            }
            if let Some(expected) = &self.request
                && call
                    .meta
                    .as_ref()
                    .and_then(|meta| meta.get("codebuddy.ai/requestId"))
                    .and_then(Value::as_str)
                    != Some(expected)
            {
                return Ok(());
            }
            let id = call.tool_call_id.to_string();
            if id.is_empty()
                || !matches!(
                    call.status,
                    ToolCallStatus::Pending | ToolCallStatus::InProgress
                )
            {
                // 初始 ToolCall 的重复完整快照也可以声明完成，不能保留旧 permit。
                self.tools.remove(&id);
                return Ok(());
            }
            if self.tools.len() >= limit && !self.tools.contains(&id) {
                return Err(Failure::QueueCount);
            }
            self.tools.insert(id);
        }
        Ok(())
    }
    /// SDK 已编码的 response 必须仍对应当前 pending typed decision。
    pub(super) fn validate_response(&self, raw: &Value) -> Result<(), Failure> {
        if let Some((id, expected, _)) = &self.pending {
            let actual = serde_json::from_value::<RequestPermissionResponse>(raw["result"].clone())
                .map_err(|_| Failure::Malformed)?;
            if raw.get("id") != Some(id) || &actual != expected {
                return Err(Failure::Malformed);
            }
        }
        Ok(())
    }
    /// 当前 client 只能激活 owner 注册的 exact Prompt。
    pub(super) fn validate_prompt(&self, raw: &Value) -> Result<(), Failure> {
        if raw["params"]["sessionId"].as_str() != Some(&self.session)
            || raw["params"]["_meta"]["codebuddy.ai/conversationRequestId"].as_str()
                != Some(&self.conversation)
        {
            return Err(Failure::State);
        }
        Ok(())
    }
    /// 物理 response 阻塞也有绝对 deadline，不依赖 owner 的 SQLite await。
    pub(super) fn expired(&self) -> bool {
        self.pending
            .as_ref()
            .is_some_and(|(_, _, deadline)| *deadline <= Instant::now())
    }
}
