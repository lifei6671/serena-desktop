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
    #[serde(default = "default_workspace_generation")]
    pub workspace_generation: u64,
    #[serde(default)]
    pub provider: Provider,
    pub mode: ExecutionMode,
    /// Generic continuation lineage. Runtime protocol identity remains separate.
    #[serde(default)]
    pub parent_execution_id: Option<String>,
    /// Temporary persisted runtime compatibility identity. It is not request identity.
    #[serde(default)]
    pub thread_id: Option<String>,
}

fn default_workspace_generation() -> u64 {
    1
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
    if input.workspace_generation == 0 || input.workspace_generation > i64::MAX as u64 {
        return Err("workspace_generation must be a positive SQLite integer".into());
    }
    let profile_json = canonical_json(&input.execution_profile);
    // A versioned JSON tuple gives an explicit, immutable field order and framing.
    // Profile is embedded as a canonical JSON string, not implementation map order.
    let bytes = serde_json::to_vec(&(
        "execution-request-v2",
        &input.agent_id,
        &input.request_key,
        &input.prompt,
        &profile_json,
        &input.workspace_id,
        &input.canonical_workspace_root,
        input.workspace_generation,
        input.provider,
        input.mode,
        &input.parent_execution_id,
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

/// Exact post-C2, pre-workspace-generation request hash for migrated rows only.
/// New request canonicalization never calls this helper.
pub(crate) fn legacy_pre_workspace_generation_hash(
    input: &CreateExecutionInput,
) -> Result<String, String> {
    if !input.execution_profile.is_object() {
        return Err("execution_profile must be a JSON object".into());
    }
    let profile_json = canonical_json(&input.execution_profile);
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
        &input.parent_execution_id,
    ))
    .map_err(|e| e.to_string())?;
    Ok(Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

pub(crate) fn matches_current_or_legacy_workspace_generation_hash(
    stored_hash: &str,
    stored_workspace_generation: u64,
    request: &CanonicalRequest,
) -> Result<bool, String> {
    Ok(stored_hash == request.request_hash()
        || (stored_workspace_generation == request.input().workspace_generation
            && stored_hash == legacy_pre_workspace_generation_hash(request.input())?))
}

/// Exact pre-C2 request-hash compatibility for persisted continuation rows only.
///
/// New request canonicalization never calls this helper. Its caller must first
/// establish that the persisted row has no `parent_execution_id`; this permits a
/// bounded retry of a row written when the final tuple slot was a source thread.
pub(crate) fn legacy_pre_c2_continuation_hash(
    input: &CreateExecutionInput,
    source_thread_id: &Option<String>,
) -> Result<String, String> {
    if !input.execution_profile.is_object() {
        return Err("execution_profile must be a JSON object".into());
    }
    let profile_json = canonical_json(&input.execution_profile);
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
        source_thread_id,
    ))
    .map_err(|e| e.to_string())?;
    Ok(Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
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
