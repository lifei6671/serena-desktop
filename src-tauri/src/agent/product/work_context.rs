//! Host submission gate only; no Runtime or MCP dependency.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fs::File, io::Read, path::Path};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct VersionedContext {
    #[serde(
        default,
        deserialize_with = "summary",
        skip_serializing_if = "Option::is_none"
    )]
    summary: Option<String>,
    #[serde(default)]
    files: Vec<FileReference>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct FileReference {
    path: String,
    sha256: String,
}

fn summary<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    String::deserialize(deserializer).map(Some)
}

impl VersionedContext {
    /// Pure canonicalization precedes idempotency; filesystem access does not.
    pub(super) fn parse(json: &str) -> Result<Self, String> {
        let invalid = || "WORK_INVALID_ARGUMENT".to_string();
        let mut context: Self = serde_json::from_str(json).map_err(|_| invalid())?;
        if context
            .summary
            .as_ref()
            .is_some_and(|s| s.trim().is_empty())
        {
            return Err(invalid());
        }
        for file in &mut context.files {
            let path = file.path.replace('\\', "/");
            if path.starts_with('/')
                || path.contains(':')
                || path.chars().any(char::is_control)
                || path.split('/').any(|part| part == "..")
                || file.sha256.len() != 64
                || !file
                    .sha256
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return Err(invalid());
            }
            file.path = path
                .split('/')
                .filter(|p| !p.is_empty() && *p != ".")
                .collect::<Vec<_>>()
                .join("/");
            if file.path.trim().is_empty() {
                return Err(invalid());
            }
        }
        context.files.sort_by(|a, b| a.path.cmp(&b.path));
        if context
            .files
            .windows(2)
            .any(|pair| pair[0].path == pair[1].path)
        {
            return Err(invalid());
        }
        Ok(context)
    }

    pub(super) fn canonical_json(&self) -> String {
        serde_json::to_string(self).expect("context string serialization")
    }

    pub(super) fn prompt(&self, task: &str) -> String {
        let mut prompt = String::from("Host verified context:\n");
        if let Some(summary) = &self.summary {
            prompt.push_str(&format!("Summary: {summary}\n"));
        }
        prompt.push_str("Versioned source references:\n");
        for file in &self.files {
            prompt.push_str(&format!("- {} @ SHA256 {}\n", file.path, file.sha256));
        }
        prompt.push_str("\nTask:\n");
        prompt.push_str(task);
        prompt
    }

    pub(super) async fn verify(self, root: String) -> Result<(), String> {
        // Full raw files can be large. Keep both memory and async executor use bounded.
        tokio::task::spawn_blocking(move || {
            let stale = || "CONTEXT_STALE".to_string();
            if self.files.is_empty() {
                return Ok(());
            }
            let root = Path::new(&root).canonicalize().map_err(|_| stale())?;
            for reference in self.files {
                let path = root
                    .join(&reference.path)
                    .canonicalize()
                    .map_err(|_| stale())?;
                if !path.starts_with(&root) || !path.is_file() {
                    return Err(stale());
                }
                let mut file = File::open(path).map_err(|_| stale())?;
                let before = file.metadata().map_err(|_| stale())?;
                if !before.is_file() {
                    return Err(stale());
                }
                let mut hash = Sha256::new();
                let mut buffer = [0u8; 64 * 1024];
                let mut remaining = before.len();
                while remaining != 0 {
                    let limit = remaining.min(buffer.len() as u64) as usize;
                    let n = file.read(&mut buffer[..limit]).map_err(|_| stale())?;
                    if n == 0 {
                        return Err(stale());
                    }
                    hash.update(&buffer[..n]);
                    remaining -= n as u64;
                }
                let after = file.metadata().map_err(|_| stale())?;
                if before.len() != after.len()
                    || before.modified().ok() != after.modified().ok()
                    || hash
                        .finalize()
                        .iter()
                        .map(|byte| format!("{byte:02x}"))
                        .collect::<String>()
                        != reference.sha256
                {
                    return Err(stale());
                }
            }
            Ok(())
        })
        .await
        .map_err(|_| "CONTEXT_STALE".to_string())?
    }
}
