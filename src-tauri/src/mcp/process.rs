use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{io::AsyncReadExt, process::Command};
use tokio_util::sync::CancellationToken;

pub fn command(path: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut cmd = Command::new(path);
    cmd.kill_on_drop(true)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    cmd.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
    cmd
}

#[derive(Debug)]
pub struct Output {
    pub text: String,
    pub truncated: bool,
}
pub async fn run(
    mut cmd: Command,
    limit: usize,
    duration: Duration,
    cancel: CancellationToken,
) -> Result<Output, String> {
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("BACKEND_UNAVAILABLE: {e}"))?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    async fn read(
        mut pipe: impl tokio::io::AsyncRead + Unpin,
        limit: usize,
    ) -> Result<(Vec<u8>, bool), String> {
        let mut result = Vec::new();
        let mut chunk = [0; 8192];
        let mut truncated = false;
        loop {
            let n = pipe.read(&mut chunk).await.map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            let take = n.min(limit.saturating_sub(result.len()));
            result.extend_from_slice(&chunk[..take]);
            truncated |= take < n;
        }
        Ok((result, truncated))
    }
    let work = async {
        let (out, err, status) =
            tokio::join!(read(stdout, limit), read(stderr, 8192), child.wait());
        let (bytes, mut truncated) = out?;
        let (errors, _) = err?;
        if !status.map_err(|e| e.to_string())?.success() {
            return Err(format!(
                "BACKEND_ERROR: {}",
                String::from_utf8_lossy(&errors)
            ));
        }
        let mut text = String::from_utf8_lossy(&bytes).into_owned();
        if truncated {
            text = text.trim_end_matches('\u{fffd}').to_string();
        }
        // Lossy decoding of binary Git data can expand a byte into three UTF-8 bytes.
        if text.len() > limit {
            let mut end = limit;
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            text.truncate(end);
            truncated = true;
        }
        Ok(Output { text, truncated })
    };
    let result = tokio::select! { result = tokio::time::timeout(duration,work) => result.unwrap_or_else(|_|Err("TOOL_TIMEOUT".into())), _ = cancel.cancelled() => Err("CANCELLED".into()) };
    if result.is_err() {
        #[cfg(windows)]
        if let Some(pid) = child.id() {
            let mut kill = command("taskkill.exe");
            kill.args(["/PID", &pid.to_string(), "/T", "/F"]);
            let _ = tokio::time::timeout(Duration::from_secs(10), kill.output()).await;
        }
        let _ = child.kill().await;
    }
    result
}

pub fn safe_relative(root: &Path, value: &str) -> Result<PathBuf, String> {
    let path = Path::new(value);
    if path.is_absolute()
        || value.contains(':')
        || path.components().any(|c| {
            matches!(
                c,
                std::path::Component::ParentDir | std::path::Component::Prefix(_)
            )
        })
    {
        return Err("INVALID_PATH: 只接受仓库内相对路径".into());
    }
    let resolved = root
        .join(path)
        .canonicalize()
        .map_err(|e| format!("INVALID_PATH: {e}"))?;
    if !resolved.starts_with(root) {
        return Err("INVALID_PATH: 路径越出项目".into());
    }
    Ok(resolved)
}
/// Serena follows links by design. Broker rejects an escaping link before a subtree query.
pub fn check_subtree(root: &Path, path: &Path) -> Result<(), String> {
    let mut pending = vec![path.to_path_buf()];
    let mut visited = std::collections::HashSet::new();
    while let Some(path) = pending.pop() {
        let actual = path
            .canonicalize()
            .map_err(|e| format!("INVALID_PATH: {e}"))?;
        if !actual.starts_with(root) {
            return Err(format!("INVALID_PATH: 链接越出项目 {}", path.display()));
        }
        if !visited.insert(actual.clone()) || !actual.is_dir() {
            continue;
        }
        for entry in std::fs::read_dir(&actual).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            if entry.file_name() == ".git" {
                continue;
            }
            let kind = entry.file_type().map_err(|e| e.to_string())?;
            if kind.is_dir() || kind.is_symlink() {
                pending.push(entry.path());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    #[tokio::test]
    async fn cancellation_and_timeout_reap_commands() {
        for cancelled in [true, false] {
            let token = CancellationToken::new();
            if cancelled {
                token.cancel();
            }
            let mut cmd = command("ping.exe");
            cmd.args(["-n", "30", "127.0.0.1"]);
            let result = run(cmd, 1024, Duration::from_millis(50), token)
                .await
                .unwrap_err();
            assert_eq!(
                result,
                if cancelled {
                    "CANCELLED"
                } else {
                    "TOOL_TIMEOUT"
                }
            );
        }
    }
    #[cfg(windows)]
    #[test]
    fn rejects_junction_escape() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        let outside = dir.path().join("outside");
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(&outside).unwrap();
        // Junction creation does not require developer-mode symlink privileges.
        let status = std::process::Command::new("cmd.exe")
            .args(["/c", "mklink", "/J"])
            .arg(root.join("link"))
            .arg(&outside)
            .output()
            .unwrap();
        assert!(status.status.success());
        let root = root.canonicalize().unwrap();
        assert!(safe_relative(&root, "link").is_err());
        assert!(check_subtree(&root, &root).is_err());
    }
}
