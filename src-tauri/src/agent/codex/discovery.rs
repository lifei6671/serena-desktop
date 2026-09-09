//! Locate the actual vendor executable, never launch a shell or npm shim.
use std::path::PathBuf;
pub async fn discover() -> Result<PathBuf, String> {
    let mut roots = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .unwrap_or_default();
    if let Some(appdata) = std::env::var_os("APPDATA") {
        roots.push(PathBuf::from(appdata).join("npm"));
    }
    let mut candidates = Vec::new();
    for root in roots {
        candidates.push(root.join("codex.exe"));
        candidates.push(root.join("node_modules/@openai/codex/node_modules/@openai/codex-win32-x64/vendor/x86_64-pc-windows-msvc/bin/codex.exe"));
        candidates.push(
            root.join("node_modules/@openai/codex/vendor/x86_64-pc-windows-msvc/bin/codex.exe"),
        );
    }
    let mut error = "BACKEND_UNAVAILABLE: actual vendor codex.exe not found".to_string();
    for path in candidates {
        if !path.is_file() {
            continue;
        }
        let path = std::fs::canonicalize(path).map_err(|e| e.to_string())?;
        match super::app_server::managed::verify(path.clone()).await {
            Ok(_) => return Ok(path),
            Err(e) => error = e.to_string(),
        }
    }
    Err(error)
}
