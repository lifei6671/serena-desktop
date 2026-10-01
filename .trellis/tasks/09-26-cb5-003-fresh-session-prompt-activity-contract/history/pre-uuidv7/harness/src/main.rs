//! CB5-003：真实 fresh session/prompt 合同；不实现产品 Provider 或权限策略。
use agent_client_protocol::{
    ByteStreams, Client,
    schema::{ProtocolVersion, v1::*},
};
use futures::io::{AsyncRead, AsyncWrite};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
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
mod transport;
use transport::*;
const READ_MARKER: &str = "SERENA_CB5_003_READ_MARKER";
const WRITE_CONTENT: &[u8] = b"SERENA_CB5_003_WRITE_MARKER\n";
const META_KEY: &str = "codebuddy.ai/conversationRequestId";

/// 官方 schema 的 metadata 机制；UUID 来自 harness 随机源，不来自 prompt。
fn prompt_request(id: SessionId, write: bool, correlation: Option<&str>) -> PromptRequest {
    let text = if write {
        "In this temporary workspace only, create exactly output.txt with UTF-8 bytes SERENA_CB5_003_WRITE_MARKER followed by one LF newline. Do not create or modify any other file or directory. Do not access files outside the current workspace. Finish with a brief confirmation."
    } else {
        "Read input.txt in the current temporary workspace using your local read tool. Return its exact marker in your final answer. Do not create, modify, or delete any files or directories. Do not access files outside the current workspace."
    };
    let request = PromptRequest::new(id, vec![ContentBlock::Text(TextContent::new(text))]);
    match correlation {
        Some(uuid) => request.meta(serde_json::Map::from_iter([(META_KEY.into(), json!(uuid))])),
        None => request,
    }
}

/// 递归读取全部文件（包括隐藏项）与目录；链接不能伪装成隔离证明。
fn manifest(root: &Path) -> io::Result<BTreeMap<String, Value>> {
    fn visit(root: &Path, dir: &Path, out: &mut BTreeMap<String, Value>) -> io::Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            let meta = std::fs::symlink_metadata(&path)?;
            let relative = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            if meta.file_type().is_symlink() {
                return Err(io::Error::other("workspace contains link"));
            }
            if meta.is_dir() {
                out.insert(relative, json!({"kind":"directory"}));
                visit(root, &path, out)?;
            } else {
                let bytes = std::fs::read(&path)?;
                out.insert(relative, json!({"kind":"file","bytes":bytes}));
            }
        }
        Ok(())
    }
    let mut result = BTreeMap::new();
    visit(root, root, &mut result)?;
    Ok(result)
}

/// 比较完整路径集合与内容；新增隐藏目录同样是 delta。
fn delta(before: &BTreeMap<String, Value>, after: &BTreeMap<String, Value>) -> Vec<String> {
    before
        .keys()
        .chain(after.keys())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .filter(|key| before.get(*key) != after.get(*key))
        .cloned()
        .collect()
}

/// 只剔除秘密/任意 metadata；公开协议、受控测试正文与身份保持原值。
fn sanitize(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, value) in map.iter_mut() {
                let lower = key.to_lowercase();
                if [
                    "token",
                    "accesstoken",
                    "refreshtoken",
                    "authorization",
                    "password",
                    "secret",
                    "apikey",
                    "api_key",
                    "credential",
                    "credentials",
                    "env",
                ]
                .contains(&lower.as_str())
                {
                    *value = json!("[REDACTED]");
                } else if key == "_meta" {
                    if let Some(meta) = value.as_object_mut() {
                        meta.retain(|k, _| k == META_KEY);
                    } else {
                        *value = Value::Null;
                    }
                } else {
                    sanitize(value);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(sanitize),
        _ => {}
    }
}

/// 逐分片恢复 NDJSON；保存完成帧的跨方向次序，非 JSON 只计字节避免泄密。
fn wire_records(wire: &Wire) -> Vec<Value> {
    let (mut tx, mut rx, mut rows) = (Vec::new(), Vec::new(), Vec::new());
    for (direction, bytes) in wire.lock().unwrap().iter() {
        let buffer = if direction == "request" {
            &mut tx
        } else {
            &mut rx
        };
        buffer.extend(bytes);
        while let Some(end) = buffer.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = buffer.drain(..=end).collect();
            if let Ok(raw) = serde_json::from_slice::<Value>(&line) {
                let mut safe = raw.clone();
                sanitize(&mut safe);
                let redacted = raw != safe;
                let raw_line = if redacted {
                    format!("{safe}\n")
                } else {
                    String::from_utf8_lossy(&line).into_owned()
                };
                rows.push(json!({"sequence":rows.len()+1,"direction":direction,"message":safe,"rawLine":raw_line,"redacted":redacted}));
            } else {
                rows.push(json!({"sequence":rows.len()+1,"direction":direction,"nonJsonBytes":line.len()}));
            }
        }
    }
    for (direction, bytes) in [("request", tx), ("response", rx)] {
        if !bytes.is_empty() {
            rows.push(json!({"sequence":rows.len()+1,"direction":direction,"incompleteFrameBytes":bytes.len()}));
        }
    }
    rows
}

/// 通过实际 RPC id 找响应，避免 update 或相邻 request 被误认为 terminal。
fn exchange(rows: &[Value], method: &str) -> Value {
    let request = rows
        .iter()
        .find(|r| r["direction"] == "request" && r["message"]["method"] == method);
    let response = request.and_then(|q| {
        rows.iter().find(|r| {
            r["direction"] == "response"
                && r["message"].get("id") == q["message"].get("id")
                && r["message"].get("method").is_none()
        })
    });
    json!({"request":request,"response":response})
}

/// SDK 能表示空字符串，故在真实外部边界显式拒绝空 sessionId。
fn valid_session(response: &NewSessionResponse) -> bool {
    serde_json::to_value(&response.session_id)
        .unwrap()
        .as_str()
        .is_some_and(|s| !s.trim().is_empty())
}

/// 单场景拥有完整进程和临时目录；所有协议失败都进入相同有界清理。
async fn probe(
    exe: &Path,
    argv: &[String],
    write: bool,
    correlation: Option<String>,
    timeout: Duration,
) -> io::Result<Value> {
    let workspace = tempfile::Builder::new().prefix("cb5-003-").tempdir()?;
    // Windows canonicalize 返回 extended path；规范化为同一绝对 Win32 路径，便于 CLI 本地工具消费。
    let canonical = workspace.path().canonicalize()?;
    let cwd = std::path::PathBuf::from(canonical.to_string_lossy().trim_start_matches(r"\\?\"));
    if !write {
        std::fs::write(cwd.join("input.txt"), format!("{READ_MARKER}\n"))?;
    }
    let before = manifest(&cwd)?;
    let mut child = Command::new(exe)
        .args(argv)
        .current_dir(&cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
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
    let mut stderr_task = tokio::spawn(async move {
        // 不保存 stderr 内容，只 drain；认证诊断可能含本地敏感上下文。
        let mut bytes = Vec::new();
        let mut buffer = [0; 4096];
        while let Ok(n) = stderr.read(&mut buffer).await {
            if n == 0 {
                break;
            }
            if bytes.len() < 8192 {
                bytes.resize((bytes.len() + n).min(8192), 0);
            }
        }
        bytes
    });
    let state = Arc::new(Mutex::new(json!({"stage":"initialize"})));
    let state_for_sdk = state.clone();
    let sdk_cwd = cwd.clone();
    let sdk_correlation = correlation.clone();
    let permission = Arc::new(tokio::sync::Notify::new());
    let permission_handler = permission.clone();
    let operation = Client
        .builder()
        .name("cb5-003-probe")
        .on_receive_notification(
            async |_notification: SessionNotification, _cx| Ok(()),
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            async move |_request: RequestPermissionRequest, _responder, _cx| {
                // 本卡不作权限选择：通知外层结束连接，不发送 allow/cancel policy。
                permission_handler.notify_one();
                std::future::pending::<Result<(), agent_client_protocol::Error>>().await
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(ByteStreams::new(outgoing, incoming), async move |cx| {
            let init = cx
                .send_request(InitializeRequest::new(ProtocolVersion::V1))
                .block_task()
                .await?;
            state_for_sdk.lock().unwrap()["initialize"] = serde_json::to_value(&init).unwrap();
            if serde_json::to_value(init.protocol_version).unwrap() != json!(1) {
                return Ok(false);
            }
            state_for_sdk.lock().unwrap()["stage"] = json!("session/new");
            let session = cx
                .send_request(NewSessionRequest::new(sdk_cwd))
                .block_task()
                .await?;
            state_for_sdk.lock().unwrap()["session"] = serde_json::to_value(&session).unwrap();
            if !valid_session(&session) {
                state_for_sdk.lock().unwrap()["failure"] = json!("EMPTY_SESSION_ID");
                return Ok(false);
            }
            state_for_sdk.lock().unwrap()["stage"] = json!("session/prompt");
            let terminal = cx
                .send_request(prompt_request(
                    session.session_id,
                    write,
                    sdk_correlation.as_deref(),
                ))
                .block_task()
                .await?;
            state_for_sdk.lock().unwrap()["terminal"] = serde_json::to_value(terminal).unwrap();
            state_for_sdk.lock().unwrap()["stage"] = json!("terminal");
            Ok(true)
        });
    let (success, error) = tokio::select! {
        result = tokio::time::timeout(timeout,operation) => match result {
            Ok(Ok(success)) => (success,None),
            Ok(Err(error)) => (false,Some(format!("SDK: {error}"))),
            Err(_) => (false,Some("RPC_TIMEOUT".into())),
        },
        _ = permission.notified() => (false,Some("PERMISSION_REQUEST_NOT_IN_SCOPE".into())),
    };
    let first_wait = tokio::time::timeout(Duration::from_secs(2), child.wait())
        .await
        .ok();
    let cleanup = cleanup_child(&mut child, &mut stderr_task, first_wait).await;
    let after_result = manifest(&cwd);
    let rows = wire_records(&wire);
    let mut typed = state.lock().unwrap().clone();
    sanitize(&mut typed);
    let after = after_result.as_ref().ok();
    let changes = after.map(|a| delta(&before, a));
    let write_exact = after
        .and_then(|a| a.get("output.txt"))
        .is_some_and(|v| v["bytes"] == json!(WRITE_CONTENT));
    let isolation = if write {
        changes == Some(vec!["output.txt".into()]) && write_exact
    } else {
        changes == Some(vec![])
    };
    let session_exchange = exchange(&rows, "session/new");
    let prompt_exchange = exchange(&rows, "session/prompt");
    let session_id = session_exchange
        .pointer("/response/message/result/sessionId")
        .cloned();
    let updates: Vec<_> = rows
        .iter()
        .filter(|r| r["message"]["method"] == "session/update")
        .cloned()
        .collect();
    let text: String = updates
        .iter()
        .filter(|r| r["message"]["params"]["update"]["sessionUpdate"] == "agent_message_chunk")
        .filter_map(|r| {
            r.pointer("/message/params/update/content/text")
                .and_then(Value::as_str)
        })
        .collect();
    let marker_returned = text.contains(READ_MARKER);
    let mut report = json!({"argv":std::iter::once(exe.to_string_lossy().into_owned()).chain(argv.iter().cloned()).collect::<Vec<_>>(),"cwd":cwd,"pid":pid,"environmentChanges":{},"timeoutSeconds":timeout.as_secs(),"protocolSucceeded":success,"error":error,"sdkTyped":typed,"initialize":exchange(&rows,"initialize"),"sessionNew":session_exchange,"prompt":prompt_exchange,"sessionId":session_id,"conversationRequestId":correlation,"wire":rows,"updates":updates,"before":before,"after":after,"manifestError":after_result.as_ref().err().map(ToString::to_string),"delta":changes,"workspaceIsolation":isolation,"exactWriteContent":write_exact,"readMarkerReturned":marker_returned,"cleanup":cleanup,"elapsedMs":start.elapsed().as_millis(),"canExecuteEvidence":success&&isolation&&(write||marker_returned),"safeErrorTerminal":"NOT_PROVEN"});
    // 临时目录只来自本函数创建；完成全部证据复制后才移除。
    let removed = workspace.close();
    report["workspaceDeleted"] = json!(removed.is_ok());
    report["workspaceDeleteError"] = json!(removed.err().map(|e| e.to_string()));
    Ok(report)
}

/// Fake peer 对外部 wire 的错误、次序与 EOF 做确定性回归。
fn fake_peer(mode: &str) {
    use std::io::{BufRead, Write};
    for line in std::io::stdin().lock().lines() {
        let request: Value = serde_json::from_str(&line.unwrap()).unwrap();
        let method = request["method"].as_str().unwrap();
        let result = match method {
            "initialize" => json!({"protocolVersion":1,"agentCapabilities":{}}),
            "session/new" => match mode {
                "missing" => json!({}),
                "empty" => json!({"sessionId":""}),
                "malformed" => json!({"sessionId":17}),
                _ => json!({"sessionId":"fixture-session"}),
            },
            "session/prompt" => {
                if mode == "eof" {
                    return;
                }
                if mode == "timeout" {
                    std::thread::sleep(Duration::from_secs(30));
                    return;
                }
                println!(
                    "{}",
                    json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"fixture-session","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":READ_MARKER}}}})
                );
                if mode == "bad-terminal" {
                    json!({"stopReason":5})
                } else {
                    json!({"stopReason":"end_turn","fixtureExtra":{"result":true}})
                }
            }
            _ => panic!("unexpected method"),
        };
        println!(
            "{}",
            json!({"jsonrpc":"2.0","id":request["id"],"result":result})
        );
        std::io::stdout().flush().unwrap();
    }
}

/// CLI 只提供本卡固定真实场景和 fake fixture，不接受任意用户工作区。
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--fake") {
        fake_peer(&args[2]);
        return Ok(());
    }
    let mode = &args[1];
    let output = Path::new(&args[2]);
    let write = mode == "write";
    let result = if mode == "fixture" {
        probe(
            &std::env::current_exe()?,
            &["--fake".into(), args[3].clone()],
            false,
            None,
            Duration::from_secs(if args[3] == "timeout" { 1 } else { 5 }),
        )
        .await?
    } else {
        assert!(mode == "read" || mode == "write");
        let mut argv=vec![r"C:\Users\lifei\AppData\Roaming\npm\node_modules\@tencent-ai\codebuddy-code\bin\codebuddy".into(),"--acp".into()];
        if write {
            argv.extend(["--permission-mode".into(), "auto".into()]);
        }
        probe(
            Path::new(r"C:\nvm4w\nodejs\node.exe"),
            &argv,
            write,
            write.then(|| uuid::Uuid::new_v4().to_string()),
            Duration::from_secs(180),
        )
        .await?
    };
    std::fs::write(output, serde_json::to_string_pretty(&result)?)?;
    println!(
        "{}",
        json!({"output":output,"canExecuteEvidence":result["canExecuteEvidence"],"error":result["error"],"delta":result["delta"]})
    );
    if result["cleanup"]["succeeded"] != true {
        return Err(io::Error::other("cleanup failed; see evidence").into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    /// 空或非法 session identity 必须阻止 prompt。
    #[test]
    fn invalid_session_identity() {
        for value in [json!({}), json!({"sessionId":12}), json!([])] {
            assert!(serde_json::from_value::<NewSessionResponse>(value).is_err());
        }
        for id in ["", " "] {
            assert!(!valid_session(&NewSessionResponse::new(id)));
        }
    }
    /// A/B 的唯一 metadata 差异由官方 schema 序列化。
    #[test]
    fn correlation_serialization() {
        let a = serde_json::to_value(prompt_request("s".into(), false, None)).unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let b = serde_json::to_value(prompt_request("s".into(), false, Some(&id))).unwrap();
        assert!(a.get("_meta").is_none());
        assert_eq!(b["_meta"][META_KEY], id);
        assert_eq!(a["prompt"], b["prompt"]);
    }
    /// 隐藏文件和新增目录不能漏出全目录 manifest。
    #[test]
    fn manifest_detects_unexpected_write() {
        let temp = tempfile::tempdir().unwrap();
        let before = manifest(temp.path()).unwrap();
        std::fs::create_dir(temp.path().join(".hidden")).unwrap();
        std::fs::write(temp.path().join(".hidden/extra"), b"x").unwrap();
        assert_eq!(
            delta(&before, &manifest(temp.path()).unwrap()),
            vec![".hidden", ".hidden/extra"]
        );
    }
    /// 脱敏不能抹掉 public identity、terminal 类型和顺序。
    #[test]
    fn sanitation_retains_contract() {
        let mut v = json!({"sessionId":"s","stopReason":"end_turn","_meta":{META_KEY:"id","private":"secret"},"token":"private"});
        sanitize(&mut v);
        assert_eq!(v["_meta"][META_KEY], "id");
        assert!(v["_meta"].get("private").is_none());
        assert_eq!(v["stopReason"], "end_turn");
        assert!(!v.to_string().contains("private"));
    }
    /// wait 错误时仍执行 kill/wait 和 stderr join，测试有外层期限。
    #[tokio::test]
    async fn cleanup_wait_failure_bounded() {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .arg("--list")
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let mut stderr = tokio::spawn(std::future::pending::<Vec<u8>>());
        let report = tokio::time::timeout(
            Duration::from_secs(9),
            cleanup_child(
                &mut child,
                &mut stderr,
                Some(Err(io::Error::other("injected wait failure"))),
            ),
        )
        .await
        .unwrap();
        assert_eq!(report["succeeded"], false);
        assert_eq!(report["directChildReaped"], true);
        assert_eq!(report["stderrJoined"], true);
    }
    /// kill 错误不能跳过最终 wait；有界返回且明确失败。
    #[tokio::test]
    async fn cleanup_kill_failure_bounded() {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .arg("--list")
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let mut stderr = tokio::spawn(async { Vec::new() });
        let report = tokio::time::timeout(
            Duration::from_secs(8),
            cleanup_with_kill(&mut child, &mut stderr, None, |_| {
                Err(io::Error::other("injected kill failure"))
            }),
        )
        .await
        .unwrap();
        assert_eq!(report["succeeded"], false);
        assert_eq!(report["directChildReaped"], true);
        assert!(
            report["errors"][0]
                .as_str()
                .unwrap()
                .contains("injected kill failure")
        );
    }
}
