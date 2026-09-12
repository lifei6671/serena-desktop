//! Internal Work product contract. Adapters supply the current workspace snapshot.
use super::{
    coordinator::now,
    store::{StateStore, WorkRunRecord, transactions::product::WorkspaceSnapshot},
    task_manager::AgentTaskManager,
};

#[derive(Clone, Debug)]
pub enum QueryAction {
    Get {
        work_run_id: String,
    },
    List {
        workspace_id: Option<String>,
        limit: Option<u32>,
    },
}

#[derive(Clone, Debug)]
pub enum UpdateAction {
    Begin {
        workspace_id: String,
        title: String,
        goal: Option<String>,
    },
    Finish {
        work_run_id: String,
        outcome: FinishOutcome,
        acceptance: Option<HostAcceptance>,
    },
    Cancel {
        work_run_id: String,
    },
}

#[derive(Clone, Copy, Debug)]
pub enum FinishOutcome {
    Completed,
    Failed,
}

#[derive(Clone, Debug)]
pub struct HostAcceptance {
    pub summary: String,
    pub execution_ids: Vec<String>,
}

pub(crate) enum TerminalAction {
    Finish {
        outcome: FinishOutcome,
        acceptance: Option<HostAcceptance>,
    },
    Cancel,
}

#[derive(Debug, PartialEq, Eq)]
pub enum QueryData {
    WorkRun(WorkRunRecord),
    List { work_runs: Vec<WorkRunRecord> },
}

pub struct WorkProductService {
    store: StateStore,
}

impl WorkProductService {
    pub fn new(store: StateStore) -> Self {
        Self { store }
    }

    pub async fn query(&self, action: QueryAction) -> Result<QueryData, String> {
        match action {
            QueryAction::Get { work_run_id } => {
                Ok(QueryData::WorkRun(self.get(work_run_id).await?))
            }
            QueryAction::List {
                workspace_id,
                limit,
            } => {
                if let Some(id) = &workspace_id {
                    validate_id(id)?;
                }
                let limit = limit.unwrap_or(20);
                if !(1..=100).contains(&limit) {
                    return Err("WORK_INVALID_ARGUMENT".into());
                }
                Ok(QueryData::List {
                    work_runs: self
                        .store
                        .list_work_runs(workspace_id, limit as usize)
                        .await?,
                })
            }
        }
    }

    pub async fn update(
        &self,
        action: UpdateAction,
        workspace: Option<WorkspaceSnapshot>,
    ) -> Result<WorkRunRecord, String> {
        match action {
            UpdateAction::Begin {
                workspace_id,
                title,
                goal,
            } => {
                validate_id(&workspace_id)?;
                let title = title.trim();
                if title.is_empty() {
                    return Err("WORK_INVALID_ARGUMENT".into());
                }
                let workspace = workspace
                    .filter(|current| current.id == workspace_id && !current.root.trim().is_empty())
                    .ok_or("WORKSPACE_CONTEXT_MISMATCH")?;
                let id = AgentTaskManager::id("work");
                self.store
                    .create_work_run(
                        id.clone(),
                        workspace_id,
                        workspace.root,
                        title.into(),
                        goal,
                        now(),
                    )
                    .await?;
                self.get(id).await
            }
            UpdateAction::Finish {
                work_run_id,
                outcome,
                acceptance,
            } => {
                validate_id(&work_run_id)?;
                self.store
                    .work_terminal(
                        work_run_id,
                        TerminalAction::Finish {
                            outcome,
                            acceptance,
                        },
                        now(),
                    )
                    .await
            }
            UpdateAction::Cancel { work_run_id } => {
                validate_id(&work_run_id)?;
                self.store
                    .work_terminal(work_run_id, TerminalAction::Cancel, now())
                    .await
            }
        }
    }

    async fn get(&self, id: String) -> Result<WorkRunRecord, String> {
        validate_id(&id)?;
        self.store
            .work_run(id)
            .await?
            .ok_or_else(|| "WORK_NOT_FOUND".into())
    }
}

// IDs are opaque: reject empty, whitespace and control characters without
// imposing a prefix or parsing the local ID generator's format.
pub(crate) fn validate_id(id: &str) -> Result<(), String> {
    if id.is_empty() || id.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("WORK_INVALID_ARGUMENT".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests;
