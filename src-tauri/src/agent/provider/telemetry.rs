use crate::agent::activity::{ActivityPhase, ToolCategory};

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
}

impl UsageEvent {
    pub fn new(execution_id: String) -> Self {
        Self { execution_id }
    }

    pub fn execution_id(&self) -> &str {
        &self.execution_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn telemetry_event_is_a_closed_activity_or_usage_set() {
        let events = [
            AgentTelemetryEvent::Activity(AgentActivityEvent::provider("execution-1".into(), 1)),
            AgentTelemetryEvent::Usage(UsageEvent::new("execution-1".into())),
        ];

        assert!(matches!(events[0], AgentTelemetryEvent::Activity(_)));
        assert!(matches!(events[1], AgentTelemetryEvent::Usage(_)));
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
