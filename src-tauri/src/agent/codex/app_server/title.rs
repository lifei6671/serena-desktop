//! One fixed-version capability, serviced by the existing server-request worker.
use super::*;
use crate::agent::store::StateStore;
use serde::Deserialize;

// 0.153.4 normalizes with trim and has no maxLength. This capability's product
// bound is 200 Unicode scalar values, advertised to the model (not an upstream limit).
const MAX_TITLE: usize = 200;

#[derive(Clone)]
pub(super) struct TitleScope {
    store: StateStore,
    execution: String,
}

impl Client {
    pub(crate) fn enable_root_title(&self, store: StateStore, execution: &str) -> Result<()> {
        let mut scope = self.shared.title_scope.lock().unwrap();
        if scope.is_some() {
            return Err(ProtocolError::invalid("Title scope already bound"));
        }
        *scope = Some(TitleScope {
            store,
            execution: execution.into(),
        });
        Ok(())
    }
}

pub(super) fn tools() -> Value {
    json!([{"type":"namespace","name":"codex_app","description":"Current Root Thread title.",
        "tools":[{"type":"function","name":"set_thread_title",
        "description":"Set the current Root conversation/thread title according to your effective instructions. Only the active Root Thread can call this tool. Supply a nonempty single-line title of at most 200 characters. Success confirms the App Server name update; no thread identifier is accepted.",
        "inputSchema":{"type":"object","properties":{"title":{"type":"string","minLength":1,"maxLength":MAX_TITLE}},"required":["title"],"additionalProperties":false}}]}])
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Call {
    thread_id: String,
    turn_id: String,
    call_id: String,
    namespace: Option<String>,
    tool: String,
    arguments: Value,
}

fn response(id: Value, success: bool, text: &str) -> Value {
    json!({"id":id,"result":{"success":success,"contentItems":[{"type":"inputText","text":text}]}})
}

pub(super) async fn reply(s: &Arc<Shared>, id: Value, params: Value) -> Value {
    let outcome = apply(s, params).await;
    match outcome {
        Ok(()) => response(id, true, "Root Thread title updated."),
        Err(message) => response(id, false, message),
    }
}

async fn apply(s: &Arc<Shared>, params: Value) -> std::result::Result<(), &'static str> {
    let call: Call =
        serde_json::from_value(params).map_err(|_| "Invalid title tool call payload.")?;
    if call.namespace.as_deref() != Some("codex_app") || call.tool != "set_thread_title" {
        return Err("Unsupported dynamic tool.");
    }
    if call.call_id.is_empty() || call.thread_id.is_empty() || call.turn_id.is_empty() {
        return Err("Missing title tool call identity.");
    }
    let scope = s
        .title_scope
        .lock()
        .unwrap()
        .clone()
        .ok_or("Root title capability unavailable.")?;
    // Never derive authority from the tool's arguments, ancestry or a notification.
    // The Provider has already durably bound this exact Execution to its Root.
    let row = scope
        .store
        .execution(scope.execution)
        .await
        .map_err(|_| "Root ownership unavailable.")?
        .ok_or("Root ownership unavailable.")?;
    if row.runtime_instance_id.as_deref() != Some(s.runtime_id.as_str())
        || row.thread_id.as_deref() != Some(call.thread_id.as_str())
        || row.turn_id.as_deref() != Some(call.turn_id.as_str())
        || row.provider_terminal_status.is_some()
        || !matches!(row.status.as_str(), "dispatch_pending" | "running")
    {
        return Err("Title call does not belong to the active Execution Root Thread and Turn.");
    }
    let args = call
        .arguments
        .as_object()
        .filter(|a| a.len() == 1)
        .ok_or("Expected only a title string.")?;
    let title = args
        .get("title")
        .and_then(Value::as_str)
        .ok_or("Expected a title string.")?
        .trim();
    if title.is_empty() || title.chars().count() > MAX_TITLE || title.chars().any(char::is_control)
    {
        return Err("Title must be nonempty, single-line, and at most 200 characters.");
    }
    rename(s, row.thread_id.unwrap(), title).await
}

async fn rename(
    s: &Arc<Shared>,
    thread: String,
    title: &str,
) -> std::result::Result<(), &'static str> {
    s.check().map_err(|_| "Title transport unavailable.")?;
    let id = s.next_id.fetch_add(1, Ordering::Relaxed);
    if id == u64::MAX {
        s.fail(ProtocolError::invalid("Request id exhausted"));
        return Err("Title transport unavailable.");
    }
    let (tx, rx) = oneshot::channel();
    let deadline = Instant::now() + RPC_TIMEOUT;
    {
        let mut pending = s.pending.lock().unwrap();
        // At most one unresolved rename, including one whose outcome is unknown.
        if pending.len() >= QUEUE_COUNT || pending.values().any(|p| p.method == "thread/name/set") {
            return Err("A title request is unresolved or transport capacity is unavailable.");
        }
        pending.insert(
            id,
            Pending {
                method: "thread/name/set".into(),
                execution: None,
                deadline,
                response: tx,
                terminal_turn: None,
            },
        );
    }
    let written = match s.enqueue(
        json!({"id":id,"method":"thread/name/set","params":{"threadId":thread,"name":title}}),
    ) {
        Ok(written) => written,
        Err(_) => {
            s.pending.lock().unwrap().remove(&id);
            return Err("Title request could not be queued.");
        }
    };
    match timeout_at(deadline, async {
        written.await.map_err(|_| ())?.map_err(|_| ())?;
        rx.await.map_err(|_| ())?.map_err(|_| ())
    })
    .await
    {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(_)) => Err("App Server could not update the title."),
        Err(_) => Err("Title request timed out; outcome unknown. It has not been replayed."),
    }
}

#[cfg(test)]
mod tests;
