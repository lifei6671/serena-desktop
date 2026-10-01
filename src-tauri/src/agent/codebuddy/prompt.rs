//! 内部单次 Prompt / terminal primitive；保留 Runtime，不授权 generic finalize 或 Claim release。
use super::{
    activity::ActivityMapper,
    fresh::PreparedFreshSession,
    protocol::{Failure, SessionFrame},
    store::{CodeBuddyStore, Mutation, Ownership, PromptState},
};
use crate::agent::{
    coordinator::now,
    execution::state::{DispatchState, Transition},
    provider::{
        ProviderOutcome, ProviderResultCompleteness, ProviderRunResult,
        port::{AgentEventSink, ProviderAcceptanceSink, ProviderFuture},
        telemetry::AgentTelemetryEvent,
    },
    store::StateStore,
};
use agent_client_protocol::schema::v1::{ContentBlock, PromptRequest, StopReason, TextContent};
use futures::FutureExt;
use serde_json::{Value, json};
use std::{collections::VecDeque, sync::Arc, time::Duration};
use tokio::sync::oneshot;

const CONVERSATION: &str = "codebuddy.ai/conversationRequestId";
const PROVIDER_REQUEST: &str = "codebuddy.ai/requestId";

#[cfg(test)]
tokio::task_local! {
    /// 确定性测试屏障：exact response 已冻结、私有事务尚未提交。
    pub(super) static BEFORE_TERMINAL_COMMIT: (Arc<tokio::sync::Notify>, Arc<tokio::sync::Notify>);
}

/// terminal 与失败均保留原 Runtime；private 中的 terminal 元数据不进入公共 result。
pub(crate) struct PromptCompletion {
    pub(crate) session: PreparedFreshSession,
    pub(crate) result: Result<ProviderRunResult, Failure>,
}

/// 消费准备对象；caller drop 是本地放弃，用户取消则由原 owner 观察 durable intent。
pub(crate) async fn prompt(
    session: PreparedFreshSession,
    store: StateStore,
    sink: Arc<dyn ProviderAcceptanceSink>,
    telemetry: Arc<dyn AgentEventSink>,
) -> Result<PromptCompletion, Failure> {
    let (cancel, cancelled) = oneshot::channel();
    // 单个有界 ownership task 保证 SQLite 写入不因 caller future drop 而悬空。
    // 放弃后仅收敛 Uncertain 并走 Runtime 的既有 Job cleanup，不重试请求。
    let task = tokio::spawn(run(session, store, sink, telemetry, cancelled));
    let completion = task.await.map_err(|_| Failure::State);
    drop(cancel);
    completion
}

/// 所有状态写入都等待事务完成；调用方放弃不会中断 MarkSent/ObserveTerminal 的提交。
pub(super) async fn run(
    mut session: PreparedFreshSession,
    store: StateStore,
    sink: Arc<dyn ProviderAcceptanceSink>,
    telemetry: Arc<dyn AgentEventSink>,
    mut cancelled: oneshot::Receiver<()>,
) -> PromptCompletion {
    let private_store = CodeBuddyStore(store.clone());
    let mut sent = false;
    let result = async {
        session.check_acceptance_ready()?;
        let row = store
            .execution(session.private.execution_id.clone())
            .await
            .map_err(|_| Failure::State)?
            .ok_or(Failure::State)?;
        let current = private_store
            .read(row.id.clone())
            .await
            .map_err(|_| Failure::State)?;
        let session_id = session.catalog.response.session_id.to_string();
        if current != session.private
            || current.prompt_state != PromptState::Prepared
            || current.acp_protocol_version != Some(1)
            || session.handshake.response.protocol_version.as_u16() != 1
            || current.session_id.as_deref() != Some(session_id.as_str())
            || current.runtime_instance_id.as_deref() != session.runtime.runtime_id()
            || current.runtime_instance_id.is_none()
            || current.runtime_instance_id != row.runtime_instance_id
            || row.provider != "codebuddy"
            || row.status != "dispatch_pending"
            || row.dispatch_state != "not_dispatched"
            || row.interrupt_requested_at.is_some()
            || row.provider_terminal_status.is_some()
        {
            return Err(Failure::State);
        }
        let owner = Ownership {
            execution_revision: row.revision,
            runtime_instance_id: row.runtime_instance_id,
        };
        let requests = session
            .runtime
            .client
            .as_ref()
            .ok_or(Failure::Closed)?
            .requests
            .clone();
        let request = PromptRequest::new(
            session_id.clone(),
            vec![ContentBlock::Text(TextContent::new(row.prompt))],
        )
        .meta(serde_json::Map::from_iter([(
            CONVERSATION.into(),
            json!(current.conversation_request_id),
        )]));
        requests.preflight(&request)?;
        let mut collector = Collector::default();
        let early_frames = std::mem::take(&mut session.early_frames);
        if cancelled.try_recv() != Err(oneshot::error::TryRecvError::Empty) {
            return Err(Failure::Closed);
        }
        let effective_profile = session.catalog.effective_execution_profile()?;
        // exact Execution/Provider/R1 已核验后先提交实际配置；失败时不得写 Sent 或发送 Prompt。
        store
            .set_effective_execution_profile(
                row.id.clone(),
                row.provider.clone(),
                owner
                    .runtime_instance_id
                    .clone()
                    .ok_or(Failure::State)?,
                effective_profile,
            )
            .await
            .map_err(|_| Failure::State)?;
        session.private = private_store
            .mutate(
                row.id.clone(),
                owner.clone(),
                current.revision,
                Mutation::MarkSent { rpc_id: None },
            )
            .await
            .map_err(|_| Failure::State)?;
        sent = true;
        if session.continued {
            // v13 约束要求 recovery lifecycle 从 durable Sent 开始；load/history 此时已验证，
            // 物理 child prompt 尚未发送，acceptance 也尚未对外发布。
            session.private = private_store
                .mutate(
                    row.id.clone(),
                    owner.clone(),
                    session.private.revision,
                    Mutation::BeginContinuationLoad {
                        session_id: session_id.clone(),
                        recovery_runtime_instance_id: current
                            .runtime_instance_id
                            .clone()
                            .ok_or(Failure::State)?,
                    },
                )
                .await
                .map_err(|_| Failure::State)?;
            session.private = private_store
                .mutate(
                    row.id.clone(),
                    owner.clone(),
                    session.private.revision,
                    Mutation::FinishContinuationLoad,
                )
                .await
                .map_err(|_| Failure::State)?;
        }
        if cancelled.try_recv() != Err(oneshot::error::TryRecvError::Empty) {
            return Err(Failure::Closed);
        }
        session.mark_accepted(sink.as_ref())?;
        store
            .provider_event(
                row.id.clone(),
                Transition::Dispatch {
                    to: DispatchState::Dispatching,
                    runtime_id: owner.runtime_instance_id.clone(),
                },
                now(),
            )
            .await
            .map_err(|_| Failure::State)?;
        let mut flush = requests
            .observe_prompt_flush(session_id.clone(), current.conversation_request_id.clone())?;
        // permit 来自上方 durable exact owner 核验；guard 在所有返回路径撤销上下文。
        let (_permission, mut permission_events) = requests.shared.register_permission(
            current.runtime_instance_id.clone().ok_or(Failure::State)?, row.id.clone(),
            session_id.clone(), current.conversation_request_id.clone(), current.provider_request_id.clone(),
            super::permission_policy::Authority {
                lease: crate::workspace_resolver::WorkspaceLease {
                    workspace_id: row.workspace_id.clone(),
                    canonical_root: row.canonical_workspace_root.clone().into(),
                    generation: row.workspace_generation,
                },
                mode: row.mode.clone(),
            })?;
        let mut flushed = false;
        let mut activity_after = 0;
        let mut activity = ActivityMapper::new(
            row.id.clone(),
            session_id.clone(),
            current.conversation_request_id.clone(),
            current.provider_request_id.clone(),
            requests.shared.limits.queue_count,
        );
        let (response, publication_pending, frames) = {
            let mut queue = VecDeque::new();
            // early 帧同样经过 exact identity；接受完成前不调用 telemetry。
            for frame in &early_frames {
                if let Some(event) = activity.map(frame)
                    && queue.len() < requests.shared.limits.queue_count
                {
                    queue.push_back(AgentTelemetryEvent::Activity(event));
                }
            }
            collector.drain(
                early_frames,
                &session_id,
                &current.conversation_request_id,
                requests.shared.limits.queue_bytes,
            );
            let mut publishing: Option<ProviderFuture<'_, ()>> = None;
            let mut permission_publication: Option<ProviderFuture<'_, ()>> = None;
            let request = requests.request(request);
            tokio::pin!(request);
            // durable Store 读取作为独立 future，不能阻塞 exact response 或绝对 deadline。
            let intent = wait_cancel_intent(&store, &session.private);
            tokio::pin!(intent);
            let mut intent_seen = false;
            let mut cancel_send: Option<ProviderFuture<'_, Result<tokio::time::Instant, Failure>>> = None;
            let mut cancel_started = false;
            let mut cancel_deadline = None;
            // 轮询间隔小于 queue TTL；每次释放 exact route 的 count/bytes 预算。
            let interval = (requests.shared.limits.queue_ttl / 4)
                .min(Duration::from_millis(10))
                .max(Duration::from_micros(1));
            let mut tick = tokio::time::interval(interval);
            let prompt_result = async { loop {
                // 单一在途 publish 与有界 FIFO，不阻塞 request/timeout，也不创建后台任务。
                if publishing.is_none() && permission_publication.is_none()
                    && let Some(event) = queue.pop_front()
                {
                    publishing = Some(telemetry.publish(event));
                }
                tokio::select! {
                    biased;
                    _ = &mut cancelled => break Err(Failure::Closed),
                    response = &mut request => {
                        let response = match response {
                            Ok(response) => response,
                            Err(error) => {
                                if cancel_started {
                                    interrupt_failed(&store, &row.id, if error == Failure::Timeout {
                                        "CODEBUDDY_CANCEL_TIMEOUT"
                                    } else { "CODEBUDDY_CANCEL_SEND_FAILED" }).await?;
                                }
                                break Err(error);
                            }
                        };
                        // exact response 优先于尚未发送的 cancel，绝不追加 late notification。
                        if !flushed {
                            tokio::time::timeout(requests.shared.limits.request_timeout, &mut flush)
                                .await.map_err(|_| Failure::Timeout)?.map_err(|_| Failure::Io)?;
                            dispatch_flushed(&store, &row.id).await?;
                        }
                        // 先冻结 exact response 的 collector 输入，再 drop 在途 cancel（会关闭 transport）。
                        let frames = requests.shared.take_session(&session_id)?;
                        if cancel_deadline.is_none() && requests.cancel_flushed_at().is_some() {
                            store.provider_event(row.id.clone(), Transition::InterruptAck, now())
                                .await.map_err(|_| Failure::State)?;
                        }
                        break Ok((response, publishing.is_some(), frames));
                    },
                    observation = &mut flush, if !flushed => {
                        observation.map_err(|_| Failure::Io)?;
                        dispatch_flushed(&store, &row.id).await?;
                        flushed = true;
                    }
                    Some(event) = permission_events.recv(), if permission_publication.is_none() => {
                        // 已收到的旧 Activity 不得在 deny 后延迟覆盖；正文仍交 collector 完整消费。
                        activity_after = event.activity_sequence;
                        queue.clear();
                        let previous = publishing.take();
                        let store = &store;
                        let timeout = requests.shared.limits.request_timeout;
                        permission_publication = Some(Box::pin(async move {
                            let _ = tokio::time::timeout(timeout, async {
                                // 已提交的 SQLite 副作用不能靠 drop 撤销，先等待旧 publication 完成再写 deny。
                                if let Some(previous) = previous { previous.await; }
                                event.project(store).await;
                            }).await;
                        }));
                    }
                    _ = async { match permission_publication.as_mut() {
                        Some(future) => future.await,
                        None => std::future::pending().await,
                    }} => { permission_publication = None; }
                    observed = &mut intent, if !intent_seen => {
                        observed?;
                        intent_seen = true;
                    }
                    // 从真实 flush 时刻计时，不被 activity 或 Store 调度延长。
                    _ = async {
                        match cancel_deadline {
                            Some(deadline) => tokio::time::sleep_until(deadline).await,
                            None => std::future::pending().await,
                        }
                    } => {
                        interrupt_failed(&store, &row.id, "CODEBUDDY_CANCEL_TIMEOUT").await?;
                        break Err(Failure::Timeout);
                    }
                    sent = async {
                        match cancel_send.as_mut() {
                            Some(future) => future.await,
                            None => std::future::pending().await,
                        }
                    } => {
                        cancel_send = None;
                        match sent {
                            Ok(at) => {
                                cancel_deadline = Some(at + requests.shared.limits.request_timeout);
                                store.provider_event(row.id.clone(), Transition::InterruptAck, now())
                                    .await.map_err(|_| Failure::State)?;
                            }
                            Err(error) => {
                                interrupt_failed(&store, &row.id, if error == Failure::Timeout {
                                    "CODEBUDDY_CANCEL_TIMEOUT"
                                } else { "CODEBUDDY_CANCEL_SEND_FAILED" }).await?;
                                break Err(error);
                            }
                        }
                    }
                    _ = async { match publishing.as_mut() {
                        Some(future) => future.await,
                        None => std::future::pending().await,
                    }} => { publishing = None; }
                    _ = tick.tick() => {
                        let frames = requests.shared.take_session(&session_id)?;
                        for frame in &frames {
                            if let Some(event) = activity.map(frame)
                                && frame.sequence > activity_after
                                && queue.len() < requests.shared.limits.queue_count
                            {
                                queue.push_back(AgentTelemetryEvent::Activity(event));
                            }
                        }
                        collector.drain(frames, &session_id,
                            &current.conversation_request_id, requests.shared.limits.queue_bytes);
                    }
                }
                if intent_seen && flushed && !cancel_started {
                    // intent 可能先于 prompt flush，发送前重新核对 durable exact ownership。
                    wait_cancel_intent(&store, &session.private).await?;
                    // Running 后重新提交幂等 intent，使通用状态由 CancelRequested 接受 ACK。
                    store.request_cancel(row.id.clone(), now()).await.map_err(|_| Failure::State)?;
                    cancel_started = true;
                    cancel_send = Some(Box::pin(requests.cancel_session(session_id.clone())));
                }
            }}.await;
            // terminal、EOF、timeout 均先有界交付已物理提交的安全决策，不能因 biased select 丢弃。
            let _ = tokio::time::timeout(requests.shared.limits.request_timeout, async {
                if let Some(publication) = permission_publication.take() { publication.await; }
                while let Ok(event) = permission_events.try_recv() {
                    activity_after = event.activity_sequence;
                    queue.clear();
                    if let Some(previous) = publishing.take() { previous.await; }
                    event.project(&store).await;
                }
            }).await;
            prompt_result.map(|(response, _, frames)| (response, publishing.is_some(), frames))
        }?;
        // SDK exact response 到达后做最后一次 drain；此后不再修改正文快照。
        publish_final_activity(
            &mut activity,
            &frames,
            telemetry.as_ref(),
            publication_pending,
            activity_after,
        );
        drop(activity);
        collector.drain(
            frames,
            &session_id,
            &current.conversation_request_id,
            requests.shared.limits.queue_bytes,
        );
        let meta = response.meta.as_ref().ok_or(Failure::Malformed)?;
        if meta.get(CONVERSATION).and_then(Value::as_str)
            != Some(current.conversation_request_id.as_str())
        {
            return Err(Failure::Malformed);
        }
        let result = collector.finish(row.id.clone(), response.stop_reason)?;
        let provider_request_id = meta.get(PROVIDER_REQUEST)
            .map(|value| value.as_str().ok_or(Failure::Malformed))
            .transpose()?.filter(|id| !id.is_empty()).map(str::to_owned);
        #[cfg(test)]
        if let Ok((ready, release)) = BEFORE_TERMINAL_COMMIT.try_with(Clone::clone) {
            ready.notify_one();
            release.notified().await;
        }
        session.private = private_store
            .observe_prompt_response(
                session.private.clone(),
                provider_request_id,
                response.stop_reason,
                now(),
            )
            .await
            .map_err(|_| Failure::State)?;
        Ok(result)
    }
    .await;
    let result = if sent && result.is_err() {
        // 失败后只重新读取收敛 Uncertain，不重试 terminal 写入；并发 terminal 永不覆盖。
        let uncertain = async {
            let current = private_store
                .read(session.private.execution_id.clone())
                .await
                .map_err(|_| Failure::State)?;
            let row = store
                .execution(current.execution_id.clone())
                .await
                .map_err(|_| Failure::State)?
                .ok_or(Failure::State)?;
            if current.runtime_instance_id != session.private.runtime_instance_id
                || current.session_id != session.private.session_id
                || current.conversation_request_id != session.private.conversation_request_id
                || current.prompt_state != PromptState::Sent
            {
                return Err(Failure::State);
            }
            if row.dispatch_state == "dispatching" {
                store
                    .provider_event(
                        row.id.clone(),
                        Transition::Dispatch {
                            to: DispatchState::Uncertain,
                            runtime_id: None,
                        },
                        now(),
                    )
                    .await
                    .map_err(|_| Failure::State)?;
            }
            let row = store
                .execution(row.id.clone())
                .await
                .map_err(|_| Failure::State)?
                .ok_or(Failure::State)?;
            private_store
                .mutate(
                    current.execution_id,
                    Ownership {
                        execution_revision: row.revision,
                        runtime_instance_id: row.runtime_instance_id,
                    },
                    current.revision,
                    Mutation::MarkUncertain,
                )
                .await
                .map_err(|_| Failure::State)
        }
        .await;
        match uncertain {
            Ok(private) => {
                session.private = private;
                result
            }
            Err(_) => Err(Failure::State),
        }
    } else {
        result
    };
    PromptCompletion { session, result }
}

/// 每次都验证原 Runtime 与完整 private identity；陈旧 owner 不获得通知 authority。
async fn wait_cancel_intent(
    store: &StateStore,
    expected: &super::store::PrivateState,
) -> Result<(), Failure> {
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tick.tick().await;
        let row = store
            .execution(expected.execution_id.clone())
            .await
            .map_err(|_| Failure::State)?
            .ok_or(Failure::State)?;
        let private = CodeBuddyStore(store.clone())
            .read(expected.execution_id.clone())
            .await
            .map_err(|_| Failure::State)?;
        if row.id != expected.execution_id
            || row.provider != "codebuddy"
            || row.runtime_instance_id != expected.runtime_instance_id
            || row.runtime_instance_id.is_none()
            || private != *expected
            || row.provider_terminal_status.is_some()
        {
            return Err(Failure::State);
        }
        if row.interrupt_requested_at.is_some() {
            return Ok(());
        }
    }
}

/// 通知失败只提交 interrupt 诊断；Runtime owner 仍须终止 Job 并收集证据。
async fn interrupt_failed(store: &StateStore, id: &str, diagnostic: &str) -> Result<(), Failure> {
    store
        .provider_event(
            id.into(),
            Transition::InterruptTimeout {
                diagnostic: diagnostic.into(),
            },
            now(),
        )
        .await
        .map_err(|_| Failure::State)
}

/// 只有 GuardedWrite 的 exact flush observation 可以推进 generic dispatch evidence。
async fn dispatch_flushed(store: &StateStore, id: &str) -> Result<(), Failure> {
    store
        .provider_event(
            id.into(),
            Transition::Dispatch {
                to: DispatchState::Dispatched,
                runtime_id: None,
            },
            now(),
        )
        .await
        .map_err(|_| Failure::State)?;
    store
        .provider_event(id.into(), Transition::Running, now())
        .await
        .map_err(|_| Failure::State)
}

/// final drain 仅串行推进立即就绪事件；Pending 的已提交副作用不能靠 drop 撤销。
fn publish_final_activity(
    activity: &mut ActivityMapper,
    frames: &[SessionFrame],
    telemetry: &dyn AgentEventSink,
    publication_pending: bool,
    activity_after: u64,
) {
    // 已有在途发布可能仍在写 Store，不再提交后续事件，避免终态边界发生逆序覆盖。
    if publication_pending {
        return;
    }
    for frame in frames {
        if let Some(event) = activity.map(frame)
            && frame.sequence > activity_after
            && telemetry
                .publish(AgentTelemetryEvent::Activity(event))
                .now_or_never()
                .is_none()
        {
            // 首次 Pending 后放弃余下 Activity；private collector 仍消费整批快照。
            break;
        }
    }
}

/// 只保留有界正文；任何无法归属或超预算片段都降级 completeness。
#[derive(Default)]
struct Collector {
    text: String,
    tainted: bool,
}

impl Collector {
    /// 只接收 exact session + conversation 的 typed Text；foreign conversation 不污染结果。
    fn drain(
        &mut self,
        frames: Vec<SessionFrame>,
        session: &str,
        conversation: &str,
        max_bytes: usize,
    ) {
        for frame in frames {
            if frame.session_id != session || frame.method != "session/update" {
                continue;
            }
            let update = &frame.params["update"];
            if update["sessionUpdate"] != "agent_message_chunk" {
                continue;
            }
            match update["_meta"][CONVERSATION].as_str() {
                Some(id) if !id.is_empty() && id != conversation => continue,
                Some(id) if id == conversation => {}
                _ => {
                    self.tainted = true;
                    continue;
                }
            }
            match serde_json::from_value::<ContentBlock>(update["content"].clone()) {
                Ok(ContentBlock::Text(content))
                    if content.text.len() <= max_bytes.saturating_sub(self.text.len()) =>
                {
                    self.text.push_str(&content.text)
                }
                _ => self.tainted = true,
            }
        }
    }

    /// typed non-exhaustive stop reason 必须明确映射；未来 variant 不默认成功。
    fn finish(self, execution_id: String, stop: StopReason) -> Result<ProviderRunResult, Failure> {
        let (outcome, complete, diagnostic) = match stop {
            StopReason::EndTurn => (ProviderOutcome::Completed, true, None),
            StopReason::Cancelled => (ProviderOutcome::Cancelled, false, None),
            StopReason::MaxTokens => (
                ProviderOutcome::Interrupted,
                false,
                Some("CODEBUDDY_PROMPT_MAX_TOKENS"),
            ),
            StopReason::MaxTurnRequests => (
                ProviderOutcome::Interrupted,
                false,
                Some("CODEBUDDY_PROMPT_MAX_TURN_REQUESTS"),
            ),
            StopReason::Refusal => (
                ProviderOutcome::Failed,
                true,
                Some("CODEBUDDY_PROMPT_REFUSED"),
            ),
            _ => return Err(Failure::Malformed),
        };
        let result_completeness = if complete && !self.tainted {
            ProviderResultCompleteness::Complete
        } else if self.text.is_empty() {
            ProviderResultCompleteness::Unknown
        } else {
            ProviderResultCompleteness::Partial
        };
        Ok(ProviderRunResult {
            execution_id,
            outcome,
            result: (!self.text.is_empty()).then(|| json!({"text":self.text})),
            result_completeness,
            diagnostic_code: diagnostic
                .or(self.tainted.then_some("CODEBUDDY_PROMPT_RESULT_INCOMPLETE"))
                .map(str::to_owned),
        })
    }
}

#[cfg(all(test, windows))]
mod tests;
