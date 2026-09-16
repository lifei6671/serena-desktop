//! Internal execution and exact-id cancellation entry points. No scheduler.
#[cfg(test)]
use super::codex::provider::CodexProvider;
use super::{
    codex::provider::register_codex_provider_with_discovery,
    coordinator::WorkspaceExecutionCoordinator,
    execution::{CreateExecutionInput, ExecutionMode, canonicalize_request},
    provider::{
        ProviderCancelContext, ProviderError, ProviderErrorCode, ProviderExecutionContext,
        ProviderId, ProviderStartupContext,
        port::{
            ProviderAcceptanceSink, ProviderContinuationContext, ProviderContinuationDecision,
            ProviderExecutionFailure, ProviderReconcileItem,
        },
        registry::{ProviderHealth, ProviderRegistry},
    },
    store::{
        StateStore,
        transactions::{
            CreateOutcome,
            product::{ContinuationCandidate, ContinuationPreflight},
        },
    },
    telemetry_projector::ExecutionTelemetryProjector,
};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

pub mod recovery;

#[derive(Clone)]
pub struct AgentTaskManager {
    store: StateStore,
    executable: PathBuf,
    pub(crate) backend_error: Option<String>,
    owner: String,
    pub(crate) runtime_pool: std::sync::Arc<super::codex::pool::CodexRuntimePool>,
    registry: Arc<Mutex<Option<Arc<ProviderRegistry>>>>,
    #[cfg(test)]
    pub(crate) test_handoff: Option<std::sync::Arc<(tokio::sync::Notify, tokio::sync::Notify)>>,
    #[cfg(test)]
    pub(crate) test_client: Option<(std::sync::Arc<super::codex::app_server::Client>, PathBuf)>,
}

enum AcceptanceState {
    Pending(Option<tokio::sync::oneshot::Sender<Result<(), String>>>),
    Accepted,
    Closed,
}

struct HostAcceptanceSink(Mutex<AcceptanceState>);

impl HostAcceptanceSink {
    fn new(receipt: Option<tokio::sync::oneshot::Sender<Result<(), String>>>) -> Self {
        Self(Mutex::new(AcceptanceState::Pending(receipt)))
    }

    fn is_accepted(&self) -> bool {
        matches!(*self.0.lock().unwrap(), AcceptanceState::Accepted)
    }

    fn reject(&self, error: String) {
        let receipt = {
            let mut state = self.0.lock().unwrap();
            match std::mem::replace(&mut *state, AcceptanceState::Closed) {
                AcceptanceState::Pending(receipt) => receipt,
                AcceptanceState::Accepted => {
                    *state = AcceptanceState::Accepted;
                    None
                }
                AcceptanceState::Closed => None,
            }
        };
        if let Some(receipt) = receipt {
            let _ = receipt.send(Err(error));
        }
    }
}

impl ProviderAcceptanceSink for HostAcceptanceSink {
    fn accepted(&self) {
        let receipt = {
            let mut state = self.0.lock().unwrap();
            match std::mem::replace(&mut *state, AcceptanceState::Accepted) {
                AcceptanceState::Pending(receipt) => receipt,
                AcceptanceState::Accepted => {
                    *state = AcceptanceState::Accepted;
                    None
                }
                AcceptanceState::Closed => {
                    *state = AcceptanceState::Closed;
                    None
                }
            }
        };
        if let Some(receipt) = receipt {
            let _ = receipt.send(Ok(()));
        }
    }
}

fn provider_error_code(code: ProviderErrorCode) -> &'static str {
    match code {
        ProviderErrorCode::AgentProviderNotFound => "AGENT_PROVIDER_NOT_FOUND",
        ProviderErrorCode::AgentProviderUnavailable => "AGENT_PROVIDER_UNAVAILABLE",
        ProviderErrorCode::AgentProviderCapabilityUnsupported => {
            "AGENT_PROVIDER_CAPABILITY_UNSUPPORTED"
        }
        ProviderErrorCode::AgentProviderContractError => "AGENT_PROVIDER_CONTRACT_ERROR",
        ProviderErrorCode::AgentProviderOperationFailed => "AGENT_PROVIDER_OPERATION_FAILED",
    }
}

fn provider_failure(error: ProviderError) -> ProviderExecutionFailure {
    provider_error_code(error.code).to_string().into()
}

fn provider_id(value: String) -> Result<ProviderId, ProviderExecutionFailure> {
    ProviderId::new(value)
        .map_err(|_| ProviderExecutionFailure::State("AGENT_PROVIDER_CONTRACT_ERROR".to_string()))
}

fn continuation_provider_error(error: ProviderError) -> String {
    match error.code {
        ProviderErrorCode::AgentProviderNotFound
        | ProviderErrorCode::AgentProviderCapabilityUnsupported => {
            "AGENT_CONTINUE_NOT_ALLOWED".into()
        }
        code => provider_error_code(code).into(),
    }
}

impl AgentTaskManager {
    async fn validate_continuation_candidate(
        &self,
        candidate: &ContinuationCandidate,
    ) -> Result<(), String> {
        self.validate_continuation_source(&candidate.source_execution_id, &candidate.provider_id)
            .await
    }

    async fn validate_continuation_source(
        &self,
        source_execution_id: &str,
        provider_value: &str,
    ) -> Result<(), String> {
        let provider_id = ProviderId::new(provider_value.into())
            .map_err(|_| "AGENT_CONTINUE_NOT_ALLOWED".to_string())?;
        let provider = self
            .registry()
            .map_err(continuation_provider_error)?
            .get_registered(&provider_id)
            .map_err(continuation_provider_error)?;
        if !provider.capabilities().can_continue {
            return Err("AGENT_CONTINUE_NOT_ALLOWED".into());
        }
        match provider
            .validate_continuation(ProviderContinuationContext {
                source_execution_id: source_execution_id.into(),
            })
            .await
            .map_err(continuation_provider_error)?
        {
            ProviderContinuationDecision::Eligible => Ok(()),
            ProviderContinuationDecision::Ineligible => Err("AGENT_CONTINUE_NOT_ALLOWED".into()),
        }
    }

    pub(crate) async fn can_continue(
        &self,
        source_execution_id: String,
        provider_id: String,
    ) -> bool {
        self.validate_continuation_source(&source_execution_id, &provider_id)
            .await
            .is_ok()
    }

    pub async fn cancel(
        &self,
        execution_id: &str,
    ) -> Result<super::store::ExecutionRecord, String> {
        let row = self
            .store
            .execution(execution_id.into())
            .await?
            .ok_or("EXECUTION_NOT_FOUND")?;
        let provider_id = ProviderId::new(row.provider)
            .map_err(|_| "AGENT_PROVIDER_CONTRACT_ERROR".to_string())?;
        let provider = self
            .registry()
            .map_err(|error| provider_error_code(error.code).to_string())?
            .get_registered(&provider_id)
            .map_err(|error| provider_error_code(error.code).to_string())?;
        if !provider.capabilities().can_cancel {
            return Err("AGENT_PROVIDER_CAPABILITY_UNSUPPORTED".into());
        }
        provider
            .cancel(ProviderCancelContext {
                execution_id: execution_id.into(),
            })
            .await
            .map_err(|error| provider_error_code(error.code).to_string())?;
        self.store
            .execution(execution_id.into())
            .await?
            .ok_or_else(|| "EXECUTION_NOT_FOUND".into())
    }
    pub fn new(store: StateStore, executable: PathBuf) -> Self {
        Self {
            store,
            executable,
            backend_error: None,
            owner: Self::id("host"),
            runtime_pool: Default::default(),
            registry: Default::default(),
            #[cfg(test)]
            test_handoff: None,
            #[cfg(test)]
            test_client: None,
        }
    }
    fn build_registry(&self) -> Result<ProviderRegistry, ProviderError> {
        let mut registry = ProviderRegistry::new();
        let discovery = self
            .backend_error
            .clone()
            .map_or_else(|| Ok(self.executable.clone()), Err);
        register_codex_provider_with_discovery(
            &mut registry,
            self.store.clone(),
            self.owner.clone(),
            self.runtime_pool.clone(),
            discovery,
        )?;
        Ok(registry)
    }
    fn registry(&self) -> Result<Arc<ProviderRegistry>, ProviderError> {
        let mut current = self.registry.lock().unwrap();
        if let Some(registry) = current.as_ref() {
            return Ok(registry.clone());
        }
        let registry = Arc::new(self.build_registry()?);
        *current = Some(registry.clone());
        Ok(registry)
    }
    pub(crate) async fn reconcile_startup(&mut self) -> Result<Vec<ProviderReconcileItem>, String> {
        let registry = self
            .registry
            .lock()
            .unwrap()
            .take()
            .map_or_else(
                || self.build_registry(),
                |registry| {
                    Arc::try_unwrap(registry).map_err(|_| ProviderError {
                        code: ProviderErrorCode::AgentProviderContractError,
                    })
                },
            )
            .map_err(|error| provider_error_code(error.code).to_string())?;
        let mut registry = registry;
        let mut report = Vec::new();
        for descriptor in registry.list_descriptors() {
            let id = descriptor.id;
            let provider = registry
                .get_registered(&id)
                .map_err(|error| provider_error_code(error.code).to_string())?;
            if !provider.capabilities().can_recover {
                continue;
            }
            match provider.startup_reconcile(ProviderStartupContext {}).await {
                Ok(summary) => report.extend(summary.items),
                Err(_) => registry
                    .set_health(&id, ProviderHealth::Unavailable)
                    .map_err(|error| provider_error_code(error.code).to_string())?,
            }
        }
        *self.registry.lock().unwrap() = Some(Arc::new(registry));
        Ok(report)
    }
    #[cfg(test)]
    fn use_registry(&mut self, registry: ProviderRegistry) {
        *self.registry.lock().unwrap() = Some(Arc::new(registry));
    }
    pub(crate) fn id(prefix: &str) -> String {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        format!(
            "{prefix}-{}-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_micros(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        )
    }
    pub(crate) async fn create(
        &self,
        input: CreateExecutionInput,
    ) -> Result<CreateOutcome, String> {
        // One fresh read-only slice uses the already accepted client policy.
        if input.mode != ExecutionMode::ReadOnly
            || input.parent_execution_id.is_some()
            || input.thread_id.is_some()
            || input.execution_profile != serde_json::json!({})
        {
            return Err("TASK006_REQUIRES_FRESH_READ_ONLY_DEFAULT_PROFILE".into());
        }
        WorkspaceExecutionCoordinator {
            store: self.store.clone(),
        }
        .create(Self::id("execution"), canonicalize_request(input)?)
        .await
    }
    pub async fn execute(
        &self,
        input: CreateExecutionInput,
    ) -> Result<CreateOutcome, ProviderExecutionFailure> {
        let mut outcome = self.create(input).await?;
        if !outcome.created {
            return Ok(outcome);
        }
        outcome.execution = self
            .dispatch_pending_execution(&outcome.execution_id)
            .await?;
        Ok(outcome)
    }
    pub async fn resume_pending_execution(
        &self,
        execution_id: &str,
    ) -> Result<super::store::ExecutionRecord, ProviderExecutionFailure> {
        self.dispatch_pending_execution(execution_id).await
    }
    pub(crate) async fn product_submit(
        &self,
        action: super::product::Action,
        workspace: Option<super::store::transactions::product::WorkspaceSnapshot>,
    ) -> Result<String, super::product::ProductError> {
        self.product_submit_with_work(action, workspace, None).await
    }

    pub(crate) async fn product_submit_with_work(
        &self,
        action: super::product::Action,
        workspace: Option<super::store::transactions::product::WorkspaceSnapshot>,
        work: Option<super::store::transactions::product::WorkExecutionContext>,
    ) -> Result<String, super::product::ProductError> {
        let manager = self.clone();
        // Host owns creation through handoff even if the adapter drops its wait.
        tokio::spawn(async move {
            use super::product::Action;
            if manager.runtime_pool.stop.is_cancelled() {
                return Err("AGENT_SHUTTING_DOWN".to_string().into());
            }
            let continuation = matches!(&action, Action::Continue { .. });
            let outcome = match action {
                Action::Start {
                    workspace_id,
                    agent_id,
                    request_key,
                    prompt,
                } => Some(
                    manager
                        .store
                        .product_create_fresh_with_work(
                            Self::id("execution"),
                            agent_id,
                            request_key,
                            prompt,
                            workspace_id,
                            workspace,
                            work,
                            super::coordinator::now(),
                        )
                        .await?,
                ),
                Action::Continue {
                    execution_id,
                    request_key,
                    prompt,
                } => {
                    let mut work = work;
                    if let Some(context) = &mut work {
                        if context
                            .parent_execution_id
                            .as_ref()
                            .is_some_and(|parent| parent != &execution_id)
                        {
                            return Err("WORK_INVALID_ARGUMENT".to_string().into());
                        }
                        context.parent_execution_id = Some(execution_id.clone());
                    }
                    let candidate = match manager
                        .store
                        .product_continuation_preflight(
                            execution_id.clone(),
                            request_key.clone(),
                            prompt.clone(),
                            work.clone(),
                        )
                        .await?
                    {
                        ContinuationPreflight::Existing(id) => return Ok(id),
                        ContinuationPreflight::Candidate(candidate) => candidate,
                    };
                    manager.validate_continuation_candidate(&candidate).await?;
                    Some(
                        manager
                            .store
                            .product_create_continuation_with_work(
                                Self::id("execution"),
                                execution_id,
                                request_key,
                                prompt,
                                work,
                                Some(candidate.source_revision),
                                super::coordinator::now(),
                            )
                            .await?,
                    )
                }
                Action::ResumePending { execution_id } => {
                    if let Some(error) = &manager.backend_error {
                        return Err(super::product::ProductError::new(
                            error.clone(),
                            Some(execution_id),
                        ));
                    }
                    let (tx, rx) = tokio::sync::oneshot::channel();
                    let id = execution_id.clone();
                    tokio::spawn(async move {
                        manager.dispatch_with_receipt(&id, Some(tx), false).await
                    });
                    rx.await.map_err(|e| e.to_string())?.map_err(|e| {
                        super::product::ProductError::new(
                            if e == "WORKSPACE_CLAIM_INCONSISTENT" {
                                "AGENT_RESUME_NOT_ALLOWED".into()
                            } else {
                                e
                            },
                            Some(execution_id.clone()),
                        )
                    })?;
                    return Ok(execution_id);
                }
                _ => return Err("AGENT_INVALID_ARGUMENT".to_string().into()),
            }
            .unwrap();
            if outcome.created {
                #[cfg(test)]
                if let Some(hook) = &manager.test_handoff {
                    hook.0.notify_one();
                    hook.1.notified().await;
                }
                let (tx, rx) = tokio::sync::oneshot::channel();
                let id = outcome.execution_id.clone();
                tokio::spawn(async move {
                    manager
                        .dispatch_with_receipt(&id, Some(tx), continuation)
                        .await
                });
                rx.await
                    .map_err(|e| {
                        super::product::ProductError::accepted(
                            e.to_string(),
                            outcome.execution_id.clone(),
                        )
                    })?
                    .map_err(|e| {
                        super::product::ProductError::accepted(e, outcome.execution_id.clone())
                    })?;
            }
            Ok(outcome.execution_id)
        })
        .await
        .map_err(|e| e.to_string())?
    }
    async fn dispatch_pending_execution(
        &self,
        execution_id: &str,
    ) -> Result<super::store::ExecutionRecord, ProviderExecutionFailure> {
        self.dispatch_with_receipt(execution_id, None, false).await
    }
    async fn dispatch_with_receipt(
        &self,
        execution_id: &str,
        receipt: Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
        _continuation: bool,
    ) -> Result<super::store::ExecutionRecord, ProviderExecutionFailure> {
        let manager = self.clone();
        let id = execution_id.to_owned();
        let backend_error = self.backend_error.clone();
        let acceptance = Arc::new(HostAcceptanceSink::new(receipt));
        let worker_acceptance = acceptance.clone();
        // The owned worker retains the permit even if its caller stops waiting.
        // Provider/ManagedClient continue to own Runtime and Job convergence.
        let joined = tokio::spawn(async move {
            let admission = async {
                let row = manager
                    .store
                    .execution(id.clone())
                    .await?
                    .ok_or_else(|| "EXECUTION_NOT_FOUND".to_string())?;
                let permit = manager.store.guard_pending_dispatch(id.clone()).await?;
                Ok::<_, ProviderExecutionFailure>((row, permit))
            }
            .await?;
            let (row, _permit) = admission;
            let provider_id = provider_id(row.provider)?;
            let provider = manager
                .registry()
                .map_err(provider_failure)?
                .get(&provider_id)
                .map_err(provider_failure)?;
            let telemetry = Arc::new(ExecutionTelemetryProjector::new(manager.store.clone(), id.clone()));

            #[cfg(test)]
            if let Some((client, database)) = manager.test_client.clone() {
                // Test-only Runtime creation boundary; reuse the TASK-006 Fake wire pipeline.
                rusqlite::Connection::open(database).unwrap().execute(
                    "INSERT INTO runtime_instances(id,owner_host_instance_id,state,created_at,updated_at) VALUES (?1,'fixture','running',1,1)",
                    [client.runtime_id()],
                ).unwrap();
                client
                    .initialize()
                    .await
                    .map_err(|e| ProviderExecutionFailure::State(e.to_string()))?;
                let provider = CodexProvider {
                    store: manager.store.clone(),
                    executable: manager.executable.clone(),
                    backend_error: manager.backend_error.clone(),
                    owner: manager.owner.clone(),
                    runtime_pool: manager.runtime_pool.clone(),
                };
                return provider
                    .run_client_with_acceptance_and_telemetry(
                        &id,
                        &client,
                        worker_acceptance.as_ref(),
                        telemetry.as_ref(),
                    )
                    .await
                    .map_err(ProviderExecutionFailure::State);
            }

            let run = provider
                .execute(
                    ProviderExecutionContext {
                        execution_id: id.clone(),
                    },
                    worker_acceptance,
                    telemetry,
                )
                .await?;
            if run.execution_id != id {
                return Err(ProviderExecutionFailure::State(
                    "AGENT_PROVIDER_CONTRACT_ERROR".into(),
                ));
            }
            manager
                .store
                .execution(id)
                .await?
                .ok_or_else(|| "EXECUTION_NOT_FOUND".to_string().into())
        })
        .await;
        let result = match joined {
            Ok(result) => result,
            Err(_) => {
                acceptance.reject("AGENT_PROVIDER_CONTRACT_ERROR".into());
                return Err(ProviderExecutionFailure::State(
                    "AGENT_PROVIDER_CONTRACT_ERROR".into(),
                ));
            }
        };

        match result {
            Ok(row) if acceptance.is_accepted() => Ok(row),
            Ok(_) => {
                acceptance.reject("AGENT_PROVIDER_CONTRACT_ERROR".into());
                Err(ProviderExecutionFailure::State(
                    "AGENT_PROVIDER_CONTRACT_ERROR".into(),
                ))
            }
            Err(error) => {
                let receipt_error = match &error {
                    ProviderExecutionFailure::State(error)
                        if error == "AGENT_PROVIDER_UNAVAILABLE" =>
                    {
                        backend_error.unwrap_or_else(|| error.clone())
                    }
                    ProviderExecutionFailure::State(error) => error.clone(),
                    ProviderExecutionFailure::Runtime { code, message } => {
                        format!("{code}: {message}")
                    }
                };
                acceptance.reject(receipt_error);
                Err(error)
            }
        }
    }
}

#[cfg(test)]
mod tests;
