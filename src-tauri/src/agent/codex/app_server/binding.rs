//! Sealed, read-only Execution snapshot. No caller-supplied protocol identities.
use super::*;
use crate::agent::store::{ExecutionRecord, StateStore};
#[derive(Clone)]
pub(super) struct ExecutionProtocolBinding {
    store: StateStore,
    record: ExecutionRecord,
}
impl std::fmt::Debug for ExecutionProtocolBinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("ExecutionProtocolBinding")
            .field(&self.record)
            .finish()
    }
}
impl ExecutionProtocolBinding {
    pub(super) async fn load(store: &StateStore, id: &str) -> Result<Self> {
        let record = store
            .execution(id.into())
            .await
            .map_err(|e| ProtocolError::new("CODEX_EXECUTION_STORE_FAILED", e))?
            .ok_or_else(|| ProtocolError::new("CODEX_EXECUTION_NOT_FOUND", "Execution missing"))?;
        if record.id != id
            || record
                .runtime_instance_id
                .as_deref()
                .is_none_or(str::is_empty)
            || record.thread_id.as_deref().is_none_or(str::is_empty)
        {
            return Err(ProtocolError::new(
                "CODEX_EXECUTION_BINDING_INVALID",
                "Execution Runtime/Thread identity missing",
            ));
        }
        Ok(Self {
            store: store.clone(),
            record,
        })
    }
    pub(super) fn record(&self) -> &ExecutionRecord {
        &self.record
    }
    pub(super) async fn validate_current(&self) -> Result<()> {
        let current = self
            .store
            .execution(self.record.id.clone())
            .await
            .map_err(|e| ProtocolError::new("CODEX_EXECUTION_STORE_FAILED", e))?;
        if current.as_ref() != Some(&self.record) {
            return Err(ProtocolError::new(
                "CODEX_EXECUTION_BINDING_STALE",
                "Execution snapshot changed; discard protocol result",
            ));
        }
        Ok(())
    }
}
