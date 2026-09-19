use crate::agent::{
    provider::{
        port::{AgentEventSink, ProviderFuture},
        telemetry::AgentTelemetryEvent,
    },
    store::StateStore,
};

pub(crate) struct ExecutionTelemetryProjector {
    store: StateStore,
    execution_id: String,
}

impl ExecutionTelemetryProjector {
    pub(crate) fn new(store: StateStore, execution_id: String) -> Self {
        Self {
            store,
            execution_id,
        }
    }
}

impl AgentEventSink for ExecutionTelemetryProjector {
    fn publish(&self, event: AgentTelemetryEvent) -> ProviderFuture<'_, ()> {
        Box::pin(async move {
            match event {
                AgentTelemetryEvent::Activity(activity)
                    if activity.execution_id() == self.execution_id =>
                {
                    if self
                        .store
                        .project_execution_activity(
                            self.execution_id.clone(),
                            activity.phase(),
                            activity.tool_category(),
                            activity.observed_at(),
                        )
                        .await
                        .is_err()
                    {
                        eprintln!("Agent activity projection dropped");
                    }
                }
                AgentTelemetryEvent::Usage(usage) if usage.execution_id() == self.execution_id => {
                    if self.store.project_execution_usage(usage).await.is_err() {
                        eprintln!("Agent usage projection dropped");
                    }
                }
                AgentTelemetryEvent::Activity(_) | AgentTelemetryEvent::Usage(_) => {}
            }
        })
    }
}

#[cfg(test)]
mod tests;
