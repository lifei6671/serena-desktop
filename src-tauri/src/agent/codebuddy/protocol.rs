//! ACP 私有资源边界；SDK 仍拥有请求关联和协议分发，不向公共错误输出 wire。

use crate::agent::provider::registry::ProviderHealth;
use futures::task::AtomicWaker;
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{sync::watch, time::Instant};

/// 本卡所有失败均为稳定码；只有精确版本不兼容可影响 Registry health。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Failure {
    Incompatible,
    Eof,
    Io,
    InvalidJson,
    Malformed,
    Timeout,
    Closed,
    FrameLimit,
    PendingLimit,
    QueueCount,
    QueueBytes,
    QueueExpired,
    RouteLimit,
    Remote,
    Launch,
    Cleanup,
    State,
    Configuration,
}

impl Failure {
    /// 返回不携带 stderr、error.data、路径或用户内容的固定诊断。
    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::Incompatible => "CODEBUDDY_ACP_INCOMPATIBLE",
            Self::Eof => "CODEBUDDY_ACP_EOF",
            Self::Io => "CODEBUDDY_ACP_IO",
            Self::InvalidJson => "CODEBUDDY_ACP_INVALID_JSON",
            Self::Malformed => "CODEBUDDY_ACP_MALFORMED",
            Self::Timeout => "CODEBUDDY_ACP_TIMEOUT",
            Self::Closed => "CODEBUDDY_ACP_CLOSED",
            Self::FrameLimit => "CODEBUDDY_ACP_FRAME_LIMIT",
            Self::PendingLimit => "CODEBUDDY_ACP_PENDING_LIMIT",
            Self::QueueCount => "CODEBUDDY_ACP_QUEUE_COUNT",
            Self::QueueBytes => "CODEBUDDY_ACP_QUEUE_BYTES",
            Self::QueueExpired => "CODEBUDDY_ACP_QUEUE_EXPIRED",
            Self::RouteLimit => "CODEBUDDY_ACP_ROUTE_LIMIT",
            Self::Remote => "CODEBUDDY_ACP_REQUEST_FAILED",
            Self::Launch => "CODEBUDDY_ACP_LAUNCH_FAILED",
            Self::Cleanup => "CODEBUDDY_ACP_CLEANUP_TIMEOUT",
            Self::State => "CODEBUDDY_PREPARATION_STATE_CONFLICT",
            Self::Configuration => "CODEBUDDY_SESSION_CONFIGURATION_INVALID",
        }
    }

    /// 不依赖错误字符串、CLI 版本或 capability 推断 Provider 兼容性。
    pub(crate) fn health_change(self) -> Option<ProviderHealth> {
        (self == Self::Incompatible).then_some(ProviderHealth::Unavailable)
    }
}

/// 固定生产上限，测试可使用更小值；调用者不能取得裸 SDK connection。
#[derive(Clone, Copy)]
pub(crate) struct Limits {
    pub(crate) frame_bytes: usize,
    pub(crate) pending: usize,
    pub(crate) queue_count: usize,
    pub(crate) queue_bytes: usize,
    pub(crate) routes: usize,
    pub(crate) queue_ttl: Duration,
    pub(crate) request_timeout: Duration,
    pub(crate) stderr_bytes: usize,
}

impl Default for Limits {
    /// 上限同时覆盖尚未注册与已注册但尚未消费的 session frame。
    fn default() -> Self {
        Self {
            frame_bytes: 256 * 1024,
            pending: 32,
            queue_count: 64,
            queue_bytes: 1024 * 1024,
            routes: 64,
            queue_ttl: Duration::from_secs(10),
            request_timeout: Duration::from_secs(15),
            stderr_bytes: 8192,
        }
    }
}

/// 只在 Provider 私有边界保存的 session 通知；不派生 Debug 避免原文日志。
pub(crate) struct SessionFrame {
    pub(crate) session_id: String,
    pub(crate) method: String,
    pub(crate) params: Value,
    bytes: usize,
    expires: Instant,
}

/// 仅 session/new 的白名单扩展；不暴露任意 response 或 request payload。
pub(crate) struct SessionNewExtensions {
    session_id: String,
    pub(crate) models: Option<Value>,
}

/// 固定计数诊断；未知与重复 response 共用稳定 UNMATCHED_RESPONSE_ID 分类。
#[derive(Clone, Copy, Default, Debug)]
pub(crate) struct Diagnostics {
    pub(crate) unmatched_response_id: u64,
    pub(crate) ignored_notification: u64,
}

/// 受同一 mutex 保护的有界状态，不保存任意错误字符串。
struct State {
    pending: HashMap<String, String>,
    inflight: bool,
    frames: VecDeque<SessionFrame>,
    bytes: usize,
    routes: HashSet<String>,
    diagnostics: Diagnostics,
    session_new_extensions: Option<SessionNewExtensions>,
}

/// ByteStreams guard、SDK handler 与请求入口共享生命周期。
pub(crate) struct Shared {
    pub(crate) limits: Limits,
    state: Mutex<State>,
    pub(crate) stop: watch::Sender<Option<Failure>>,
    pub(crate) reader: AtomicWaker,
}

impl Shared {
    /// 创建单 Runtime 的私有 bounded 状态。
    pub(crate) fn new(limits: Limits) -> Arc<Self> {
        let (stop, _) = watch::channel(None);
        Arc::new(Self {
            limits,
            stop,
            reader: AtomicWaker::new(),
            state: Mutex::new(State {
                pending: HashMap::new(),
                inflight: false,
                frames: VecDeque::new(),
                bytes: 0,
                routes: HashSet::new(),
                diagnostics: Diagnostics::default(),
                session_new_extensions: None,
            }),
        })
    }

    /// 保留首个故障，并清理所有 session/关联状态；唤醒 driver、reader 和请求 waiter。
    pub(crate) fn fail(&self, failure: Failure) -> Failure {
        self.stop.send_if_modified(|slot| {
            if slot.is_none() {
                *slot = Some(failure);
                true
            } else {
                false
            }
        });
        let mut state = self.state.lock().unwrap();
        state.pending.clear();
        state.session_new_extensions = None;
        state.frames.clear();
        state.routes.clear();
        state.bytes = 0;
        state.inflight = false;
        drop(state);
        self.reader.wake();
        self.failure().unwrap()
    }

    /// 读取首个稳定失败，调用方不再接纳新请求或 session frame。
    pub(crate) fn failure(&self) -> Option<Failure> {
        *self.stop.borrow()
    }

    /// SDK 完成 notification/response dispatch 或 server reply flush 后再允许下一帧。
    pub(crate) fn acknowledge(&self) {
        self.state.lock().unwrap().inflight = false;
        self.reader.wake();
    }

    /// 入口最多一帧未被 SDK 消费，避免 SDK 内部无界 channel 被无限填充。
    pub(crate) fn waiting_for_dispatch(&self) -> bool {
        self.state.lock().unwrap().inflight
    }

    /// 出站完整 frame 在真正写入前记录 SDK 生成的 id；不会自己生成/分发请求。
    pub(crate) fn outgoing(&self, raw: &Value) -> Result<(), Failure> {
        if let Some(error) = self.failure() {
            return Err(error);
        }
        if let (Some(method), Some(id)) = (raw.get("method").and_then(Value::as_str), raw.get("id"))
        {
            let mut state = self.state.lock().unwrap();
            if state.pending.len() >= self.limits.pending {
                return Err(Failure::PendingLimit);
            }
            if state
                .pending
                .insert(id.to_string(), method.into())
                .is_some()
            {
                return Err(Failure::Malformed);
            }
        }
        Ok(())
    }

    /// 严格验证 envelope 后交 SDK；未知/重复 id 明确诊断并忽略，不影响其他 pending。
    pub(crate) fn incoming(&self, raw: &Value) -> Result<bool, Failure> {
        if let Some(error) = self.failure() {
            return Err(error);
        }
        validate_envelope(raw)?;
        let mut state = self.state.lock().unwrap();
        // SDK 对部分 control notification 会在 handler 前消费；本卡只投递 session/update。
        // 其余 notification 明确诊断/忽略，不占用需要 dispatcher ack 的窗口，也不回复。
        if raw.get("id").is_none()
            && raw.get("method").and_then(Value::as_str) != Some("session/update")
        {
            state.diagnostics.ignored_notification =
                state.diagnostics.ignored_notification.saturating_add(1);
            return Ok(false);
        }
        if raw.get("method").is_none() {
            let id = raw["id"].to_string();
            let Some(method) = state.pending.remove(&id) else {
                state.diagnostics.unmatched_response_id =
                    state.diagnostics.unmatched_response_id.saturating_add(1);
                return Ok(false);
            };
            if method == "initialize"
                && let Some(result) = raw.get("result")
            {
                // 非数值/缺失版本是 malformed，不是假定的协议不兼容。
                let version = result
                    .get("protocolVersion")
                    .and_then(Value::as_u64)
                    .ok_or(Failure::Malformed)?;
                if version != 1 {
                    return Err(Failure::Incompatible);
                }
            }
            if method == "session/new"
                && let Some(result) = raw.get("result")
            {
                let session_id = result
                    .get("sessionId")
                    .and_then(Value::as_str)
                    .filter(|id| !id.is_empty())
                    .ok_or(Failure::Malformed)?;
                if state.session_new_extensions.is_some()
                    || result
                        .get("models")
                        .is_some_and(|models| !models.is_object())
                {
                    return Err(Failure::Malformed);
                }
                // 原 frame 已通过 GuardedRead 的 bytes 上限；最多持有一个未消费 snapshot。
                state.session_new_extensions = Some(SessionNewExtensions {
                    session_id: session_id.into(),
                    models: result.get("models").cloned(),
                });
            }
        }
        state.inflight = true;
        Ok(true)
    }

    /// SDK 完成 typed response 后再次核对 exact session；不参与 SDK waiter 的完成。
    pub(crate) fn take_session_new_extensions(
        &self,
        session_id: &str,
    ) -> Result<SessionNewExtensions, Failure> {
        if let Some(error) = self.failure() {
            return Err(error);
        }
        let snapshot = self.state.lock().unwrap().session_new_extensions.take();
        match snapshot {
            Some(snapshot) if snapshot.session_id == session_id && !session_id.is_empty() => {
                Ok(snapshot)
            }
            _ => Err(self.fail(Failure::Malformed)),
        }
    }

    /// session/update 仅在 exact sessionId 下入队；没有 id 的其他通知只诊断。
    pub(crate) fn notification(&self, method: String, params: Value) -> Result<(), Failure> {
        if let Some(error) = self.failure() {
            return Err(error);
        }
        if !method.starts_with("session/") {
            let mut state = self.state.lock().unwrap();
            state.diagnostics.ignored_notification =
                state.diagnostics.ignored_notification.saturating_add(1);
            return Ok(());
        }
        let session_id = params
            .get("sessionId")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .ok_or(Failure::Malformed)?
            .to_owned();
        let bytes = serde_json::to_vec(&params)
            .map_err(|_| Failure::Malformed)?
            .len()
            + method.len();
        let mut state = self.state.lock().unwrap();
        if state.frames.len() >= self.limits.queue_count {
            return Err(Failure::QueueCount);
        }
        if bytes > self.limits.queue_bytes.saturating_sub(state.bytes) {
            return Err(Failure::QueueBytes);
        }
        state.bytes += bytes;
        state.frames.push_back(SessionFrame {
            session_id,
            method,
            params,
            bytes,
            expires: Instant::now() + self.limits.queue_ttl,
        });
        Ok(())
    }

    /// 注册精确路由，后续 take_session 按 wire 原顺序取得早到和实时帧。
    pub(crate) fn register_route(&self, session_id: &str) -> Result<(), Failure> {
        self.check_expiry()?;
        if session_id.is_empty() || session_id.len() > self.limits.frame_bytes {
            return Err(Failure::Malformed);
        }
        let mut state = self.state.lock().unwrap();
        if !state.routes.contains(session_id) && state.routes.len() >= self.limits.routes {
            return Err(Failure::RouteLimit);
        }
        state.routes.insert(session_id.into());
        Ok(())
    }

    /// 只消费已注册的 exact route，其他 session 不串流、不丢弃。
    pub(crate) fn take_session(&self, session_id: &str) -> Result<Vec<SessionFrame>, Failure> {
        self.check_expiry()?;
        let mut state = self.state.lock().unwrap();
        if !state.routes.contains(session_id) {
            return Ok(Vec::new());
        }
        let mut result = Vec::new();
        let mut retained = VecDeque::new();
        while let Some(frame) = state.frames.pop_front() {
            if frame.session_id == session_id {
                state.bytes -= frame.bytes;
                result.push(frame);
            } else {
                retained.push_back(frame);
            }
        }
        state.frames = retained;
        Ok(result)
    }

    /// TTL 不静默丢帧；过期明确关闭整个本地 operation。
    pub(crate) fn check_expiry(&self) -> Result<(), Failure> {
        if let Some(error) = self.failure() {
            return Err(error);
        }
        let expired = self
            .state
            .lock()
            .unwrap()
            .frames
            .front()
            .is_some_and(|frame| frame.expires <= Instant::now());
        if expired {
            Err(self.fail(Failure::QueueExpired))
        } else {
            Ok(())
        }
    }

    /// 仅暴露常数大小的诊断计数，不包含 session/wire 数据。
    pub(crate) fn diagnostics(&self) -> Diagnostics {
        self.state.lock().unwrap().diagnostics
    }
}

/// 外部 JSON-RPC 边界禁止 ambiguous envelopes、batch、null/fractional ids。
fn validate_envelope(raw: &Value) -> Result<(), Failure> {
    if !raw.is_object() || raw.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Err(Failure::Malformed);
    }
    let id = raw.get("id");
    if id.is_some_and(|id| !id.is_string() && !id.is_i64()) {
        return Err(Failure::Malformed);
    }
    if let Some(method) = raw.get("method") {
        if method.as_str().is_none_or(str::is_empty)
            || raw.get("result").is_some()
            || raw.get("error").is_some()
            || raw
                .get("params")
                .is_some_and(|p| !p.is_object() && !p.is_array())
        {
            return Err(Failure::Malformed);
        }
    } else {
        if id.is_none() || (raw.get("result").is_some() == raw.get("error").is_some()) {
            return Err(Failure::Malformed);
        }
        if let Some(error) = raw.get("error")
            && (error.get("code").and_then(Value::as_i64).is_none()
                || error.get("message").and_then(Value::as_str).is_none())
        {
            return Err(Failure::Malformed);
        }
    }
    // 与 SDK 实际 wire schema 对齐，防止其丢弃 malformed response 后无法 ack 输入窗口。
    // 例如 JSON-RPC error code 在官方 schema 中是 i32，不是任意 i64。
    serde_json::from_value::<agent_client_protocol::RawJsonRpcMessage>(raw.clone())
        .map_err(|_| Failure::Malformed)?;
    Ok(())
}

/// stderr 私有滚动尾部；持续 drain，不把内容放进 Debug 或公开错误。
pub(crate) struct StderrTail {
    bytes: VecDeque<u8>,
    limit: usize,
}
impl StderrTail {
    /// 只分配固定上限空间。
    pub(crate) fn new(limit: usize) -> Self {
        Self {
            bytes: VecDeque::new(),
            limit,
        }
    }
    /// 超过上限时滚动丢弃最旧诊断字节，非关键协议帧。
    pub(crate) fn push(&mut self, bytes: &[u8]) {
        for byte in bytes {
            if self.limit == 0 {
                break;
            }
            if self.bytes.len() == self.limit {
                self.bytes.pop_front();
            }
            self.bytes.push_back(*byte);
        }
    }
    #[cfg(test)]
    /// 测试可观察保留尾部，生产没有公开 payload 出口。
    pub(crate) fn bytes(&self) -> Vec<u8> {
        self.bytes.iter().copied().collect()
    }
}
