//! 内部单次 Prompt / terminal primitive；保留 Runtime，不授权 generic finalize 或 Claim release。
use super::{
    activity::ActivityMapper,
    fresh::PreparedFreshSession,
    protocol::{Failure, SessionFrame},
    store::{CodeBuddyStore, Mutation, Ownership, PromptState},
};
use crate::agent::{
    coordinator::now,
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

/// terminal 与失败均保留原 Runtime；private 中的 terminal 元数据不进入公共 result。
pub(crate) struct PromptCompletion {
    pub(crate) session: PreparedFreshSession,
    pub(crate) result: Result<ProviderRunResult, Failure>,
}

/// 消费准备对象；取消等待只发本地放弃信号，不实现 ACP session/cancel。
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
async fn run(
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
        if cancelled.try_recv() != Err(oneshot::error::TryRecvError::Empty) {
            return Err(Failure::Closed);
        }
        session.mark_accepted(sink.as_ref())?;
        let mut activity = ActivityMapper::new(
            row.id.clone(),
            session_id.clone(),
            current.conversation_request_id.clone(),
            current.provider_request_id.clone(),
            requests.shared.limits.queue_count,
        );
        let (response, publication_pending) = {
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
            let request = requests.request(request);
            tokio::pin!(request);
            // 轮询间隔小于 queue TTL；每次释放 exact route 的 count/bytes 预算。
            let interval = (requests.shared.limits.queue_ttl / 4)
                .min(Duration::from_millis(10))
                .max(Duration::from_micros(1));
            let mut tick = tokio::time::interval(interval);
            loop {
                // 单一在途 publish 与有界 FIFO，不阻塞 request/timeout，也不创建后台任务。
                if publishing.is_none()
                    && let Some(event) = queue.pop_front()
                {
                    publishing = Some(telemetry.publish(event));
                }
                tokio::select! {
                    biased;
                    _ = &mut cancelled => break Err(Failure::Closed),
                    response = &mut request => break response.map(|response| (response, publishing.is_some())),
                    _ = async { match publishing.as_mut() {
                        Some(future) => future.await,
                        None => std::future::pending().await,
                    }} => { publishing = None; }
                    _ = tick.tick() => {
                        let frames = requests.shared.take_session(&session_id)?;
                        for frame in &frames {
                            if let Some(event) = activity.map(frame)
                                && queue.len() < requests.shared.limits.queue_count
                            {
                                queue.push_back(AgentTelemetryEvent::Activity(event));
                            }
                        }
                        collector.drain(frames, &session_id,
                            &current.conversation_request_id, requests.shared.limits.queue_bytes);
                    }
                }
            }
        }?;
        // SDK exact response 到达后做最后一次 drain；此后不再修改正文快照。
        let frames = requests.shared.take_session(&session_id)?;
        publish_final_activity(&mut activity, &frames, telemetry.as_ref(), publication_pending);
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
        if let Some(value) = meta.get(PROVIDER_REQUEST) {
            let id = value.as_str().ok_or(Failure::Malformed)?;
            if !id.is_empty() {
                session.private = private_store
                    .mutate(
                        row.id.clone(),
                        owner.clone(),
                        session.private.revision,
                        Mutation::ExactProviderRequest(id.into()),
                    )
                    .await
                    .map_err(|_| Failure::State)?;
            }
        }
        session.private = private_store
            .mutate(
                row.id,
                owner,
                session.private.revision,
                Mutation::ObserveTerminal {
                    session_id,
                    conversation_request_id: current.conversation_request_id,
                    stop_reason: response.stop_reason,
                    observed_at: now(),
                },
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

/// final drain 仅串行推进立即就绪事件；Pending 的已提交副作用不能靠 drop 撤销。
fn publish_final_activity(
    activity: &mut ActivityMapper,
    frames: &[SessionFrame],
    telemetry: &dyn AgentEventSink,
    publication_pending: bool,
) {
    // 已有在途发布可能仍在写 Store，不再提交后续事件，避免终态边界发生逆序覆盖。
    if publication_pending {
        return;
    }
    for frame in frames {
        if let Some(event) = activity.map(frame)
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

#[cfg(test)]
mod tests;
