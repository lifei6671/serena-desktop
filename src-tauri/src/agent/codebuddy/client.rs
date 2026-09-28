//! 官方 ACP SDK 的受管字节流接线；不持有进程 spawn authority。

use super::protocol::{Failure, Limits, Shared};
use agent_client_protocol::{
    Agent, ByteStreams, Client, ConnectionTo, Dispatch, HandleDispatchFrom, Handled,
    JsonRpcRequest,
    schema::{
        ProtocolVersion,
        v1::{
            CancelNotification, InitializeRequest, InitializeResponse, RequestPermissionRequest,
            RequestPermissionResponse,
        },
    },
};
use futures::io::{AsyncRead, AsyncWrite};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    io,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::Duration,
};
use tokio::{
    sync::{Semaphore, oneshot},
    task::JoinHandle,
};
use tracing::instrument::WithSubscriber;

/// 仅保留 typed initialize 结果；未知扩展可缺省，不投影 public capabilities。
pub(crate) struct Handshake {
    pub(crate) response: InitializeResponse,
}

/// 受 admission/timeout/shutdown 约束的请求入口；不泄漏裸 SDK connection。
#[derive(Clone)]
pub(crate) struct Requests {
    connection: ConnectionTo<Agent>,
    pub(crate) shared: Arc<Shared>,
    slots: Arc<Semaphore>,
    prompt_flush: Arc<Mutex<Option<PromptFlush>>>,
    cancel: Arc<Mutex<CancelSlot>>,
}

/// 每个原始 client 最多授权一次 cancel；消费后不可重新注册。
#[derive(Default)]
struct CancelSlot {
    used: bool,
    permit: Option<(String, oneshot::Sender<tokio::time::Instant>)>,
    flushed_at: Option<tokio::time::Instant>,
}

/// 单个 fresh Prompt 的 exact 身份与有界一次性物理 flush 观察者。
struct PromptFlush {
    session: String,
    conversation: String,
    sender: oneshot::Sender<Value>,
}

/// 请求 future 被放弃时必须关闭 transport，不能留下 SDK pending 或自动 cancel 产品接线。
struct RequestLifetime {
    shared: Arc<Shared>,
    completed: bool,
}
impl Drop for RequestLifetime {
    /// 正常完成解除 guard；取消/放弃通过统一 shutdown 唤醒全部 waiter。
    fn drop(&mut self) {
        if !self.completed {
            self.shared.fail(Failure::Closed);
        }
    }
}

impl Requests {
    /// terminal 分支可读取已经发生的物理证据，绝不轮询/启动尚未发送的 notification。
    pub(crate) fn cancel_flushed_at(&self) -> Option<tokio::time::Instant> {
        self.cancel.lock().ok().and_then(|slot| slot.flushed_at)
    }
    /// SDK enqueue 不代表 ACK；只有 exact 通知的物理 flush 完成才能返回。
    pub(crate) async fn cancel_session(
        &self,
        session: String,
    ) -> Result<tokio::time::Instant, Failure> {
        if let Some(error) = self.shared.failure() {
            return Err(error);
        }
        let (sender, receiver) = oneshot::channel();
        {
            let mut slot = self.cancel.lock().map_err(|_| Failure::State)?;
            if slot.used {
                return Err(Failure::Closed);
            }
            slot.used = true;
            slot.permit = Some((session.clone(), sender));
        }
        let mut lifetime = RequestLifetime {
            shared: self.shared.clone(),
            completed: false,
        };
        let mut stopped = self.shared.stop.subscribe();
        let result = match self
            .connection
            .send_notification(CancelNotification::new(session))
        {
            Err(_) => Err(Failure::Io),
            Ok(()) => tokio::select! {
                biased;
                observed = async {
                    match receiver.await {
                        // exact response 已先到达输入 guard：取消不再发送，等待原 request 消费它。
                        Err(_) if self.shared.prompt_response_received() => std::future::pending().await,
                        observed => observed.map_err(|_| Failure::Io),
                    }
                } => observed,
                _ = stopped.wait_for(|value| value.is_some()) => Err(self.shared.failure().unwrap_or(Failure::Closed)),
                _ = tokio::time::sleep(self.shared.limits.request_timeout) => Err(Failure::Timeout),
            },
        };
        self.cancel
            .lock()
            .map_err(|_| Failure::State)?
            .permit
            .take();
        lifetime.completed = true;
        result.map_err(|error| self.shared.fail(error))
    }

    /// 在 SDK 编码前注册 exact Prompt；id 仍完全由 SDK 生成。
    pub(crate) fn observe_prompt_flush(
        &self,
        session: String,
        conversation: String,
    ) -> Result<oneshot::Receiver<Value>, Failure> {
        let mut observer = self.prompt_flush.lock().map_err(|_| Failure::State)?;
        if observer.is_some() {
            return Err(Failure::State);
        }
        let (sender, receiver) = oneshot::channel();
        *observer = Some(PromptFlush {
            session,
            conversation,
            sender,
        });
        Ok(receiver)
    }

    /// Prompt send-intent 前与实际 request 共用完整 typed payload 大小及 admission 检查。
    pub(crate) fn preflight<R: JsonRpcRequest>(&self, request: &R) -> Result<(), Failure> {
        if let Some(error) = self.shared.failure() {
            return Err(error);
        }
        if self.slots.available_permits() == 0 {
            return Err(Failure::PendingLimit);
        }
        let raw = request
            .to_untyped_message()
            .map_err(|_| Failure::Malformed)?;
        let size = serde_json::to_vec(raw.params())
            .map_err(|_| Failure::Malformed)?
            .len()
            + raw.method().len()
            + 128;
        if size > self.shared.limits.frame_bytes {
            return Err(Failure::FrameLimit);
        }
        Ok(())
    }

    /// 先限制 pending，再由官方 SDK 生成 id 和完成对应 response。
    pub(crate) async fn request<R: JsonRpcRequest>(
        &self,
        request: R,
    ) -> Result<R::Response, Failure> {
        self.preflight(&request)?;
        let _slot = self
            .slots
            .try_acquire()
            .map_err(|_| Failure::PendingLimit)?;
        let mut stopped = self.shared.stop.subscribe();
        let is_prompt = request
            .to_untyped_message()
            .map_err(|_| Failure::Malformed)?
            .method()
            == "session/prompt";
        let future = self.connection.send_request(request).block_task();
        let mut lifetime = RequestLifetime {
            shared: self.shared.clone(),
            completed: false,
        };
        let result = tokio::select! {
            biased;
            _ = stopped.wait_for(|value| value.is_some()) => Err(self.shared.failure().unwrap_or(Failure::Closed)),
            _ = async {
                tokio::time::sleep(self.shared.limits.request_timeout).await;
                // cancel 已开始后由 owner 的 send/physical-flush deadline 接管，不能沿用旧 Prompt 起点。
                let cancelling = self.cancel.lock().map(|slot| slot.used).unwrap_or(false);
                if is_prompt && cancelling { std::future::pending::<()>().await; }
            } => Err(self.shared.fail(Failure::Timeout)),
            response = future => response.map_err(|_| self.shared.failure().unwrap_or(Failure::Remote)),
        };
        lifetime.completed = true;
        if let Err(error) = result {
            self.shared.fail(error);
        }
        result
    }

    /// 精确发送 v1；raw 版本已由输入 guard 校验，typed 再次核验。
    pub(crate) async fn initialize(&self) -> Result<Handshake, Failure> {
        let response = self
            .request(InitializeRequest::new(ProtocolVersion::V1))
            .await?;
        if response.protocol_version != ProtocolVersion::V1 {
            return Err(self.shared.fail(Failure::Incompatible));
        }
        Ok(Handshake { response })
    }
}

/// 独占 driver 生命周期；drop 也能关闭所有 clone 的请求入口。
pub(crate) struct ManagedClient {
    pub(crate) requests: Requests,
    driver: Option<JoinHandle<()>>,
}

impl ManagedClient {
    /// 仅连接已由调用方拥有的流；没有 Command/AcpAgent/Stdio spawn helper。
    pub(crate) async fn connect<W, R>(
        outgoing: W,
        incoming: R,
        limits: Limits,
    ) -> Result<Self, Failure>
    where
        W: AsyncWrite + Unpin + Send + 'static,
        R: AsyncRead + Unpin + Send + 'static,
    {
        let shared = Shared::new(limits);
        let (ready_tx, ready_rx) = oneshot::channel();
        let state = shared.clone();
        let prompt_flush = Arc::new(Mutex::new(None));
        let driver_flush = prompt_flush.clone();
        let cancel = Arc::new(Mutex::new(CancelSlot::default()));
        let driver_cancel = cancel.clone();
        let driver = tokio::spawn(async move {
            let mut stopped = state.stop.subscribe();
            let maintenance = async {
                loop {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                    if state.check_expiry().is_err() { break; }
                }
            };
            let sdk = Client.builder().name("codebuddy-managed")
                .with_handler(Dispatcher(state.clone()))
                .on_close({ let state = state.clone(); async move |_cx| { state.fail(Failure::Eof); Ok(()) } })
                .connect_with(ByteStreams::new(GuardedWrite::with_cancel(outgoing, state.clone(), driver_flush, driver_cancel.clone()), GuardedRead::new(incoming, state.clone())),
                    async move |connection| {
                        ready_tx.send(connection).map_err(|_| agent_client_protocol::Error::internal_error())?;
                        std::future::pending::<Result<(), agent_client_protocol::Error>>().await
                    });
            tokio::select! {
                biased;
                _ = stopped.wait_for(|value| value.is_some()) => {},
                _ = maintenance => {},
                result = sdk => { state.fail(if result.is_ok() { Failure::Eof } else { Failure::Io }); },
            }
            state.fail(Failure::Closed);
            driver_cancel.lock().unwrap().permit.take();
        }.with_subscriber(tracing::subscriber::NoSubscriber::default()));
        match ready_rx.await {
            Ok(connection) => Ok(Self {
                requests: Requests {
                    connection,
                    shared,
                    slots: Arc::new(Semaphore::new(limits.pending)),
                    prompt_flush,
                    cancel,
                },
                driver: Some(driver),
            }),
            Err(_) => {
                let _ = driver.await;
                Err(shared.failure().unwrap_or(Failure::Io))
            }
        }
    }

    /// 显式关闭并 join SDK driver，pending waiter 通过共享首错确定失败。
    pub(crate) async fn shutdown(mut self) {
        self.requests.shared.fail(Failure::Closed);
        if let Some(driver) = self.driver.take() {
            let _ = driver.await;
        }
    }
}

impl Drop for ManagedClient {
    /// owner 消失时不让裸 connection clone 继续活动。
    fn drop(&mut self) {
        self.requests.shared.fail(Failure::Closed);
    }
}

/// SDK 分流回调；绝不在 handler 内等待另一个请求。
struct Dispatcher(Arc<Shared>);
impl HandleDispatchFrom<Agent> for Dispatcher {
    /// Responder/ResponseRouter 均由 SDK 提供，保持 exact id authority。
    async fn handle_dispatch_from(
        &mut self,
        message: Dispatch,
        _connection: ConnectionTo<Agent>,
    ) -> Result<Handled<Dispatch>, agent_client_protocol::Error> {
        let result = match message {
            Dispatch::Request(request, responder) => {
                // 官方 typed request/response 与 SDK responder 保持 exact id；失败关闭原 Runtime。
                if request.method() == "session/request_permission" {
                    let decision = serde_json::from_value::<RequestPermissionRequest>(
                        request.params().clone(),
                    )
                    .map_err(|_| Failure::Malformed)
                    .and_then(|request| {
                        self.0.permission_response(
                            &request,
                            serde_json::to_value(responder.id()).map_err(|_| Failure::Malformed)?,
                        )
                    });
                    match decision {
                        Ok(response) => responder
                            .cast::<RequestPermissionResponse>()
                            .respond(response),
                        Err(error) => {
                            self.0.fail(error);
                            return Err(agent_client_protocol::Error::invalid_params());
                        }
                    }
                } else {
                    responder.respond_with_error(agent_client_protocol::Error::method_not_found())
                }
                // 请求读窗口留给 GuardedWrite：直到 reply 真正 flush 才开放。
            }
            Dispatch::Notification(notification) => {
                let (method, params) = notification.into_parts();
                if let Err(error) = self.0.notification(method, params) {
                    self.0.fail(error);
                }
                self.0.acknowledge();
                Ok(())
            }
            Dispatch::Response(result, router) => {
                let result = router.route_with_result(result);
                self.0.acknowledge();
                result
            }
        };
        if result.is_err() {
            self.0.fail(Failure::Io);
        }
        result.map(|_| Handled::Yes)
    }

    /// SDK 调试链只显示固定名称。
    fn describe_chain(&self) -> impl std::fmt::Debug {
        "CodeBuddyDispatcher"
    }
}

/// 为 SDK 提供完整、预验证且有上限的行；不完成任何 request waiter。
struct GuardedRead<R> {
    inner: R,
    shared: Arc<Shared>,
    staged: VecDeque<u8>,
    frame: Vec<u8>,
    offset: usize,
    validated: bool,
}
impl<R> GuardedRead<R> {
    /// 物理 read 分片固定 4KiB，未分发 frame 最多一份。
    fn new(inner: R, shared: Arc<Shared>) -> Self {
        Self {
            inner,
            shared,
            staged: VecDeque::new(),
            frame: Vec::new(),
            offset: 0,
            validated: false,
        }
    }
}

/// I/O 层只返回固定失败码，不拼接 provider 原文。
fn io_failure(shared: &Shared, failure: Failure) -> io::Error {
    io::Error::other(shared.fail(failure).code())
}

/// Win32 已关闭 peer 的 pipe 写入实测为 BrokenPipe；与读侧 EOF 使用相同首错分类。
fn pipe_failure(shared: &Shared, error: io::Error) -> io::Error {
    io_failure(
        shared,
        if error.kind() == io::ErrorKind::BrokenPipe {
            Failure::Eof
        } else {
            Failure::Io
        },
    )
}

impl<R: AsyncRead + Unpin> AsyncRead for GuardedRead<R> {
    /// 即使连续未知 id 或无换行输入也保持单次 poll 有界。
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        target: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        if target.is_empty() {
            return Poll::Ready(Ok(0));
        }
        for _ in 0..16 {
            if let Some(error) = this.shared.failure() {
                return Poll::Ready(Err(io_failure(&this.shared, error)));
            }
            if this.validated && this.offset < this.frame.len() {
                let count = target.len().min(this.frame.len() - this.offset);
                target[..count].copy_from_slice(&this.frame[this.offset..this.offset + count]);
                this.offset += count;
                return Poll::Ready(Ok(count));
            }
            if this.validated {
                this.frame.clear();
                this.offset = 0;
                this.validated = false;
            }
            this.shared.reader.register(cx.waker());
            if this.shared.waiting_for_dispatch() {
                return Poll::Pending;
            }
            while let Some(byte) = this.staged.pop_front() {
                if this.frame.len() >= this.shared.limits.frame_bytes {
                    return Poll::Ready(Err(io_failure(&this.shared, Failure::FrameLimit)));
                }
                this.frame.push(byte);
                if byte == b'\n' {
                    let raw = match serde_json::from_slice::<Value>(&this.frame) {
                        Ok(raw) => raw,
                        Err(_) => {
                            return Poll::Ready(Err(io_failure(
                                &this.shared,
                                Failure::InvalidJson,
                            )));
                        }
                    };
                    match this.shared.incoming(&raw) {
                        Ok(true) => {
                            this.validated = true;
                            break;
                        }
                        Ok(false) => {
                            this.frame.clear();
                            break;
                        }
                        Err(error) => return Poll::Ready(Err(io_failure(&this.shared, error))),
                    }
                }
            }
            if this.validated || !this.staged.is_empty() {
                continue;
            }
            let mut bytes = [0; 4096];
            match Pin::new(&mut this.inner).poll_read(cx, &mut bytes) {
                Poll::Ready(Ok(0)) => {
                    return Poll::Ready(Err(io_failure(
                        &this.shared,
                        if this.frame.is_empty() {
                            Failure::Eof
                        } else {
                            Failure::InvalidJson
                        },
                    )));
                }
                Poll::Ready(Ok(count)) => this.staged.extend(&bytes[..count]),
                Poll::Ready(Err(error)) => {
                    return Poll::Ready(Err(pipe_failure(&this.shared, error)));
                }
                Poll::Pending => return Poll::Pending,
            }
        }
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}

/// 官方 SDK 编码后的出站 frame gate；server response flush 为输入 admission 的确认。
struct GuardedWrite<W> {
    inner: W,
    shared: Arc<Shared>,
    frame: Vec<u8>,
    offset: usize,
    complete: bool,
    response: bool,
    prompt_flush: Arc<Mutex<Option<PromptFlush>>>,
    flushed_prompt: Option<(Value, oneshot::Sender<Value>)>,
    cancel: Arc<Mutex<CancelSlot>>,
    flushed_cancel: Option<oneshot::Sender<tokio::time::Instant>>,
}
impl<W> GuardedWrite<W> {
    /// 仅缓冲一条正在写出的 SDK frame。
    fn new(inner: W, shared: Arc<Shared>) -> Self {
        Self::with_prompt_flush(inner, shared, Arc::new(Mutex::new(None)))
    }

    /// 生产 driver 与 Requests 共享单个 Prompt observer，不干涉 SDK response waiter。
    fn with_prompt_flush(
        inner: W,
        shared: Arc<Shared>,
        prompt_flush: Arc<Mutex<Option<PromptFlush>>>,
    ) -> Self {
        Self::with_cancel(
            inner,
            shared,
            prompt_flush,
            Arc::new(Mutex::new(CancelSlot::default())),
        )
    }

    /// 单槽 cancel authority 与原 client 共用，未授权 notification 仍全部拒绝。
    fn with_cancel(
        inner: W,
        shared: Arc<Shared>,
        prompt_flush: Arc<Mutex<Option<PromptFlush>>>,
        cancel: Arc<Mutex<CancelSlot>>,
    ) -> Self {
        Self {
            inner,
            shared,
            prompt_flush,
            flushed_prompt: None,
            cancel,
            flushed_cancel: None,
            frame: Vec::new(),
            offset: 0,
            complete: false,
            response: false,
        }
    }
}
impl<W: AsyncWrite + Unpin> AsyncWrite for GuardedWrite<W> {
    /// frame 写入真实 pipe 之前检查上限并记录 SDK id。
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        if let Some(error) = this.shared.failure() {
            return Poll::Ready(Err(io_failure(&this.shared, error)));
        }
        let count = bytes
            .iter()
            .position(|b| *b == b'\n')
            .map_or(bytes.len(), |i| i + 1);
        if this.complete
            || count
                > this
                    .shared
                    .limits
                    .frame_bytes
                    .saturating_sub(this.frame.len())
        {
            return Poll::Ready(Err(io_failure(&this.shared, Failure::FrameLimit)));
        }
        this.frame.extend_from_slice(&bytes[..count]);
        if this.frame.last() == Some(&b'\n') {
            let raw: Value = match serde_json::from_slice(&this.frame) {
                Ok(raw) => raw,
                Err(_) => return Poll::Ready(Err(io_failure(&this.shared, Failure::Malformed))),
            };
            // 只允许单槽 exact cancel；SDK drop-time 或其它 notification 没有额外 authority。
            if raw["method"] == "session/cancel" && raw.get("id").is_some() {
                return Poll::Ready(Err(io_failure(&this.shared, Failure::Malformed)));
            }
            if raw.get("method").is_some() && raw.get("id").is_none() {
                let mut slot = this
                    .cancel
                    .lock()
                    .map_err(|_| io_failure(&this.shared, Failure::State))?;
                let Some((session, _)) = slot.permit.as_ref() else {
                    return Poll::Ready(Err(io_failure(&this.shared, Failure::Closed)));
                };
                if raw["method"] != "session/cancel"
                    || raw["params"] != json!({"sessionId":session})
                    || raw.as_object().is_none_or(|frame| frame.len() != 3)
                {
                    return Poll::Ready(Err(io_failure(&this.shared, Failure::Malformed)));
                }
                this.flushed_cancel = Some(slot.permit.take().expect("checked permit").1);
            }
            if let Err(error) = this.shared.outgoing(&raw) {
                return Poll::Ready(Err(io_failure(&this.shared, error)));
            }
            if raw["method"] == "session/prompt" {
                let mut observer = this
                    .prompt_flush
                    .lock()
                    .map_err(|_| io_failure(&this.shared, Failure::State))?;
                if let Some(expected) = observer.as_ref() {
                    if raw["params"]["sessionId"].as_str() != Some(expected.session.as_str())
                        || raw["params"]["_meta"]["codebuddy.ai/conversationRequestId"].as_str()
                            != Some(expected.conversation.as_str())
                        || raw.get("id").is_none_or(Value::is_null)
                    {
                        return Poll::Ready(Err(io_failure(&this.shared, Failure::Malformed)));
                    }
                    let expected = observer.take().expect("checked observer");
                    this.flushed_prompt = Some((raw["id"].clone(), expected.sender));
                }
            }
            this.response = raw.get("method").is_none();
            this.complete = true;
        }
        Poll::Ready(Ok(count))
    }

    /// 确认物理写入与 flush 后才释放 server request 的读窗口。
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if let Some(error) = this.shared.failure() {
            return Poll::Ready(Err(io_failure(&this.shared, error)));
        }
        // exact Prompt response 可以先到达 read guard、稍后才唤醒 owner；这个窗口也不得发 late cancel。
        if this.flushed_cancel.is_some() && this.shared.prompt_response_received() {
            this.flushed_cancel.take();
            this.frame.clear();
            this.offset = 0;
            this.complete = false;
            this.response = false;
            return Poll::Ready(Ok(()));
        }
        while this.offset < this.frame.len() {
            match Pin::new(&mut this.inner).poll_write(cx, &this.frame[this.offset..]) {
                Poll::Ready(Err(error)) => {
                    return Poll::Ready(Err(pipe_failure(&this.shared, error)));
                }
                Poll::Ready(Ok(0)) => {
                    return Poll::Ready(Err(io_failure(&this.shared, Failure::Io)));
                }
                Poll::Ready(Ok(count)) => this.offset += count,
                Poll::Pending => return Poll::Pending,
            }
        }
        match Pin::new(&mut this.inner).poll_flush(cx) {
            Poll::Ready(Ok(())) => {
                if this.response {
                    if let Err(error) = this.shared.permission_flushed() {
                        return Poll::Ready(Err(io_failure(&this.shared, error)));
                    }
                    this.shared.acknowledge();
                }
                // 整 frame 已真实写入且 inner.flush 成功，才发布 SDK exact id。
                if this.complete
                    && let Some((id, sender)) = this.flushed_prompt.take()
                {
                    this.shared.activate_permission();
                    let _ = sender.send(id);
                }
                if this.complete
                    && let Some(sender) = this.flushed_cancel.take()
                {
                    let at = tokio::time::Instant::now();
                    this.cancel
                        .lock()
                        .map_err(|_| io_failure(&this.shared, Failure::State))?
                        .flushed_at = Some(at);
                    let _ = sender.send(at);
                }
                this.frame.clear();
                this.offset = 0;
                this.complete = false;
                this.response = false;
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Err(error)) => Poll::Ready(Err(pipe_failure(&this.shared, error))),
            Poll::Pending => Poll::Pending,
        }
    }

    /// shutdown 由外层 owner 管理，禁止未完成数据被当成已写入。
    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_close(cx)
    }
}

#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;
