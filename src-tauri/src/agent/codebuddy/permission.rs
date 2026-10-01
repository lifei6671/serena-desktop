//! 原 Runtime/Prompt 的单槽 permission permit；本地确定性审批，不拥有 terminal authority。
use super::*;
use crate::agent::codebuddy::permission_policy::{Authority, Decision, evaluate};
use crate::agent::store::StateStore;
use agent_client_protocol::schema::v1::ToolCallUpdateFields;
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
    authority: Authority,
    tools: HashMap<String, ToolCallUpdateFields>,
    snapshot_bytes: usize,
    pending: Option<(Value, RequestPermissionResponse, Instant, bool)>,
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

/// 只按 typed kind 选单次选项；allow 不可用时退回唯一 RejectOnce。
fn select(
    request: &RequestPermissionRequest,
    decision: Decision,
) -> Result<(RequestPermissionResponse, bool), Failure> {
    let mut ids = HashSet::new();
    let mut allows = Vec::new();
    let mut rejects = Vec::new();
    for option in &request.options {
        if option.option_id.0.trim().is_empty() || !ids.insert(&option.option_id) {
            return Err(Failure::Malformed);
        }
        match option.kind {
            PermissionOptionKind::AllowOnce => allows.push(option.option_id.clone()),
            PermissionOptionKind::RejectOnce => rejects.push(option.option_id.clone()),
            _ => {}
        }
    }
    let (selected, rejected) = if decision == Decision::AutoAllowOnce && allows.len() == 1 {
        (allows.remove(0), false)
    } else if rejects.len() == 1 {
        (rejects.remove(0), true)
    } else {
        return Err(Failure::PermissionOptions);
    };
    Ok((
        RequestPermissionResponse::new(RequestPermissionOutcome::Selected(
            SelectedPermissionOutcome::new(selected),
        )),
        rejected,
    ))
}

/// SDK 的宽容 optional-field 解码不能把显式坏路径静默丢弃后自动批准。
pub(super) fn validate_tool_fields(raw: &Value) -> Result<(), Failure> {
    use agent_client_protocol::schema::v1::{ToolCallContent, ToolCallLocation, ToolKind};
    if !raw.is_object() {
        return Err(Failure::Malformed);
    }
    if let Some(status) = raw.get("status").filter(|v| !v.is_null()) {
        serde_json::from_value::<ToolCallStatus>(status.clone()).map_err(|_| Failure::Malformed)?;
    }
    if let Some(kind) = raw.get("kind").filter(|v| !v.is_null()) {
        serde_json::from_value::<ToolKind>(kind.clone()).map_err(|_| Failure::Malformed)?;
    }
    if let Some(locations) = raw.get("locations").filter(|v| !v.is_null()) {
        serde_json::from_value::<Vec<ToolCallLocation>>(locations.clone())
            .map_err(|_| Failure::Malformed)?;
    }
    if let Some(content) = raw.get("content").filter(|v| !v.is_null()) {
        serde_json::from_value::<Vec<ToolCallContent>>(content.clone())
            .map_err(|_| Failure::Malformed)?;
    }
    for key in ["name", "title"] {
        if raw.get(key).is_some_and(|v| !v.is_null() && !v.is_string()) {
            return Err(Failure::Malformed);
        }
    }
    if raw
        .get("_meta")
        .is_some_and(|v| !v.is_null() && !v.is_object())
    {
        return Err(Failure::Malformed);
    }
    Ok(())
}

/// 更新只替换显式字段，不借同名工具共享数据；不保存 raw output 或 title。
fn merge(snapshot: &mut ToolCallUpdateFields, update: &ToolCallUpdateFields) {
    if let Some(kind) = &update.kind {
        snapshot.kind = Some(*kind);
    }
    if let Some(status) = &update.status {
        snapshot.status = Some(*status);
    }
    if let Some(name) = &update.name {
        snapshot.name = Some(name.clone());
    }
    if let Some(locations) = &update.locations {
        snapshot.locations = Some(locations.clone());
    }
    if let Some(input) = &update.raw_input {
        snapshot.raw_input = Some(input.clone());
    }
    if let Some(content) = &update.content {
        snapshot.content = Some(content.clone());
    }
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
        authority: Authority,
    ) -> Result<(PermissionLease, mpsc::Receiver<PermissionDenied>), Failure> {
        if self.failure().is_some()
            || authority.lease.workspace_id.is_empty()
            || !authority.lease.canonical_root.is_absolute()
            || !matches!(authority.mode.as_str(), "workspace_write" | "read_only")
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
            authority,
            tools: HashMap::new(),
            snapshot_bytes: 0,
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

    /// exact server response 已生成但尚未完成物理 flush 时保持 pending，
    /// Prompt owner 在消费 terminal 前用它收敛 permission publication 顺序。
    pub(crate) fn permission_response_pending(&self) -> bool {
        self.state
            .lock()
            .unwrap()
            .permission
            .as_ref()
            .is_some_and(|context| context.pending.is_some())
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
                .contains_key(request.tool_call.tool_call_id.0.as_ref())
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
        let mut snapshot = context
            .tools
            .get(request.tool_call.tool_call_id.0.as_ref())
            .ok_or(Failure::State)?
            .clone();
        merge(&mut snapshot, &request.tool_call.fields);
        let old_size = serde_json::to_vec(
            context
                .tools
                .get(request.tool_call.tool_call_id.0.as_ref())
                .ok_or(Failure::State)?,
        )
        .map_err(|_| Failure::Malformed)?
        .len();
        let new_size = serde_json::to_vec(&snapshot)
            .map_err(|_| Failure::Malformed)?
            .len();
        let snapshot_bytes = context
            .snapshot_bytes
            .saturating_sub(old_size)
            .checked_add(new_size)
            .ok_or(Failure::QueueBytes)?;
        if snapshot_bytes > self.limits.queue_bytes {
            return Err(Failure::QueueBytes);
        }
        let (response, rejected) =
            select(request, evaluate(&context.authority, &snapshot, request))?;
        context
            .tools
            .insert(request.tool_call.tool_call_id.to_string(), snapshot);
        context.snapshot_bytes = snapshot_bytes;
        context.pending = Some((
            id,
            response.clone(),
            Instant::now() + self.limits.request_timeout,
            rejected,
        ));
        Ok(response)
    }

    /// server reply 真正 flush 后仅拒绝决策交付固定 Activity；read window 仍由原 guard 管理。
    pub(crate) fn permission_flushed(&self) -> Result<(), Failure> {
        let mut state = self.state.lock().unwrap();
        let activity_sequence = state.notification_sequence;
        if let Some(context) = &mut state.permission
            && let Some((_, _, _, true)) = context.pending.take()
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
    /// 保存有界 exact tool 快照；未知更新不能注册身份，外来 metadata 不能修改或删除。
    pub(super) fn notification(
        &mut self,
        params: &Value,
        limit: usize,
        byte_limit: usize,
    ) -> Result<(), Failure> {
        if !self.active || params["sessionId"].as_str() != Some(&self.session) {
            return Ok(());
        }
        for (key, expected) in [
            (
                "codebuddy.ai/conversationRequestId",
                Some(&self.conversation),
            ),
            ("codebuddy.ai/requestId", self.request.as_ref()),
        ] {
            if let (Some(expected), Some(actual)) =
                (expected, params.get("_meta").and_then(|m| m.get(key)))
                && actual.as_str() != Some(expected.as_str())
            {
                return Ok(());
            }
        }
        if matches!(
            params["update"]["sessionUpdate"].as_str(),
            Some("tool_call" | "tool_call_update")
        ) {
            validate_tool_fields(&params["update"])?;
        }
        let Ok(update) = serde_json::from_value::<SessionUpdate>(params["update"].clone()) else {
            return Ok(());
        };
        let (id, fields, initial, meta) = match update {
            SessionUpdate::ToolCall(call) => {
                let mut fields = ToolCallUpdateFields::default();
                fields.kind = Some(call.kind);
                fields.status = Some(call.status);
                fields.name = call.name;
                fields.locations = Some(call.locations);
                fields.raw_input = call.raw_input;
                fields.content = Some(call.content);
                (call.tool_call_id.to_string(), fields, true, call.meta)
            }
            SessionUpdate::ToolCallUpdate(call) => {
                (call.tool_call_id.to_string(), call.fields, false, call.meta)
            }
            _ => return Ok(()),
        };
        let meta = meta.as_ref();
        for (key, expected) in [
            (
                "codebuddy.ai/conversationRequestId",
                Some(&self.conversation),
            ),
            ("codebuddy.ai/requestId", self.request.as_ref()),
        ] {
            if let Some(expected) = expected {
                let actual = meta.and_then(|m| m.get(key));
                if (initial || actual.is_some())
                    && actual.and_then(Value::as_str) != Some(expected.as_str())
                {
                    return Ok(());
                }
            }
        }
        if id.is_empty() || (!initial && !self.tools.contains_key(&id)) {
            return Ok(());
        }
        if matches!(
            fields.status,
            Some(ToolCallStatus::Completed | ToolCallStatus::Failed)
        ) {
            self.tools.remove(&id);
        } else {
            if self.tools.len() >= limit && !self.tools.contains_key(&id) {
                return Err(Failure::QueueCount);
            }
            let mut snapshot = self.tools.get(&id).cloned().unwrap_or_default();
            merge(&mut snapshot, &fields);
            let size = serde_json::to_vec(&snapshot)
                .map_err(|_| Failure::Malformed)?
                .len();
            let old_size = self
                .tools
                .get(&id)
                .map(|s| serde_json::to_vec(s).map(|v| v.len()).unwrap_or(0))
                .unwrap_or(0);
            if size > byte_limit.saturating_sub(self.snapshot_bytes.saturating_sub(old_size)) {
                return Err(Failure::QueueBytes);
            }
            self.tools.insert(id, snapshot);
        }
        self.snapshot_bytes = self
            .tools
            .values()
            .map(|s| serde_json::to_vec(s).map(|v| v.len()).unwrap_or(0))
            .sum();
        Ok(())
    }
    /// SDK 已编码的 response 必须仍对应当前 pending typed decision。
    pub(super) fn validate_response(&self, raw: &Value) -> Result<(), Failure> {
        if let Some((id, expected, _, _)) = &self.pending {
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
            .is_some_and(|(_, _, deadline, _)| *deadline <= Instant::now())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 冻结权限只使用测试目录；不依赖 active Workspace Registry。
    fn authority(root: &std::path::Path) -> Authority {
        Authority {
            lease: crate::workspace_resolver::WorkspaceLease {
                workspace_id: "frozen".into(),
                canonical_root: std::fs::canonicalize(root).unwrap(),
                generation: 7,
            },
            mode: "workspace_write".into(),
        }
    }

    /// 请求显式字段补全当前工具，allow flush 不发送拒绝事件或干扰后续响应。
    #[test]
    fn allow_flush_and_partial_snapshots_preserve_normal_activity() {
        let root = tempfile::tempdir().unwrap();
        let authority = authority(root.path());
        let inside = authority.lease.canonical_root.join("new.txt");
        let shared = Shared::new(Limits::default());
        let (_lease, mut events) = shared
            .register_permission(
                "r".into(),
                "e".into(),
                "s".into(),
                "c".into(),
                Some("p".into()),
                authority,
            )
            .unwrap();
        shared.activate_permission();
        let initial = json!({"sessionId":"s","update":{"sessionUpdate":"tool_call","toolCallId":"t","title":"private","status":"pending","kind":"other","_meta":{"codebuddy.ai/conversationRequestId":"c","codebuddy.ai/requestId":"p"}}});
        shared
            .notification("session/update".into(), initial)
            .unwrap();
        let update = json!({"sessionId":"s","update":{"sessionUpdate":"tool_call_update","toolCallId":"t","kind":"edit","rawInput":{"file_path":inside}}});
        shared
            .notification("session/update".into(), update.clone())
            .unwrap();
        let params = json!({"sessionId":"s","toolCall":{"toolCallId":"t"},"options":[{"optionId":"yes","name":"x","kind":"allow_once"},{"optionId":"no","name":"x","kind":"reject_once"}]});
        let request: RequestPermissionRequest = serde_json::from_value(params.clone()).unwrap();
        let response = shared.permission_response(&request, json!(1)).unwrap();
        assert_eq!(
            serde_json::to_value(response).unwrap()["outcome"]["optionId"],
            "yes"
        );
        assert!(shared.permission_response_pending());
        let sequence = shared.state.lock().unwrap().notification_sequence;
        shared.permission_flushed().unwrap();
        assert!(!shared.permission_response_pending());
        assert!(matches!(
            events.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));
        assert_eq!(shared.state.lock().unwrap().notification_sequence, sequence);
        assert!(!shared.prompt_response_received());
        // 外来终态不能删除 exact 工具；未知 update 也不能创建新工具。
        for (session, id, meta) in [
            ("other", "t", json!({})),
            (
                "s",
                "t",
                json!({"codebuddy.ai/conversationRequestId":"other"}),
            ),
            ("s", "t", json!({"codebuddy.ai/requestId":"other"})),
            ("s", "unknown", json!({})),
        ] {
            shared.notification("session/update".into(), json!({"sessionId":session,"update":{"sessionUpdate":"tool_call_update","toolCallId":id,"status":"completed","_meta":meta}})).unwrap();
        }
        assert!(
            shared
                .state
                .lock()
                .unwrap()
                .permission
                .as_ref()
                .unwrap()
                .tools
                .contains_key("t")
        );
        assert!(
            !shared
                .state
                .lock()
                .unwrap()
                .permission
                .as_ref()
                .unwrap()
                .tools
                .contains_key("unknown")
        );
        // request 字段可以覆盖同一工具，实际拒绝才产生私有投影事件。
        let mut reject = params.clone();
        reject["toolCall"]["rawInput"] =
            json!({"file_path": root.path().parent().unwrap().join("outside")});
        let response = shared
            .permission_response(&serde_json::from_value(reject).unwrap(), json!(2))
            .unwrap();
        assert_eq!(
            serde_json::to_value(response).unwrap()["outcome"]["optionId"],
            "no"
        );
        shared.permission_flushed().unwrap();
        assert_eq!(events.try_recv().unwrap().execution, "e");
        shared.notification("session/update".into(), json!({"sessionId":"s","update":{"sessionUpdate":"tool_call_update","toolCallId":"t","status":"failed"}})).unwrap();
        assert_eq!(
            shared.permission_response(&request, json!(3)).unwrap_err(),
            Failure::State
        );
    }

    /// typed allow 缺失或不唯一退回 reject；全局 ID 验证不因 allow 放宽。
    #[test]
    fn option_fallback_and_malformed_ids() {
        let mut value = json!({"sessionId":"s","toolCall":{"toolCallId":"t"},"options":[{"optionId":"no","name":"allow misleading label","kind":"reject_once"}]});
        let request = serde_json::from_value(value.clone()).unwrap();
        let (response, rejected) = select(&request, Decision::AutoAllowOnce).unwrap();
        assert!(rejected);
        assert_eq!(
            serde_json::to_value(response).unwrap()["outcome"]["optionId"],
            "no"
        );
        value["options"].as_array_mut().unwrap().extend([
            json!({"optionId":"a","name":"x","kind":"allow_once"}),
            json!({"optionId":"b","name":"x","kind":"allow_once"}),
        ]);
        assert!(
            select(
                &serde_json::from_value(value.clone()).unwrap(),
                Decision::AutoAllowOnce
            )
            .unwrap()
            .1
        );
        value["options"][2]["optionId"] = json!("a");
        assert_eq!(
            select(
                &serde_json::from_value(value.clone()).unwrap(),
                Decision::AutoAllowOnce
            )
            .unwrap_err(),
            Failure::Malformed
        );
        value["options"][2]["optionId"] = json!(" ");
        assert_eq!(
            select(
                &serde_json::from_value(value).unwrap(),
                Decision::RejectOnce
            )
            .unwrap_err(),
            Failure::Malformed
        );
    }

    /// malformed optional 字段不能被 SDK 默认值掩盖后形成允许快照。
    #[test]
    fn malformed_structured_fields_fail_closed() {
        for raw in [
            json!({"locations":[{"path":1}]}),
            json!({"locations":[{"path":"inside"},{}]}),
            json!({"kind":123}),
            json!({"status":123}),
            json!({"content":[{"type":"diff","path":1,"newText":"x"}]}),
            json!({"_meta":"wrong"}),
        ] {
            assert_eq!(validate_tool_fields(&raw).unwrap_err(), Failure::Malformed);
        }
    }

    /// 累计快照有独立 byte budget；terminal 删除释放快照空间。
    #[test]
    fn snapshots_have_bounded_total_bytes() {
        let root = tempfile::tempdir().unwrap();
        let shared = Shared::new(Limits::default());
        let (_lease, _events) = shared
            .register_permission(
                "r".into(),
                "e".into(),
                "s".into(),
                "c".into(),
                None,
                authority(root.path()),
            )
            .unwrap();
        shared.activate_permission();
        let mut state = shared.state.lock().unwrap();
        let context = state.permission.as_mut().unwrap();
        let initial = json!({"sessionId":"s","update":{"sessionUpdate":"tool_call","toolCallId":"t","title":"x","_meta":{"codebuddy.ai/conversationRequestId":"c"}}});
        assert_eq!(
            context.notification(&initial, 4, 1).unwrap_err(),
            Failure::QueueBytes
        );
        context.notification(&initial, 4, 4096).unwrap();
        context.notification(&json!({"sessionId":"s","update":{"sessionUpdate":"tool_call_update","toolCallId":"t","status":"completed"}}),4,4096).unwrap();
        assert_eq!(context.snapshot_bytes, 0);
    }
}
