//! Usage 持久化、epoch baseline、terminal telemetry lifecycle 与公共投影；不包含 parser。

#[cfg(test)]
use super::ObservabilityFault;
use super::{ExecutionRecord, StateStore, execution_record};
use crate::agent::{provider::telemetry::UsageEvent, usage::UsageSnapshot};
use rusqlite::{Connection, OptionalExtension, Row, Transaction, params, types::Type};
use serde_json::{Value, json};

/// Codex baseline 的 Provider-private 证据意图，不进入公共 Usage domain。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CodexUsageBaselineIntent {
    FreshZero,
    WarmObservedSameEpoch,
    Unknown,
}

/// 本任务唯一稳定的 Usage telemetry 回归错误码。
pub(crate) const USAGE_COUNTER_REGRESSION: &str = "USAGE_COUNTER_REGRESSION";

/// Usage 已跨过可接收边界；调用者只能丢弃该 telemetry，不能修改 Execution lifecycle。
pub(crate) const USAGE_TELEMETRY_FROZEN: &str = "USAGE_TELEMETRY_FROZEN";

/// 非 Codex Execution 不得进入 Codex 私有 Usage 读写路径。
pub(crate) const USAGE_PROVIDER_UNSUPPORTED: &str = "USAGE_PROVIDER_UNSUPPORTED";

/// Codex terminal 后仍可接收 exact Usage 的唯一有界窗口。
pub(crate) const USAGE_TERMINAL_GRACE_MS: i64 = 2_000;

/// 仅保存可安全用于后续同 epoch 比较的累计 shape，绝不保存 `last` 或 raw payload。
fn safe_cumulative_json(event: &UsageEvent) -> String {
    json!({
        "totalTokens": event.cumulative_total_tokens(),
        "inputTokens": event.input_tokens(),
        "cachedInputTokens": event.cached_input_tokens(),
        "cacheWriteInputTokens": event.cache_write_input_tokens(),
        "outputTokens": event.output_tokens(),
        "reasoningOutputTokens": event.reasoning_output_tokens(),
        "modelContextWindow": event.model_context_window(),
    })
    .to_string()
}

/// 严格读取 Provider 明确给出的 total；损坏、负数、浮点或缺失均不能作为 baseline。
fn cumulative_total(json_text: &str) -> Option<i64> {
    serde_json::from_str::<Value>(json_text)
        .ok()?
        .get("totalTokens")?
        .as_i64()
        .filter(|value| *value >= 0)
}

/// 私有 baseline 的可比较 total；unknown 或损坏证据一律返回未知。
fn baseline_total(state: &CodexExecutionUsageStateRecord) -> Option<i64> {
    match state.baseline_kind.as_str() {
        "fresh_zero" | "observed_same_epoch" => {
            state.baseline_json.as_deref().and_then(cumulative_total)
        }
        _ => None,
    }
}

/// 只允许 P4-004 控制的公共语义字段参与 revision 比较。
fn same_public_usage(
    existing: &ExecutionUsageRecord,
    total_tokens: Option<i64>,
    model_context_window: Option<i64>,
    completeness: &str,
) -> bool {
    existing.provider_id == "codex"
        && existing.input_tokens.is_none()
        && existing.cached_input_tokens.is_none()
        && existing.cache_write_input_tokens.is_none()
        && existing.output_tokens.is_none()
        && existing.reasoning_tokens.is_none()
        && existing.total_tokens == total_tokens
        && existing.model_context_window == model_context_window
        && existing.completeness == completeness
}

/// 写入或按语义 no-op 公共 Usage；所有 breakdown 保持 NULL。
fn write_public_usage(
    tx: &Transaction<'_>,
    row: &ExecutionRecord,
    total_tokens: Option<i64>,
    model_context_window: Option<i64>,
    completeness: &str,
    observed_at: i64,
) -> Result<(), String> {
    if row.provider != "codex" {
        return Err(USAGE_PROVIDER_UNSUPPORTED.into());
    }
    let execution_id = row.id.as_str();
    let existing = execution_usage_record(tx, execution_id).map_err(|error| error.to_string())?;
    match existing {
        Some(existing) if same_public_usage(&existing, total_tokens, model_context_window, completeness) => Ok(()),
        Some(_) => tx
            .execute(
                "UPDATE execution_usage SET provider_id='codex', input_tokens=NULL,
                 cached_input_tokens=NULL, cache_write_input_tokens=NULL, output_tokens=NULL,
                 reasoning_tokens=NULL, total_tokens=?2, model_context_window=?3, completeness=?4,
                 usage_revision=usage_revision+1, updated_at=?5 WHERE execution_id=?1",
                params![execution_id, total_tokens, model_context_window, completeness, observed_at],
            )
            .map(|_| ())
            .map_err(|error| error.to_string()),
        None => tx
            .execute(
                "INSERT INTO execution_usage(execution_id,provider_id,input_tokens,cached_input_tokens,
                 cache_write_input_tokens,output_tokens,reasoning_tokens,total_tokens,
                 model_context_window,completeness,usage_revision,updated_at)
                 VALUES(?1,'codex',NULL,NULL,NULL,NULL,NULL,?2,?3,?4,1,?5)",
                params![execution_id, total_tokens, model_context_window, completeness, observed_at],
            )
            .map(|_| ())
            .map_err(|error| error.to_string()),
    }
}

/// 用当前 Execution identity 建立 fail-safe unknown 私有状态，绝不推测 baseline。
fn insert_unknown_state(tx: &Transaction<'_>, row: &ExecutionRecord) -> Result<(), String> {
    if row.provider != "codex" {
        return Err(USAGE_PROVIDER_UNSUPPORTED.into());
    }
    let runtime = row
        .runtime_instance_id
        .as_deref()
        .ok_or("USAGE_IDENTITY_UNAVAILABLE")?;
    let thread = row
        .thread_id
        .as_deref()
        .ok_or("USAGE_IDENTITY_UNAVAILABLE")?;
    let turn = row.turn_id.as_deref().ok_or("USAGE_IDENTITY_UNAVAILABLE")?;
    tx.execute(
        "INSERT INTO codex_execution_usage_state(execution_id,runtime_instance_id,thread_id,turn_id,
         baseline_kind,baseline_json,latest_cumulative_json,telemetry_state,terminal_at,freeze_at,last_event_at)
         VALUES(?1,?2,?3,?4,'unknown',NULL,NULL,'accepting',NULL,NULL,NULL)",
        params![row.id, runtime, thread, turn],
    )
    .map(|_| ())
    .map_err(|error| error.to_string())
}

impl StateStore {
    /// 在 Provider terminal 后固定 Codex Usage grace 的首次边界；不触碰 public Usage 或 Execution lifecycle。
    pub(crate) async fn enter_codex_usage_terminal_grace(
        &self,
        execution_id: String,
        runtime_instance_id: String,
        thread_id: String,
        turn_id: String,
        terminal_at: i64,
    ) -> Result<(), String> {
        #[cfg(test)]
        if self.take_observability_failure(ObservabilityFault::UsageTerminalGrace) {
            return Err("INJECTED_USAGE_TERMINAL_GRACE_FAILURE".into());
        }
        self.write(move |tx| {
            let row = execution_record(tx, &execution_id)
                .map_err(|error| error.to_string())?
                .ok_or("EXECUTION_NOT_FOUND")?;
            if row.provider != "codex" {
                return Err(USAGE_PROVIDER_UNSUPPORTED.into());
            }
            let state = codex_execution_usage_state_record(tx, &execution_id)
                .map_err(|error| error.to_string())?
                .ok_or("USAGE_GRACE_STATE_UNAVAILABLE")?;
            if row.runtime_instance_id.as_deref() != Some(runtime_instance_id.as_str())
                || row.thread_id.as_deref() != Some(thread_id.as_str())
                || row.turn_id.as_deref() != Some(turn_id.as_str())
                || state.runtime_instance_id != runtime_instance_id
                || state.thread_id != thread_id
                || state.turn_id.as_deref() != Some(turn_id.as_str())
            {
                return Err("USAGE_GRACE_IDENTITY_MISMATCH".into());
            }
            match state.telemetry_state.as_str() {
                "accepting" => tx
                    .execute(
                        "UPDATE codex_execution_usage_state
                         SET telemetry_state='terminal_grace', terminal_at=?2
                         WHERE execution_id=?1 AND telemetry_state='accepting'",
                        params![execution_id, terminal_at],
                    )
                    .map(|_| ())
                    .map_err(|error| error.to_string()),
                "terminal_grace" | "frozen" => Ok(()),
                _ => Err("USAGE_TELEMETRY_STATE_INVALID".into()),
            }
        })
        .await
    }

    /// 冻结 Codex Usage telemetry；仅记录首次时间，不推导 public finality。
    pub(crate) async fn freeze_codex_usage(
        &self,
        execution_id: String,
        freeze_at: i64,
    ) -> Result<(), String> {
        #[cfg(test)]
        if self.take_observability_failure(ObservabilityFault::UsageFreeze) {
            return Err("INJECTED_USAGE_FREEZE_FAILURE".into());
        }
        self.write(move |tx| {
            let row = execution_record(tx, &execution_id)
                .map_err(|error| error.to_string())?
                .ok_or("EXECUTION_NOT_FOUND")?;
            if row.provider != "codex" {
                return Err(USAGE_PROVIDER_UNSUPPORTED.into());
            }
            let state = codex_execution_usage_state_record(tx, &execution_id)
                .map_err(|error| error.to_string())?
                .ok_or("USAGE_FREEZE_STATE_UNAVAILABLE")?;
            match state.telemetry_state.as_str() {
                "accepting" | "terminal_grace" => tx
                    .execute(
                        "UPDATE codex_execution_usage_state
                         SET telemetry_state='frozen', freeze_at=?2
                         WHERE execution_id=?1 AND telemetry_state IN ('accepting','terminal_grace')",
                        params![execution_id, freeze_at],
                    )
                    .map(|_| ())
                    .map_err(|error| error.to_string()),
                "frozen" => Ok(()),
                _ => Err("USAGE_TELEMETRY_STATE_INVALID".into()),
            }
        })
        .await
    }

    /// 在 Thread 已 bind、Turn 尚未创建的单一即时事务内冻结 Codex Usage baseline。
    pub(crate) async fn prepare_codex_usage_baseline(
        &self,
        execution_id: String,
        runtime_instance_id: String,
        thread_id: String,
        intent: CodexUsageBaselineIntent,
        _now: i64,
    ) -> Result<(), String> {
        #[cfg(test)]
        if self.take_observability_failure(ObservabilityFault::UsageBaseline) {
            return Err("INJECTED_USAGE_BASELINE_FAILURE".into());
        }
        self.write(move |tx| {
            let row = execution_record(tx, &execution_id)
                .map_err(|error| error.to_string())?
                .ok_or("EXECUTION_NOT_FOUND")?;
            if row.provider != "codex" {
                return Err(USAGE_PROVIDER_UNSUPPORTED.into());
            }
            if row.runtime_instance_id.as_deref() != Some(runtime_instance_id.as_str())
                || row.thread_id.as_deref() != Some(thread_id.as_str())
                || row.turn_id.is_some()
            {
                return Err("USAGE_BASELINE_IDENTITY_UNAVAILABLE".into());
            }
            if let Some(state) = codex_execution_usage_state_record(tx, &execution_id)
                .map_err(|error| error.to_string())?
            {
                if state.runtime_instance_id == runtime_instance_id && state.thread_id == thread_id {
                    return Ok(());
                }
                return Err("USAGE_BASELINE_ALREADY_FROZEN".into());
            }
            let (baseline_kind, baseline_json) = match intent {
                CodexUsageBaselineIntent::FreshZero => {
                    let epoch = codex_thread_usage_epoch_record(tx, &execution_id, &runtime_instance_id, &thread_id)
                        .map_err(|error| error.to_string())?;
                    if epoch.is_none() {
                        ("fresh_zero", Some(json!({"totalTokens": 0}).to_string()))
                    } else {
                        ("unknown", None)
                    }
                }
                CodexUsageBaselineIntent::WarmObservedSameEpoch => {
                    let total = codex_thread_usage_epoch_record(tx, &execution_id, &runtime_instance_id, &thread_id)
                        .map_err(|error| error.to_string())?
                        .and_then(|epoch| cumulative_total(&epoch.latest_cumulative_json));
                    match total {
                        Some(total) => ("observed_same_epoch", Some(json!({"totalTokens": total}).to_string())),
                        None => ("unknown", None),
                    }
                }
                CodexUsageBaselineIntent::Unknown => ("unknown", None),
            };
            tx.execute(
                "INSERT INTO codex_execution_usage_state(execution_id,runtime_instance_id,thread_id,turn_id,
                 baseline_kind,baseline_json,latest_cumulative_json,telemetry_state,terminal_at,freeze_at,last_event_at)
                 VALUES(?1,?2,?3,NULL,?4,?5,NULL,'accepting',NULL,NULL,NULL)",
                params![execution_id, runtime_instance_id, thread_id, baseline_kind, baseline_json],
            )
            .map(|_| ())
            .map_err(|error| error.to_string())
        })
        .await
    }

    /// 投影已绑定的 Codex Usage；epoch/private/public 均在同一事务中更新或完全不更新。
    pub(crate) async fn project_execution_usage(&self, event: UsageEvent) -> Result<(), String> {
        #[cfg(test)]
        if self.take_observability_failure(ObservabilityFault::UsageProjection) {
            return Err("INJECTED_USAGE_PROJECTION_FAILURE".into());
        }
        let execution_id = event.execution_id().to_string();
        let provider_id = event.provider_id().as_str().to_string();
        let incoming_total = event.cumulative_total_tokens();
        let model_context_window = event.model_context_window();
        let observed_at = event.observed_at();
        let cumulative_json = safe_cumulative_json(&event);
        self.write(move |tx| -> Result<Result<(), String>, String> {
            let row = execution_record(tx, &execution_id)
                .map_err(|error| error.to_string())?
                .ok_or("EXECUTION_NOT_FOUND")?;
            if row.provider != "codex" {
                return Err(USAGE_PROVIDER_UNSUPPORTED.into());
            }
            if provider_id != "codex" {
                return Err("USAGE_PROVIDER_MISMATCH".into());
            }
            if row.runtime_instance_id.is_none() || row.thread_id.is_none() || row.turn_id.is_none() {
                return Err("USAGE_IDENTITY_UNAVAILABLE".into());
            }
            let state = match codex_execution_usage_state_record(tx, &execution_id)
                .map_err(|error| error.to_string())?
            {
                Some(state) => state,
                None => {
                    insert_unknown_state(tx, &row)?;
                    codex_execution_usage_state_record(tx, &execution_id)
                        .map_err(|error| error.to_string())?
                        .ok_or("USAGE_STATE_INSERT_FAILED")?
                }
            };
            match state.telemetry_state.as_str() {
                "accepting" => {}
                "terminal_grace" => {
                    let Some(terminal_at) = state.terminal_at else {
                        tx.execute(
                            "UPDATE codex_execution_usage_state
                             SET telemetry_state='frozen', freeze_at=?2
                             WHERE execution_id=?1 AND telemetry_state='terminal_grace'",
                            params![execution_id, observed_at],
                        )
                        .map_err(|error| error.to_string())?;
                        return Ok(Err(USAGE_TELEMETRY_FROZEN.into()));
                    };
                    let deadline = terminal_at.saturating_add(USAGE_TERMINAL_GRACE_MS);
                    if observed_at > deadline {
                        tx.execute(
                            "UPDATE codex_execution_usage_state
                             SET telemetry_state='frozen', freeze_at=?2
                             WHERE execution_id=?1 AND telemetry_state='terminal_grace'",
                            params![execution_id, deadline],
                        )
                        .map_err(|error| error.to_string())?;
                        return Ok(Err(USAGE_TELEMETRY_FROZEN.into()));
                    }
                }
                "frozen" => return Ok(Err(USAGE_TELEMETRY_FROZEN.into())),
                _ => return Err("USAGE_TELEMETRY_STATE_INVALID".into()),
            }
            let identity_matches = state.runtime_instance_id == row.runtime_instance_id.as_deref().unwrap_or_default()
                && state.thread_id == row.thread_id.as_deref().unwrap_or_default()
                && state.turn_id == row.turn_id;
            if !identity_matches {
                return Ok(write_public_usage(
                    tx,
                    &row,
                    None,
                    model_context_window,
                    "unknown",
                    observed_at,
                ));
            }
            let previous_epoch = codex_thread_usage_epoch_record(
                tx,
                &execution_id,
                row.runtime_instance_id.as_deref().unwrap_or_default(),
                row.thread_id.as_deref().unwrap_or_default(),
            )
            .map_err(|error| error.to_string())?;
            if previous_epoch
                .as_ref()
                .and_then(|epoch| cumulative_total(&epoch.latest_cumulative_json))
                .is_some_and(|total| incoming_total < total)
            {
                return Err(USAGE_COUNTER_REGRESSION.into());
            }
            if baseline_total(&state).is_some_and(|total| incoming_total < total) {
                return Err(USAGE_COUNTER_REGRESSION.into());
            }
            tx.execute(
                "INSERT INTO codex_thread_usage_epochs(runtime_instance_id,thread_id,latest_cumulative_json,latest_turn_id,captured_at)
                 VALUES(?1,?2,?3,?4,?5)
                 ON CONFLICT(runtime_instance_id,thread_id) DO UPDATE SET latest_cumulative_json=excluded.latest_cumulative_json,
                 latest_turn_id=excluded.latest_turn_id,captured_at=excluded.captured_at",
                params![row.runtime_instance_id, row.thread_id, cumulative_json, row.turn_id, observed_at],
            )
            .map_err(|error| error.to_string())?;
            tx.execute(
                "UPDATE codex_execution_usage_state SET latest_cumulative_json=?2,last_event_at=?3 WHERE execution_id=?1",
                params![execution_id, cumulative_json, observed_at],
            )
            .map_err(|error| error.to_string())?;
            Ok(match baseline_total(&state) {
                Some(total) => write_public_usage(
                    tx,
                    &row,
                    Some(incoming_total - total),
                    model_context_window,
                    "partial",
                    observed_at,
                ),
                None => write_public_usage(tx, &row, None, model_context_window, "unknown", observed_at),
            })
        })
        .await?
    }

    /// 旧 Turn Usage 到达时使当前 Execution 的同 identity baseline 失效，且不发布该旧事件。
    pub(crate) async fn invalidate_codex_usage_baseline(
        &self,
        execution_id: String,
        runtime_instance_id: String,
        thread_id: String,
        observed_at: i64,
    ) -> Result<(), String> {
        #[cfg(test)]
        if self.take_observability_failure(ObservabilityFault::UsageInvalidation) {
            return Err("INJECTED_USAGE_INVALIDATION_FAILURE".into());
        }
        self.write(move |tx| {
            let row = execution_record(tx, &execution_id)
                .map_err(|error| error.to_string())?
                .ok_or("EXECUTION_NOT_FOUND")?;
            if row.provider != "codex" {
                return Err(USAGE_PROVIDER_UNSUPPORTED.into());
            }
            if row.runtime_instance_id.as_deref() != Some(runtime_instance_id.as_str())
                || row.thread_id.as_deref() != Some(thread_id.as_str())
            {
                return Err("USAGE_INVALIDATION_IDENTITY_MISMATCH".into());
            }
            let Some(state) = codex_execution_usage_state_record(tx, &execution_id)
                .map_err(|error| error.to_string())?
            else {
                return Ok(());
            };
            if state.runtime_instance_id != runtime_instance_id || state.thread_id != thread_id {
                return Err("USAGE_INVALIDATION_IDENTITY_MISMATCH".into());
            }
            tx.execute(
                "UPDATE codex_execution_usage_state SET baseline_kind='unknown',baseline_json=NULL WHERE execution_id=?1",
                [&execution_id],
            )
            .map_err(|error| error.to_string())?;
            if let Some(public) = execution_usage_record(tx, &execution_id).map_err(|error| error.to_string())?
                && public.completeness == "partial"
            {
                write_public_usage(
                    tx,
                    &row,
                    None,
                    public.model_context_window,
                    "unknown",
                    observed_at,
                )?;
            }
            Ok(())
        })
        .await
    }
}

/// 公共 `execution_usage` 行；转换失败时绝不猜测或修正持久化数据。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExecutionUsageRecord {
    pub(crate) execution_id: String,
    pub(crate) provider_id: String,
    pub(crate) input_tokens: Option<i64>,
    pub(crate) cached_input_tokens: Option<i64>,
    pub(crate) cache_write_input_tokens: Option<i64>,
    pub(crate) output_tokens: Option<i64>,
    pub(crate) reasoning_tokens: Option<i64>,
    pub(crate) total_tokens: Option<i64>,
    pub(crate) model_context_window: Option<i64>,
    pub(crate) completeness: String,
    pub(crate) usage_revision: i64,
    pub(crate) updated_at: i64,
}

/// Codex thread epoch 的 Provider-private 只读行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CodexThreadUsageEpochRecord {
    pub(super) runtime_instance_id: String,
    pub(super) thread_id: String,
    pub(super) latest_cumulative_json: String,
    pub(super) latest_turn_id: Option<String>,
    pub(super) captured_at: i64,
}

/// Codex Execution Usage 状态的 Provider-private 只读行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CodexExecutionUsageStateRecord {
    pub(super) execution_id: String,
    pub(super) runtime_instance_id: String,
    pub(super) thread_id: String,
    pub(super) turn_id: Option<String>,
    pub(super) baseline_kind: String,
    pub(super) baseline_json: Option<String>,
    pub(super) latest_cumulative_json: Option<String>,
    pub(super) telemetry_state: String,
    pub(super) terminal_at: Option<i64>,
    pub(super) freeze_at: Option<i64>,
    pub(super) last_event_at: Option<i64>,
}

impl TryFrom<ExecutionUsageRecord> for UsageSnapshot {
    type Error = String;

    /// 将数据库行安全投影为 P4-001 公共快照，不计算或补齐任何字段。
    fn try_from(record: ExecutionUsageRecord) -> Result<Self, Self::Error> {
        // 复用 P4-001 的唯一公共 validation 入口，防止持久化异常绕过 wire 契约。
        UsageSnapshot::from_json(&json!({
            "providerId": record.provider_id,
            "executionId": record.execution_id,
            "inputTokens": record.input_tokens,
            "cachedInputTokens": record.cached_input_tokens,
            "cacheWriteInputTokens": record.cache_write_input_tokens,
            "outputTokens": record.output_tokens,
            "reasoningTokens": record.reasoning_tokens,
            "totalTokens": record.total_tokens,
            "modelContextWindow": record.model_context_window,
            "completeness": record.completeness,
            "revision": record.usage_revision,
            "updatedAt": record.updated_at,
        }))
        .map_err(|error| error.to_string())
    }
}

/// 读取公共 Usage 行；无行由调用者保留为 unknown/null 语义。
pub(super) fn execution_usage_record(
    connection: &Connection,
    execution_id: &str,
) -> rusqlite::Result<Option<ExecutionUsageRecord>> {
    connection
        .query_row(
            "SELECT execution_id, provider_id, input_tokens, cached_input_tokens,
                    cache_write_input_tokens, output_tokens, reasoning_tokens, total_tokens,
                    model_context_window, completeness, usage_revision, updated_at
             FROM execution_usage WHERE execution_id = ?1",
            [execution_id],
            execution_usage_record_from_row,
        )
        .optional()
}

/// 读取 Codex thread epoch 行；仅供 store 内部后续持久化逻辑使用。
pub(super) fn codex_thread_usage_epoch_record(
    connection: &Connection,
    execution_id: &str,
    runtime_instance_id: &str,
    thread_id: &str,
) -> rusqlite::Result<Option<CodexThreadUsageEpochRecord>> {
    let provider: Option<String> = connection
        .query_row(
            "SELECT provider FROM executions WHERE id=?1",
            [execution_id],
            |row| row.get(0),
        )
        .optional()?;
    if provider.as_deref() != Some("codex") {
        return Ok(None);
    }
    connection
        .query_row(
            "SELECT runtime_instance_id, thread_id, latest_cumulative_json, latest_turn_id, captured_at
             FROM codex_thread_usage_epochs
             WHERE runtime_instance_id = ?1 AND thread_id = ?2",
            [runtime_instance_id, thread_id],
            |row| {
                Ok(CodexThreadUsageEpochRecord {
                    runtime_instance_id: row.get(0)?,
                    thread_id: row.get(1)?,
                    latest_cumulative_json: row.get(2)?,
                    latest_turn_id: row.get(3)?,
                    captured_at: row.get(4)?,
                })
            },
        )
        .optional()
}

/// 读取 Codex Execution 状态行；不向公共 DTO 传播 Provider-private identity。
pub(super) fn codex_execution_usage_state_record(
    connection: &Connection,
    execution_id: &str,
) -> rusqlite::Result<Option<CodexExecutionUsageStateRecord>> {
    let provider: Option<String> = connection
        .query_row(
            "SELECT provider FROM executions WHERE id=?1",
            [execution_id],
            |row| row.get(0),
        )
        .optional()?;
    if provider.as_deref() != Some("codex") {
        return Ok(None);
    }
    connection
        .query_row(
            "SELECT execution_id, runtime_instance_id, thread_id, turn_id, baseline_kind,
                    baseline_json, latest_cumulative_json, telemetry_state, terminal_at,
                    freeze_at, last_event_at
             FROM codex_execution_usage_state WHERE execution_id = ?1",
            [execution_id],
            |row| {
                Ok(CodexExecutionUsageStateRecord {
                    execution_id: row.get(0)?,
                    runtime_instance_id: row.get(1)?,
                    thread_id: row.get(2)?,
                    turn_id: row.get(3)?,
                    baseline_kind: row.get(4)?,
                    baseline_json: row.get(5)?,
                    latest_cumulative_json: row.get(6)?,
                    telemetry_state: row.get(7)?,
                    terminal_at: row.get(8)?,
                    freeze_at: row.get(9)?,
                    last_event_at: row.get(10)?,
                })
            },
        )
        .optional()
}

/// 将公共 Usage 行转换错误保留为 SQLite read failure，避免返回伪造快照。
pub(super) fn execution_usage_snapshot(
    connection: &Connection,
    execution_id: &str,
) -> rusqlite::Result<Option<UsageSnapshot>> {
    execution_usage_record(connection, execution_id)?
        .map(usage_snapshot_from_record)
        .transpose()
}

/// 将 LEFT JOIN 的 Usage 列转换为公共快照；NULL identity 唯一表示无 Usage 行。
pub(super) fn execution_usage_snapshot_from_left_join(
    row: &Row<'_>,
    offset: usize,
) -> rusqlite::Result<Option<UsageSnapshot>> {
    let execution_id: Option<String> = row.get(offset)?;
    execution_id
        .map(|execution_id| {
            usage_snapshot_from_record(ExecutionUsageRecord {
                execution_id,
                provider_id: row.get(offset + 1)?,
                input_tokens: row.get(offset + 2)?,
                cached_input_tokens: row.get(offset + 3)?,
                cache_write_input_tokens: row.get(offset + 4)?,
                output_tokens: row.get(offset + 5)?,
                reasoning_tokens: row.get(offset + 6)?,
                total_tokens: row.get(offset + 7)?,
                model_context_window: row.get(offset + 8)?,
                completeness: row.get(offset + 9)?,
                usage_revision: row.get(offset + 10)?,
                updated_at: row.get(offset + 11)?,
            })
        })
        .transpose()
}

/// 保留 P4-001 validation failure 为 SQLite read failure，避免 Product 伪造 Usage。
fn usage_snapshot_from_record(record: ExecutionUsageRecord) -> rusqlite::Result<UsageSnapshot> {
    UsageSnapshot::try_from(record).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            Type::Text,
            Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, error)),
        )
    })
}

/// 将公共 Usage 行转换为 record，保留 NULL 与零的区别。
fn execution_usage_record_from_row(row: &Row<'_>) -> rusqlite::Result<ExecutionUsageRecord> {
    Ok(ExecutionUsageRecord {
        execution_id: row.get(0)?,
        provider_id: row.get(1)?,
        input_tokens: row.get(2)?,
        cached_input_tokens: row.get(3)?,
        cache_write_input_tokens: row.get(4)?,
        output_tokens: row.get(5)?,
        reasoning_tokens: row.get(6)?,
        total_tokens: row.get(7)?,
        model_context_window: row.get(8)?,
        completeness: row.get(9)?,
        usage_revision: row.get(10)?,
        updated_at: row.get(11)?,
    })
}
