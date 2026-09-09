//! Read-only result information. No Provider Evidence construction or Claim API.
use super::*;
use serde::Serialize;
const RECOVERY_TIMEOUT: Duration = Duration::from_secs(30);

/// Cross-Runtime construction checks durable Job evidence; fields are private.
/// The source Runtime identity is never relabelled as the recovery Runtime.
#[derive(Debug, Clone)]
pub struct RecoveryScope {
    source_runtime_id: String,
    recovery_runtime_id: String,
    thread_id: String,
    turn_id: String,
    provider_terminal: Option<TurnStatus>,
    binding: Option<binding::ExecutionProtocolBinding>,
}
impl RecoveryScope {
    pub async fn same_runtime_for_execution(
        store: &crate::agent::store::StateStore,
        execution: &str,
        runtime: &str,
    ) -> Result<Self> {
        let binding = binding::ExecutionProtocolBinding::load(store, execution).await?;
        let scope = Self::bound(binding, runtime)?;
        if scope.source_runtime_id != runtime {
            return Err(ProtocolError::incompatible(
                "Execution belongs to another Runtime",
            ));
        }
        Ok(scope)
    }
    pub async fn after_termination_for_execution(
        store: &crate::agent::store::StateStore,
        execution: &str,
        recovery: &str,
    ) -> Result<Self> {
        let binding = binding::ExecutionProtocolBinding::load(store, execution).await?;
        let scope = Self::bound(binding, recovery)?;
        Self::from_parts(
            store,
            &scope.source_runtime_id,
            recovery,
            &scope.thread_id,
            &scope.turn_id,
            scope.provider_terminal,
        )
        .await?;
        scope.validate_current().await?;
        Ok(scope)
    }
    fn bound(binding: binding::ExecutionProtocolBinding, recovery: &str) -> Result<Self> {
        let row = binding.record();
        let source = row.runtime_instance_id.as_deref().unwrap();
        let turn = row
            .turn_id
            .as_deref()
            .filter(|v| !v.is_empty())
            .ok_or_else(|| {
                ProtocolError::new("CODEX_EXECUTION_BINDING_INVALID", "Execution Turn missing")
            })?;
        let terminal = match row.provider_terminal_status.as_deref() {
            None if row.provider_terminal_evidence_runtime_instance_id.is_none() => None,
            Some(status)
                if row
                    .provider_terminal_evidence_runtime_instance_id
                    .as_deref()
                    == Some(source) =>
            {
                Some(match status {
                    "completed" => TurnStatus::Completed,
                    "failed" => TurnStatus::Failed,
                    "interrupted" => TurnStatus::Interrupted,
                    _ => {
                        return Err(ProtocolError::new(
                            "CODEX_RESULT_EVIDENCE_CONFLICT",
                            "Persisted terminal has no approved Codex status mapping",
                        ));
                    }
                })
            }
            _ => {
                return Err(ProtocolError::new(
                    "CODEX_RESULT_EVIDENCE_CONFLICT",
                    "Provider evidence does not belong to Execution Runtime",
                ));
            }
        };
        Ok(Self {
            source_runtime_id: source.into(),
            recovery_runtime_id: recovery.into(),
            thread_id: row.thread_id.clone().unwrap(),
            turn_id: turn.into(),
            provider_terminal: terminal,
            binding: Some(binding),
        })
    }
    async fn validate_current(&self) -> Result<()> {
        if let Some(binding) = &self.binding {
            return binding.validate_current().await;
        }
        #[cfg(test)]
        {
            Ok(())
        }
        #[cfg(not(test))]
        {
            Err(ProtocolError::new(
                "CODEX_EXECUTION_BINDING_INVALID",
                "Missing persisted binding",
            ))
        }
    }
    #[cfg(test)]
    pub(super) async fn after_termination(
        store: &crate::agent::store::StateStore,
        source: &str,
        recovery: &str,
        thread: &str,
        turn: &str,
        terminal: Option<TurnStatus>,
    ) -> Result<Self> {
        Self::from_parts(store, source, recovery, thread, turn, terminal).await
    }
    #[cfg(test)]
    pub(super) fn same_runtime(
        runtime: &str,
        thread: &str,
        turn: &str,
        terminal: Option<TurnStatus>,
    ) -> Self {
        Self {
            source_runtime_id: runtime.into(),
            recovery_runtime_id: runtime.into(),
            thread_id: thread.into(),
            turn_id: turn.into(),
            provider_terminal: terminal,
            binding: None,
        }
    }
    async fn from_parts(
        store: &crate::agent::store::StateStore,
        source: &str,
        recovery: &str,
        thread: &str,
        turn: &str,
        terminal: Option<TurnStatus>,
    ) -> Result<Self> {
        let row = store
            .runtime(source.into())
            .await
            .map_err(|e| ProtocolError::new("CODEX_RUNTIME_STORE_FAILED", e))?
            .ok_or_else(|| {
                ProtocolError::new("CODEX_RUNTIME_NOT_FOUND", "Source Runtime missing")
            })?;
        if source == recovery
            || row.state != "terminated"
            || row.termination_evidence_state != "complete"
            || row.termination_evidence_at.is_none()
            || !matches!(
                row.termination_evidence_type.as_deref(),
                Some("job_active_processes_zero" | "managed_job_destroyed")
            )
        {
            return Err(ProtocolError::new(
                "CODEX_RESULT_RECOVERY_UNSAFE",
                "Original Runtime Job termination is not confirmed",
            ));
        }
        Ok(Self {
            source_runtime_id: source.into(),
            recovery_runtime_id: recovery.into(),
            thread_id: thread.into(),
            turn_id: turn.into(),
            provider_terminal: terminal,
            binding: None,
        })
    }
}
/// Only full, validated recovery can construct this record. Errors carry no
/// complete result. Serialization is suitable for final_result_json, not Evidence.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveredResult {
    execution_id: Option<String>,
    execution_revision: Option<i64>,
    thread_id: String,
    turn_id: String,
    history_mode: HistoryMode,
    source_runtime_id: String,
    recovered_by_runtime_id: String,
    terminal_turn: Turn,
    final_result: Vec<Value>,
    result_completeness: &'static str,
}
impl RecoveredResult {
    pub fn thread_id(&self) -> &str {
        &self.thread_id
    }
    pub fn turn_id(&self) -> &str {
        &self.turn_id
    }
    pub fn terminal_status(&self) -> TurnStatus {
        self.terminal_turn.status
    }
    pub fn final_result(&self) -> &[Value] {
        &self.final_result
    }
}
pub(super) fn checked_thread(value: Value, expected: &str) -> Result<Thread> {
    let thread = thread_response(value)?;
    if thread.id != expected || expected.is_empty() {
        return Err(ProtocolError::incompatible("Response for wrong Thread"));
    }
    Ok(thread)
}
impl Client {
    pub async fn recover_result(&self, scope: RecoveryScope) -> Result<RecoveredResult> {
        self.recover_until(scope, Instant::now() + RECOVERY_TIMEOUT)
            .await
    }
    pub(super) async fn recover_until(
        &self,
        scope: RecoveryScope,
        deadline: Instant,
    ) -> Result<RecoveredResult> {
        let result=async {
            scope.validate_current().await?;
            self.check_recovery(&scope,deadline)?;
            let meta=self.rpc("thread/read",json!({"threadId":scope.thread_id,"includeTurns":false}),None,deadline.min(Instant::now()+RPC_TIMEOUT),false).await?;
            let meta=checked_thread(meta,&scope.thread_id)?;
            let mut turn=match meta.history_mode {
                HistoryMode::Legacy=> {
                    let value=self.rpc("thread/read",json!({"threadId":scope.thread_id,"includeTurns":true}),None,deadline.min(Instant::now()+RPC_TIMEOUT),false).await?;
                    let thread=checked_thread(value,&scope.thread_id)?;
                    if thread.history_mode!=HistoryMode::Legacy {return Err(ProtocolError::incompatible("Thread historyMode changed"));}
                    let mut targets=thread.turns.into_iter().filter(|t|t.id==scope.turn_id);
                    let turn=targets.next().ok_or_else(missing_turn)?;
                    if targets.next().is_some() || turn.items_view!="full" {return Err(ProtocolError::incompatible("Ambiguous or incomplete legacy Turn"));}
                    turn
                }
                HistoryMode::Paginated=> {
                    let mut cursor=None;
                    let mut seen=HashSet::new();
                    let target=loop {
                        let page=self.history_page("thread/turns/list",json!({"threadId":scope.thread_id,"cursor":cursor,"limit":20,"sortDirection":"desc","itemsView":"summary"}),&scope,deadline).await?;
                        let next=next_page(page.next_cursor,&mut seen)?;
                        let mut target=None;
                        for value in page.data {
                            let candidate:Turn=serde_json::from_value(value).map_err(|e|ProtocolError::incompatible(e.to_string()))?;
                            if candidate.id==scope.turn_id {if target.is_some(){return Err(ProtocolError::incompatible("Duplicate target Turn"));}target=Some(candidate);}
                        }
                        if let Some(target)=target {break target;}
                        cursor=next;
                        if cursor.is_none(){return Err(missing_turn());}
                    };
                    validate_terminal(&target,&scope)?;
                    let mut items=Vec::new();let mut bytes=0;let mut cursor=None;let mut seen=HashSet::new();
                    loop {
                        let page=self.history_page("thread/items/list",json!({"threadId":scope.thread_id,"turnId":scope.turn_id,"cursor":cursor,"limit":20,"sortDirection":"asc"}),&scope,deadline).await?;
                        let next=next_page(page.next_cursor,&mut seen)?;
                        for entry in page.data {
                            if entry.get("turnId").and_then(Value::as_str)!=Some(scope.turn_id.as_str()){return Err(ProtocolError::incompatible("History item belongs to wrong Turn"));}
                            let item=entry.get("item").cloned().ok_or_else(||ProtocolError::incompatible("Missing persisted item"))?;
                            bytes+=serde_json::to_vec(&item).map_err(|e|ProtocolError::invalid(e.to_string()))?.len();
                            if bytes>QUEUE_BYTES {return Err(ProtocolError::new("CODEX_RESULT_TOO_LARGE","Recovered items exceed 32 MiB bound"));}
                            items.push(item);
                        }
                        cursor=next;if cursor.is_none(){break;}
                    }
                    Turn {items,items_view:"full".into(),..target}
                }
            };
            scope.validate_current().await?;
            self.check_recovery(&scope,deadline)?;
            validate_terminal(&turn,&scope)?;
            let mut final_result=Vec::new();
            for item in &turn.items {
                if !item.get("type").is_some_and(Value::is_string) || !item.get("id").is_some_and(Value::is_string) {return Err(ProtocolError::incompatible("Invalid persisted item"));}
                if item["type"]=="agentMessage" {
                    if !matches!(item.get("phase"), None | Some(Value::Null))
                        && !matches!(item.get("phase").and_then(Value::as_str), Some("commentary" | "final_answer")) {
                        return Err(ProtocolError::incompatible("Invalid agent message phase"));
                    }
                    if !item.get("text").is_some_and(Value::is_string){return Err(ProtocolError::incompatible("Agent message missing text"));}
                    // Persist every agent message with its actual phase; do not
                    // infer final content from the prompt or compare live item IDs.
                    final_result.push(item.clone());
                }
            }
            turn.items_view="full".into();
            Ok(RecoveredResult {execution_id:scope.binding.as_ref().map(|b|b.record().id.clone()),execution_revision:scope.binding.as_ref().map(|b|b.record().revision),thread_id:scope.thread_id,turn_id:scope.turn_id,history_mode:meta.history_mode,source_runtime_id:scope.source_runtime_id,recovered_by_runtime_id:self.runtime_id().into(),terminal_turn:turn,final_result,result_completeness:"complete"})
        }.await;
        self.validated(result)
    }
    fn check_recovery(&self, scope: &RecoveryScope, deadline: Instant) -> Result<()> {
        self.shared.check()?;
        if scope.recovery_runtime_id != self.runtime_id()
            || scope.thread_id.is_empty()
            || scope.turn_id.is_empty()
        {
            return Err(ProtocolError::incompatible(
                "Recovery Runtime/Thread/Turn identity mismatch",
            ));
        }
        if Instant::now() >= deadline {
            return Err(ProtocolError::new(
                "CODEX_RESULT_RECOVERY_TIMEOUT",
                "Total recovery deadline exceeded",
            ));
        }
        Ok(())
    }
    async fn history_page(
        &self,
        method: &str,
        params: Value,
        scope: &RecoveryScope,
        deadline: Instant,
    ) -> Result<TerminalPage> {
        self.check_recovery(scope, deadline)?;
        let value = self
            .rpc(
                method,
                params,
                None,
                deadline.min(Instant::now() + RPC_TIMEOUT),
                false,
            )
            .await?;
        self.check_recovery(scope, deadline)?;
        if value
            .get("threadId")
            .is_some_and(|v| v != &json!(scope.thread_id))
        {
            return Err(ProtocolError::incompatible(
                "History response for wrong Thread",
            ));
        }
        TerminalPage::parse(value)
    }
}
fn missing_turn() -> ProtocolError {
    ProtocolError::new(
        "CODEX_RESULT_TARGET_NOT_FOUND",
        "Exact target Turn absent; result remains unknown",
    )
}
fn validate_terminal(turn: &Turn, scope: &RecoveryScope) -> Result<()> {
    if turn.id != scope.turn_id || turn.status == TurnStatus::InProgress {
        return Err(ProtocolError::new(
            "CODEX_RESULT_NOT_TERMINAL",
            "Exact target Turn must be terminal",
        ));
    }
    if scope
        .provider_terminal
        .is_some_and(|status| status != turn.status)
    {
        return Err(ProtocolError::new(
            "CODEX_RESULT_EVIDENCE_CONFLICT",
            "Recovered status conflicts with original Provider Terminal Evidence",
        ));
    }
    Ok(())
}
fn next_page(cursor: NextCursor, seen: &mut HashSet<String>) -> Result<Option<String>> {
    match cursor {
        NextCursor::Missing => Err(ProtocolError::incompatible("Missing nextCursor")),
        NextCursor::Null => Ok(None),
        NextCursor::Cursor(c) => {
            if seen.len() >= QUEUE_COUNT
                || seen.iter().map(String::len).sum::<usize>() + c.len() > QUEUE_BYTES
                || !seen.insert(c.clone())
            {
                return Err(ProtocolError::incompatible(
                    "Repeated cursor or cursor budget exceeded",
                ));
            }
            Ok(Some(c))
        }
    }
}
