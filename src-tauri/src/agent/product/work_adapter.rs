//! Typed internal adapters for the future Agent query/execute tools. No transport registration.
use super::work_context::VersionedContext;
use super::*;
use crate::agent::store::transactions::product::WorkExecutionContext;

#[derive(Clone, Debug)]
pub enum AgentQueryAction {
    Get {
        execution_id: String,
        include_result: Option<bool>,
    },
    List {
        work_run_id: String,
        limit: Option<u32>,
    },
    Observe {
        execution_id: String,
        known_revision: Option<String>,
        wait_ms: Option<u32>,
        include_result: Option<bool>,
    },
}

#[derive(Clone, Debug)]
pub enum AgentExecuteAction {
    Start {
        work_run_id: String,
        request_key: String,
        prompt: String,
        delegation_context_json: Option<String>,
    },
    Continue {
        work_run_id: String,
        parent_execution_id: String,
        request_key: String,
        prompt: String,
        delegation_context_json: Option<String>,
    },
    Cancel {
        work_run_id: String,
        execution_id: String,
    },
    ResumePending {
        work_run_id: String,
        execution_id: String,
    },
}

impl AgentProductService {
    pub async fn agent_query(&self, action: AgentQueryAction) -> Result<ProductData, ProductError> {
        match action {
            AgentQueryAction::Get {
                execution_id,
                include_result,
            } => {
                validate_id(&execution_id)?;
                Ok(ProductData::Execution(Box::new(
                    self.observe(execution_id, include_result.unwrap_or(false))
                        .await?,
                )))
            }
            AgentQueryAction::List { work_run_id, limit } => {
                validate_id(&work_run_id)?;
                let limit = limit.unwrap_or(20);
                if !(1..=100).contains(&limit) {
                    return Err(invalid_argument());
                }
                self.store
                    .work_run(work_run_id.clone())
                    .await?
                    .ok_or_else(|| ProductError::from("WORK_NOT_FOUND".to_string()))?;
                let links = self.store.work_execution_links(work_run_id).await?;
                let mut executions = Vec::new();
                for link in links.into_iter().take(limit as usize) {
                    executions.push(self.observe(link.execution_id, false).await?);
                }
                Ok(ProductData::List { executions })
            }
            AgentQueryAction::Observe {
                execution_id,
                known_revision,
                wait_ms,
                include_result,
            } => {
                validate_id(&execution_id)?;
                let wait_ms = wait_ms.unwrap_or(15_000);
                if wait_ms > 20_000 || known_revision.as_ref().is_some_and(|r| r.trim().is_empty())
                {
                    return Err(invalid_argument());
                }
                Ok(ProductData::Execution(Box::new(
                    self.observe_wait(
                        execution_id,
                        known_revision,
                        wait_ms,
                        include_result.unwrap_or(false),
                        WakeOn::Control,
                    )
                    .await?,
                )))
            }
        }
    }

    pub async fn agent_execute(
        &self,
        action: AgentExecuteAction,
        workspace: Option<WorkspaceSnapshot>,
    ) -> Result<ExecutionView, ProductError> {
        let (mut action, mut work) = match action {
            AgentExecuteAction::Start {
                work_run_id,
                request_key,
                prompt,
                delegation_context_json,
            } => {
                validate_id(&work_run_id)?;
                validate_submission(&request_key, &prompt)?;
                let work = self
                    .store
                    .work_run(work_run_id.clone())
                    .await?
                    .ok_or_else(|| ProductError::from("WORK_NOT_FOUND".to_string()))?;
                (
                    Action::Start {
                        agent_id: work_run_id.clone(),
                        workspace_id: work.workspace_id,
                        request_key,
                        prompt,
                    },
                    Some(WorkExecutionContext {
                        work_run_id,
                        parent_execution_id: None,
                        delegation_context_json,
                    }),
                )
            }
            AgentExecuteAction::Continue {
                work_run_id,
                parent_execution_id,
                request_key,
                prompt,
                delegation_context_json,
            } => {
                validate_id(&work_run_id)?;
                validate_id(&parent_execution_id)?;
                validate_submission(&request_key, &prompt)?;
                self.work_membership(&work_run_id, &parent_execution_id)
                    .await?;
                (
                    Action::Continue {
                        execution_id: parent_execution_id.clone(),
                        request_key,
                        prompt,
                    },
                    Some(WorkExecutionContext {
                        work_run_id,
                        parent_execution_id: Some(parent_execution_id),
                        delegation_context_json,
                    }),
                )
            }
            AgentExecuteAction::Cancel {
                work_run_id,
                execution_id,
            } => {
                validate_id(&work_run_id)?;
                validate_id(&execution_id)?;
                self.work_membership(&work_run_id, &execution_id).await?;
                let row = self.manager.cancel(&execution_id).await?;
                if row.status == "unknown" {
                    return Err("AGENT_MANUAL_RESOLUTION_REQUIRED".to_string().into());
                }
                return self
                    .observe(execution_id.clone(), false)
                    .await
                    .map_err(|error| ProductError::accepted(error, execution_id));
            }
            AgentExecuteAction::ResumePending {
                work_run_id,
                execution_id,
            } => {
                validate_id(&work_run_id)?;
                validate_id(&execution_id)?;
                self.work_membership(&work_run_id, &execution_id).await?;
                self.active_work(&work_run_id).await?;
                (Action::ResumePending { execution_id }, None)
            }
        };
        if let Some(work) = &mut work {
            let context = match work
                .delegation_context_json
                .as_deref()
                .map(VersionedContext::parse)
                .transpose()
            {
                Ok(context) => context,
                Err(error) => {
                    // Invalid new input is rejected, but a changed request using an
                    // existing key must still report the frozen-request conflict.
                    self.store
                        .product_work_preflight(action.clone(), work.clone())
                        .await
                        .map_err(|e| submission_error(e.into()))?;
                    return Err(error.into());
                }
            };
            if let Some(context) = &context {
                work.delegation_context_json = Some(context.canonical_json());
                let prompt = match &mut action {
                    Action::Start { prompt, .. } | Action::Continue { prompt, .. } => prompt,
                    _ => unreachable!("only submissions carry new context"),
                };
                *prompt = context.prompt(prompt);
            }
            if let Some(id) = self
                .store
                .product_work_preflight(action.clone(), work.clone())
                .await
                .map_err(|e| submission_error(e.into()))?
            {
                return self
                    .observe(id.clone(), false)
                    .await
                    .map_err(|e| ProductError::accepted(e, id));
            }
            let current = self.active_work(&work.work_run_id).await?;
            if let Some(context) = context {
                context.verify(current.canonical_workspace_root).await?;
            }
        }
        // The existing owned submit worker handles durable creation, handoff and
        // dispatch. Dropping this adapter's wait does not drop that worker.
        let id = self
            .manager
            .product_submit_with_work(action, workspace, work)
            .await
            .map_err(submission_error)?;
        self.observe(id.clone(), false)
            .await
            .map_err(|e| ProductError::accepted(e, id))
    }

    async fn active_work(
        &self,
        id: &str,
    ) -> Result<crate::agent::store::WorkRunRecord, ProductError> {
        let work = self
            .store
            .work_run(id.into())
            .await?
            .ok_or_else(|| ProductError::from("WORK_NOT_FOUND".to_string()))?;
        if work.status != "active" {
            return Err("WORK_NOT_ACTIVE".to_string().into());
        }
        Ok(work)
    }

    async fn work_membership(&self, work: &str, execution: &str) -> Result<(), ProductError> {
        let link = self.store.work_execution_link(execution.into()).await?;
        if link.as_ref().is_none_or(|l| l.work_run_id != work) {
            return Err("EXECUTION_NOT_IN_WORK".to_string().into());
        }
        Ok(())
    }
}

// Work exposes the frozen request conflict directly; legacy Product mapping stays intact.
fn submission_error(mut error: ProductError) -> ProductError {
    if error.message.starts_with("EXECUTION_REQUEST_KEY_CONFLICT") {
        error.code = "EXECUTION_REQUEST_KEY_CONFLICT".into();
    }
    error
}

fn invalid_argument() -> ProductError {
    "WORK_INVALID_ARGUMENT".to_string().into()
}

fn validate_id(id: &str) -> Result<(), ProductError> {
    if id.is_empty() || id.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(invalid_argument());
    }
    Ok(())
}

fn validate_submission(key: &str, prompt: &str) -> Result<(), ProductError> {
    if key.trim().is_empty() || prompt.trim().is_empty() {
        return Err(invalid_argument());
    }
    Ok(())
}
