use super::{process, serena};
use crate::config::Workspace;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{io::Read, path::PathBuf, time::SystemTime};
use tokio_util::sync::CancellationToken;

#[derive(Debug, PartialEq, Eq)]
struct Version {
    path: String,
    sha256: String,
    modified: SystemTime,
    len: u64,
}

async fn capture(
    root: PathBuf,
    relative: String,
    cancel: CancellationToken,
) -> Result<Version, String> {
    let worker_cancel = cancel.clone();
    let work = tokio::task::spawn_blocking(move || {
        if worker_cancel.is_cancelled() {
            return Err("CANCELLED".into());
        }
        let checked = process::safe_relative(&root, &relative)?;
        process::check_subtree(&root, &checked)?;
        let invalid_path = |e| format!("INVALID_PATH: {e}");
        if !std::fs::metadata(&checked).map_err(invalid_path)?.is_file() {
            return Err("INVALID_PATH: expected a regular file".into());
        }
        let mut file = std::fs::File::open(&checked).map_err(invalid_path)?;
        let before = file.metadata().map_err(invalid_path)?;
        let modified = before.modified().map_err(invalid_path)?;
        let mut hash = Sha256::new();
        let mut chunk = [0; 64 * 1024];
        // Bound the read to the initial file length, including for a growing file.
        let mut remaining = before.len();
        while remaining > 0 {
            if worker_cancel.is_cancelled() {
                return Err("CANCELLED".into());
            }
            let size = remaining.min(chunk.len() as u64) as usize;
            let n = file.read(&mut chunk[..size]).map_err(invalid_path)?;
            if n == 0 {
                return Err("SOURCE_READ_CHANGED".into());
            }
            hash.update(&chunk[..n]);
            remaining -= n as u64;
        }
        let after = file.metadata().map_err(invalid_path)?;
        if after.len() != before.len() || after.modified().map_err(invalid_path)? != modified {
            return Err("SOURCE_READ_CHANGED".into());
        }
        let path = checked.strip_prefix(&root).map_err(|_| "INVALID_PATH")?;
        let path = path
            .components()
            .map(|part| part.as_os_str().to_str().ok_or("INVALID_PATH"))
            .collect::<Result<Vec<_>, _>>()?
            .join("/");
        Ok(Version {
            path,
            sha256: hash
                .finalize()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
            modified,
            len: before.len(),
        })
    });
    tokio::select! {
        result = work => result.map_err(|e| e.to_string())?,
        _ = cancel.cancelled() => Err("CANCELLED".into()),
    }
}

pub(super) async fn read(
    workspace: &Workspace,
    client: &serena::Client,
    remote: Value,
    limit: usize,
    cancel: CancellationToken,
) -> Result<Value, String> {
    // The caller supplies the same validated Source input used for Serena's text.
    let relative = remote["relative_path"].as_str().unwrap().to_owned();
    let before = capture(workspace.root.clone(), relative.clone(), cancel.clone()).await?;
    let text = tokio::select! {
        result = client.call("read_file", remote) => result?,
        _ = cancel.cancelled() => return Err("CANCELLED".into()),
    };
    if text.len() > limit {
        return Err("OUTPUT_LIMIT_EXCEEDED: 缩小路径、行范围或匹配条件".into());
    }
    // Revalidate the path too: replacement or link retargeting must not silently
    // label Serena text with a different local version. No text is reinterpreted.
    let after = capture(workspace.root.clone(), relative, cancel)
        .await
        .map_err(|e| {
            if e == "CANCELLED" {
                e
            } else {
                "SOURCE_READ_CHANGED".into()
            }
        })?;
    if before != after {
        return Err("SOURCE_READ_CHANGED".into());
    }
    Ok(
        json!({"workspace": workspace, "text": text, "truncated": false,
        "path": before.path, "sha256": before.sha256}),
    )
}

#[cfg(test)]
#[path = "source_read_tests.rs"]
mod tests;
