//! Product guards layered on the unchanged Foundation creation transaction.
use super::*;
use crate::agent::execution::{
    AgentTaskRole, CreateExecutionInput, ExecutionMode, canonicalize_request,
    legacy_pre_c2_continuation_hash,
};
use crate::agent::provider::ProviderId;
use crate::agent::usage::UsageSnapshot;

mod work;
pub use work::WorkExecutionContext;
use work::{create_with_work, require_membership, require_retry_context, validate_new_work};

#[derive(Clone, Debug)]
pub struct WorkspaceSnapshot {
    pub id: String,
    pub root: String,
    pub generation: u64,
}

/// Start 的持久化输入；同步管理协调与普通异步提交共用同一事务逻辑。
struct FreshProductCreate {
    id: String,
    agent: String,
    request_key: String,
    prompt: String,
    expected_workspace_id: String,
    workspace: Option<WorkspaceSnapshot>,
    work: Option<WorkExecutionContext>,
    now: i64,
}

/// 在一个 SQLite 事务中创建 Execution、Workspace Claim 与可选 Work link。
fn create_fresh_with_work(
    tx: &Transaction<'_>,
    creation: &FreshProductCreate,
) -> Result<CreateOutcome, String> {
    if creation
        .work
        .as_ref()
        .is_some_and(|work| work.parent_execution_id.is_some())
    {
        return Err("WORK_INVALID_ARGUMENT".into());
    }
    if let Some(row) = key(tx, &creation.agent, &creation.request_key)? {
        if row.workspace_id != creation.expected_workspace_id {
            return Err("EXECUTION_REQUEST_KEY_CONFLICT".into());
        }
        let mut retry = input(
            tx,
            &row,
            creation.request_key.clone(),
            creation.prompt.clone(),
            None,
            None,
        )?;
        if let Some(workspace) = creation
            .workspace
            .as_ref()
            .filter(|workspace| workspace.id == creation.expected_workspace_id)
        {
            retry.workspace_id = workspace.id.clone();
            retry.canonical_workspace_root = workspace.root.clone();
            retry.workspace_generation = workspace.generation;
        }
        let request = canonicalize_request(retry)?;
        if let Some(work) = &creation.work {
            require_retry_context(tx, &row.id, work)?;
        }
        return prior_outcome(tx, row, &request);
    }
    if let Some(work) = &creation.work {
        let root = creation
            .workspace
            .as_ref()
            .filter(|workspace| workspace.id == creation.expected_workspace_id)
            .map(|workspace| workspace.root.as_str());
        let generation = creation
            .workspace
            .as_ref()
            .filter(|workspace| workspace.id == creation.expected_workspace_id)
            .map(|workspace| workspace.generation);
        validate_new_work(tx, work, &creation.expected_workspace_id, root, generation)?;
    }
    let history: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM executions WHERE agent_id=?1)",
            [&creation.agent],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if history {
        return Err("AGENT_LINEAGE_CONFLICT".into());
    }
    let workspace = creation
        .workspace
        .as_ref()
        .ok_or("AGENT_NO_ACTIVE_WORKSPACE")?;
    if workspace.id != creation.expected_workspace_id {
        return Err("AGENT_WORKSPACE_CHANGED".into());
    }
    let request = canonicalize_request(CreateExecutionInput {
        agent_id: creation.agent.clone(),
        request_key: creation.request_key.clone(),
        prompt: creation.prompt.clone(),
        execution_profile: json!({}),
        workspace_id: workspace.id.clone(),
        canonical_workspace_root: workspace.root.clone(),
        workspace_generation: workspace.generation,
        provider: ProviderId::new("codex".into()).expect("static Codex provider id is valid"),
        task_role: AgentTaskRole::General,
        mode: ExecutionMode::WorkspaceWrite,
        parent_execution_id: None,
        thread_id: None,
    })?;
    create_with_work(
        tx,
        &creation.id,
        &request,
        creation.work.as_ref(),
        creation.now,
    )
}
#[derive(Debug)]
pub struct ProductSnapshot {
    pub execution: ExecutionRecord,
    /// 与 Execution 同一读取事务取得的冻结角色，不从当前路由策略推断。
    pub task_role: String,
    /// 已在 product_read 的 LEFT JOIN 中验证的公共 Usage；None 表示历史无行。
    pub usage: Option<UsageSnapshot>,
    pub thread_name: Option<String>,
    pub owns_claim: bool,
    pub runtime_attempt_exists: bool,
    pub claim_free: bool,
    pub agent_free: bool,
    pub created_at: i64,
    pub updated_at: i64,
    pub completed_at: Option<i64>,
}
pub struct ControlContext {
    pub accepted_id: Option<String>,
    pub related_id: Option<String>,
    pub related_unknown: bool,
    pub related_can_continue: bool,
    pub blocker_id: Option<String>,
}
/// Product-owned lifecycle eligibility. Provider provenance remains behind the
/// Provider port; P1-008C2 intentionally retains legacy request identity.
pub fn continuation_core_eligible(row: &ExecutionRecord) -> bool {
    if !matches!(
        row.status.as_str(),
        "completed" | "failed" | "cancelled" | "interrupted"
    ) || row.release_evidence_state != "complete"
        || row.mode != "workspace_write"
        || row.execution_profile_json != "{}"
        // Continue 只能继承父 Execution 已冻结的完整快照，绝不猜测当前 Workspace。
        || row.workspace_id.trim().is_empty()
        || row.canonical_workspace_root.trim().is_empty()
        || row.workspace_generation == 0
    {
        return false;
    }
    true
}

/// Provider-opaque handoff after Store-owned retry and lifecycle checks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ContinuationCandidate {
    pub source_execution_id: String,
    pub provider_id: String,
    pub source_revision: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ContinuationPreflight {
    Existing(String),
    Candidate(ContinuationCandidate),
}
fn key(c: &Connection, agent: &str, request: &str) -> Result<Option<ExecutionRecord>, String> {
    let id: Option<String> = c
        .query_row(
            "SELECT id FROM executions WHERE agent_id=?1 AND request_key=?2",
            params![agent, request],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    id.map(|id| {
        execution_record(c, &id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "EXECUTION_NOT_FOUND".into())
    })
    .transpose()
}
/// Product Start 固定 General；Continue 从父 Execution 的持久化角色构造请求身份。
fn input(
    tx: &Connection,
    row: &ExecutionRecord,
    key: String,
    prompt: String,
    parent_execution_id: Option<String>,
    thread: Option<String>,
) -> Result<CreateExecutionInput, String> {
    Ok(CreateExecutionInput {
        agent_id: row.agent_id.clone(),
        request_key: key,
        prompt,
        execution_profile: serde_json::from_str(&row.execution_profile_json)
            .map_err(|e| e.to_string())?,
        workspace_id: row.workspace_id.clone(),
        canonical_workspace_root: row.canonical_workspace_root.clone(),
        workspace_generation: row.workspace_generation,
        provider: serde_json::from_value(serde_json::json!(row.provider))
            .map_err(|e| e.to_string())?,
        // Start 仍为 General；Continue 从同一事务中的父行继承冻结角色。
        task_role: if parent_execution_id.is_some() {
            serde_json::from_value(serde_json::json!(persisted_task_role(tx, &row.id)?))
                .map_err(|_| "EXECUTION_REQUEST_KEY_CONFLICT".to_string())?
        } else {
            AgentTaskRole::General
        },
        mode: serde_json::from_value(serde_json::json!(row.mode)).map_err(|e| e.to_string())?,
        parent_execution_id,
        thread_id: thread,
    })
}
fn prior_outcome(
    tx: &Connection,
    row: ExecutionRecord,
    request: &CanonicalRequest,
) -> Result<CreateOutcome, String> {
    if !request_matches_prior(tx, &row, request)? {
        return Err("EXECUTION_REQUEST_KEY_CONFLICT".into());
    }
    Ok(CreateOutcome {
        execution_id: row.id.clone(),
        execution: row,
        created: false,
    })
}

/// The only pre-C2 compatibility path. A v6 row has an explicit parent and is
/// always compared through current canonicalization; only a NULL parent plus an
/// exact persisted old hash may retry through the old source-thread tuple.
fn continuation_prior_outcome(
    tx: &Connection,
    prior: ExecutionRecord,
    source: &ExecutionRecord,
    request: &CanonicalRequest,
) -> Result<CreateOutcome, String> {
    match prior_outcome(tx, prior.clone(), request) {
        Ok(outcome) => Ok(outcome),
        Err(error)
            if prior.parent_execution_id.is_none()
                && request.input().task_role == AgentTaskRole::General
                && persisted_task_role(tx, &prior.id)? == "general"
                && row_identity_matches_request(&prior, request, false) =>
        {
            if prior.request_hash
                == legacy_pre_c2_continuation_hash(request.input(), &source.thread_id)?
            {
                Ok(CreateOutcome {
                    execution_id: prior.id.clone(),
                    execution: prior,
                    created: false,
                })
            } else {
                Err(error)
            }
        }
        Err(error) => Err(error),
    }
}
impl StateStore {
    /// Resolves an exact retry before Provider validation. The canonical request
    /// uses the source execution ID as generic continuation request identity.
    pub(crate) async fn product_continuation_preflight(
        &self,
        source: String,
        request_key: String,
        prompt: String,
        work: Option<WorkExecutionContext>,
    ) -> Result<ContinuationPreflight, String> {
        self.read(move |c| {
            let tx = c.unchecked_transaction()?;
            Ok((|| -> Result<ContinuationPreflight, String> {
                if let Some(work) = &work {
                    require_membership(&tx, &source, &work.work_run_id)?;
                }
                let row = execution_record(&tx, &source)
                    .map_err(|e| e.to_string())?
                    .ok_or("EXECUTION_NOT_FOUND")?;
                let request = canonicalize_request(input(
                    &tx,
                    &row,
                    request_key.clone(),
                    prompt,
                    Some(row.id.clone()),
                    None,
                )?)?;
                if let Some(prior) = key(&tx, &row.agent_id, &request_key)? {
                    if let Some(work) = &work {
                        require_retry_context(&tx, &prior.id, work)?;
                    }
                    return Ok(ContinuationPreflight::Existing(
                        continuation_prior_outcome(&tx, prior, &row, &request)?.execution_id,
                    ));
                }
                if let Some(work) = &work {
                    validate_new_work(
                        &tx,
                        work,
                        &request.input().workspace_id,
                        Some(&request.input().canonical_workspace_root),
                        Some(request.input().workspace_generation),
                    )?;
                }
                let claimed: bool = tx
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM workspace_claims WHERE execution_id=?1)",
                        [&source],
                        |r| r.get(0),
                    )
                    .map_err(|e| e.to_string())?;
                if !continuation_core_eligible(&row) || claimed {
                    return Err("AGENT_CONTINUE_NOT_ALLOWED".into());
                }
                Ok(ContinuationPreflight::Candidate(ContinuationCandidate {
                    source_execution_id: row.id,
                    provider_id: row.provider,
                    source_revision: row.revision,
                }))
            })())
        })
        .await?
    }

    /// Desktop history cursor: creation time plus exact identity, never OFFSET.
    pub async fn product_history_ids(
        &self,
        before: Option<String>,
        workspace: Option<String>,
    ) -> Result<Vec<String>, String> {
        self.read(move |c| {
            let cursor = before.map(|id| c.query_row("SELECT created_at,id FROM executions WHERE id=?1", [id], |r| Ok((r.get::<_,i64>(0)?, r.get::<_,String>(1)?)))).transpose()?;
            let (time, id) = cursor.map_or((None, None), |(t, id)| (Some(t), Some(id)));
            let mut q = c.prepare("SELECT id FROM executions WHERE (?1 IS NULL OR created_at<?1 OR (created_at=?1 AND id<?2)) AND (?3 IS NULL OR canonical_workspace_root=?3) ORDER BY created_at DESC,id DESC LIMIT 6")?;
            q.query_map(params![time,id,workspace], |r| r.get(0))?.collect()
        }).await
    }

    /// 只读统计所有未进入产品终态的 Execution，供 Host 状态展示复用。
    pub(crate) async fn product_nonterminal_count(&self) -> Result<usize, String> {
        self.read(|connection| {
            let count: i64 = connection.query_row(
                "SELECT COUNT(*) FROM executions WHERE status NOT IN ('completed','failed','cancelled','interrupted')",
                [],
                |row| row.get(0),
            )?;
            usize::try_from(count).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Integer,
                    Box::new(error),
                )
            })
        })
        .await
    }

    /// Read-only attribution after an operation error; reuse the frozen canonical request.
    pub async fn product_control_context(
        &self,
        action: crate::agent::product::Action,
        workspace: Option<WorkspaceSnapshot>,
        error_code: String,
    ) -> Result<ControlContext, String> {
        self.read(move |c| {
            let tx = c.unchecked_transaction()?;
            Ok((|| -> Result<ControlContext, String> {
                use crate::agent::product::Action;
                let mut context = ControlContext { accepted_id: None, related_id: None, related_unknown: false, related_can_continue: false, blocker_id: None };
                let (agent, root) = match action {
                    Action::Start { agent_id, request_key, prompt, workspace_id } => {
                        if let Some(row) = key(&tx, &agent_id, &request_key)? {
                            let request = canonicalize_request(input(&tx, &row, request_key, prompt, None, None)?)?;
                            if row.workspace_id == workspace_id && request_matches_prior(&tx, &row, &request)? { context.accepted_id = Some(row.id); }
                        }
                        if let Some(id) = tx.query_row("SELECT id FROM executions WHERE agent_id=?1 ORDER BY created_at DESC,id DESC LIMIT 1", [&agent_id], |r| r.get::<_, String>(0)).optional().map_err(|e| e.to_string())?
                            && let Some(row) = execution_record(&tx, &id).map_err(|e| e.to_string())?
                        {
                            context.related_unknown = row.status == "unknown";
                            context.related_can_continue = continuation_core_eligible(&row);
                            context.related_id = Some(row.id);
                        }
                        (Some(agent_id), workspace.map(|w| w.root))
                    }
                    Action::Continue { execution_id, request_key, prompt } => {
                        if let Some(row) = execution_record(&tx, &execution_id).map_err(|e| e.to_string())? {
                            let request = canonicalize_request(input(&tx, &row, request_key.clone(), prompt, Some(row.id.clone()), None)?)?;
                            if let Some(prior) = key(&tx, &row.agent_id, &request_key)?
                                && let Ok(outcome) = continuation_prior_outcome(&tx, prior, &row, &request) { context.accepted_id = Some(outcome.execution_id); }
                            context.related_unknown = row.status == "unknown";
                            context.related_id = Some(row.id);
                            (Some(row.agent_id), Some(row.canonical_workspace_root))
                        } else { (None, None) }
                    }
                    Action::Observe { execution_id, .. } | Action::Cancel { execution_id } | Action::ResumePending { execution_id } => {
                        if let Some(row) = execution_record(&tx, &execution_id).map_err(|e| e.to_string())? {
                            context.accepted_id = Some(row.id.clone());
                            context.related_id = Some(row.id);
                            (Some(row.agent_id), Some(row.canonical_workspace_root))
                        } else { (None, None) }
                    }
                    Action::List { .. } => (None, None),
                };
                context.blocker_id = if error_code == "WORKSPACE_CLAIM_CONFLICT" {
                    tx.query_row("SELECT e.id FROM workspace_claims c JOIN executions e ON e.id=c.execution_id AND e.canonical_workspace_root=c.canonical_workspace_root WHERE c.canonical_workspace_root=?1", [root], |r| r.get(0)).optional().map_err(|e| e.to_string())?
                } else if error_code == "AGENT_BUSY" {
                    tx.query_row("SELECT id FROM executions WHERE agent_id=?1 AND status NOT IN ('completed','failed','cancelled','interrupted') ORDER BY created_at DESC,id DESC LIMIT 1", [agent], |r| r.get(0)).optional().map_err(|e| e.to_string())?
                } else { None };
                Ok(context)
            })())
        }).await?
    }
    #[allow(clippy::too_many_arguments)]
    pub async fn product_create_fresh(
        &self,
        id: String,
        agent: String,
        request_key: String,
        prompt: String,
        expected_workspace_id: String,
        workspace: Option<WorkspaceSnapshot>,
        now: i64,
    ) -> Result<CreateOutcome, String> {
        self.product_create_fresh_with_work(
            id,
            agent,
            request_key,
            prompt,
            expected_workspace_id,
            workspace,
            None,
            now,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn product_create_fresh_with_work(
        &self,
        id: String,
        agent: String,
        request_key: String,
        prompt: String,
        expected_workspace_id: String,
        workspace: Option<WorkspaceSnapshot>,
        work: Option<WorkExecutionContext>,
        now: i64,
    ) -> Result<CreateOutcome, String> {
        let creation = FreshProductCreate {
            id,
            agent,
            request_key,
            prompt,
            expected_workspace_id,
            workspace,
            work,
            now,
        };
        self.write(move |tx| create_fresh_with_work(tx, &creation))
            .await
    }

    /// 仅供 Supervisor operation mutex 持有期间的 Workspace Start 线性化调用。
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn product_create_fresh_with_work_blocking(
        &self,
        id: String,
        agent: String,
        request_key: String,
        prompt: String,
        expected_workspace_id: String,
        workspace: WorkspaceSnapshot,
        work: Option<WorkExecutionContext>,
        now: i64,
    ) -> Result<CreateOutcome, String> {
        let creation = FreshProductCreate {
            id,
            agent,
            request_key,
            prompt,
            expected_workspace_id,
            workspace: Some(workspace),
            work,
            now,
        };
        self.write_blocking(|tx| create_fresh_with_work(tx, &creation))
    }
    #[cfg(test)]
    pub(crate) async fn product_create_continuation(
        &self,
        id: String,
        source: String,
        request_key: String,
        prompt: String,
        now: i64,
    ) -> Result<CreateOutcome, String> {
        self.product_create_continuation_with_work(id, source, request_key, prompt, None, None, now)
            .await
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "stable atomic continuation creation transaction boundary; changing it for one lint would churn established call sites"
    )]
    pub(crate) async fn product_create_continuation_with_work(
        &self,
        id: String,
        source: String,
        request_key: String,
        prompt: String,
        mut work: Option<WorkExecutionContext>,
        expected_source_revision: Option<i64>,
        now: i64,
    ) -> Result<CreateOutcome, String> {
        self.write(move |tx| {
            if let Some(work) = &mut work {
                if work
                    .parent_execution_id
                    .as_ref()
                    .is_some_and(|parent| parent != &source)
                {
                    return Err("WORK_INVALID_ARGUMENT".into());
                }
                // Source is authoritative; an omitted parent is recorded as source too.
                work.parent_execution_id = Some(source.clone());
                require_membership(tx, &source, &work.work_run_id)?;
            }
            let row = execution_record(tx, &source)
                .map_err(|e| e.to_string())?
                .ok_or("EXECUTION_NOT_FOUND")?;
            let request = canonicalize_request(input(
                tx,
                &row,
                request_key.clone(),
                prompt,
                Some(row.id.clone()),
                None,
            )?)?;
            if let Some(prior) = key(tx, &row.agent_id, &request_key)? {
                if let Some(work) = &work {
                    require_retry_context(tx, &prior.id, work)?;
                }
                return continuation_prior_outcome(tx, prior, &row, &request);
            }
            if expected_source_revision.is_some_and(|revision| row.revision != revision) {
                return Err("AGENT_CONTINUE_NOT_ALLOWED".into());
            }
            if let Some(work) = &work {
                validate_new_work(
                    tx,
                    work,
                    &request.input().workspace_id,
                    Some(&request.input().canonical_workspace_root),
                    Some(request.input().workspace_generation),
                )?;
            }
            let claimed: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM workspace_claims WHERE execution_id=?1)",
                    [&source],
                    |r| r.get(0),
                )
                .map_err(|e| e.to_string())?;
            if !continuation_core_eligible(&row) || claimed {
                return Err("AGENT_CONTINUE_NOT_ALLOWED".into());
            }
            create_with_work(tx, &id, &request, work.as_ref(), now)
        })
        .await
    }
    pub fn product_worker_owned(&self, id: &str) -> bool {
        PENDING_DISPATCH.lock().unwrap().contains(&(
            self.database_identity.to_string_lossy().to_lowercase(),
            id.into(),
        ))
    }
    pub async fn product_read(
        &self,
        id: Option<String>,
        agent: Option<String>,
        workspace: Option<String>,
        limit: u32,
    ) -> Result<Vec<ProductSnapshot>, String> {
        self.read(move |c| {
            let tx=c.unchecked_transaction()?;
            let query = if id.is_some() {
                "SELECT e.id,e.created_at,e.updated_at,e.completed_at,
                        u.execution_id,u.provider_id,u.input_tokens,u.cached_input_tokens,
                        u.cache_write_input_tokens,u.output_tokens,u.reasoning_tokens,u.total_tokens,
                        u.model_context_window,u.completeness,u.usage_revision,u.updated_at,e.task_role
                 FROM executions e LEFT JOIN execution_usage u ON u.execution_id=e.id
                 WHERE e.id=?1 AND (?2 IS NULL OR e.agent_id=?2) AND (?3 IS NULL OR e.workspace_id=?3) LIMIT ?4"
            } else {
                "SELECT e.id,e.created_at,e.updated_at,e.completed_at,
                        u.execution_id,u.provider_id,u.input_tokens,u.cached_input_tokens,
                        u.cache_write_input_tokens,u.output_tokens,u.reasoning_tokens,u.total_tokens,
                        u.model_context_window,u.completeness,u.usage_revision,u.updated_at,e.task_role
                 FROM executions e LEFT JOIN execution_usage u ON u.execution_id=e.id
                 WHERE (?1 IS NULL OR e.id=?1) AND (?2 IS NULL OR e.agent_id=?2) AND (?3 IS NULL OR e.workspace_id=?3)
                 ORDER BY e.created_at DESC,e.id DESC LIMIT ?4"
            };
            let mut q=tx.prepare(query)?;
            let rows=q.query_map(params![id,agent,workspace,limit],|r|Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?,r.get::<_,i64>(2)?,r.get::<_,Option<i64>>(3)?,super::super::usage::execution_usage_snapshot_from_left_join(r,4)?,r.get::<_,String>(16)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
            rows.into_iter().map(|(id,created_at,updated_at,completed_at,usage,task_role)|{
                let row=execution_record(&tx,&id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
                if usage.as_ref().is_some_and(|usage| usage.execution_id != row.id || usage.provider_id.as_str() != row.provider) {
                    // 持久化 public Usage 不得与 Execution identity 拼接为一个伪造事实。
                    return Err(rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, "PERSISTED_USAGE_IDENTITY_MISMATCH"))));
                }
                let thread_name = tx.query_row("SELECT name FROM thread_names WHERE thread_id=?1", [&row.thread_id], |r| r.get::<_, Option<String>>(0)).optional()?.flatten();
                let owns:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM workspace_claims WHERE execution_id=?1 AND canonical_workspace_root=?2)",params![id,row.canonical_workspace_root],|r|r.get(0))?;
                let claimed:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM workspace_claims WHERE execution_id=?1 OR canonical_workspace_root=?2)",params![id,row.canonical_workspace_root],|r|r.get(0))?;
                let busy:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM executions WHERE agent_id=?1 AND status NOT IN ('completed','failed','cancelled','interrupted'))",[&row.agent_id],|r|r.get(0))?;
                let runtime_attempt_exists:bool=crate::agent::store::runtime_attempts::runtime_attempt_exists(&tx,&id)?;
                Ok(ProductSnapshot{execution:row,task_role,usage,thread_name,owns_claim:owns,runtime_attempt_exists,claim_free:!claimed,agent_free:!busy,created_at,updated_at,completed_at})
            }).collect()
        }).await
    }
}

impl StateStore {
    /// Presentation metadata shared by all executions of the same official Thread.
    /// Does not modify execution identity, CAS revisions, timestamps, or claims.
    pub(crate) async fn save_thread_name(
        &self,
        thread_id: String,
        name: Option<String>,
    ) -> Result<(), String> {
        self.read(move |c| {
            c.execute("INSERT INTO thread_names(thread_id,name) VALUES (?1,?2) ON CONFLICT(thread_id) DO UPDATE SET name=excluded.name", params![thread_id,name])?;
            Ok(())
        }).await
    }
}
