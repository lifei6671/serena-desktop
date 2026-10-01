//! CB5-002：只做 initialize；SDK 仅取得 harness 创建好的 external streams。
use agent_client_protocol::{
    ByteStreams, Client,
    schema::{
        ProtocolVersion,
        v1::{InitializeRequest, InitializeResponse},
    },
};
use futures::io::{AsyncRead, AsyncWrite};
use serde_json::{Value, json};
use std::{
    io,
    path::Path,
    pin::Pin,
    process::Stdio,
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::{Duration, Instant},
};
use tokio::{io::AsyncReadExt, process::Command};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
// 冻结设计要求 stable ACP v1，不从 SDK 或产品版本推断。
const SUPPORTED: u64 = 1;
type Wire = Arc<Mutex<Vec<(String, Vec<u8>)>>>;
/// Tee 只复制真实读写成功的字节，SDK 负责 JSON-RPC 编解码。
struct Tee<T> {
    inner: T,
    direction: &'static str,
    wire: Wire,
}
impl<T: AsyncRead + Unpin> AsyncRead for Tee<T> {
    /// 捕获接收分片。
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        match Pin::new(&mut self.inner).poll_read(cx, buf) {
            Poll::Ready(Ok(n)) => {
                if n > 0 {
                    self.wire
                        .lock()
                        .unwrap()
                        .push((self.direction.into(), buf[..n].to_vec()));
                }
                Poll::Ready(Ok(n))
            }
            other => other,
        }
    }
}
impl<T: AsyncWrite + Unpin> AsyncWrite for Tee<T> {
    /// 仅记录成功写入部分，避免把失败发送当成 wire 证据。
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match Pin::new(&mut self.inner).poll_write(cx, buf) {
            Poll::Ready(Ok(n)) => {
                if n > 0 {
                    self.wire
                        .lock()
                        .unwrap()
                        .push((self.direction.into(), buf[..n].to_vec()));
                }
                Poll::Ready(Ok(n))
            }
            other => other,
        }
    }
    /// 刷新真实写入管道。
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }
    /// SDK 关闭时关闭外部写入流。
    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_close(cx)
    }
}
/// 唯一兼容门禁：成功的 initialize 响应且版本完全相等。
fn compatible(response: Option<&InitializeResponse>) -> bool {
    response.is_some_and(|r| serde_json::to_value(r.protocol_version).unwrap() == json!(SUPPORTED))
}
/// 分片重组成有序 JSONL，只保留 initialize 公开字段。
fn sanitize_wire(wire: &Wire) -> Vec<Value> {
    let (mut request, mut response, mut records) = (Vec::new(), Vec::new(), Vec::new());
    for (direction, chunk) in wire.lock().unwrap().iter() {
        let buffer = if direction == "request" {
            &mut request
        } else {
            &mut response
        };
        buffer.extend(chunk);
        while let Some(end) = buffer.iter().position(|b| *b == b'\n') {
            let bytes: Vec<u8> = buffer.drain(..=end).collect();
            if let Ok(raw) = serde_json::from_slice::<Value>(&bytes) {
                // 不保留任意元数据或认证描述；协议字段保持原 JSON 类型和值。
                let mut safe = raw.clone();
                if let Some(result) = safe.get_mut("result").and_then(Value::as_object_mut) {
                    result.retain(|key, _| {
                        ["protocolVersion", "agentCapabilities"].contains(&key.as_str())
                    });
                }
                if let Some(error) = safe.get_mut("error").and_then(Value::as_object_mut) {
                    error.retain(|key, _| key == "code");
                }
                let raw_line = if safe == raw {
                    String::from_utf8_lossy(&bytes).into_owned()
                } else {
                    format!("{}\n", safe)
                };
                records.push(json!({"sequence":records.len()+1,"direction":direction,"message":safe,"rawLine":raw_line,"redacted":safe!=raw}));
            } else {
                records.push(json!({"sequence":records.len()+1,"direction":direction,"nonJsonBytes":bytes.len(),"nonJsonText":String::from_utf8_lossy(&bytes)}));
            }
        }
    }
    records
}
/// 所有 initialize 结果共享 close/terminate/wait 路径；Child 不交给 SDK。
async fn probe(exe: &Path, args: &[String], real: bool, timeout: Duration) -> io::Result<Value> {
    let cwd = tempfile::Builder::new().prefix("cb5-002-").tempdir()?;
    let mut command = Command::new(exe);
    command
        .args(args)
        .current_dir(cwd.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if real {
        command
            .env("ELECTRON_RUN_AS_NODE", "1")
            .env_remove("VSCODE_DEV");
    }
    let mut child = command.spawn()?;
    let pid = child.id();
    let start = Instant::now();
    let wire: Wire = Arc::default();
    let outgoing = Tee {
        inner: child.stdin.take().unwrap().compat_write(),
        direction: "request",
        wire: wire.clone(),
    };
    let incoming = Tee {
        inner: child.stdout.take().unwrap().compat(),
        direction: "response",
        wire: wire.clone(),
    };
    let mut stderr = child.stderr.take().unwrap();
    // 持续 drain stderr 防止背压；只保留受控入口的前 8KiB 诊断，交付前核查脱敏。
    let mut stderr_task = tokio::spawn(async move {
        let mut bytes = Vec::new();
        let mut buf = [0; 4096];
        while let Ok(n) = stderr.read(&mut buf).await {
            if n == 0 {
                break;
            }
            if bytes.len() < 8192 {
                bytes.extend_from_slice(&buf[..n.min(8192 - bytes.len())]);
            }
        }
        bytes
    });
    let initialize = Client.builder().name("cb5-002-probe").connect_with(
        ByteStreams::new(outgoing, incoming),
        async |cx| {
            cx.send_request(InitializeRequest::new(ProtocolVersion::V1))
                .block_task()
                .await
        },
    );
    let (response, error) = match tokio::time::timeout(timeout, initialize).await {
        Ok(Ok(response)) => (Some(response), None),
        Ok(Err(error)) => (None, Some(format!("SDK: {error}"))),
        Err(_) => (None, Some("INITIALIZE_TIMEOUT".into())),
    };
    let initialize_ms = start.elapsed().as_millis();
    // 第一次等待的任何错误都交由同一 cleanup 收集，禁止提前返回。
    let first_wait = tokio::time::timeout(Duration::from_secs(2), child.wait())
        .await
        .ok();
    let cleanup = cleanup_child(&mut child, &mut stderr_task, first_wait).await;
    let safe_response = response.as_ref().map(
        |r| json!({"protocolVersion":r.protocol_version,"agentCapabilities":r.agent_capabilities}),
    );
    Ok(
        json!({"argv":std::iter::once(exe.to_string_lossy().into_owned()).chain(args.iter().cloned()).collect::<Vec<_>>(),"environmentChanges":if real {json!({"ELECTRON_RUN_AS_NODE":"1","VSCODE_DEV":null})} else {json!({})},"cwd":cwd.path(),"pid":pid,"initializeTimeoutSeconds":timeout.as_secs(),"initializeElapsedMs":initialize_ms,"initializeSucceeded":response.is_some(),"protocolCompatible":compatible(response.as_ref()),"supportedProtocolVersion":SUPPORTED,"response":safe_response,"error":error,"wire":sanitize_wire(&wire),"stderrSanitized":cleanup["stderrSanitized"],"cleanup":cleanup,"totalElapsedMs":start.elapsed().as_millis()}),
    )
}
/// 清理失败也继续执行后续回收；每一步有界，失败事实写入 evidence。
async fn cleanup_child(
    child: &mut tokio::process::Child,
    stderr_task: &mut tokio::task::JoinHandle<Vec<u8>>,
    first_wait: Option<io::Result<std::process::ExitStatus>>,
) -> Value {
    let mut errors = Vec::new();
    let mut status = match first_wait {
        Some(Ok(status)) => Some(status),
        Some(Err(error)) => {
            errors.push(format!("initial_wait: {error}"));
            None
        }
        None => None,
    };
    let terminated = status.is_none();
    if terminated {
        if let Err(error) = child.start_kill() {
            errors.push(format!("kill: {error}"));
        }
        // 即使 kill 返回错误，也必须尝试 wait；不伪造进程退出证据。
        match tokio::time::timeout(Duration::from_secs(5), child.wait()).await {
            Ok(Ok(exit)) => status = Some(exit),
            Ok(Err(error)) => errors.push(format!("final_wait: {error}")),
            Err(_) => errors.push("final_wait: timeout".into()),
        }
    }
    let mut stderr_joined = false;
    let stderr_bytes = match tokio::time::timeout(Duration::from_secs(1), &mut *stderr_task).await {
        Ok(Ok(bytes)) => {
            stderr_joined = true;
            Some(bytes)
        }
        Ok(Err(error)) => {
            stderr_joined = true;
            errors.push(format!("stderr_join: {error}"));
            None
        }
        Err(_) => {
            stderr_task.abort();
            match tokio::time::timeout(Duration::from_secs(1), &mut *stderr_task).await {
                Ok(_) => stderr_joined = true,
                Err(_) => errors.push("stderr_abort_join: timeout".into()),
            }
            None
        }
    };
    json!({"streamsClosed":true,"terminated":terminated,"waited":status.is_some(),"exitCode":status.and_then(|s|s.code()),"directChildReaped":status.is_some(),"stderrJoined":stderr_joined,"succeeded":errors.is_empty()&&status.is_some()&&stderr_joined,"errors":errors,"stderrSanitized":stderr_bytes.map(|b|String::from_utf8_lossy(&b).into_owned()),"windowsJobAtCreationProven":false})
}
/// fake peer 仅处理 initialize，不允许 session 或 prompt。
fn fake_peer(mode: &str) {
    use std::io::{BufRead, Write};
    // 仅 fake peer 暴露本进程 PID，供外层 watchdog 精确回收，不枚举用户进程。
    if let Some(path) = std::env::var_os("CB5_TEST_PEER_PID_FILE") {
        std::fs::write(path, std::process::id().to_string()).unwrap();
    }
    if mode == "eof-before" {
        return;
    }
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line).unwrap();
    if mode == "eof-during" {
        return;
    }
    if mode == "hang" {
        std::thread::sleep(Duration::from_secs(30));
        return;
    }
    let request: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(request["method"], "initialize");
    let version = if mode == "mismatch" { 999 } else { 1 };
    println!(
        "{}",
        json!({"jsonrpc":"2.0","id":request["id"],"result":{"protocolVersion":version,"agentCapabilities":{}}})
    );
    std::io::stdout().flush().unwrap();
    // 等待客户端关 stdin，覆盖正常退出路径。
    line.clear();
    std::io::stdin().lock().read_line(&mut line).unwrap();
}
/// 命令行只运行固定的本卡场景。
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--fake") {
        fake_peer(&args[2]);
        return Ok(());
    }
    let output = Path::new(&args[2]);
    let result = if args[1] == "fixture" {
        probe(
            &std::env::current_exe()?,
            &["--fake".into(), args[3].clone()],
            false,
            Duration::from_secs(if args[3] == "hang" { 1 } else { 5 }),
        )
        .await?
    } else {
        let exe = Path::new(r"C:\Users\lifei\AppData\Local\Programs\CodeBuddy CN\CodeBuddy CN.exe");
        let mut argv = vec![
            r"C:\Users\lifei\AppData\Local\Programs\CodeBuddy CN\resources\app\out\cli.js".into(),
            "--acp".into(),
        ];
        if args.get(3).is_some_and(|s| s == "auto") {
            argv.extend(["--permission-mode".into(), "auto".into()]);
        }
        probe(exe, &argv, true, Duration::from_secs(20)).await?
    };
    std::fs::write(output, serde_json::to_string_pretty(&result)?)?;
    println!(
        "{}",
        json!({"output":output,"initializeSucceeded":result["initializeSucceeded"],"protocolCompatible":result["protocolCompatible"],"error":result["error"]})
    );
    if result["cleanup"]["succeeded"] != true {
        return Err(io::Error::other("CLEANUP_FAILED; see saved evidence").into());
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    /// 不匹配版本能够 deserialize，但必须拒绝连接。
    #[test]
    fn mismatch_is_rejected() {
        let r: InitializeResponse =
            serde_json::from_value(json!({"protocolVersion":999,"agentCapabilities":{}})).unwrap();
        assert!(!compatible(Some(&r)));
    }
    /// 缺 optional capability 仍协议兼容，loadSession 默认为 false。
    #[test]
    fn missing_capability_is_compatible() {
        let r: InitializeResponse =
            serde_json::from_value(json!({"protocolVersion":1,"agentCapabilities":{}})).unwrap();
        assert!(compatible(Some(&r)));
        assert!(!r.agent_capabilities.load_session);
    }
    /// 缺响应绝不通过兼容门禁。
    #[test]
    fn no_response_is_rejected() {
        assert!(!compatible(None));
    }
    /// Wire 必须保留实际字段，不能把 SDK 错误响应伪造成空 request。
    #[test]
    fn wire_preserves_frames_and_non_json() {
        let wire:Wire=Arc::new(Mutex::new(vec![
            ("request".into(),b"{\"method\":\"initialize\",\"params\":{\"protocolVersion\":1}}\n".to_vec()),
            ("response".into(),b"CLI is not ACP\n".to_vec()),
            ("request".into(),b"{\"jsonrpc\":\"2.0\",\"id\":null,\"error\":{\"code\":-32700,\"data\":\"private\"}}\n".to_vec())]));
        let rows = sanitize_wire(&wire);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0]["message"]["params"]["protocolVersion"], 1);
        assert_eq!(rows[1]["nonJsonText"], "CLI is not ACP\n");
        assert_eq!(rows[2]["message"]["error"]["code"], -32700);
        assert!(rows[2]["message"].get("method").is_none());
        assert!(!rows[2]["rawLine"].as_str().unwrap().contains("private"));
    }
    /// 模拟首次 wait 失败：仍回收真实 Child、终止并 join 挂起的 stderr reader。
    #[tokio::test]
    async fn cleanup_wait_failure_still_reaps_and_joins() {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .arg("--list")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut stderr_task = tokio::spawn(std::future::pending::<Vec<u8>>());
        let report = cleanup_child(
            &mut child,
            &mut stderr_task,
            Some(Err(io::Error::other("injected wait failure"))),
        )
        .await;
        assert_eq!(report["succeeded"], false);
        assert_eq!(report["directChildReaped"], true);
        assert_eq!(report["stderrJoined"], true);
        assert!(
            report["errors"][0]
                .as_str()
                .unwrap()
                .contains("injected wait failure")
        );
    }
}
