//! Canonical creation input, independent of dispatch and lifecycle semantics.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

pub mod state;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    #[default]
    Codex,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionMode {
    ReadOnly,
    WorkspaceWrite,
}

/// Complete creation payload. Runtime IDs, generated execution ID and timestamps
/// are persistence metadata, not request inputs. Root is the existing Workspace
/// canonical identity, never a display path; this layer does not recanonicalize it.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateExecutionInput {
    pub agent_id: String,
    pub request_key: String,
    pub prompt: String,
    pub execution_profile: Value,
    pub workspace_id: String,
    pub canonical_workspace_root: String,
    #[serde(default)]
    pub provider: Provider,
    pub mode: ExecutionMode,
    #[serde(default)]
    pub thread_id: Option<String>,
}

/// Immutable result: callers cannot pair a payload with an arbitrary hash.
#[derive(Clone, Debug)]
pub struct CanonicalRequest {
    input: CreateExecutionInput,
    profile_json: String,
    bytes: Vec<u8>,
    hash: String,
}

impl CanonicalRequest {
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn request_hash(&self) -> &str {
        &self.hash
    }
    pub fn input(&self) -> &CreateExecutionInput {
        &self.input
    }
    pub fn execution_profile_json(&self) -> &str {
        &self.profile_json
    }
}

/// Single canonicalization/hash entry point. See docs/tasks/TASK-001-implementation.md.
pub fn canonicalize_request(input: CreateExecutionInput) -> Result<CanonicalRequest, String> {
    if !input.execution_profile.is_object() {
        return Err("execution_profile must be a JSON object".into());
    }
    let profile_json = canonical_json(&input.execution_profile);
    // A versioned JSON tuple gives an explicit, immutable field order and framing.
    // Profile is embedded as a canonical JSON string, not implementation map order.
    let bytes = serde_json::to_vec(&(
        "execution-request-v1",
        &input.agent_id,
        &input.request_key,
        &input.prompt,
        &profile_json,
        &input.workspace_id,
        &input.canonical_workspace_root,
        input.provider,
        input.mode,
        &input.thread_id,
    ))
    .map_err(|e| e.to_string())?;
    let hash = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    Ok(CanonicalRequest {
        input,
        profile_json,
        bytes,
        hash,
    })
}

fn canonical_json(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let mut entries: Vec<_> = map.iter().collect();
            entries.sort_unstable_by_key(|(key, _)| *key);
            format!(
                "{{{}}}",
                entries
                    .into_iter()
                    .map(|(key, value)| {
                        format!(
                            "{}:{}",
                            serde_json::to_string(key).expect("string serialization"),
                            canonical_json(value)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(",")
            )
        }
        Value::Array(items) => format!(
            "[{}]",
            items
                .iter()
                .map(canonical_json)
                .collect::<Vec<_>>()
                .join(",")
        ),
        _ => serde_json::to_string(value).expect("JSON Value serialization"),
    }
}

#[cfg(test)]
mod tests;
