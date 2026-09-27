//! 官方 ACP SDK 的受管字节流接线；不持有进程 spawn authority。

use super::protocol::{Failure, Limits, Shared};
use agent_client_protocol::{
    Agent, ByteStreams, Client, ConnectionTo, Dispatch, HandleDispatchFrom, Handled,
    JsonRpcRequest,
    schema::{
        ProtocolVersion,
        v1::{InitializeRequest, InitializeResponse},
    },
};
use futures::io::{AsyncRead, AsyncWrite};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    io,
    pin::Pin,
    sync::Arc,
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
        let future = self.connection.send_request(request).block_task();
        let mut lifetime = RequestLifetime {
            shared: self.shared.clone(),
            completed: false,
        };
        let result = tokio::select! {
            biased;
            _ = stopped.wait_for(|value| value.is_some()) => Err(self.shared.failure().unwrap_or(Failure::Closed)),
            _ = tokio::time::sleep(self.shared.limits.request_timeout) => Err(self.shared.fail(Failure::Timeout)),
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
                .connect_with(ByteStreams::new(GuardedWrite::new(outgoing, state.clone()), GuardedRead::new(incoming, state.clone())),
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
        }.with_subscriber(tracing::subscriber::NoSubscriber::default()));
        match ready_rx.await {
            Ok(connection) => Ok(Self {
                requests: Requests {
                    connection,
                    shared,
                    slots: Arc::new(Semaphore::new(limits.pending)),
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
                // permission baseline 永远取消，不宣告文件系统、terminal 或 elicitation 能力。
                if request.method() == "session/request_permission" {
                    responder.respond(json!({"outcome":{"outcome":"cancelled"}}))
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
}
impl<W> GuardedWrite<W> {
    /// 仅缓冲一条正在写出的 SDK frame。
    fn new(inner: W, shared: Arc<Shared>) -> Self {
        Self {
            inner,
            shared,
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
            // 本卡不发送 notification（包括 SDK 的 drop-time cancel）。
            if raw.get("method").is_some() && raw.get("id").is_none() {
                return Poll::Ready(Err(io_failure(&this.shared, Failure::Closed)));
            }
            if let Err(error) = this.shared.outgoing(&raw) {
                return Poll::Ready(Err(io_failure(&this.shared, error)));
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
                    this.shared.acknowledge();
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
