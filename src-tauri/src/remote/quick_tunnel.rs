use super::*;
use crate::{
    mcp::{Broker, process},
    oauth::Runtime,
};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    path::Path,
    sync::Arc,
    time::Duration,
};
use tokio::io::AsyncReadExt;
use tokio_util::sync::CancellationToken;

const VERSION: &str = "2026.9.0";
const MAX_DOWNLOAD: usize = 100 * 1024 * 1024;
#[cfg(all(test, windows))]
#[path = "host_crash_tests.rs"]
mod host_crash_tests;

// Official release asset digests, pinned with this Desktop release.
// https://api.github.com/repos/cloudflare/cloudflared/releases/tags/2026.9.0
fn artifact() -> Result<(&'static str, &'static str), String> {
    artifact_for(std::env::consts::OS, std::env::consts::ARCH)
}
fn artifact_for(os: &str, arch: &str) -> Result<(&'static str, &'static str), String> {
    match (os, arch) {
        ("windows", "x86_64") => Ok((
            "cloudflared-windows-amd64.exe",
            "547057326266f0e1c7d50d102dbd22ff283d740c055bd61e94f10e2c606f89af",
        )),
        ("linux", "x86_64") => Ok((
            "cloudflared-linux-amd64",
            "53b7a7a5420d188758d24341294acb0d1bca54296548ac05e38811a694ac6134",
        )),
        ("linux", "aarch64") => Ok((
            "cloudflared-linux-arm64",
            "98aca3173f73248fad6180fc75dade2d186a6e54fa807e088108cb4345de8efe",
        )),
        ("macos", "x86_64") => Ok((
            "cloudflared-darwin-amd64.tgz",
            "8f2ecf41776d942bcc8070a56e7bafa4c5de70a1d1781110e2eb3774cca512a8",
        )),
        ("macos", "aarch64") => Ok((
            "cloudflared-darwin-arm64.tgz",
            "c0eccb3758420d1f4e46cbf2b8ecde01d9802a154232a817f25133340009fcc7",
        )),
        _ => Err("CLOUDFLARED_INCOMPATIBLE: 当前平台没有固定的受管组件".into()),
    }
}
fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn executable(bytes: &[u8], archive: bool) -> Result<Vec<u8>, String> {
    if !archive {
        return Ok(bytes.to_vec());
    }
    let gzip = flate2::read::GzDecoder::new(bytes);
    let mut archive = tar::Archive::new(gzip);
    for entry in archive
        .entries()
        .map_err(|_| "CLOUDFLARED_INSTALL_FAILED")?
    {
        let entry = entry.map_err(|_| "CLOUDFLARED_INSTALL_FAILED")?;
        if entry
            .path()
            .map_err(|_| "CLOUDFLARED_INSTALL_FAILED")?
            .as_ref()
            == Path::new("cloudflared")
            && entry.header().entry_type().is_file()
        {
            let mut bytes = Vec::new();
            entry
                .take(MAX_DOWNLOAD as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| "CLOUDFLARED_INSTALL_FAILED")?;
            if bytes.len() > MAX_DOWNLOAD {
                return Err("CLOUDFLARED_INSTALL_FAILED".into());
            }
            return Ok(bytes);
        }
    }
    Err("CLOUDFLARED_INSTALL_FAILED: 归档缺少可执行文件".into())
}
async fn resolve(
    root: &Path,
    installed: Option<std::path::PathBuf>,
    cancel: CancellationToken,
) -> Result<std::path::PathBuf, String> {
    if let Some(path) = installed {
        let mut cmd = process::command(&path);
        cmd.arg("--version");
        let output = process::run(cmd, 4096, Duration::from_secs(10), cancel)
            .await
            .map_err(
                |_| "CLOUDFLARED_INCOMPATIBLE: 已找到本机 cloudflared，但无法运行，请修复本机安装",
            )?;
        if !output.text.starts_with("cloudflared version ") {
            return Err("CLOUDFLARED_INCOMPATIBLE: 本机 cloudflared 版本检查失败".into());
        }
        return Ok(path);
    }
    install(root, cancel).await
}

pub(super) async fn install(
    root: &Path,
    cancel: CancellationToken,
) -> Result<std::path::PathBuf, String> {
    let (name, expected) = artifact()?;
    let directory = root.join("cloudflared").join(VERSION);
    tokio::fs::create_dir_all(&directory)
        .await
        .map_err(|_| "CLOUDFLARED_INSTALL_FAILED")?;
    // Keep the signed-by-release digest source, including macOS's archive.
    let asset = directory.join(name);
    let mut bytes = tokio::fs::read(&asset).await.unwrap_or_default();
    if digest(&bytes) != expected {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(|_| "CLOUDFLARED_INSTALL_FAILED")?;
        let url =
            format!("https://github.com/cloudflare/cloudflared/releases/download/{VERSION}/{name}");
        let work = async {
            let mut response = client
                .get(url)
                .send()
                .await
                .map_err(|_| "CLOUDFLARED_INSTALL_FAILED: 下载失败")?
                .error_for_status()
                .map_err(|_| "CLOUDFLARED_INSTALL_FAILED: 下载响应失败")?;
            let mut bytes = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|_| "CLOUDFLARED_INSTALL_FAILED")?
            {
                if bytes.len() + chunk.len() > MAX_DOWNLOAD {
                    return Err("CLOUDFLARED_INSTALL_FAILED: 下载超过大小限制");
                }
                bytes.extend_from_slice(&chunk);
            }
            Ok(bytes)
        };
        bytes = tokio::select! { r = work => r.map_err(str::to_owned)?, _ = cancel.cancelled() => return Err("CANCELLED".into()) };
        if digest(&bytes) != expected {
            return Err("CLOUDFLARED_INSTALL_FAILED: SHA-256 校验失败".into());
        }
    }
    let binary = executable(&bytes, name.ends_with(".tgz"))?;
    let path = directory.join(if cfg!(windows) {
        "cloudflared.exe"
    } else {
        "cloudflared"
    });
    // Verify before execution on every start, not just first install.
    if tokio::fs::read(&path).await.ok().as_deref() != Some(binary.as_slice()) {
        let mut temp = tempfile::NamedTempFile::new_in(&directory)
            .map_err(|_| "CLOUDFLARED_INSTALL_FAILED")?;
        temp.write_all(&binary)
            .map_err(|_| "CLOUDFLARED_INSTALL_FAILED")?;
        temp.as_file()
            .sync_all()
            .map_err(|_| "CLOUDFLARED_INSTALL_FAILED")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            temp.as_file()
                .set_permissions(std::fs::Permissions::from_mode(0o755))
                .map_err(|_| "CLOUDFLARED_INSTALL_FAILED")?;
        }
        temp.persist(&path)
            .map_err(|_| "CLOUDFLARED_INSTALL_FAILED")?;
    }
    if !asset.exists()
        || tokio::fs::read(&asset)
            .await
            .ok()
            .is_none_or(|b| digest(&b) != expected)
    {
        let mut temp = tempfile::NamedTempFile::new_in(&directory)
            .map_err(|_| "CLOUDFLARED_INSTALL_FAILED")?;
        temp.write_all(&bytes)
            .map_err(|_| "CLOUDFLARED_INSTALL_FAILED")?;
        temp.persist(&asset)
            .map_err(|_| "CLOUDFLARED_INSTALL_FAILED")?;
    }
    let mut cmd = process::command(&path);
    cmd.arg("--version");
    let output = process::run(cmd, 4096, Duration::from_secs(10), cancel)
        .await
        .map_err(|_| "CLOUDFLARED_INCOMPATIBLE")?;
    if !output
        .text
        .starts_with(&format!("cloudflared version {VERSION} "))
    {
        return Err("CLOUDFLARED_INCOMPATIBLE".into());
    }
    Ok(path)
}

fn parse_origin(text: &str) -> Option<String> {
    text.split("https://").skip(1).find_map(|part| {
        let host: String = part
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.'))
            .collect();
        let label = host.strip_suffix(".trycloudflare.com")?;
        if label.is_empty()
            || label.len() > 63
            || label.starts_with('-')
            || label.ends_with('-')
            || !label
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
        {
            return None;
        }
        Some(format!("https://{host}"))
    })
}

pub(super) struct ProbeStage<'a> {
    name: &'static str,
    host: &'a str,
    started: std::time::Instant,
}
impl<'a> ProbeStage<'a> {
    pub(super) fn new(name: &'static str, host: &'a str) -> Self {
        Self {
            name,
            host,
            started: std::time::Instant::now(),
        }
    }
    fn fail(&self, category: &str, status: Option<u16>) -> String {
        format!(
            "REMOTE_PUBLIC_PROBE_FAILED: stage={} category={category} host={} elapsed_ms={} status={}",
            self.name,
            self.host,
            self.started.elapsed().as_millis(),
            status.map_or_else(|| "none".into(), |s| s.to_string())
        )
    }
    pub(super) fn network(&self, error: &reqwest::Error) -> String {
        let category = if error.is_timeout() {
            "timeout"
        } else {
            std::error::Error::source(error)
                .and_then(network_category)
                .unwrap_or(if error.is_connect() {
                    "connect"
                } else {
                    "network"
                })
        };
        self.fail(category, error.status().map(|s| s.as_u16()))
    }
    async fn status(
        &self,
        mut response: reqwest::Response,
        expected: u16,
    ) -> Result<reqwest::Response, String> {
        let status = response.status().as_u16();
        if status == expected {
            return Ok(response);
        }
        let mut category = match status {
            401 => "oauth_reject",
            407 => "proxy",
            _ => "http_status",
        };
        if status == 403 {
            // Recognize only fixed local rejection messages, never expose upstream bodies.
            let mut bytes = Vec::new();
            while let Some(chunk) = response.chunk().await.map_err(|e| self.network(&e))? {
                if bytes.len() + chunk.len() > 256 {
                    bytes.clear();
                    break;
                }
                bytes.extend_from_slice(&chunk);
            }
            category = match bytes.as_slice() {
                b"Forbidden: Host header is not allowed" => "host_reject",
                b"Origin is not allowed" | b"Forbidden: Origin header is not allowed" => {
                    "origin_reject"
                }
                _ => "http_status",
            };
        }
        Err(self.fail(category, Some(status)))
    }
    async fn json(
        &self,
        mut response: reqwest::Response,
        limit: usize,
    ) -> Result<serde_json::Value, String> {
        let status = response.status().as_u16();
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|e| self.network(&e))? {
            if bytes.len() + chunk.len() > limit {
                return Err(self.fail("response_size", Some(status)));
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes).map_err(|_| self.fail("invalid_json", Some(status)))
    }
}

fn network_category(error: &(dyn std::error::Error + 'static)) -> Option<&'static str> {
    // reqwest/hyper wrap resolver and TLS errors. Inspect their chain only to
    // classify; no error text (URLs, proxy credentials, etc.) reaches logs/UI.
    let mut source = Some(error);
    let mut category = None;
    while let Some(error) = source {
        let text = error.to_string().to_ascii_lowercase();
        if text.contains("proxy") || text.contains("tunnel unsuccessful") {
            return Some("proxy");
        }
        if text.contains("dns") || text.contains("lookup") || text.contains("name resolution") {
            category = Some("dns");
        } else if text.contains("tls") || text.contains("certificate") || text.contains("ssl") {
            category = Some("tls");
        }
        source = error.source();
    }
    category
}

pub(super) async fn probe(context: &RemotePublicContext, credential: &str) -> Result<(), String> {
    let origin = &context.public_origin;
    let url = url::Url::parse(origin).map_err(|_| "PUBLIC_ORIGIN_INVALID")?;
    let host = url.host_str().ok_or("PUBLIC_ORIGIN_INVALID")?;
    let stage = ProbeStage::new("oauth_metadata", host);
    // Keep reqwest's configured environment/system proxy behavior. No silent
    // direct-network fallback, DNS override, or TLS verification bypass.
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| stage.network(&e))?;
    let response = client
        .get(format!("{origin}/.well-known/oauth-authorization-server"))
        .send()
        .await
        .map_err(|e| stage.network(&e))?;
    let response = stage.status(response, 200).await?;
    let value = stage.json(response, 16384).await?;
    if value["issuer"] != *origin || value["token_endpoint"] != format!("{origin}/oauth/token") {
        return Err(stage.fail("metadata_mismatch", Some(200)));
    }
    let stage = ProbeStage::new("unauthorized_mcp", host);
    let response = client
        .post(&context.mcp_resource)
        .header("Accept", "application/json, text/event-stream")
        .json(&serde_json::json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}))
        .send()
        .await
        .map_err(|e| stage.network(&e))?;
    let response = stage.status(response, 401).await?;
    let expected = format!(
        "Bearer resource_metadata=\"{origin}/.well-known/oauth-protected-resource/mcp\", scope=\"serena:mcp\""
    );
    if response
        .headers()
        .get("www-authenticate")
        .and_then(|h| h.to_str().ok())
        != Some(expected.as_str())
    {
        return Err(stage.fail("oauth_challenge_mismatch", Some(401)));
    }
    let stage = ProbeStage::new("resource_metadata", host);
    let response = client
        .get(format!("{origin}/.well-known/oauth-protected-resource/mcp"))
        .send()
        .await
        .map_err(|e| stage.network(&e))?;
    let response = stage.status(response, 200).await?;
    let resource = stage.json(response, 16384).await?;
    if resource["resource"] != context.mcp_resource
        || resource["authorization_servers"] != serde_json::json!([origin])
    {
        return Err(stage.fail("metadata_mismatch", Some(200)));
    }
    for (name, body) in [
        (
            "initialize",
            serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"SerenaDesktop probe","version":"1"}}}),
        ),
        (
            "tools_list",
            serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
        ),
    ] {
        let stage = ProbeStage::new(name, host);
        let response = client
            .post(&context.mcp_resource)
            .bearer_auth(credential)
            .header("Accept", "application/json, text/event-stream")
            .header("MCP-Protocol-Version", "2025-11-25")
            .header("Mcp-Method", body["method"].as_str().unwrap())
            .json(&body)
            .send()
            .await
            .map_err(|e| stage.network(&e))?;
        let response = stage.status(response, 200).await?;
        if !response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with("application/json"))
        {
            return Err(stage.fail("transport", Some(200)));
        }
        let value = stage.json(response, 4 * 1024 * 1024).await?;
        if value["id"] != body["id"]
            || value.get("error").is_some()
            || value.get("result").is_none()
        {
            return Err(stage.fail("mcp_result", Some(200)));
        }
        if name == "initialize" && value["result"]["protocolVersion"] != "2025-11-25" {
            return Err(stage.fail("protocol_version", Some(200)));
        }
        if name == "tools_list" && !value["result"]["tools"].is_array() {
            return Err(stage.fail("mcp_result", Some(200)));
        }
    }
    Ok(())
}

pub(super) async fn run(
    remote: &Arc<Remote>,
    broker: &Arc<Broker>,
    cancel: CancellationToken,
) -> Result<(), String> {
    remote.status(Status::Installing);
    let installed = crate::serena::find_executable("cloudflared")
        .or_else(|| crate::serena::user_local_candidate("cloudflared"));
    let path = match resolve(
        &broker.supervisor.paths.runtime_directory,
        installed,
        cancel.clone(),
    )
    .await
    {
        Ok(path) => path,
        Err(_) if cancel.is_cancelled() => return Ok(()),
        Err(e) => return Err(e),
    };
    if cancel.is_cancelled() {
        return Ok(());
    }
    // Explicit empty config isolates this managed process from user tunnel settings.
    let config = tempfile::NamedTempFile::new().map_err(|_| "QUICK_TUNNEL_START_FAILED")?;
    std::fs::write(config.path(), "{}\n").map_err(|_| "QUICK_TUNNEL_START_FAILED")?;
    let mut cmd = process::command(path);
    let port = broker.config().broker.port;
    cmd.args(["tunnel", "--config"]).arg(config.path()).args([
        "--no-autoupdate",
        "--url",
        &format!("http://127.0.0.1:{port}"),
        "--http-host-header",
        &format!("127.0.0.1:{port}"),
    ]);
    cmd.stdout(std::process::Stdio::null());
    // Do not inherit tunnel-specific settings from a user's unrelated installation.
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("TUNNEL_") {
            cmd.env_remove(key);
        }
    }
    let mut child = super::process::ManagedChild::spawn(&mut cmd)?;
    broker.log(&format!(
        "Remote cloudflared owned · pid={}",
        child.id().unwrap()
    ));
    let mut stderr = child.stderr.take().ok_or("QUICK_TUNNEL_START_FAILED")?;
    remote.status(Status::DiscoveringUrl);
    let work = async {
        let origin = tokio::time::timeout(Duration::from_secs(60), async {
            let mut buffer = Vec::new();
            loop {
                let mut chunk = [0u8; 2048];
                let n = stderr
                    .read(&mut chunk)
                    .await
                    .map_err(|_| "QUICK_TUNNEL_URL_NOT_FOUND")?;
                if n == 0 {
                    return Err("QUICK_TUNNEL_URL_NOT_FOUND");
                }
                buffer.extend_from_slice(&chunk[..n]);
                if let Some(origin) = parse_origin(&String::from_utf8_lossy(&buffer)) {
                    return Ok(origin);
                }
                if buffer.len() > 8192 {
                    buffer.drain(..buffer.len() - 4096);
                }
            }
        })
        .await
        .map_err(|_| "QUICK_TUNNEL_URL_NOT_FOUND")??;
        let context = RemotePublicContext::new(&origin)?;
        {
            let mut inner = remote.inner.lock().unwrap();
            if cancel.is_cancelled() {
                return Ok(());
            }
            inner.oauth = Some(Runtime::new(context.clone()));
        }
        broker.log(&format!("Remote quick_tunnel discovered · {origin}"));
        remote.status(Status::Verifying);
        // Drain logs while verifying/running; never store credential-bearing output.
        let drain = async {
            let mut chunk = [0u8; 4096];
            while stderr.read(&mut chunk).await.unwrap_or(0) > 0 {}
        };
        let lifecycle = async {
            // DNS/edge propagation is part of this startup, not a new tunnel.
            let mut last_error = None;
            tokio::time::timeout(Duration::from_secs(45), async {
                loop {
                    match remote.probe().await {
                        Ok(()) => break,
                        Err(error) => last_error = Some(error),
                    }
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            })
            .await
            .map_err(|_| {
                last_error.unwrap_or_else(|| {
                    ProbeStage::new(
                        "oauth_metadata",
                        url::Url::parse(&origin).unwrap().host_str().unwrap(),
                    )
                    .fail("timeout", None)
                })
            })?;
            remote.status(Status::Ready);
            broker.log("Remote quick_tunnel ready · authorized MCP initialize/tools/list verified");
            std::future::pending::<Result<(), String>>().await
        };
        tokio::pin!(drain);
        tokio::pin!(lifecycle);
        tokio::select! {
            result = &mut lifecycle => result,
            _ = child.wait() => Err("QUICK_TUNNEL_DISCONNECTED".into()),
            _ = &mut drain => Err("QUICK_TUNNEL_DISCONNECTED".into()),
        }
    };
    let result = tokio::select! { result = work => result, _ = cancel.cancelled() => Ok(()) };
    remote.inner.lock().unwrap().oauth = None;
    // Do not return, restore passthrough, or lose process ownership before confirmed exit.
    if stop_child(&mut child).await.is_err() {
        *remote.pending_child.lock().await = Some(child);
        return Err("QUICK_TUNNEL_STOP_FAILED".into());
    }
    result
}

pub(super) async fn stop_child(child: &mut super::process::ManagedChild) -> Result<(), String> {
    if child
        .try_wait()
        .map_err(|_| "QUICK_TUNNEL_STOP_FAILED")?
        .is_none()
    {
        child.start_kill().map_err(|_| "QUICK_TUNNEL_STOP_FAILED")?;
        tokio::time::timeout(Duration::from_secs(10), child.wait())
            .await
            .map_err(|_| "QUICK_TUNNEL_STOP_FAILED")?
            .map_err(|_| "QUICK_TUNNEL_STOP_FAILED")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_digest_asset_and_url_match_official_fixture() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("fixtures/cloudflared-2026.9.0.json")).unwrap();
        assert_eq!(fixture["tag_name"], VERSION);
        for (os, arch) in [
            ("windows", "x86_64"),
            ("linux", "x86_64"),
            ("linux", "aarch64"),
            ("macos", "x86_64"),
            ("macos", "aarch64"),
        ] {
            let (name, digest) = artifact_for(os, arch).unwrap();
            let asset = fixture["assets"]
                .as_array()
                .unwrap()
                .iter()
                .find(|a| a["name"] == name)
                .unwrap();
            assert_eq!(asset["digest"], format!("sha256:{digest}"));
            assert_eq!(
                asset["browser_download_url"],
                format!(
                    "https://github.com/cloudflare/cloudflared/releases/download/{VERSION}/{name}"
                )
            );
        }
    }
    #[tokio::test]
    #[ignore = "probes an existing public tunnel without changing its lifecycle; requires QUICK_TUNNEL_TEST_ORIGIN"]
    async fn existing_public_tunnel_probe() {
        let origin = std::env::var("QUICK_TUNNEL_TEST_ORIGIN").expect("public origin required");
        probe(
            &RemotePublicContext::new(&origin).unwrap(),
            &std::env::var("QUICK_TUNNEL_TEST_CREDENTIAL").expect("controlled credential required"),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn probe_reports_metadata_http_failure_without_response_body() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let app = axum::Router::new().route(
            "/.well-known/oauth-authorization-server",
            axum::routing::get(|| async {
                (
                    axum::http::StatusCode::BAD_GATEWAY,
                    "private upstream detail",
                )
            }),
        );
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let context = RemotePublicContext {
            public_origin: origin.clone(),
            mcp_resource: format!("{origin}/mcp"),
            instance_id: "test".into(),
        };
        let result = probe(&context, "unused-test-credential").await;
        server.abort();
        let error = result.unwrap_err();
        assert!(error.contains("stage=oauth_metadata category=http_status"));
        assert!(error.contains("status=502"));
        assert!(!error.contains("private upstream detail"));
    }
    #[tokio::test]
    async fn stopping_owned_process_waits_for_actual_exit() {
        #[cfg(windows)]
        let mut command = process::command(crate::serena::find_executable("ping.exe").unwrap());
        #[cfg(windows)]
        command.args(["-n", "30", "127.0.0.1"]);
        #[cfg(not(windows))]
        let mut command = process::command("sleep");
        #[cfg(not(windows))]
        command.arg("30");
        let mut child = super::super::process::ManagedChild::spawn(&mut command).unwrap();
        assert!(child.try_wait().unwrap().is_none());
        stop_child(&mut child).await.unwrap();
        assert!(child.try_wait().unwrap().is_some());
        stop_child(&mut child).await.unwrap();
    }
    #[tokio::test]
    async fn invalid_local_installation_never_downloads() {
        let directory = tempfile::tempdir().unwrap();
        let missing = directory.path().join("missing-cloudflared");
        let runtime = directory.path().join("runtime");
        let error = resolve(&runtime, Some(missing), CancellationToken::new())
            .await
            .unwrap_err();
        assert!(error.starts_with("CLOUDFLARED_INCOMPATIBLE"));
        assert!(!runtime.exists());
    }

    #[tokio::test]
    #[ignore = "requires a local cloudflared installation; verifies reuse without downloading"]
    async fn existing_cloudflared_is_reused_without_download() {
        let path = crate::serena::find_executable("cloudflared")
            .or_else(|| crate::serena::user_local_candidate("cloudflared"))
            .expect("local cloudflared installation required");
        let directory = tempfile::tempdir().unwrap();
        let runtime = directory.path().join("runtime");
        let resolved = resolve(&runtime, Some(path.clone()), CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(resolved, path);
        assert!(!runtime.exists());
    }

    #[tokio::test]
    #[ignore = "downloads the official pinned binary; run explicitly for managed-runtime verification"]
    async fn official_managed_cloudflared_install_and_reuse() {
        let directory = tempfile::tempdir().unwrap();
        let first = install(directory.path(), CancellationToken::new())
            .await
            .unwrap();
        let original = tokio::fs::read(&first).await.unwrap();
        let second = install(directory.path(), CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(first, second);
        assert_eq!(tokio::fs::read(&second).await.unwrap(), original);
        // Tampering is repaired from the verified release asset before execution.
        tokio::fs::write(&second, b"not an executable")
            .await
            .unwrap();
        install(directory.path(), CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(tokio::fs::read(&second).await.unwrap(), original);
    }
    #[test]
    fn accepts_only_exact_quick_tunnel_hosts() {
        assert_eq!(
            parse_origin("| https://calm-lake-42.trycloudflare.com |"),
            Some("https://calm-lake-42.trycloudflare.com".into())
        );
        for value in [
            "https://trycloudflare.com",
            "https://evil.trycloudflare.com.attacker.test",
            "http://a.trycloudflare.com",
            "https://a.b.trycloudflare.com",
            "https://-a.trycloudflare.com",
        ] {
            assert!(parse_origin(value).is_none(), "{value}");
        }
    }
    #[test]
    fn origin_is_independent_of_tunnel_provider() {
        let c = RemotePublicContext::new("https://example.com").unwrap();
        assert_eq!(c.mcp_resource, "https://example.com/mcp");
        for value in [
            "http://example.com",
            "https://example.com/path",
            "https://user@example.com",
            "https://example.com?host=evil",
        ] {
            assert!(RemotePublicContext::new(value).is_err());
        }
        assert_ne!(
            c.instance_id,
            RemotePublicContext::new("https://example.com")
                .unwrap()
                .instance_id
        );
    }
}

#[cfg(test)]
#[path = "probe_tests.rs"]
mod probe_tests;
