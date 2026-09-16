use std::{future::Future, pin::Pin, sync::Arc};

use super::{
    ProviderCancelContext, ProviderCapabilities, ProviderDescriptor, ProviderError,
    ProviderExecutionContext, ProviderRunResult, ProviderStartupContext,
    telemetry::AgentTelemetryEvent,
};

pub type ProviderFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, PartialEq, Eq)]
pub enum ProviderExecutionFailure {
    State(String),
    Runtime { code: String, message: String },
}

impl From<String> for ProviderExecutionFailure {
    fn from(error: String) -> Self {
        Self::State(error)
    }
}

pub trait AgentEventSink: Send + Sync {
    fn publish<'a>(&'a self, _event: AgentTelemetryEvent) -> ProviderFuture<'a, ()> {
        Box::pin(async {})
    }
}

pub trait ProviderAcceptanceSink: Send + Sync {
    fn accepted(&self);
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderReconcileSummary {
    pub items: Vec<ProviderReconcileItem>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderReconcileItem {
    pub subject_id: String,
    pub kind: ProviderReconcileKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderContinuationContext {
    pub source_execution_id: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderContinuationDecision {
    Eligible,
    Ineligible,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderReconcileKind {
    OrphanResourceRecovered,
    OrphanResourceUnknown,
    ExecutionReleased,
    ExecutionInconsistent,
    ExecutionPendingExplicitResume,
    ExecutionUnknown,
    ExecutionProviderFailure,
    ExecutionInterrupted,
}

pub trait AgentProvider: Send + Sync {
    fn descriptor(&self) -> ProviderDescriptor;

    fn capabilities(&self) -> ProviderCapabilities;

    fn execute<'a>(
        &'a self,
        context: ProviderExecutionContext,
        acceptance: Arc<dyn ProviderAcceptanceSink>,
        telemetry: Arc<dyn AgentEventSink>,
    ) -> ProviderFuture<'a, Result<ProviderRunResult, ProviderExecutionFailure>>;

    fn cancel<'a>(
        &'a self,
        context: ProviderCancelContext,
    ) -> ProviderFuture<'a, Result<(), ProviderError>>;

    fn validate_continuation<'a>(
        &'a self,
        _context: ProviderContinuationContext,
    ) -> ProviderFuture<'a, Result<ProviderContinuationDecision, ProviderError>> {
        Box::pin(async {
            Err(ProviderError {
                code: super::ProviderErrorCode::AgentProviderCapabilityUnsupported,
            })
        })
    }

    fn startup_reconcile<'a>(
        &'a self,
        context: ProviderStartupContext,
    ) -> ProviderFuture<'a, Result<ProviderReconcileSummary, ProviderError>>;
}

#[cfg(test)]
mod tests;
