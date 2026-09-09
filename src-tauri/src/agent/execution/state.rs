//! Database state contracts; these types do not collect Runtime or Provider evidence.
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    DispatchPending,
    Running,
    CancelRequested,
    Cancelling,
    Finalizing,
    Reconciling,
    Completed,
    Failed,
    Cancelled,
    Interrupted,
    Unknown,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DispatchPending => "dispatch_pending",
            Self::Running => "running",
            Self::CancelRequested => "cancel_requested",
            Self::Cancelling => "cancelling",
            Self::Finalizing => "finalizing",
            Self::Reconciling => "reconciling",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Interrupted => "interrupted",
            Self::Unknown => "unknown",
        }
    }
    pub fn terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Cancelled | Self::Interrupted
        )
    }
    /// Section 21 graph only. Event-specific evidence is checked in the store.
    pub fn allows(self, next: Self) -> bool {
        use Status::*;
        matches!(
            (self, next),
            (
                DispatchPending,
                Running | Finalizing | Reconciling | Cancelled
            ) | (Running, CancelRequested | Finalizing | Reconciling)
                | (CancelRequested, Cancelling | Finalizing | Reconciling)
                | (Cancelling, Finalizing | Reconciling)
                | (
                    Finalizing,
                    Reconciling | Completed | Failed | Cancelled | Interrupted
                )
                | (
                    Reconciling,
                    Completed | Failed | Cancelled | Interrupted | Unknown
                )
                | (Unknown, Reconciling)
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DispatchState {
    NotDispatched,
    Dispatching,
    Dispatched,
    Uncertain,
}
impl DispatchState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotDispatched => "not_dispatched",
            Self::Dispatching => "dispatching",
            Self::Dispatched => "dispatched",
            Self::Uncertain => "uncertain",
        }
    }
    pub fn allows(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::NotDispatched, Self::Dispatching)
                | (Self::Dispatching, Self::Dispatched | Self::Uncertain)
        )
    }
}

#[derive(Debug, Clone)]
pub enum RecoveryBasis {
    /// References newly persisted Job-level evidence, not a caller's boolean.
    RuntimeTermination {
        runtime_id: String,
        evidence_at: i64,
    },
    /// Internal local Resolve initiation only; this never releases a Claim.
    LocalResolve { diagnostic: String },
}

#[derive(Debug, Clone)]
pub enum Transition {
    Running,
    RequestCancel,
    InterruptAck,
    InterruptTimeout {
        diagnostic: String,
    },
    ProviderTerminal {
        runtime_id: String,
        status: Status,
    },
    /// Produced by a future verified same-Runtime, all-pages cleanup collector.
    CleanupEmpty {
        runtime_id: String,
    },
    Reconcile,
    MarkUnknown,
    ResumeRecovery(RecoveryBasis),
    Dispatch {
        to: DispatchState,
        runtime_id: Option<String>,
    },
}

#[derive(Debug, Clone, Copy)]
pub enum ReleaseBasis {
    SameRuntimeCleanup,
    RuntimeTerminated,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResultCompleteness {
    Unknown,
    Partial,
    Complete,
}

#[derive(Debug, Clone)]
pub struct Finalization {
    pub terminal: Status,
    pub basis: ReleaseBasis,
    pub result: Option<Value>,
    pub completeness: ResultCompleteness,
}
