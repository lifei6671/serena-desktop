//! 单 Prompt 的 typed ACP 安全分类；不拥有 Store 或 terminal authority。
use super::protocol::SessionFrame;
use crate::agent::{
    activity::ToolCategory, coordinator::now, provider::telemetry::AgentActivityEvent,
};
use agent_client_protocol::schema::v1::{SessionUpdate, ToolCallStatus, ToolKind};
use serde_json::Value;
use std::collections::HashMap;

/// 仅保存有界结构化工具分类；生命周期结束时随 Prompt 一并释放。
pub(super) struct ActivityMapper {
    execution: String,
    session: String,
    conversation: String,
    request: Option<String>,
    tools: HashMap<String, ToolCategory>,
    limit: usize,
}
impl ActivityMapper {
    /// 身份来自经过验证的 durable 快照，永不从 Activity 反向写入。
    pub(super) fn new(
        execution: String,
        session: String,
        conversation: String,
        request: Option<String>,
        limit: usize,
    ) -> Self {
        Self {
            execution,
            session,
            conversation,
            request,
            tools: HashMap::new(),
            limit,
        }
    }
    /// 只读 typed variant 的 identity/kind/status；文本永不进入事件。
    pub(super) fn map(&mut self, frame: &SessionFrame) -> Option<AgentActivityEvent> {
        if frame.session_id != self.session || frame.method != "session/update" {
            return None;
        }
        let update: SessionUpdate =
            serde_json::from_value(frame.params.get("update")?.clone()).ok()?;
        let meta = match &update {
            SessionUpdate::ToolCall(v) => v.meta.as_ref(),
            SessionUpdate::ToolCallUpdate(v) => v.meta.as_ref(),
            SessionUpdate::AgentMessageChunk(v) | SessionUpdate::AgentThoughtChunk(v) => {
                v.meta.as_ref()
            }
            _ => return None,
        }?;
        if meta
            .get("codebuddy.ai/conversationRequestId")
            .and_then(Value::as_str)
            != Some(self.conversation.as_str())
        {
            return None;
        }
        if let (Some(expected), Some(actual)) = (&self.request, meta.get("codebuddy.ai/requestId"))
            && actual.as_str() != Some(expected.as_str())
        {
            return None;
        }
        let category = match update {
            SessionUpdate::ToolCall(call) => {
                let category = category(call.kind);
                // 初始帧即使已完成也保留结构化分类，供后续同 id 的增量更新使用。
                self.remember(call.tool_call_id.to_string(), category);
                match call.status {
                    ToolCallStatus::Pending | ToolCallStatus::InProgress => Some(category),
                    ToolCallStatus::Completed | ToolCallStatus::Failed => None,
                    _ => return None,
                }
            }
            SessionUpdate::ToolCallUpdate(call) => {
                let id = call.tool_call_id.to_string();
                let category = call
                    .fields
                    .kind
                    .map(category)
                    .or_else(|| self.tools.get(&id).copied());
                if let Some(category) = category {
                    self.remember(id, category);
                }
                match call.fields.status {
                    Some(ToolCallStatus::Pending | ToolCallStatus::InProgress) => Some(category?),
                    Some(ToolCallStatus::Completed | ToolCallStatus::Failed) => None,
                    _ => return None,
                }
            }
            SessionUpdate::AgentMessageChunk(_) | SessionUpdate::AgentThoughtChunk(_) => None,
            _ => return None,
        };
        Some(match category {
            Some(category) => AgentActivityEvent::tool(self.execution.clone(), category, now()),
            None => AgentActivityEvent::provider(self.execution.clone(), now()),
        })
    }
    /// 达到容量后只更新已知 id，拒绝为新 id 记忆，避免 provider 控制内存增长。
    fn remember(&mut self, id: String, category: ToolCategory) {
        if self.tools.contains_key(&id) || self.tools.len() < self.limit {
            self.tools.insert(id, category);
        }
    }
}
/// 仅结构化 kind 决定分类；未来 kind 保守归入 Tool。
fn category(kind: ToolKind) -> ToolCategory {
    match kind {
        ToolKind::Read => ToolCategory::Read,
        ToolKind::Edit | ToolKind::Delete | ToolKind::Move => ToolCategory::Edit,
        ToolKind::Execute => ToolCategory::Command,
        _ => ToolCategory::Tool,
    }
}
#[cfg(test)]
mod tests;
