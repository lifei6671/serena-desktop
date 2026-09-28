//! Activity 与真实 Prompt/Store 的集成边界，所有等待都有时限。
use super::*;
use crate::agent::{
    provider::telemetry::AgentActivityEvent, telemetry_projector::ExecutionTelemetryProjector,
};
use std::sync::Mutex;

/// 记录公共事件，快照只能包含四个安全字段。
#[derive(Default)]
struct Recording(Mutex<Vec<Value>>);
impl AgentEventSink for Recording {
    /// 在轮询时记录，验证终态之后不会继续推进 pending 发布。
    fn publish(&self, event: AgentTelemetryEvent) -> ProviderFuture<'_, ()> {
        Box::pin(async move {
            let AgentTelemetryEvent::Activity(event) = event else {
                panic!("Usage forbidden")
            };
            self.0.lock().unwrap().push(json!({"execution_id":event.execution_id(),"phase":event.phase(),"tool_category":event.tool_category(),"observed_at":event.observed_at()}));
        })
    }
}
/// 生成 exact-correlated 工具帧，正文与伪 terminal 不参与 authority。
fn tool_frame(conversation: &str) -> Value {
    json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"exact-session","update":{"sessionUpdate":"tool_call","toolCallId":"t","kind":"execute","title":"cargo test","status":"in_progress","rawInput":{"command":"secret"},"rawOutput":"secret","stopReason":"cancelled","terminal":true,"_meta":{CONVERSATION:conversation,PROVIDER_REQUEST:"activity-only-id"}}}})
}
/// 数据库全部非 Activity 字段作为不可变快照，覆盖 Claim、usage、release 和 private identity。
fn authority_snapshot(dir: &Path) -> Vec<Vec<String>> {
    let db = rusqlite::Connection::open(dir.join("agent-state.db")).unwrap();
    let mut snapshot = Vec::new();
    for table in [
        "executions",
        "workspace_claims",
        "execution_usage",
        "runtime_instances",
        "codebuddy_execution_state",
    ] {
        let mut query = db
            .prepare(&format!("SELECT * FROM {table} ORDER BY 1"))
            .unwrap();
        let columns = query
            .column_names()
            .iter()
            .enumerate()
            .filter(|(_, name)| {
                ![
                    "last_activity_at",
                    "activity_phase",
                    "tool_category",
                    "activity_summary_code",
                    "activity_sequence",
                ]
                .contains(name)
            })
            .map(|(i, _)| i)
            .collect::<Vec<_>>();
        let rows = query
            .query_map([], |row| {
                Ok(columns
                    .iter()
                    .map(|&i| format!("{:?}", row.get_ref(i).unwrap()))
                    .collect::<Vec<_>>())
            })
            .unwrap();
        snapshot.extend(rows.map(Result::unwrap));
    }
    snapshot
}
/// 等待真实事件或落库条件，不把固定 sleep 当作成功依据。
async fn wait_until(mut ready: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !ready() {
        assert!(tokio::time::Instant::now() < deadline, "activity timeout");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

#[tokio::test]
/// 真正 async Store projector 成功写入分类；wrong sink、失败和 Activity 不触碰其他 authority。
async fn projector_preserves_all_authority_columns() {
    let bin = tempfile::tempdir().unwrap();
    let base = build(bin.path());
    let (dir, store, session, _) = setup(&base, Limits::default()).await;
    let id = session.private.execution_id.clone();
    let snapshot = authority_snapshot(dir.path());
    let mut mapper = ActivityMapper::new(
        id.clone(),
        "exact-session".into(),
        session.private.conversation_request_id.clone(),
        None,
        4,
    );
    let shared = Shared::new(Limits::default());
    shared.register_route("exact-session").unwrap();
    shared
        .notification(
            "session/update".into(),
            tool_frame(&session.private.conversation_request_id)["params"].clone(),
        )
        .unwrap();
    let event = mapper
        .map(&shared.take_session("exact-session").unwrap()[0])
        .unwrap();
    let wrong = ExecutionTelemetryProjector::new(store.clone(), "wrong-execution".into());
    wrong
        .publish(AgentTelemetryEvent::Activity(event.clone()))
        .await;
    assert!(
        store
            .execution(id.clone())
            .await
            .unwrap()
            .unwrap()
            .last_activity_at
            .is_none()
    );
    let projector = ExecutionTelemetryProjector::new(store.clone(), id.clone());
    projector
        .publish(AgentTelemetryEvent::Activity(event.clone()))
        .await;
    let row = store.execution(id.clone()).await.unwrap().unwrap();
    assert_eq!(row.last_activity_at, Some(event.observed_at()));
    assert_eq!(row.activity_phase.as_deref(), Some("tool"));
    assert_eq!(row.tool_category.as_deref(), Some("command"));
    // 目前 public Execution 仍 dispatch_pending，摘要由公共 projector 按其进度派生。
    assert_eq!(row.activity_summary_code.as_deref(), Some("tool.command"));
    assert_eq!(authority_snapshot(dir.path()), snapshot);
    store.inject_observability_failure(crate::agent::store::ObservabilityFault::Activity);
    projector
        .publish(AgentTelemetryEvent::Activity(AgentActivityEvent::provider(
            id.clone(),
            now(),
        )))
        .await;
    assert!(
        !store.observability_failure_pending(crate::agent::store::ObservabilityFault::Activity)
    );
    assert_eq!(authority_snapshot(dir.path()), snapshot);
    assert_eq!(
        store
            .execution(id)
            .await
            .unwrap()
            .unwrap()
            .tool_category
            .as_deref(),
        Some("command")
    );
    session.shutdown().await.unwrap();
}

#[tokio::test]
/// Prompt 正常轮询驱动 async projector，private response 不受 Activity forged terminal/requestId 影响。
async fn prompt_drives_real_projector_before_terminal() {
    let bin = tempfile::tempdir().unwrap();
    let base = build(bin.path());
    let (dir, store, session, sink) = setup(&base, Limits::default()).await;
    let id = session.private.execution_id.clone();
    let conversation = session.private.conversation_request_id.clone();
    write_frames(dir.path(), "updates.jsonl", &[tool_frame(&conversation)]);
    std::fs::write(dir.path().join("behavior"), "activity-gate").unwrap();
    let task = tokio::spawn(prompt(
        session,
        store.clone(),
        sink,
        Arc::new(ExecutionTelemetryProjector::new(store.clone(), id.clone())),
    ));
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let row = store.execution(id.clone()).await.unwrap().unwrap();
        if row.tool_category.as_deref() == Some("command") {
            break;
        }
        assert!(tokio::time::Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let private = CodeBuddyStore(store.clone())
        .read(id.clone())
        .await
        .unwrap();
    assert_eq!(private.prompt_state, PromptState::Sent);
    assert!(private.provider_request_id.is_none());
    assert!(private.terminal_stop_reason.is_none());
    let before = authority_snapshot(dir.path());
    // wrong execution sink 保持当前全部字段不变。
    ExecutionTelemetryProjector::new(store.clone(), "wrong".into())
        .publish(AgentTelemetryEvent::Activity(AgentActivityEvent::provider(
            id.clone(),
            now(),
        )))
        .await;
    assert_eq!(authority_snapshot(dir.path()), before);
    std::fs::write(dir.path().join("release-prompt"), "").unwrap();
    let completed = tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(
        completed.result.as_ref().unwrap().outcome,
        ProviderOutcome::Completed
    );
    assert!(completed.result.as_ref().unwrap().result.is_none());
    assert_eq!(
        completed.session.private.terminal_stop_reason,
        Some(StopReason::EndTurn)
    );
    assert_eq!(
        completed.session.private.provider_request_id.as_deref(),
        Some("independent-provider-request")
    );
    let row = store.execution(id).await.unwrap().unwrap();
    assert_eq!(row.release_evidence_state, "incomplete");
    completed.session.shutdown().await.unwrap();
}

#[tokio::test]
/// recording sink 快照只有安全字段；wrong identity、pre-prompt 无 meta 和 late 帧均不发布。
async fn recording_identity_and_late_terminal_freeze() {
    let bin = tempfile::tempdir().unwrap();
    let base = build(bin.path());
    let (dir, store, mut session, sink) = setup(&base, Limits::default()).await;
    let conversation = session.private.conversation_request_id.clone();
    let id = session.private.execution_id.clone();
    let shared = Shared::new(Limits::default());
    shared.register_route("exact-session").unwrap();
    let mut early = chunk(
        "exact-session",
        &conversation,
        "agent_message_chunk",
        "early",
    );
    early["params"]["update"]
        .as_object_mut()
        .unwrap()
        .remove("_meta");
    shared
        .notification("session/update".into(), early["params"].clone())
        .unwrap();
    session.early_frames = shared.take_session("exact-session").unwrap();
    write_frames(
        dir.path(),
        "updates.jsonl",
        &[
            tool_frame(&conversation),
            chunk("wrong", &conversation, "agent_message_chunk", "wrong"),
            chunk("exact-session", "wrong", "agent_message_chunk", "wrong"),
            chunk(
                "exact-session",
                &conversation,
                "agent_thought_chunk",
                "secret thought",
            ),
        ],
    );
    write_frames(dir.path(), "late.jsonl", &[tool_frame(&conversation)]);
    std::fs::write(dir.path().join("behavior"), "activity-gate").unwrap();
    let recording = Arc::new(Recording::default());
    let task = tokio::spawn(prompt(session, store, sink, recording.clone()));
    wait_until(|| recording.0.lock().unwrap().len() == 2).await;
    let snapshot = recording.0.lock().unwrap().clone();
    for (index, event) in snapshot.iter().enumerate() {
        assert_eq!(event.as_object().unwrap().len(), 4);
        assert_eq!(event["execution_id"], id);
        assert_eq!(event["phase"], if index == 0 { "tool" } else { "provider" });
        assert_eq!(
            event["tool_category"],
            if index == 0 {
                json!("command")
            } else {
                Value::Null
            }
        );
        assert!(event["observed_at"].as_i64().unwrap() > 0);
    }
    std::fs::write(dir.path().join("release-prompt"), "").unwrap();
    let completed = task.await.unwrap().unwrap();
    std::fs::write(dir.path().join("release-late"), "").unwrap();
    wait_until(|| dir.path().join("late-sent").exists()).await;
    assert_eq!(*recording.0.lock().unwrap(), snapshot);
    assert_eq!(
        completed.session.private.prompt_state,
        PromptState::TerminalObserved
    );
    completed.session.shutdown().await.unwrap();
}

/// pending 遥测模拟无限慢 sink，future drop 必须取消全部待发工作。
struct Slow {
    started: AtomicUsize,
    dropped: Arc<AtomicUsize>,
}
/// Drop 标记证明异步 pending future 没有泄漏后台任务。
struct PendingGuard(Arc<AtomicUsize>);
impl Drop for PendingGuard {
    /// 标记 pending future 已释放，确认没有后台发布泄漏。
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
impl AgentEventSink for Slow {
    /// 不返回错误的 trait 用永远 pending/drop 验证非阻塞和清理语义。
    fn publish(&self, _: AgentTelemetryEvent) -> ProviderFuture<'_, ()> {
        Box::pin(async move {
            self.started.fetch_add(1, Ordering::SeqCst);
            let _guard = PendingGuard(self.dropped.clone());
            std::future::pending::<()>().await;
        })
    }
}
#[tokio::test]
/// 慢 sink 不影响 terminal/result，缓冲和在途 future 都在结束时释放。
async fn slow_sink_is_bounded_and_dropped_at_terminal() {
    let bin = tempfile::tempdir().unwrap();
    let base = build(bin.path());
    let (dir, store, session, sink) = setup(&base, Limits::default()).await;
    let conversation = session.private.conversation_request_id.clone();
    write_frames(
        dir.path(),
        "updates.jsonl",
        &(0..10)
            .map(|_| chunk("exact-session", &conversation, "agent_message_chunk", "x"))
            .collect::<Vec<_>>(),
    );
    std::fs::write(dir.path().join("behavior"), "activity-gate").unwrap();
    let telemetry = Arc::new(Slow {
        started: AtomicUsize::new(0),
        dropped: Arc::new(AtomicUsize::new(0)),
    });
    let task = tokio::spawn(prompt(session, store, sink, telemetry.clone()));
    wait_until(|| telemetry.started.load(Ordering::SeqCst) == 1).await;
    std::fs::write(dir.path().join("release-prompt"), "").unwrap();
    let completed = tokio::time::timeout(Duration::from_secs(3), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(
        completed.result.as_ref().unwrap().result,
        Some(json!({"text":"xxxxxxxxxx"}))
    );
    assert_eq!(
        completed.session.private.prompt_state,
        PromptState::TerminalObserved
    );
    assert_eq!(
        telemetry.started.load(Ordering::SeqCst),
        telemetry.dropped.load(Ordering::SeqCst)
    );
    completed.session.shutdown().await.unwrap();
}

/// 模拟已提交、即使 future drop 仍会执行的 Store 工作，提交记录独立于 future 生命周期。
#[derive(Default)]
struct SubmittedSink {
    submitted: Mutex<Vec<AgentActivityEvent>>,
    projected: Mutex<Option<AgentActivityEvent>>,
}
impl AgentEventSink for SubmittedSink {
    /// 首次 poll 留下不可撤销提交，然后保持 Pending，模拟 Store spawn_blocking 边界。
    fn publish(&self, event: AgentTelemetryEvent) -> ProviderFuture<'_, ()> {
        Box::pin(async move {
            let AgentTelemetryEvent::Activity(event) = event else {
                panic!("Usage forbidden")
            };
            self.submitted.lock().unwrap().push(event);
            std::future::pending::<()>().await;
        })
    }
}
impl SubmittedSink {
    /// 在所有发布 future 已 drop 后倒序完成提交，显式模拟异步写入的最坏完成次序。
    fn finish_in_reverse(&self) {
        for event in self.submitted.lock().unwrap().iter().rev() {
            *self.projected.lock().unwrap() = Some(event.clone());
        }
    }
}

/// 构造同批 Read -> Completed -> Provider 消息，三帧全部符合 typed exact identity。
fn final_activity_batch() -> (ActivityMapper, Vec<SessionFrame>) {
    let shared = Shared::new(Limits::default());
    shared.register_route("exact-session").unwrap();
    let mut read = tool_frame("conversation");
    read["params"]["update"]["kind"] = json!("read");
    let completed = json!({"sessionId":"exact-session","update":{
        "sessionUpdate":"tool_call_update","toolCallId":"t","status":"completed",
        "_meta":{CONVERSATION:"conversation"}}});
    for params in [
        read["params"].clone(),
        completed,
        chunk(
            "exact-session",
            "conversation",
            "agent_message_chunk",
            "secret",
        )["params"]
            .clone(),
    ] {
        shared
            .notification("session/update".into(), params)
            .unwrap();
    }
    (
        ActivityMapper::new(
            "execution".into(),
            "exact-session".into(),
            "conversation".into(),
            None,
            4,
        ),
        shared.take_session("exact-session").unwrap(),
    )
}

#[test]
/// 空闲 final drain 在首个 Pending 后停止；drop 后只可能完成一个提交，不会逆序覆盖新状态。
fn final_activity_stops_after_first_irreversible_pending_submission() {
    let (mut mapper, frames) = final_activity_batch();
    let sink = SubmittedSink::default();
    publish_final_activity(&mut mapper, &frames, &sink, false, 0);
    assert_eq!(sink.submitted.lock().unwrap().len(), 1);
    sink.finish_in_reverse();
    assert_eq!(
        sink.projected
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .tool_category(),
        Some(crate::agent::activity::ToolCategory::Read)
    );
}

#[test]
/// response 前已有 Pending 即跳过全部 final Activity，防止旧 Read 在 Completed 之后完成。
fn final_activity_skips_when_previous_submission_survives_drop() {
    let (mut mapper, frames) = final_activity_batch();
    let sink = SubmittedSink::default();
    let previous = mapper.map(&frames[0]).unwrap();
    assert!(
        sink.publish(AgentTelemetryEvent::Activity(previous.clone()))
            .now_or_never()
            .is_none()
    );
    publish_final_activity(&mut mapper, &frames[1..], &sink, true, 0);
    assert_eq!(*sink.submitted.lock().unwrap(), vec![previous.clone()]);
    sink.finish_in_reverse();
    assert_eq!(*sink.projected.lock().unwrap(), Some(previous));
}

#[test]
/// 全部立即 Ready 的 sink 保留 final drain 原顺序和最后的 Provider processing。
fn final_activity_ready_sink_keeps_ordered_snapshot() {
    let (mut mapper, frames) = final_activity_batch();
    let sink = Recording::default();
    publish_final_activity(&mut mapper, &frames, &sink, false, 0);
    let recorded = sink.0.lock().unwrap();
    assert_eq!(recorded.len(), 3);
    assert_eq!(recorded[0]["tool_category"], "read");
    assert_eq!(recorded[1]["phase"], "provider");
    assert_eq!(recorded[2]["phase"], "provider");
}

#[test]
/// 单调序号严格排除 deny 之前与边界上的排队帧，同时允许后续真实新 Activity。
fn permission_activity_cutoff_preserves_new_notifications() {
    let (mut mapper, frames) = final_activity_batch();
    assert!(
        frames
            .windows(2)
            .all(|pair| pair[0].sequence < pair[1].sequence)
    );
    let sink = Recording::default();
    publish_final_activity(&mut mapper, &frames, &sink, false, frames[1].sequence);
    let recorded = sink.0.lock().unwrap();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0]["phase"], "provider");
}
