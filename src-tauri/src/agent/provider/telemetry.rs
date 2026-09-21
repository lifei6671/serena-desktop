use crate::agent::activity::{ActivityPhase, ToolCategory};
use crate::agent::provider::ProviderId;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentTelemetryEvent {
    Activity(AgentActivityEvent),
    Usage(UsageEvent),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentActivityEvent {
    execution_id: String,
    phase: ActivityPhase,
    tool_category: Option<ToolCategory>,
    observed_at: i64,
}

impl AgentActivityEvent {
    pub fn provider(execution_id: String, observed_at: i64) -> Self {
        Self {
            execution_id,
            phase: ActivityPhase::Provider,
            tool_category: None,
            observed_at,
        }
    }

    pub fn tool(execution_id: String, tool_category: ToolCategory, observed_at: i64) -> Self {
        Self {
            execution_id,
            phase: ActivityPhase::Tool,
            tool_category: Some(tool_category),
            observed_at,
        }
    }

    pub fn execution_id(&self) -> &str {
        &self.execution_id
    }

    pub const fn phase(&self) -> ActivityPhase {
        self.phase
    }

    pub const fn tool_category(&self) -> Option<ToolCategory> {
        self.tool_category
    }

    pub const fn observed_at(&self) -> i64 {
        self.observed_at
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UsageEvent {
    execution_id: String,
    provider_id: ProviderId,
    cumulative_total_tokens: i64,
    input_tokens: Option<i64>,
    cached_input_tokens: Option<i64>,
    cache_write_input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    reasoning_output_tokens: Option<i64>,
    model_context_window: Option<i64>,
    observed_at: i64,
}

impl UsageEvent {
    /// 构造已完成 Provider 私有 identity 绑定的安全累计 Usage event。
    #[allow(clippy::too_many_arguments)]
    pub fn cumulative(
        execution_id: String,
        provider_id: ProviderId,
        cumulative_total_tokens: i64,
        input_tokens: Option<i64>,
        cached_input_tokens: Option<i64>,
        cache_write_input_tokens: Option<i64>,
        output_tokens: Option<i64>,
        reasoning_output_tokens: Option<i64>,
        model_context_window: Option<i64>,
        observed_at: i64,
    ) -> Self {
        Self {
            execution_id,
            provider_id,
            cumulative_total_tokens,
            input_tokens,
            cached_input_tokens,
            cache_write_input_tokens,
            output_tokens,
            reasoning_output_tokens,
            model_context_window,
            observed_at,
        }
    }

    pub fn execution_id(&self) -> &str {
        &self.execution_id
    }

    /// 返回 event 的 Provider 公共身份。
    pub fn provider_id(&self) -> &ProviderId {
        &self.provider_id
    }

    /// 返回 Provider 原样提供的累计 total，不从 breakdown 计算。
    pub const fn cumulative_total_tokens(&self) -> i64 {
        self.cumulative_total_tokens
    }

    /// 返回 Provider 的可选累计输入 breakdown。
    pub const fn input_tokens(&self) -> Option<i64> {
        self.input_tokens
    }

    /// 返回 Provider 的可选累计缓存输入 breakdown。
    pub const fn cached_input_tokens(&self) -> Option<i64> {
        self.cached_input_tokens
    }

    /// 返回 Provider 的可选累计缓存写入输入 breakdown。
    pub const fn cache_write_input_tokens(&self) -> Option<i64> {
        self.cache_write_input_tokens
    }

    /// 返回 Provider 的可选累计输出 breakdown。
    pub const fn output_tokens(&self) -> Option<i64> {
        self.output_tokens
    }

    /// 返回 Provider 的可选累计 reasoning 输出 breakdown。
    pub const fn reasoning_output_tokens(&self) -> Option<i64> {
        self.reasoning_output_tokens
    }

    /// 返回 Provider 的可选模型上下文窗口 metadata。
    pub const fn model_context_window(&self) -> Option<i64> {
        self.model_context_window
    }

    /// 返回 SerenaDesktop 观察到通知的时刻。
    pub const fn observed_at(&self) -> i64 {
        self.observed_at
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn telemetry_event_is_a_closed_activity_or_usage_set() {
        let events = [
            AgentTelemetryEvent::Activity(AgentActivityEvent::provider("execution-1".into(), 1)),
            AgentTelemetryEvent::Usage(UsageEvent::cumulative(
                "execution-1".into(),
                ProviderId::new("codex".into()).unwrap(),
                16,
                Some(11),
                Some(12),
                None,
                Some(14),
                Some(15),
                Some(258400),
                20,
            )),
        ];

        assert!(matches!(events[0], AgentTelemetryEvent::Activity(_)));
        assert!(matches!(events[1], AgentTelemetryEvent::Usage(_)));
    }

    #[test]
    fn usage_event_keeps_only_safe_cumulative_fields() {
        let event = UsageEvent::cumulative(
            "execution-1".into(),
            ProviderId::new("codex".into()).unwrap(),
            16,
            Some(11),
            Some(12),
            None,
            Some(14),
            Some(15),
            Some(258400),
            20,
        );

        assert_eq!(event.execution_id(), "execution-1");
        assert_eq!(event.provider_id().as_str(), "codex");
        assert_eq!(event.cumulative_total_tokens(), 16);
        assert_eq!(event.cache_write_input_tokens(), None);
        assert_eq!(event.model_context_window(), Some(258400));
        assert_eq!(event.observed_at(), 20);
    }

    #[test]
    fn activity_constructors_enforce_the_phase_and_tool_category_pairing() {
        let provider = AgentActivityEvent::provider("execution-1".into(), 10);
        assert_eq!(provider.execution_id(), "execution-1");
        assert_eq!(provider.phase(), ActivityPhase::Provider);
        assert_eq!(provider.tool_category(), None);
        assert_eq!(provider.observed_at(), 10);

        let tool = AgentActivityEvent::tool("execution-2".into(), ToolCategory::Test, 20);
        assert_eq!(tool.execution_id(), "execution-2");
        assert_eq!(tool.phase(), ActivityPhase::Tool);
        assert_eq!(tool.tool_category(), Some(ToolCategory::Test));
        assert_eq!(tool.observed_at(), 20);
    }
}
