//! Adaptation for the approved 0.153.4 binary, not a permissive schema fallback.
use crate::agent::{
    activity::{ActivityPhase, ToolCategory},
    usage::USAGE_EVENT_INVALID,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::Duration;

pub const VERSION: &str = super::compatibility::WINDOWS_X86_64.codex_version;
pub const BINARY_SHA256: &str = super::compatibility::WINDOWS_X86_64.binary_sha256;
pub const SCHEMA_SHA256: &str = super::compatibility::WINDOWS_X86_64.protocol_schema_sha256;
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
    /// 使用调用方明确提供的平台/架构目标验证精确 allowlist entry。
    pub(crate) fn check(&self, target: super::compatibility::Target) -> Result<()> {
        super::compatibility::check_entry(target, self)
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
    pub name: Option<String>,
    #[serde(rename = "historyMode")]
    pub history_mode: HistoryMode,
    pub turns: Vec<Turn>,
}
// Exact ErrorNotification / TurnError definitions exported by Codex 0.153.4.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnError {
    pub message: String,
    pub codex_error_info: Option<CodexErrorInfo>,
    pub additional_details: Option<String>,
    pub misalignment: Option<MisalignmentErrorDetails>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MisalignmentErrorDetails {
    pub detailed_explanation: Option<String>,
    pub error_type: Option<String>,
    pub steer: Option<MisalignmentSteer>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MisalignmentSteer {
    pub message: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub enum CodexErrorInfo {
    ContextWindowExceeded,
    SessionBudgetExceeded,
    UsageLimitExceeded,
    RateLimitExceeded,
    ServerOverloaded,
    CyberPolicy,
    MisalignmentPolicyViolation,
    InternalServerError,
    Unauthorized,
    BadRequest,
    ThreadRollbackFailed,
    SandboxError,
    Other,
    HttpConnectionFailed {
        #[serde(rename = "httpStatusCode")]
        http_status_code: Option<u16>,
    },
    ResponseStreamConnectionFailed {
        #[serde(rename = "httpStatusCode")]
        http_status_code: Option<u16>,
    },
    ResponseStreamDisconnected {
        #[serde(rename = "httpStatusCode")]
        http_status_code: Option<u16>,
    },
    ResponseTooManyFailedAttempts {
        #[serde(rename = "httpStatusCode")]
        http_status_code: Option<u16>,
    },
    ActiveTurnNotSteerable {
        #[serde(rename = "turnKind")]
        turn_kind: NonSteerableTurnKind,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NonSteerableTurnKind {
    Review,
    Compact,
}
#[derive(Debug, Clone)]
pub enum Notification {
    ThreadStarted(Thread),
    SubAgentStarted {
        thread_id: String,
        turn_id: String,
        agent_thread_id: String,
    },
    TurnError {
        thread_id: String,
        turn_id: String,
        error: TurnError,
        will_retry: bool,
    },
    ThreadNameUpdated {
        thread_id: String,
        name: Option<String>,
    },
    TurnStarted {
        thread_id: String,
        turn: Turn,
    },
    TurnCompleted {
        thread_id: String,
        turn: Turn,
    },
    Activity(Activity),
    Usage(Usage),
    PermissionDenied {
        thread_id: String,
        turn_id: String,
        kind: PermissionRequestKind,
    },
    Other {
        method: String,
        params: Value,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Activity {
    pub thread_id: String,
    pub turn_id: String,
    pub phase: ActivityPhase,
    pub tool_category: Option<ToolCategory>,
    /// Time SerenaDesktop decoded the Provider event, not the later DB write time.
    pub observed_at: i64,
}

/// Codex 私有的累计 usage 通知；Thread/Turn 只能在 adapter 绑定时使用。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Usage {
    pub thread_id: String,
    pub turn_id: String,
    pub total: UsageSnapshot,
    pub last: UsageSnapshot,
    pub model_context_window: Option<i64>,
    /// SerenaDesktop 解码 Provider 通知的时刻，不是后续 Store 写入时间。
    pub observed_at: i64,
}

/// Codex 私有的 fixed-schema token snapshot；不得由 breakdown 推导 total。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UsageSnapshot {
    pub total_tokens: i64,
    pub input_tokens: Option<i64>,
    pub cached_input_tokens: Option<i64>,
    pub cache_write_input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub reasoning_output_tokens: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionRequestKind {
    Command,
    FileChange,
    Permissions,
}

impl PermissionRequestKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Command => "command",
            Self::FileChange => "file_change",
            Self::Permissions => "permissions",
        }
    }
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
    // Fixed-schema descendant classification; discovery may follow Turn events.
    if matches!(method.as_str(), "item/started" | "item/completed")
        && params["item"]["type"] == "subAgentActivity"
        && params["item"]["kind"] == "started"
    {
        for value in [
            &params["threadId"],
            &params["turnId"],
            &params["item"]["id"],
            &params["item"]["agentThreadId"],
            &params["item"]["agentPath"],
        ] {
            if value.as_str().is_none_or(str::is_empty) {
                return Err(ProtocolError::incompatible(
                    "Invalid subAgentActivity identity",
                ));
            }
        }
        return Ok(Notification::SubAgentStarted {
            thread_id: params["threadId"].as_str().unwrap().into(),
            turn_id: params["turnId"].as_str().unwrap().into(),
            agent_thread_id: params["item"]["agentThreadId"].as_str().unwrap().into(),
        });
    }
    if let Some(activity) = activity_notification(&method, &params)? {
        return Ok(Notification::Activity(activity));
    }
    match method.as_str() {
        "thread/tokenUsage/updated" => Ok(Notification::Usage(usage_notification(&params)?)),
        "error" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct ErrorNotification {
                thread_id: String,
                turn_id: String,
                error: TurnError,
                will_retry: bool,
            }
            // Ordinary serde structs accept positional arrays; this wire schema
            // requires JSON objects at each of these boundaries.
            let error = params
                .get("error")
                .filter(|value| value.is_object())
                .ok_or_else(|| {
                    ProtocolError::incompatible("ErrorNotification requires an error object")
                })?;
            if let Some(details) = error.get("misalignment").filter(|value| !value.is_null())
                && (!details.is_object()
                    || details
                        .get("steer")
                        .is_some_and(|steer| !steer.is_null() && !steer.is_object()))
            {
                return Err(ProtocolError::incompatible(
                    "Misalignment details and steer must be objects",
                ));
            }
            // Serde also accepts {"unitVariant":null}; the fixed schema does not.
            if let Some(info) = params
                .get("error")
                .and_then(|e| e.get("codexErrorInfo"))
                .and_then(Value::as_object)
            {
                if info.len() != 1
                    || !info.keys().all(|key| {
                        matches!(
                            key.as_str(),
                            "httpConnectionFailed"
                                | "responseStreamConnectionFailed"
                                | "responseStreamDisconnected"
                                | "responseTooManyFailedAttempts"
                                | "activeTurnNotSteerable"
                        )
                    })
                {
                    return Err(ProtocolError::incompatible(
                        "Invalid codexErrorInfo object variant",
                    ));
                }
                if let Some(active) = info.get("activeTurnNotSteerable")
                    && !active.get("turnKind").is_some_and(Value::is_string)
                {
                    return Err(ProtocolError::incompatible(
                        "turnKind must be a schema enum string",
                    ));
                }
            }
            let notification: ErrorNotification = from_value(params)?;
            if notification.thread_id.is_empty() || notification.turn_id.is_empty() {
                return Err(ProtocolError::incompatible(
                    "Empty error notification Thread/Turn identity",
                ));
            }
            Ok(Notification::TurnError {
                thread_id: notification.thread_id,
                turn_id: notification.turn_id,
                error: notification.error,
                will_retry: notification.will_retry,
            })
        }
        "thread/started" => Ok(Notification::ThreadStarted(thread_response(params)?)),
        "thread/name/updated" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct NameUpdate {
                thread_id: String,
                thread_name: Option<String>,
            }
            let update: NameUpdate = from_value(params)?;
            Ok(Notification::ThreadNameUpdated {
                thread_id: update.thread_id,
                name: update.thread_name,
            })
        }
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

/// 解析 pinned Codex 0.153.4 usage wire，并把所有 schema 错误归一为稳定错误码。
fn usage_notification(params: &Value) -> Result<Usage> {
    let params = params
        .as_object()
        .ok_or_else(|| usage_invalid("Usage notification must be an object"))?;
    if !params
        .keys()
        .all(|key| matches!(key.as_str(), "threadId" | "turnId" | "tokenUsage"))
    {
        return Err(usage_invalid("Unexpected Usage notification field"));
    }
    let identity = |name: &str| {
        params
            .get(name)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| usage_invalid(format!("Missing Usage notification {name}")))
    };
    let token_usage = params
        .get("tokenUsage")
        .and_then(Value::as_object)
        .ok_or_else(|| usage_invalid("tokenUsage must be an object"))?;
    if !token_usage
        .keys()
        .all(|key| matches!(key.as_str(), "total" | "last" | "modelContextWindow"))
    {
        return Err(usage_invalid("Unexpected tokenUsage field"));
    }
    let snapshot = |name: &str| {
        token_usage
            .get(name)
            .ok_or_else(|| usage_invalid(format!("Missing tokenUsage.{name}")))
            .and_then(usage_snapshot)
    };
    let model_context_window = match token_usage
        .get("modelContextWindow")
        .ok_or_else(|| usage_invalid("Missing modelContextWindow"))?
    {
        Value::Null => None,
        value => Some(non_negative_i64(value, "modelContextWindow")?),
    };
    Ok(Usage {
        thread_id: identity("threadId")?,
        turn_id: identity("turnId")?,
        total: snapshot("total")?,
        last: snapshot("last")?,
        model_context_window,
        observed_at: crate::agent::coordinator::now(),
    })
}

/// 解析 total 或 last 的 exact breakdown schema，仅 cache-write 可保留 absence。
fn usage_snapshot(value: &Value) -> Result<UsageSnapshot> {
    let value = value
        .as_object()
        .ok_or_else(|| usage_invalid("Usage snapshot must be an object"))?;
    if !value.keys().all(|key| {
        matches!(
            key.as_str(),
            "totalTokens"
                | "inputTokens"
                | "cachedInputTokens"
                | "cacheWriteInputTokens"
                | "outputTokens"
                | "reasoningOutputTokens"
        )
    }) {
        return Err(usage_invalid("Unexpected Usage snapshot field"));
    }
    let required = |name: &str| {
        value
            .get(name)
            .ok_or_else(|| usage_invalid(format!("Missing {name}")))
            .and_then(|value| non_negative_i64(value, name))
    };
    let optional_cache_write = || {
        value
            .get("cacheWriteInputTokens")
            .map(|value| non_negative_i64(value, "cacheWriteInputTokens"))
            .transpose()
    };
    Ok(UsageSnapshot {
        total_tokens: required("totalTokens")?,
        input_tokens: Some(required("inputTokens")?),
        cached_input_tokens: Some(required("cachedInputTokens")?),
        cache_write_input_tokens: optional_cache_write()?,
        output_tokens: Some(required("outputTokens")?),
        reasoning_output_tokens: Some(required("reasoningOutputTokens")?),
    })
}

/// 只接受可完整表示为非负 i64 的 JSON integer，禁止浮点、字符串与截断转换。
fn non_negative_i64(value: &Value, field: &str) -> Result<i64> {
    let Value::Number(number) = value else {
        return Err(usage_invalid(format!(
            "{field} must be a non-negative i64 integer"
        )));
    };
    number
        .as_i64()
        .or_else(|| number.as_u64().and_then(|value| i64::try_from(value).ok()))
        .filter(|value| *value >= 0)
        .ok_or_else(|| usage_invalid(format!("{field} must be a non-negative i64 integer")))
}

/// 固定 Usage wire 的所有不可信输入共用 P4-001 的公开错误合同。
fn usage_invalid(message: impl Into<String>) -> ProtocolError {
    ProtocolError::new(USAGE_EVENT_INVALID, message)
}

fn activity_notification(method: &str, params: &Value) -> Result<Option<Activity>> {
    if !matches!(method, "item/started" | "item/completed") {
        return Ok(None);
    }
    let Some(item_type) = params.pointer("/item/type").and_then(Value::as_str) else {
        return Ok(None);
    };
    let tool_category = match (method, item_type) {
        ("item/started", "commandExecution") => {
            let command = params
                .pointer("/item/command")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    ProtocolError::incompatible("commandExecution requires a command string")
                })?;
            Some(classify_command(command, &params["item"]["commandActions"]))
        }
        ("item/started", "fileChange") => Some(ToolCategory::Edit),
        ("item/started", "mcpToolCall" | "dynamicToolCall" | "webSearch") => {
            Some(ToolCategory::Tool)
        }
        (
            "item/completed",
            "commandExecution" | "fileChange" | "mcpToolCall" | "dynamicToolCall" | "webSearch",
        )
        | (_, "agentMessage" | "reasoning") => None,
        _ => return Ok(None),
    };
    let phase = if method == "item/started" && tool_category.is_some() {
        ActivityPhase::Tool
    } else {
        ActivityPhase::Provider
    };
    let (thread_id, turn_id) = notification_identity(params)?;
    Ok(Some(Activity {
        thread_id,
        turn_id,
        phase,
        tool_category,
        observed_at: crate::agent::coordinator::now(),
    }))
}

fn notification_identity(params: &Value) -> Result<(String, String)> {
    let field = |name: &str| {
        params
            .get(name)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| ProtocolError::incompatible(format!("Missing notification {name}")))
    };
    Ok((field("threadId")?, field("turnId")?))
}

fn classify_command(command: &str, actions: &Value) -> ToolCategory {
    let category = classify_direct_command(command).or_else(|| {
        let actions = actions.as_array()?;
        if actions.len() != 1 {
            return None;
        }
        classify_direct_command(actions[0].get("command")?.as_str()?)
    });
    if let Some(category) = category {
        category
    } else if actions.as_array().is_some_and(|actions| {
        !actions.is_empty()
            && actions.iter().all(|action| {
                action
                    .get("type")
                    .and_then(Value::as_str)
                    .is_some_and(|kind| matches!(kind, "read" | "listFiles" | "search"))
            })
    }) {
        ToolCategory::Read
    } else {
        ToolCategory::Command
    }
}

fn classify_direct_command(command: &str) -> Option<ToolCategory> {
    const CLASSIFICATION_LIMIT: usize = 4096;
    let unsafe_to_classify = command.len() > CLASSIFICATION_LIMIT
        || command
            .chars()
            .any(|character| matches!(character, ';' | '&' | '|' | '<' | '>' | '$' | '\n' | '\r'));
    let mut words = command.split_ascii_whitespace();
    let executable = words.next().map(|word| {
        let word = word.trim_matches(|character| character == '"' || character == '\'');
        let basename = word.rsplit(['/', '\\']).next().unwrap_or(word);
        let lowercase = basename.to_ascii_lowercase();
        [".exe", ".cmd", ".bat"]
            .iter()
            .find_map(|suffix| lowercase.strip_suffix(suffix))
            .unwrap_or(&lowercase)
            .to_owned()
    });
    let arguments = words
        .take(2)
        .map(|word| {
            word.trim_matches(|character| character == '"' || character == '\'')
                .to_ascii_lowercase()
        })
        .collect::<Vec<_>>();
    let first = arguments.first().map(String::as_str);
    let second = arguments.get(1).map(String::as_str);
    if unsafe_to_classify {
        return None;
    }
    match (executable.as_deref(), first, second) {
        (Some("cargo" | "go"), Some("test"), _)
        | (Some("npm" | "pnpm"), Some("test"), _)
        | (Some("npm" | "pnpm"), Some("run"), Some("test"))
        | (Some("pytest" | "vitest" | "jest"), _, _) => Some(ToolCategory::Test),
        (Some("cargo"), Some("build" | "check"), _)
        | (Some("go"), Some("build"), _)
        | (Some("npm" | "pnpm"), Some("build"), _)
        | (Some("npm" | "pnpm"), Some("run"), Some("build"))
        | (Some("tsc"), _, _) => Some(ToolCategory::Build),
        _ => None,
    }
}

pub fn permission_denied_notification(
    method: &str,
    params: &Value,
) -> Result<Option<Notification>> {
    let kind = match method {
        "item/commandExecution/requestApproval" => PermissionRequestKind::Command,
        "item/fileChange/requestApproval" => PermissionRequestKind::FileChange,
        "item/permissions/requestApproval" => PermissionRequestKind::Permissions,
        _ => return Ok(None),
    };
    let (thread_id, turn_id) = notification_identity(params)?;
    Ok(Some(Notification::PermissionDenied {
        thread_id,
        turn_id,
        kind,
    }))
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

#[cfg(test)]
mod activity_tests {
    use super::*;

    fn activity(method: &str, item: Value) -> Activity {
        match notification(
            method.into(),
            json!({"threadId":"ROOT","turnId":"TURN","item":item}),
        )
        .unwrap()
        {
            Notification::Activity(activity) => activity,
            other => panic!("expected activity, got {other:?}"),
        }
    }

    #[test]
    fn fixed_item_activity_mapping_is_small_and_typed() {
        for (item, category) in [
            (
                json!({"type":"commandExecution","command":"git status","commandActions":[]}),
                ToolCategory::Command,
            ),
            (json!({"type":"fileChange"}), ToolCategory::Edit),
            (json!({"type":"mcpToolCall"}), ToolCategory::Tool),
            (json!({"type":"dynamicToolCall"}), ToolCategory::Tool),
            (json!({"type":"webSearch"}), ToolCategory::Tool),
        ] {
            let started = activity("item/started", item.clone());
            assert_eq!(started.thread_id, "ROOT");
            assert_eq!(started.turn_id, "TURN");
            assert_eq!(started.phase, ActivityPhase::Tool);
            assert_eq!(started.tool_category, Some(category));
            assert!(started.observed_at > 0);
            let completed = activity("item/completed", item);
            assert_eq!(completed.phase, ActivityPhase::Provider);
            assert_eq!(completed.tool_category, None);
        }
        for item_type in ["agentMessage", "reasoning"] {
            for method in ["item/started", "item/completed"] {
                let provider = activity(method, json!({"type":item_type}));
                assert_eq!(provider.phase, ActivityPhase::Provider);
                assert_eq!(provider.tool_category, None);
            }
        }
    }

    #[test]
    fn engineering_commands_use_bounded_best_effort_categories() {
        for command in [
            "cargo test",
            "go test ./...",
            "pytest -q",
            "npm test",
            "pnpm test",
            "vitest run",
            "jest",
        ] {
            assert_eq!(classify_command(command, &json!([])), ToolCategory::Test);
        }
        for command in [
            "cargo build",
            "cargo check",
            "go build ./cmd/app",
            "npm run build",
            "pnpm build",
            "tsc --noEmit",
        ] {
            assert_eq!(classify_command(command, &json!([])), ToolCategory::Build);
        }
        for command in [
            "git status",
            "python script.py",
            "not a known command",
            "echo cargo test",
            "python -c \"print('cargo test')\"",
            "Write-Output \"npm run build\"",
            "cargo test; echo finished",
            "cargo test | tee results.txt",
            "cargo test && cargo build",
        ] {
            assert_eq!(classify_command(command, &json!([])), ToolCategory::Command);
        }
        assert_eq!(
            classify_command("cargo.exe test", &json!([])),
            ToolCategory::Test
        );
        assert_eq!(
            classify_command("tsc.cmd --noEmit", &json!([])),
            ToolCategory::Build
        );
        assert_eq!(
            classify_command(
                "\"C:\\Program Files\\PowerShell\\7\\pwsh.exe\" -Command 'cargo test --offline'",
                &json!([{"type":"unknown","command":"cargo test --offline"}]),
            ),
            ToolCategory::Test
        );
        assert_eq!(
            classify_command(
                "pwsh.exe -Command 'echo cargo test'",
                &json!([{"type":"unknown","command":"echo cargo test"}]),
            ),
            ToolCategory::Command
        );
        assert_eq!(
            classify_command(&format!("cargo test {}", "x".repeat(4096)), &json!([])),
            ToolCategory::Command
        );
        assert_eq!(
            classify_command(
                "Get-Content secret.txt",
                &json!([{"type":"read","command":"sensitive","name":"read","path":"C:\\private"}]),
            ),
            ToolCategory::Read
        );
    }

    #[test]
    fn raw_command_and_streaming_output_never_enter_activity() {
        let secret = "token=secret password=hunter2 C:\\Users\\private";
        let parsed = activity(
            "item/started",
            json!({"type":"commandExecution","command":secret,"commandActions":[]}),
        );
        assert!(!format!("{parsed:?}").contains(secret));
        assert!(matches!(
            notification(
                "item/commandExecution/outputDelta".into(),
                json!({"threadId":"ROOT","turnId":"TURN","itemId":"I","delta":secret})
            )
            .unwrap(),
            Notification::Other { .. }
        ));
    }
}

#[cfg(test)]
mod usage_tests {
    use super::*;

    /// 固定已接受 P0-008 合同的 notification 外形；数字仅用于断言原样透传。
    fn accepted_usage_contract_fixture() -> Value {
        json!({
            "threadId": "thread-1",
            "turnId": "turn-1",
            "tokenUsage": {
                "total": {
                    "totalTokens": 100,
                    "inputTokens": 60,
                    "cachedInputTokens": 10,
                    "outputTokens": 30,
                    "reasoningOutputTokens": 5
                },
                "last": {
                    "totalTokens": 40,
                    "inputTokens": 25,
                    "cachedInputTokens": 5,
                    "cacheWriteInputTokens": 0,
                    "outputTokens": 10,
                    "reasoningOutputTokens": 2
                },
                "modelContextWindow": 258400
            }
        })
    }

    /// 解析 Usage notification，失败时直接暴露稳定错误码。
    fn usage(value: Value) -> Usage {
        match notification("thread/tokenUsage/updated".into(), value).unwrap() {
            Notification::Usage(usage) => usage,
            other => panic!("expected Usage, got {other:?}"),
        }
    }

    /// 验证 recognized Usage 的 schema 失败不会退回 Other。
    fn assert_usage_invalid(value: Value) {
        assert_eq!(
            notification("thread/tokenUsage/updated".into(), value)
                .unwrap_err()
                .code,
            USAGE_EVENT_INVALID
        );
    }

    #[test]
    fn accepted_usage_contract_fixture_parses_the_cumulative_snapshot_without_using_last() {
        let usage = usage(accepted_usage_contract_fixture());

        assert_eq!(usage.thread_id, "thread-1");
        assert_eq!(usage.turn_id, "turn-1");
        assert_eq!(usage.total.total_tokens, 100);
        assert_eq!(usage.total.cache_write_input_tokens, None);
        assert_eq!(usage.last.cache_write_input_tokens, Some(0));
        assert_eq!(usage.model_context_window, Some(258400));
        assert!(usage.observed_at > 0);
    }

    #[test]
    fn cache_write_absence_zero_and_nullable_context_remain_distinct() {
        let mut absent = accepted_usage_contract_fixture();
        absent["tokenUsage"]["last"]
            .as_object_mut()
            .unwrap()
            .remove("cacheWriteInputTokens");
        absent["tokenUsage"]["modelContextWindow"] = Value::Null;
        let absent = usage(absent);
        assert_eq!(absent.last.cache_write_input_tokens, None);
        assert_eq!(absent.model_context_window, None);

        let present = usage(accepted_usage_contract_fixture());
        assert_eq!(present.last.cache_write_input_tokens, Some(0));

        for snapshot in ["total", "last"] {
            let mut null = accepted_usage_contract_fixture();
            null["tokenUsage"][snapshot]["cacheWriteInputTokens"] = Value::Null;
            assert_usage_invalid(null);
        }
    }

    #[test]
    fn rejects_every_non_integer_token_and_context_value_with_the_usage_error() {
        for field in [
            "totalTokens",
            "inputTokens",
            "cachedInputTokens",
            "cacheWriteInputTokens",
            "outputTokens",
            "reasoningOutputTokens",
        ] {
            for invalid in [
                json!(1.5),
                json!(-1),
                json!("1"),
                json!(9223372036854775808u64),
            ] {
                let mut value = accepted_usage_contract_fixture();
                value["tokenUsage"]["total"][field] = invalid;
                assert_usage_invalid(value);
            }
        }
        for invalid in [
            json!(1.5),
            json!(-1),
            json!("258400"),
            json!(9223372036854775808u64),
        ] {
            let mut value = accepted_usage_contract_fixture();
            value["tokenUsage"]["modelContextWindow"] = invalid;
            assert_usage_invalid(value);
        }
        for invalid in [
            json!(1.5),
            json!(-1),
            json!("40"),
            json!(9223372036854775808u64),
        ] {
            let mut value = accepted_usage_contract_fixture();
            value["tokenUsage"]["last"]["totalTokens"] = invalid;
            assert_usage_invalid(value);
        }
    }

    #[test]
    fn rejects_missing_total_malformed_usage_shapes_and_empty_identity() {
        let mut missing_total = accepted_usage_contract_fixture();
        missing_total["tokenUsage"]["total"]
            .as_object_mut()
            .unwrap()
            .remove("totalTokens");
        assert_usage_invalid(missing_total);

        let mut missing_context = accepted_usage_contract_fixture();
        missing_context["tokenUsage"]
            .as_object_mut()
            .unwrap()
            .remove("modelContextWindow");
        assert_usage_invalid(missing_context);

        for (path, invalid) in [
            ("tokenUsage", json!([])),
            ("total", json!([])),
            ("last", json!("not-an-object")),
        ] {
            let mut value = accepted_usage_contract_fixture();
            if path == "tokenUsage" {
                value[path] = invalid;
            } else {
                value["tokenUsage"][path] = invalid;
            }
            assert_usage_invalid(value);
        }
        for identity in ["threadId", "turnId"] {
            let mut value = accepted_usage_contract_fixture();
            value[identity] = json!("");
            assert_usage_invalid(value);
        }
    }

    #[test]
    fn rejects_missing_or_null_required_breakdowns_in_total_and_last() {
        const REQUIRED_FIELDS: [&str; 5] = [
            "totalTokens",
            "inputTokens",
            "cachedInputTokens",
            "outputTokens",
            "reasoningOutputTokens",
        ];
        for snapshot in ["total", "last"] {
            for field in REQUIRED_FIELDS {
                let mut missing = accepted_usage_contract_fixture();
                missing["tokenUsage"][snapshot]
                    .as_object_mut()
                    .unwrap()
                    .remove(field);
                assert_usage_invalid(missing);

                let mut null = accepted_usage_contract_fixture();
                null["tokenUsage"][snapshot][field] = Value::Null;
                assert_usage_invalid(null);
            }
        }
    }

    #[test]
    fn unrelated_notification_still_remains_other() {
        assert!(matches!(
            notification("unrelated/notification".into(), json!({"opaque": true})).unwrap(),
            Notification::Other { .. }
        ));
    }
}

#[cfg(test)]
mod error_tests {
    use super::*;
    #[test]
    fn fixed_error_notification_schema() {
        let base = json!({"threadId":"T", "turnId":"U", "willRetry":true, "error":{"message":"diagnostic"}});
        for info in [
            Value::Null,
            json!("sandboxError"),
            json!("other"),
            json!({"responseStreamDisconnected":{"httpStatusCode":503}}),
            json!({"responseTooManyFailedAttempts":{}}),
            json!({"activeTurnNotSteerable":{"turnKind":"compact"}}),
        ] {
            let mut p = base.clone();
            p["error"]["codexErrorInfo"] = info;
            assert!(matches!(
                notification("error".into(), p).unwrap(),
                Notification::TurnError {
                    will_retry: true,
                    ..
                }
            ));
        }
        for key in ["threadId", "turnId", "willRetry", "error"] {
            let mut p = base.clone();
            p.as_object_mut().unwrap().remove(key);
            assert_eq!(
                notification("error".into(), p).unwrap_err().code,
                "CODEX_APP_SERVER_INCOMPATIBLE"
            );
        }
        assert!(notification("error".into(), json!(["T", "U", {"message":"x"}, false])).is_err());
        for (key, value) in [
            ("threadId", json!("")),
            ("turnId", json!("")),
            ("willRetry", json!("false")),
            ("error", json!({})),
            ("error", json!({"message":1})),
            ("error", json!({"message":"x","additionalDetails":false})),
            ("error", json!({"message":"x","misalignment":{"steer":{}}})),
            ("error", json!(["x", null, null, null])),
            (
                "error",
                json!({"message":"x","misalignment":[null,null,null]}),
            ),
            (
                "error",
                json!({"message":"x","misalignment":{"steer":["x"]}}),
            ),
        ] {
            let mut p = base.clone();
            p[key] = value;
            assert!(notification("error".into(), p).is_err(), "{key}");
        }
        for info in [
            json!("unknownFutureError"),
            json!("responseStreamDisconnected"),
            json!({"responseStreamDisconnected":{"httpStatusCode":65536}}),
            json!({"activeTurnNotSteerable":{"turnKind":"normal"}}),
            json!({"sandboxError":{}}),
            json!({"sandboxError":null}),
            json!({"other":null}),
            json!({"activeTurnNotSteerable":{"turnKind":{"review":null}}}),
        ] {
            let mut p = base.clone();
            p["error"]["codexErrorInfo"] = info;
            assert!(notification("error".into(), p).is_err());
        }
    }
}

#[cfg(test)]
mod subagent_tests {
    use super::*;
    #[test]
    fn fixed_subagent_activity_requires_explicit_identity() {
        let base = json!({"threadId":"T","turnId":"U","item":{"type":"subAgentActivity","id":"I","kind":"started","agentThreadId":"C","agentPath":"/root/review"}});
        assert!(matches!(
            notification("item/started".into(), base.clone()).unwrap(),
            Notification::SubAgentStarted { .. }
        ));
        for field in ["agentThreadId", "agentPath", "id"] {
            let mut p = base.clone();
            p["item"].as_object_mut().unwrap().remove(field);
            assert!(notification("item/started".into(), p).is_err());
        }
        for field in ["threadId", "turnId"] {
            let mut p = base.clone();
            p[field] = json!("");
            assert!(notification("item/completed".into(), p).is_err());
        }
    }
}
