mod binding;
// Bounded protocol client. It never mutates Executions or releases Claims.
use super::protocol::{self, *};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, SystemTime},
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    sync::{OwnedSemaphorePermit, Semaphore, mpsc, oneshot, watch},
    time::{Instant, timeout_at},
};
use tokio_util::sync::CancellationToken;

struct Queued<T> {
    value: T,
    _bytes: OwnedSemaphorePermit,
    _slot: Option<OwnedSemaphorePermit>,
}
struct Pending {
    method: String,
    execution: Option<String>,
    deadline: Instant,
    response: oneshot::Sender<Result<Value>>,
    terminal_turn: Option<String>,
}
type WriteItem = Queued<(Vec<u8>, oneshot::Sender<Result<()>>)>;
struct Shared {
    runtime_id: String,
    pending: Mutex<HashMap<u64, Pending>>,
    next_id: AtomicU64,
    ready: AtomicBool,
    initializing: AtomicBool,
    cancel: CancellationToken,
    failure: watch::Sender<Option<ProtocolError>>,
    writer: mpsc::Sender<WriteItem>,
    writer_bytes: Arc<Semaphore>,
    stderr: Mutex<VecDeque<u8>>,
}
struct RequestGuard {
    shared: Arc<Shared>,
    active: bool,
}
impl Drop for RequestGuard {
    fn drop(&mut self) {
        if self.active {
            self.shared.fail(ProtocolError::new(
                "CODEX_REQUEST_CANCELLED",
                "Request future abandoned; reconcile original Runtime without replay",
            ));
        }
    }
}
impl Shared {
    fn fail(&self, error: ProtocolError) {
        let mut pending = self.pending.lock().unwrap();
        if self.failure.borrow().is_none() {
            self.failure.send_replace(Some(error.clone()));
        }
        self.ready.store(false, Ordering::Release);
        self.cancel.cancel();
        for (_, p) in pending.drain() {
            let _ = p.response.send(Err(error.clone()));
        }
    }
    fn check(&self) -> Result<()> {
        self.failure.borrow().clone().map_or(Ok(()), Err)
    }
    fn enqueue(&self, value: Value) -> Result<oneshot::Receiver<Result<()>>> {
        self.check()?;
        let bytes = encode(&value)?;
        let permit = self
            .writer_bytes
            .clone()
            .try_acquire_many_owned(bytes.len() as u32)
            .map_err(|_| {
                ProtocolError::new("CODEX_PROTOCOL_QUEUE_FULL", "Writer byte budget exhausted")
            })?;
        let (tx, rx) = oneshot::channel();
        self.writer
            .try_send(Queued {
                value: (bytes, tx),
                _bytes: permit,
                _slot: None,
            })
            .map_err(|_| {
                ProtocolError::new("CODEX_PROTOCOL_QUEUE_FULL", "Writer queue unavailable")
            })?;
        Ok(rx)
    }
}

/// Each delivered event retains its queue byte reservation until consumed.
/// The receiver is the protocol dispatcher boundary; TASK-006 supplies Execution routing.
pub struct Event {
    pub runtime_id: String,
    pub notification: Notification,
    _bytes: OwnedSemaphorePermit,
    _slot: Option<OwnedSemaphorePermit>,
}
pub struct Client {
    shared: Arc<Shared>,
    events: tokio::sync::Mutex<mpsc::Receiver<Event>>,
}
impl Drop for Client {
    fn drop(&mut self) {
        self.shared.fail(ProtocolError::new(
            "CODEX_RUNTIME_CANCELLED",
            "Client dropped; reconcile original Runtime",
        ));
    }
}
impl Client {
    /// Transport attachment is crate-internal; production callers use managed::connect,
    /// which verifies the executable and exports schema before launching the Runtime.
    #[cfg(test)]
    pub(crate) fn product_test_transport<R, W, E>(
        runtime_id: String,
        reader: R,
        writer: W,
        stderr: E,
    ) -> Self
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
        E: AsyncRead + Unpin + Send + 'static,
    {
        Self::transport(runtime_id, reader, writer, stderr)
    }
    pub(super) fn transport<R, W, E>(runtime_id: String, reader: R, writer: W, stderr: E) -> Self
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
        E: AsyncRead + Unpin + Send + 'static,
    {
        #[cfg(test)]
        let writer = tests::record_stdin(writer, &runtime_id);
        let (write_tx, mut write_rx) =
            mpsc::channel::<Queued<(Vec<u8>, oneshot::Sender<Result<()>>)>>(QUEUE_COUNT);
        let (events_tx, events) = mpsc::channel(QUEUE_COUNT);
        let (server_tx, mut server_rx) =
            mpsc::channel::<Queued<(Value, String, Value)>>(QUEUE_COUNT);
        let (failure, _) = watch::channel(None);
        let shared = Arc::new(Shared {
            runtime_id,
            pending: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            ready: AtomicBool::new(false),
            initializing: AtomicBool::new(false),
            cancel: CancellationToken::new(),
            failure,
            writer: write_tx,
            writer_bytes: Arc::new(Semaphore::new(QUEUE_BYTES)),
            stderr: Mutex::new(VecDeque::new()),
        });
        let s = shared.clone();
        tokio::spawn(async move {
            let mut writer = writer;
            loop {
                tokio::select! {
                    biased;
                    _ = s.cancel.cancelled() => break,
                    item = write_rx.recv() => {
                        let Some(item) = item else { break };
                        let write = async { writer.write_all(&item.value.0).await?; writer.flush().await };
                        let result = tokio::select! {
                            _ = s.cancel.cancelled() => break,
                            r = write => r.map_err(|e| ProtocolError::new("CODEX_STDIO_WRITE_FAILED", e.to_string())),
                        };
                        let _ = item.value.1.send(result.clone());
                        if let Err(e) = result { s.fail(e); break; }
                    }
                }
            }
        });
        let s = shared.clone();
        tokio::spawn(async move {
            while let Some(item) =
                tokio::select! { _ = s.cancel.cancelled() => None, v = server_rx.recv() => v }
            {
                let (id, method, params) = item.value;
                let response = server_reply(id, &method, &params);
                match s.enqueue(response) {
                    Ok(written) => {
                        let outcome = timeout_at(Instant::now() + RPC_TIMEOUT, written).await;
                        if !matches!(outcome, Ok(Ok(Ok(())))) {
                            s.fail(ProtocolError::new(
                                "CODEX_STDIO_WRITE_FAILED",
                                "Server response could not be flushed",
                            ));
                            break;
                        }
                    }
                    Err(e) => {
                        s.fail(e);
                        break;
                    }
                }
            }
        });
        let s = shared.clone();
        tokio::spawn(async move {
            let mut stderr = stderr;
            let mut bytes = [0; 4096];
            loop {
                let n = tokio::select! { _ = s.cancel.cancelled() => break, r = stderr.read(&mut bytes) => r };
                match n {
                    Ok(0) => break,
                    Ok(n) => {
                        let mut tail = s.stderr.lock().unwrap();
                        tail.extend(&bytes[..n]);
                        while tail.len() > STDERR_BYTES {
                            tail.pop_front();
                        }
                    }
                    Err(e) => {
                        s.fail(ProtocolError::new("CODEX_STDIO_READ_FAILED", e.to_string()));
                        break;
                    }
                }
            }
        });
        let s = shared.clone();
        tokio::spawn(async move {
            let event_bytes = Arc::new(Semaphore::new(QUEUE_BYTES));
            let event_count = Arc::new(Semaphore::new(QUEUE_COUNT));
            let mut reader = tokio::io::BufReader::new(reader);
            loop {
                let frame = tokio::select! { _ = s.cancel.cancelled() => break, r = read_frame(&mut reader) => r };
                let result = match frame {
                    Ok(frame) => dispatch(
                        &s,
                        &events_tx,
                        &server_tx,
                        &event_bytes,
                        &event_count,
                        frame,
                    ),
                    Err(e) => Err(e),
                };
                if let Err(e) = result {
                    s.fail(e);
                    break;
                }
            }
        });
        Self {
            shared,
            events: tokio::sync::Mutex::new(events),
        }
    }
    pub fn runtime_id(&self) -> &str {
        &self.shared.runtime_id
    }
    pub fn is_ready(&self) -> bool {
        self.shared.ready.load(Ordering::Acquire) && self.shared.check().is_ok()
    }
    pub fn failure(&self) -> watch::Receiver<Option<ProtocolError>> {
        self.shared.failure.subscribe()
    }
    pub fn cancel(&self) {
        self.shared.fail(ProtocolError::new(
            "CODEX_RUNTIME_CANCELLED",
            "Runtime cancellation requested",
        ));
    }
    pub fn stderr_tail(&self) -> Vec<u8> {
        self.shared.stderr.lock().unwrap().iter().copied().collect()
    }
    pub async fn next_event(&mut self) -> Result<Event> {
        self.receive_event().await
    }
    /// Allows the serial Provider dispatcher to observe events while an ACK is pending.
    pub(crate) async fn receive_event(&self) -> Result<Event> {
        let mut events = self.events.lock().await;
        tokio::select! {
            biased;
            _ = self.shared.cancel.cancelled() => Err(self.shared.failure.borrow().clone().unwrap()),
            event = events.recv() => event.ok_or_else(|| ProtocolError::new("CODEX_STDIO_EOF", "Notification dispatcher closed")),
        }
    }
    async fn rpc(
        &self,
        method: &str,
        params: Value,
        execution: Option<String>,
        deadline: Instant,
        initializing: bool,
    ) -> Result<Value> {
        self.rpc_with_flush(method, params, execution, deadline, initializing, None)
            .await
    }
    async fn rpc_with_flush(
        &self,
        method: &str,
        params: Value,
        execution: Option<String>,
        deadline: Instant,
        initializing: bool,
        flushed: Option<oneshot::Sender<()>>,
    ) -> Result<Value> {
        self.shared.check()?;
        if !initializing && !self.is_ready() {
            return Err(ProtocolError::new(
                "CODEX_APP_SERVER_INIT_FAILED",
                "initialize/initialized not complete",
            ));
        }
        let id = self.shared.next_id.fetch_add(1, Ordering::Relaxed);
        if id == u64::MAX {
            let e = ProtocolError::invalid("Request id exhausted");
            self.shared.fail(e.clone());
            return Err(e);
        }
        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self.shared.pending.lock().unwrap();
            if pending.len() >= QUEUE_COUNT {
                drop(pending);
                let e = ProtocolError::new(
                    "CODEX_PROTOCOL_QUEUE_FULL",
                    "Pending request map exhausted",
                );
                self.shared.fail(e.clone());
                return Err(e);
            }
            pending.insert(
                id,
                Pending {
                    method: method.into(),
                    execution,
                    deadline,
                    response: tx,
                    terminal_turn: None,
                },
            );
        }
        let mut guard = RequestGuard {
            shared: self.shared.clone(),
            active: true,
        };
        // Registration precedes serialization/queueing; any failure drains the entry.
        let operation = async {
            let written = self
                .shared
                .enqueue(json!({"id":id,"method":method,"params":params}))?;
            written
                .await
                .map_err(|_| ProtocolError::new("CODEX_STDIO_WRITE_FAILED", "Writer closed"))??;
            if let Some(flushed) = flushed {
                flushed.send(()).map_err(|_| {
                    ProtocolError::new("CODEX_REQUEST_CANCELLED", "Dispatch observer dropped")
                })?;
            }
            rx.await
                .map_err(|_| ProtocolError::invalid("Pending response closed"))?
        };
        let result = match timeout_at(deadline, operation).await {
            Ok(r) => r,
            Err(_) => Err(ProtocolError::new(
                "CODEX_RPC_TIMEOUT",
                format!("{method} timed out; dispatch may be uncertain; no replay"),
            )),
        };
        if let Err(e) = &result {
            self.shared.fail(e.clone());
        }
        guard.active = false;
        result
    }
    pub async fn initialize(&self) -> Result<()> {
        if self.shared.initializing.swap(true, Ordering::AcqRel) {
            return Err(ProtocolError::new(
                "CODEX_APP_SERVER_INIT_FAILED",
                "initialize already attempted",
            ));
        }
        let deadline = Instant::now() + INIT_TIMEOUT;
        let result = async {
            let response = self.rpc("initialize", json!({"clientInfo":{"name":"serena-desktop","title":"SerenaDesktop","version":env!("CARGO_PKG_VERSION")},"capabilities":{"experimentalApi":true}}), None, deadline, true).await?;
            for field in ["userAgent", "codexHome", "platformFamily", "platformOs"] {
                if !response.get(field).is_some_and(Value::is_string) { return Err(ProtocolError::incompatible(format!("initialize response lacks {field}"))); }
            }
            let written = self.shared.enqueue(json!({"method":"initialized"}))?;
            timeout_at(deadline, written).await.map_err(|_| ProtocolError::new("CODEX_APP_SERVER_INIT_FAILED", "initialized flush timed out"))?.map_err(|_| ProtocolError::new("CODEX_APP_SERVER_INIT_FAILED", "Writer closed"))??;
            self.shared.check()?;
            self.shared.ready.store(true, Ordering::Release);
            Ok(())
        }.await;
        if let Err(e) = &result {
            self.shared.fail(e.clone());
        }
        result
    }
    async fn control(&self, method: &str, params: Value) -> Result<Value> {
        self.rpc(method, params, None, Instant::now() + RPC_TIMEOUT, false)
            .await
    }
    fn validated<T>(&self, result: Result<T>) -> Result<T> {
        if let Err(e) = &result {
            self.shared.fail(e.clone());
        }
        result
    }
    pub async fn thread_start(
        &self,
        cwd: &str,
        mode: crate::agent::execution::ExecutionMode,
    ) -> Result<Thread> {
        let sandbox = match mode {
            crate::agent::execution::ExecutionMode::ReadOnly => "read-only",
            crate::agent::execution::ExecutionMode::WorkspaceWrite => "workspace-write",
        };
        let value = self.control("thread/start", json!({"cwd":cwd,"ephemeral":false,"historyMode":"paginated","approvalPolicy":"never","sandbox":sandbox})).await?;
        self.validated(thread_response(value).and_then(|thread| {
            if thread.id.is_empty() || thread.history_mode != HistoryMode::Paginated {
                Err(ProtocolError::incompatible(
                    "Managed Thread must return paginated historyMode and nonempty id",
                ))
            } else {
                Ok(thread)
            }
        }))
    }
    pub async fn thread_resume(&self, thread: &str) -> Result<Thread> {
        // Continuing a Thread is separate from recovering its history.
        let value = self
            .control(
                "thread/resume",
                json!({"threadId":thread,"excludeTurns":true}),
            )
            .await?;
        self.validated(checked_thread(value, thread))
    }
    pub async fn thread_read(&self, thread: &str) -> Result<Thread> {
        let value = self
            .control(
                "thread/read",
                json!({"threadId":thread,"includeTurns":false}),
            )
            .await?;
        self.validated(checked_thread(value, thread))
    }
    pub async fn turn_start(&self, thread: &str, execution: &str, text: &str) -> Result<Turn> {
        let v = self
            .rpc(
                "turn/start",
                json!({"threadId":thread,"input":[{"type":"text","text":text,"text_elements":[]}]}),
                Some(execution.into()),
                Instant::now() + RPC_TIMEOUT,
                false,
            )
            .await?;
        self.validated(turn_response(v))
    }
    /// Flush evidence only; the Provider persists Dispatch state through StateStore.
    pub(crate) async fn turn_start_observed(
        &self,
        thread: &str,
        execution: &str,
        text: &str,
        flushed: oneshot::Sender<()>,
    ) -> Result<Turn> {
        let value = self
            .rpc_with_flush(
                "turn/start",
                json!({"threadId":thread,"input":[{"type":"text","text":text,"text_elements":[]}]}),
                Some(execution.into()),
                Instant::now() + RPC_TIMEOUT,
                false,
                Some(flushed),
            )
            .await?;
        self.validated(turn_response(value))
    }
    pub async fn turn_interrupt(&self, thread: &str, turn: &str) -> Result<()> {
        let v = self
            .control("turn/interrupt", json!({"threadId":thread,"turnId":turn}))
            .await?;
        self.validated(empty_response(v))
    }
    pub async fn clean(&self, thread: &str) -> Result<CleanAccepted> {
        self.clean_until(thread, Instant::now() + RPC_TIMEOUT).await
    }
    async fn clean_until(&self, thread: &str, deadline: Instant) -> Result<CleanAccepted> {
        let v = self
            .rpc(
                "thread/backgroundTerminals/clean",
                json!({"threadId":thread}),
                None,
                deadline,
                false,
            )
            .await?;
        self.validated(empty_response(v))?;
        Ok(CleanAccepted {
            runtime_id: self.runtime_id().into(),
            thread_id: thread.into(),
        })
    }
    pub async fn list(&self, thread: &str, cursor: Option<&str>) -> Result<TerminalPage> {
        self.list_until(thread, cursor, Instant::now() + RPC_TIMEOUT)
            .await
    }
    async fn list_until(
        &self,
        thread: &str,
        cursor: Option<&str>,
        deadline: Instant,
    ) -> Result<TerminalPage> {
        let v = self
            .rpc(
                "thread/backgroundTerminals/list",
                json!({"threadId":thread,"cursor":cursor,"limit":100}),
                None,
                deadline.min(Instant::now() + RPC_TIMEOUT),
                false,
            )
            .await?;
        if v.get("threadId").is_some_and(|id| id != thread) {
            return self.validated(Err(ProtocolError::incompatible(
                "Cleanup response for wrong Thread",
            )));
        }
        let page = self.validated(TerminalPage::parse(v))?;
        if page.next_cursor == NextCursor::Missing {
            return self.validated(Err(ProtocolError::incompatible(
                "Raw list response omitted nextCursor",
            )));
        }
        Ok(page)
    }
    /// Every page is from this immutable client transport. A different Runtime
    /// cannot supply pages for the caller's original Execution identity.
    pub async fn cleanup(&self, scope: CleanupScope) -> Result<EmptyEvidence> {
        self.cleanup_until(scope, Instant::now() + CLEANUP_TIMEOUT)
            .await
    }
    async fn cleanup_until(&self, scope: CleanupScope, deadline: Instant) -> Result<EmptyEvidence> {
        let result = async {
            scope.validate_current().await?;
            if scope.runtime_id != self.runtime_id() {
                return Err(ProtocolError::incompatible(
                    "Cleanup Runtime differs from Execution Runtime",
                ));
            }
            self.clean_until(&scope.thread_id, deadline.min(Instant::now() + RPC_TIMEOUT))
                .await?;
            loop {
                let mut cursor: Option<String> = None;
                let mut seen = HashSet::new();
                let mut empty = true;
                loop {
                    if Instant::now() >= deadline {
                        return Err(ProtocolError::new(
                            "CODEX_CLEANUP_TIMEOUT",
                            "Cleanup scan incomplete",
                        ));
                    }
                    let page = self
                        .list_until(&scope.thread_id, cursor.as_deref(), deadline)
                        .await?;
                    empty &= page.data.is_empty();
                    match page.next_cursor {
                        NextCursor::Missing => {
                            return Err(ProtocolError::incompatible("Missing nextCursor"));
                        }
                        NextCursor::Null => break,
                        NextCursor::Cursor(next) => {
                            // Cursor history itself is bounded; no unbounded page allocation.
                            if seen.len() >= QUEUE_COUNT
                                || seen.iter().map(String::len).sum::<usize>() + next.len()
                                    > QUEUE_BYTES
                                || !seen.insert(next.clone())
                            {
                                return Err(ProtocolError::incompatible(
                                    "Repeated cursor or scan page bound exceeded",
                                ));
                            }
                            cursor = Some(next);
                        }
                    }
                }
                if empty {
                    self.shared.check()?;
                    scope.validate_current().await?;
                    return Ok(EmptyEvidence {
                        scope,
                        completed_at: SystemTime::now(),
                    });
                }
                tokio::time::sleep_until(
                    (Instant::now() + Duration::from_millis(250)).min(deadline),
                )
                .await;
            }
        }
        .await;
        if let Err(e) = &result {
            self.shared.fail(e.clone());
        }
        result
    }
}
fn empty_response(v: Value) -> Result<()> {
    if v.is_object() {
        Ok(())
    } else {
        Err(ProtocolError::incompatible("Expected object response"))
    }
}
#[derive(Debug)]
pub struct CleanAccepted {
    pub runtime_id: String,
    pub thread_id: String,
}
#[derive(Debug, Clone)]
pub struct CleanupScope {
    runtime_id: String,
    thread_id: String,
    execution_id: String,
    binding: Option<binding::ExecutionProtocolBinding>,
}
impl CleanupScope {
    pub async fn for_execution(
        store: &crate::agent::store::StateStore,
        execution_id: &str,
    ) -> Result<Self> {
        let binding = binding::ExecutionProtocolBinding::load(store, execution_id).await?;
        let row = binding.record();
        Ok(Self {
            runtime_id: row.runtime_instance_id.clone().unwrap(),
            thread_id: row.thread_id.clone().unwrap(),
            execution_id: row.id.clone(),
            binding: Some(binding),
        })
    }
    pub fn execution_id(&self) -> &str {
        &self.execution_id
    }
    pub fn runtime_id(&self) -> &str {
        &self.runtime_id
    }
    pub fn thread_id(&self) -> &str {
        &self.thread_id
    }
    pub fn revision(&self) -> Option<i64> {
        self.binding.as_ref().map(|b| b.record().revision)
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
    fn fixture(runtime: &str, thread: &str, execution: &str) -> Self {
        Self {
            runtime_id: runtime.into(),
            thread_id: thread.into(),
            execution_id: execution.into(),
            binding: None,
        }
    }
}
#[derive(Debug)]
pub struct EmptyEvidence {
    scope: CleanupScope,
    completed_at: SystemTime,
}
impl EmptyEvidence {
    pub fn scope(&self) -> &CleanupScope {
        &self.scope
    }
    pub fn completed_at(&self) -> SystemTime {
        self.completed_at
    }
}
async fn read_frame<R: AsyncRead + Unpin>(reader: &mut R) -> Result<Vec<u8>> {
    // A buffered reader outside this function supplies efficient single-byte reads.
    let mut frame = Vec::new();
    loop {
        let byte = reader.read_u8().await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::UnexpectedEof {
                ProtocolError::new(
                    "CODEX_STDIO_EOF",
                    if frame.is_empty() {
                        "stdout EOF"
                    } else {
                        "EOF partial JSONL frame"
                    },
                )
            } else {
                ProtocolError::new("CODEX_STDIO_READ_FAILED", e.to_string())
            }
        })?;
        if frame.len() == MAX_MESSAGE {
            return Err(ProtocolError::new(
                "CODEX_PROTOCOL_MESSAGE_TOO_LARGE",
                "Frame exceeds 16 MiB",
            ));
        }
        frame.push(byte);
        if byte == b'\n' {
            return Ok(frame);
        }
    }
}
fn dispatch(
    s: &Arc<Shared>,
    events: &mpsc::Sender<Event>,
    server: &mpsc::Sender<Queued<(Value, String, Value)>>,
    budget: &Arc<Semaphore>,
    count: &Arc<Semaphore>,
    frame: Vec<u8>,
) -> Result<()> {
    let length = frame.len();
    match decode(&frame)? {
        Message::Response { id, result } => {
            let p = s.pending.lock().unwrap().remove(&id).ok_or_else(|| {
                ProtocolError::invalid("Duplicate, late or impossible response id")
            })?;
            // Exact durable terminal evidence can supersede the ACK's timing,
            // never its Turn identity. All other RPC deadlines remain unchanged.
            if let Some(turn) = &p.terminal_turn {
                let value = result.as_ref().map_err(|_| {
                    ProtocolError::incompatible("turn/start ACK conflicts with durable terminal")
                })?;
                if turn_response(value.clone())?.id != *turn {
                    return Err(ProtocolError::incompatible(
                        "turn/start ACK for wrong terminal Turn",
                    ));
                }
            } else if Instant::now() > p.deadline {
                return Err(ProtocolError::new(
                    "CODEX_RPC_TIMEOUT",
                    format!(
                        "Late {} response for {:?}; never replay",
                        p.method, p.execution
                    ),
                ));
            }
            let result = result.map_err(|e| {
                ProtocolError::new(
                    if e.get("code").and_then(Value::as_i64) == Some(-32601) {
                        "CODEX_APP_SERVER_INCOMPATIBLE"
                    } else if p.method == "initialize" {
                        "CODEX_APP_SERVER_INIT_FAILED"
                    } else {
                        "CODEX_RPC_FAILED"
                    },
                    e.to_string(),
                )
            });
            let _ = p.response.send(result);
        }
        Message::Notification { method, params } => {
            let notification = protocol::notification(method, params)?;
            let slot = count.clone().try_acquire_owned().map_err(|_| {
                ProtocolError::new(
                    "CODEX_PROTOCOL_QUEUE_FULL",
                    "Combined event queue exhausted",
                )
            })?;
            let permit = budget
                .clone()
                .try_acquire_many_owned(length as u32)
                .map_err(|_| {
                    ProtocolError::new("CODEX_PROTOCOL_QUEUE_FULL", "Event byte budget exhausted")
                })?;
            events
                .try_send(Event {
                    runtime_id: s.runtime_id.clone(),
                    notification,
                    _bytes: permit,
                    _slot: Some(slot),
                })
                .map_err(|_| {
                    ProtocolError::new(
                        "CODEX_PROTOCOL_QUEUE_FULL",
                        "Notification queue unavailable",
                    )
                })?;
        }
        Message::ServerRequest { id, method, params } => {
            let slot = count.clone().try_acquire_owned().map_err(|_| {
                ProtocolError::new(
                    "CODEX_PROTOCOL_QUEUE_FULL",
                    "Combined event queue exhausted",
                )
            })?;
            let permit = budget
                .clone()
                .try_acquire_many_owned(length as u32)
                .map_err(|_| {
                    ProtocolError::new("CODEX_PROTOCOL_QUEUE_FULL", "Event byte budget exhausted")
                })?;
            server
                .try_send(Queued {
                    value: (id, method, params),
                    _bytes: permit,
                    _slot: Some(slot),
                })
                .map_err(|_| {
                    ProtocolError::new(
                        "CODEX_PROTOCOL_QUEUE_FULL",
                        "Server request queue unavailable",
                    )
                })?;
        }
    }
    Ok(())
}
#[cfg(windows)]
pub mod managed;
#[cfg(test)]
mod tests;

pub mod recovery;

use recovery::checked_thread;
