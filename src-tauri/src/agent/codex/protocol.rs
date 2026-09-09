//! Adaptation for the approved 0.153.4 binary, not a permissive schema fallback.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::Duration;

pub const VERSION: &str = "codex-cli 0.153.4";
pub const BINARY_SHA256: &str = "444A3F0008050605CAE73CD9B7A2DCAC61294062DFAAB56DD20430FD6498518B";
pub const SCHEMA_SHA256: &str = "B06F77062369D481A59CC70720C12B89CB9DD49C385863923262102D3AD6C978";
pub const SOURCE_COMMIT: &str = "3d2ee51ca2d5db578f328aa75e20aa22c0197c9a";
pub const WIRE_CONTRACT: &str = "rust-v0.153.4/nextCursor-explicit-null/5-fresh-10-list/2026-09-08";
pub const MAX_MESSAGE: usize = 16 * 1024 * 1024;
pub const QUEUE_COUNT: usize = 128;
pub const QUEUE_BYTES: usize = 32 * 1024 * 1024;
pub const STDERR_BYTES: usize = 64 * 1024;
pub const RPC_TIMEOUT: Duration = Duration::from_secs(15);
pub const INIT_TIMEOUT: Duration = Duration::from_secs(30);
pub const CLEANUP_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtocolError {
    pub code: &'static str,
    pub message: String,
}
impl ProtocolError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
    pub fn incompatible(message: impl Into<String>) -> Self {
        Self::new("CODEX_APP_SERVER_INCOMPATIBLE", message)
    }
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new("CODEX_PROTOCOL_INVALID_MESSAGE", message)
    }
}
impl std::fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for ProtocolError {}
pub type Result<T> = std::result::Result<T, ProtocolError>;

/// Installation path is deliberately not part of compatibility equality.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompatibilityIdentity {
    pub version: String,
    pub binary_sha256: String,
    pub protocol_schema_sha256: String,
}
impl CompatibilityIdentity {
    pub fn check(&self) -> Result<()> {
        if self.version != VERSION
            || self.binary_sha256 != BINARY_SHA256
            || self.protocol_schema_sha256 != SCHEMA_SHA256
        {
            return Err(ProtocolError::incompatible(
                "Version, binary digest or freshly exported schema digest is not whitelisted",
            ));
        }
        Ok(())
    }
}

#[derive(Debug)]
pub enum Message {
    Response {
        id: u64,
        result: std::result::Result<Value, Value>,
    },
    Notification {
        method: String,
        params: Value,
    },
    ServerRequest {
        id: Value,
        method: String,
        params: Value,
    },
}
pub fn decode(bytes: &[u8]) -> Result<Message> {
    if bytes.len() > MAX_MESSAGE {
        return Err(ProtocolError::new(
            "CODEX_PROTOCOL_MESSAGE_TOO_LARGE",
            "Inbound frame exceeds 16 MiB including LF",
        ));
    }
    let value: Value =
        serde_json::from_slice(bytes).map_err(|e| ProtocolError::invalid(e.to_string()))?;
    let o = value
        .as_object()
        .ok_or_else(|| ProtocolError::invalid("Message must be an object"))?;
    if let Some(method) = o.get("method") {
        let method = method
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| ProtocolError::invalid("Invalid method"))?
            .to_owned();
        if o.contains_key("result") || o.contains_key("error") {
            return Err(ProtocolError::invalid("Method message has response fields"));
        }
        let params = o.get("params").cloned().unwrap_or(Value::Null);
        if let Some(id) = o.get("id") {
            if !(id.is_string() || id.is_i64() || id.is_u64()) {
                return Err(ProtocolError::invalid("Invalid server request id"));
            }
            Ok(Message::ServerRequest {
                id: id.clone(),
                method,
                params,
            })
        } else {
            Ok(Message::Notification { method, params })
        }
    } else {
        let id = o
            .get("id")
            .and_then(Value::as_u64)
            .ok_or_else(|| ProtocolError::invalid("Response id is not a client request id"))?;
        match (o.get("result"), o.get("error")) {
            (Some(v), None) => Ok(Message::Response {
                id,
                result: Ok(v.clone()),
            }),
            (None, Some(e))
                if e.get("code").is_some_and(Value::is_i64)
                    && e.get("message").is_some_and(Value::is_string) =>
            {
                Ok(Message::Response {
                    id,
                    result: Err(e.clone()),
                })
            }
            _ => Err(ProtocolError::invalid(
                "Response requires exactly one result/error",
            )),
        }
    }
}
pub fn encode(value: &Value) -> Result<Vec<u8>> {
    struct Bounded(Vec<u8>, bool);
    impl std::io::Write for Bounded {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if self.0.len().saturating_add(bytes.len()) >= MAX_MESSAGE {
                self.1 = true;
                return Err(std::io::Error::other("Frame limit"));
            }
            self.0.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut output = Bounded(Vec::new(), false);
    let encoded = serde_json::to_writer(&mut output, value);
    if output.1 {
        return Err(ProtocolError::new(
            "CODEX_PROTOCOL_MESSAGE_TOO_LARGE",
            "Outbound frame exceeds 16 MiB including LF",
        ));
    }
    encoded.map_err(|e| ProtocolError::invalid(e.to_string()))?;
    let mut bytes = output.0;
    bytes.push(b'\n');
    Ok(bytes)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NextCursor {
    Missing,
    Null,
    Cursor(String),
}
#[derive(Debug)]
pub struct TerminalPage {
    pub data: Vec<Value>,
    pub next_cursor: NextCursor,
}
impl TerminalPage {
    pub fn parse(value: Value) -> Result<Self> {
        let data = value
            .get("data")
            .and_then(Value::as_array)
            .ok_or_else(|| ProtocolError::incompatible("list response lacks array data"))?
            .clone();
        let next_cursor = match value.get("nextCursor") {
            None => NextCursor::Missing,
            Some(Value::Null) => NextCursor::Null,
            Some(Value::String(s)) if !s.is_empty() => NextCursor::Cursor(s.clone()),
            _ => return Err(ProtocolError::incompatible("Malformed nextCursor")),
        };
        Ok(Self { data, next_cursor })
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TurnStatus {
    Completed,
    Interrupted,
    Failed,
    InProgress,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Turn {
    #[serde(default = "full_items_view", rename = "itemsView")]
    pub items_view: String,
    #[serde(flatten)]
    pub details: serde_json::Map<String, Value>,
    pub id: String,
    pub status: TurnStatus,
    pub items: Vec<Value>,
}
#[derive(Debug, Clone, Deserialize)]
pub struct Thread {
    pub id: String,
    #[serde(rename = "historyMode")]
    pub history_mode: HistoryMode,
    pub turns: Vec<Turn>,
}
#[derive(Debug, Clone)]
pub enum Notification {
    ThreadStarted(Thread),
    TurnStarted { thread_id: String, turn: Turn },
    TurnCompleted { thread_id: String, turn: Turn },
    Other { method: String, params: Value },
}
fn from_value<T: serde::de::DeserializeOwned>(v: Value) -> Result<T> {
    serde_json::from_value(v).map_err(|e| ProtocolError::incompatible(e.to_string()))
}
pub fn thread_response(value: Value) -> Result<Thread> {
    from_value(
        value
            .get("thread")
            .cloned()
            .ok_or_else(|| ProtocolError::incompatible("Missing thread"))?,
    )
}
pub fn turn_response(value: Value) -> Result<Turn> {
    from_value(
        value
            .get("turn")
            .cloned()
            .ok_or_else(|| ProtocolError::incompatible("Missing turn"))?,
    )
}
pub fn notification(method: String, params: Value) -> Result<Notification> {
    match method.as_str() {
        "thread/started" => Ok(Notification::ThreadStarted(thread_response(params)?)),
        "turn/started" | "turn/completed" => {
            let thread_id = params
                .get("threadId")
                .and_then(Value::as_str)
                .ok_or_else(|| ProtocolError::incompatible("Missing notification threadId"))?
                .to_owned();
            let turn = turn_response(params)?;
            if method == "turn/completed" {
                if turn.status == TurnStatus::InProgress {
                    return Err(ProtocolError::incompatible(
                        "Terminal notification contains inProgress",
                    ));
                }
                Ok(Notification::TurnCompleted { thread_id, turn })
            } else {
                Ok(Notification::TurnStarted { thread_id, turn })
            }
        }
        _ => Ok(Notification::Other { method, params }),
    }
}

/// Never approves. Refusal results follow the exact exported response schemas.
/// Auth/attestation/time requests are explicitly rejected with a JSON-RPC error.
pub fn server_reply(id: Value, method: &str, params: &Value) -> Value {
    if !params.is_object() {
        return json!({"id":id,"error":{"code":-32602,"message":"Object params required"}});
    }
    let result = match method {
        "item/commandExecution/requestApproval" | "item/fileChange/requestApproval" => {
            json!({"decision":"cancel"})
        }
        "applyPatchApproval" | "execCommandApproval" => json!({"decision":"abort"}),
        "item/permissions/requestApproval" => json!({"permissions":{},"scope":"turn"}),
        "item/tool/requestUserInput" => json!({"answers":{}}),
        "mcpServer/elicitation/request" => json!({"action":"cancel","content":null}),
        "item/tool/call" => json!({"contentItems":[],"success":false}),
        "account/chatgptAuthTokens/refresh" | "attestation/generate" | "currentTime/read" => {
            return json!({"id":id,"error":{"code":-32000,"message":"Client cannot fulfill this server request"}});
        }
        _ => return json!({"id":id,"error":{"code":-32601,"message":"Unsupported server request"}}),
    };
    json!({"id":id,"result":result})
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HistoryMode {
    Paginated,
    Legacy,
}
fn full_items_view() -> String {
    "full".into()
}
