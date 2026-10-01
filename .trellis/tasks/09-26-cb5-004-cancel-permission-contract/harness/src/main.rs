//! CB5-004：仅限临时工作区的 cancel/permission 一手合同探针。
use agent_client_protocol::{
    ByteStreams, Client,
    schema::{ProtocolVersion, v1::*},
};
use futures::io::{AsyncRead, AsyncWrite};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
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
mod base;
use base::*;
const META_KEY: &str = "codebuddy.ai/conversationRequestId";
const MARKER: &[u8] = b"SERENA_CB5_004_SIDE_EFFECT\n";

/// 所有真实场景共享 Gold Band 最小 argv；权限/工具配置不能提前注入。
fn canonical_argv() -> Vec<String> {
    vec![
        r"C:\Users\lifei\AppData\Roaming\npm\node_modules\@tencent-ai\codebuddy-code\bin\codebuddy"
            .into(),
        "--acp".into(),
    ]
}

/// 从本次成功 new 返回的 typed catalog 取出实际 mode ID；缺失就不猜测。
fn advertised_auto(session: &NewSessionResponse) -> Option<SessionModeId> {
    session
        .modes
        .as_ref()?
        .available_modes
        .iter()
        .find(|mode| mode.id.to_string() == "auto")
        .map(|mode| mode.id.clone())
}

/// catalog 只保留公开配置键，剔除任意 metadata 和未定义字段。
fn catalog(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(catalog).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .filter(|(k, _)| {
                    [
                        "currentModeId",
                        "availableModes",
                        "id",
                        "name",
                        "description",
                        "category",
                        "type",
                        "options",
                        "currentValue",
                        "value",
                        "group",
                        "groupName",
                    ]
                    .contains(&k.as_str())
                })
                .map(|(k, v)| (k.clone(), catalog(v)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// 单个 runtime/session/prompt 的身份与一次性决策；没有产品状态映射。
#[derive(Default)]
struct Identity {
    session: String,
    correlation: String,
    active: bool,
    tools: BTreeSet<String>,
    responded: BTreeSet<String>,
}
impl Identity {
    /// 只允许当前仍 active 的精确身份触发 cancel。
    fn can_cancel(&self, session: &str, correlation: &str) -> bool {
        self.active
            && !self.session.is_empty()
            && self.session == session
            && self.correlation == correlation
    }
    /// 必须有同 session 的既有 tool update；未知/重复/late 请求不发选择。
    fn deny(
        &mut self,
        request: &RequestPermissionRequest,
    ) -> Result<RequestPermissionResponse, &'static str> {
        let tool = request.tool_call.tool_call_id.to_string();
        if !self.active
            || request.session_id.to_string() != self.session
            || !self.tools.contains(&tool)
        {
            return Err("IDENTITY_OR_LATE_REQUEST");
        }
        if self.responded.contains(&tool) {
            return Err("DUPLICATE_RESPONDER");
        }
        let mut ids = BTreeSet::new();
        for option in &request.options {
            if option.option_id.to_string().trim().is_empty()
                || !ids.insert(option.option_id.to_string())
            {
                return Err("MALFORMED_OPTIONS");
            }
        }
        let option = request
            .options
            .iter()
            .find(|o| o.kind == PermissionOptionKind::RejectOnce)
            .ok_or("NO_REJECT_ONCE")?;
        self.responded.insert(tool);
        Ok(RequestPermissionResponse::new(
            RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(
                option.option_id.clone(),
            )),
        ))
    }
}

/// 只有磁盘上的完整实际字节可触发；文件不存在/半写入/链接均不接受。
fn marker_ready(cwd: &Path) -> bool {
    let path = cwd.join("before-cancel.txt");
    std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_file() && !m.file_type().is_symlink())
        && std::fs::read(path).is_ok_and(|bytes| {
            bytes == MARKER
                && format!("{:x}", Sha256::digest(&bytes))
                    == "613841732b16579f9417ffa2ffeb2a86dab63957b0c8381976403c6ee4960301"
        })
}

/// 持久证据严格白名单，不落 prompt、command、label、source 或任意 Provider 输出。
fn safe_message(raw: &Value) -> Value {
    let mut out = json!({});
    for k in ["jsonrpc", "id", "method"] {
        if let Some(v) = raw.get(k) {
            out[k] = v.clone();
        }
    }
    if let Some(p) = raw.get("params") {
        let mut q = json!({});
        if let Some(v) = p.get("sessionId") {
            q["sessionId"] = v.clone();
        }
        for key in ["cwd", "mcpServers", "modeId", "configId", "value"] {
            if let Some(v) = p.get(key) {
                q[key] = v.clone();
            }
        }
        if let Some(v) = p.get("protocolVersion") {
            q["protocolVersion"] = v.clone();
        }
        if let Some(v) = p.get("_meta").and_then(|m| m.get(META_KEY)) {
            q["_meta"] = json!({META_KEY:v});
        }
        if let Some(u) = p.get("update") {
            q["update"] = json!({});
            for k in ["sessionUpdate", "toolCallId", "status", "kind"] {
                if let Some(v) = u.get(k) {
                    q["update"][k] = v.clone();
                }
            }
            if let Some(v) = u.get("_meta").and_then(|m| m.get(META_KEY)) {
                q["update"]["_meta"] = json!({META_KEY:v});
            }
        }
        if let Some(t) = p.get("toolCall") {
            q["toolCall"] =
                json!({"toolCallId":t["toolCallId"],"status":t["status"],"kind":t["kind"]});
        }
        if let Some(options) = p.get("options").and_then(Value::as_array) {
            q["options"] = json!(
                options
                    .iter()
                    .map(|o| json!({"optionId":o["optionId"],"kind":o["kind"]}))
                    .collect::<Vec<_>>()
            );
        }
        out["params"] = q;
    }
    if let Some(r) = raw.get("result") {
        out["result"] = json!({});
        for k in ["sessionId", "protocolVersion", "stopReason", "outcome"] {
            if let Some(v) = r.get(k) {
                out["result"][k] = v.clone();
            }
        }
        for key in ["modes", "configOptions"] {
            if let Some(v) = r.get(key) {
                out["result"][key] = catalog(v);
            }
        }
        if let Some(v) = r.get("_meta").and_then(|m| m.get(META_KEY)) {
            out["result"]["_meta"] = json!({META_KEY:v});
        }
    }
    if let Some(e) = raw.get("error") {
        out["error"] = json!({"code":e["code"],"detailsOmitted":true});
        // 只持久化静态分类；不保留外部错误原文或潜在认证数据。
        let text = e.to_string();
        out["error"]["classification"] = json!(if text.contains("status code 500") {
            "HTTP_500"
        } else if text.contains("ENOENT") {
            "LOCAL_PATH_NOT_FOUND"
        } else if text.contains("401") || text.contains("403") {
            "HTTP_AUTH_STATUS"
        } else {
            "UNCLASSIFIED"
        });
    }
    out
}

/// 从已成功通过 Tee 的完整帧生成有序、脱敏证据。
fn safe_rows(wire: &Wire) -> Vec<Value> {
    wire_records(wire).into_iter().map(|r|{
        let message=safe_message(&r["message"]);
        json!({"sequence":r["sequence"],"direction":r["direction"],"sanitizedRawLine":format!("{message}\n"),"message":message})
    }).collect()
}

/// 固定场景，不接受任意用户 cwd 或命令。
fn prompt(session: SessionId, mode: &str, correlation: &str) -> PromptRequest {
    let text = match mode {
        "before" => {
            "This is an isolated temporary workspace contract test. Do not use any tools or access any files. Produce a long, detailed numbered explanation of counting integers from 1 to 10000, one paragraph per integer. Continue until all 10000 are explained."
        }
        "after" => {
            "In this temporary workspace only, first create exactly before-cancel.txt with UTF-8 bytes SERENA_CB5_004_SIDE_EFFECT followed by a single LF newline. Then perform a safe foreground wait for 60 seconds using PowerShell Start-Sleep -Seconds 60. Do not create or modify any other file or directory. Do not access anything outside this workspace. Do not spawn background work. Stop if cancelled."
        }
        _ => {
            "In this isolated temporary workspace, use the Write tool to create denied.txt containing DENIED_TEST followed by LF. Request permission if required. If permission is denied, stop immediately and briefly report denial; do not retry, use other tools, or perform alternate operations. Do not read or modify anything outside this workspace."
        }
    };
    PromptRequest::new(session, vec![ContentBlock::Text(TextContent::new(text))]).meta(
        serde_json::Map::from_iter([(META_KEY.into(), json!(correlation))]),
    )
}

/// 每次 probe 自有 Child、全部 streams、fresh cwd；所有失败都回到 bounded cleanup。
async fn probe(mode: &str, output: &Path, fake: Option<&str>) -> io::Result<Value> {
    let workspace = tempfile::Builder::new().prefix("cb5-004-").tempdir()?;
    let canonical = workspace.path().canonicalize()?;
    let cwd = std::path::PathBuf::from(canonical.to_string_lossy().trim_start_matches(r"\\?\"));
    let before = manifest(&cwd)?;
    let correlation = uuid::Uuid::now_v7().simple().to_string();
    // 在发送 prompt 前 durable；不存入 workspace，避免改变被测 manifest。
    let private_path = output.with_extension("identity.json");
    let mut durable = std::fs::File::create(&private_path)?;
    use std::io::Write;
    durable.write_all(
        serde_json::to_string(
            &json!({"conversationRequestId":correlation,"generatedBeforeSpawn":true}),
        )?
        .as_bytes(),
    )?;
    durable.sync_all()?;
    let (exe, argv) = if let Some(f) = fake {
        (std::env::current_exe()?, vec!["--fake".into(), f.into()])
    } else {
        (
            std::path::PathBuf::from(r"C:\nvm4w\nodejs\node.exe"),
            canonical_argv(),
        )
    };
    let mut child = Command::new(&exe)
        .args(&argv)
        .current_dir(&cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let pid = child.id();
    let started = Instant::now();
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
        let mut buffer = [0; 4096];
        let mut count = 0;
        while let Ok(n) = stderr.read(&mut buffer).await {
            if n == 0 {
                break;
            }
            count += n;
        }
        vec![0; count.min(8192)]
    });
    let identity = Arc::new(Mutex::new(Identity::default()));
    let state = Arc::new(Mutex::new(
        json!({"stage":"initialize","permissionDecisions":[]}),
    ));
    let ni = identity.clone();
    let pi = identity.clone();
    let ps = state.clone();
    let pcwd = cwd.clone();
    let si = identity.clone();
    let ss = state.clone();
    let sw = wire.clone();
    let scwd = cwd.clone();
    let corr = correlation.clone();
    let post_timeout = if fake.is_some() {
        Duration::from_millis(600)
    } else {
        Duration::from_secs(20)
    };
    let operation=Client.builder().name("cb5-004-probe")
        .on_receive_notification(async move |n:SessionNotification,_cx| {
            let mut i=ni.lock().unwrap();
            if i.active && n.session_id.to_string()==i.session {
                match &n.update {SessionUpdate::ToolCall(t)=>{i.tools.insert(t.tool_call_id.to_string());}, SessionUpdate::ToolCallUpdate(t)=>{i.tools.insert(t.tool_call_id.to_string());},_=>{}}
            }
            Ok(())
        },agent_client_protocol::on_receive_notification!())
        .on_receive_request(async move |r:RequestPermissionRequest,responder,_cx| {
            let before_deny=manifest(&pcwd).ok();
            let decision=pi.lock().unwrap().deny(&r);
            let options=r.options.iter().map(|o|json!({"optionId":o.option_id,"kind":o.kind})).collect::<Vec<_>>();
            match decision {
                Ok(response)=>{
                    let response_value=serde_json::to_value(&response).unwrap();
                    responder.respond(response)?;
                    ps.lock().unwrap()["permissionDecisions"].as_array_mut().unwrap().push(json!({"sessionId":r.session_id,"toolCallId":r.tool_call.tool_call_id,"options":options,"response":response_value,"manifestBeforeDeny":before_deny,"typedResponderConsumed":true}));
                },
                Err(reason)=>{ps.lock().unwrap()["permissionRejected"]=json!(reason);responder.respond_with_internal_error(reason)?;}
            }
            Ok(())
        },agent_client_protocol::on_receive_request!())
        .connect_with(ByteStreams::new(outgoing,incoming),async move |cx| {
            let init=cx.send_request(InitializeRequest::new(ProtocolVersion::V1)).block_task().await?;
            ss.lock().unwrap()["protocolVersion"]=serde_json::to_value(init.protocol_version).unwrap();
            ss.lock().unwrap()["stage"]=json!("session/new");
            let session=cx.send_request(NewSessionRequest::new(scwd.clone()).mcp_servers(vec![])).block_task().await?;
            ss.lock().unwrap()["sessionNew"]=safe_message(&json!({"result":&session}))["result"].clone();
            if session.session_id.to_string().trim().is_empty() {ss.lock().unwrap()["stage"]=json!("invalid_session_id");return Ok(());}
            if mode=="diagnostic" {ss.lock().unwrap()["stage"]=json!("session_ready_no_prompt");return Ok(());}
            if mode=="after" {
                let Some(selected)=advertised_auto(&session) else {ss.lock().unwrap()["stage"]=json!("auto_mode_not_advertised");return Ok(());};
                ss.lock().unwrap()["stage"]=json!("session/set_mode");
                ss.lock().unwrap()["selectedModeFromCatalog"]=json!(selected);
                cx.send_request(SetSessionModeRequest::new(session.session_id.clone(),selected)).block_task().await?;
                ss.lock().unwrap()["setModeAcknowledged"]=json!(true);
            }
            {let mut i=si.lock().unwrap();i.session=session.session_id.to_string();i.correlation=corr.clone();i.active=true;}
            ss.lock().unwrap()["stage"]=json!("prompt");
            let pending=cx.send_request(prompt(session.session_id.clone(),mode,&corr)).block_task();
            tokio::pin!(pending);
            let mut acted_at=None;
            loop {
                tokio::select! {
                    terminal=&mut pending => {
                        si.lock().unwrap().active=false;
                        ss.lock().unwrap()["terminal"]=safe_message(&json!({"result":terminal?}))["result"].clone();
                        ss.lock().unwrap()["stage"]=json!("terminal");
                        // 保持接收窗口，单独保留 terminal 后的 late updates。
                        tokio::time::sleep(Duration::from_millis(250)).await;
                        break;
                    }
                    _=tokio::time::sleep(Duration::from_millis(10))=>{
                        if acted_at.is_none() {
                            let rows=safe_rows(&sw);
                            let request=exchange(&rows,"session/prompt")["request"].clone();
                            let active=rows.iter().any(|r|r["message"]["method"]=="session/update" && r["message"]["params"]["sessionId"]==json!(session.session_id) && r["message"]["params"]["update"]["_meta"][META_KEY]==corr && r["sequence"].as_u64()>request["sequence"].as_u64());
                            let trigger=if mode=="before" {active && manifest(&scwd).is_ok_and(|m|m.is_empty())} else if mode=="after" {marker_ready(&scwd)} else {false};
                            if trigger && !request.is_null() && si.lock().unwrap().can_cancel(&session.session_id.to_string(),&corr) {
                                ss.lock().unwrap()["trigger"]=json!({"kind":if mode=="before"{"correlated_activity_and_zero_delta"}else{"actual_marker_exact_bytes"},"manifest":manifest(&scwd).ok(),"promptRpcId":request["message"]["id"],"observedThroughSequence":rows.last().map(|r|&r["sequence"])});
                                cx.send_notification(CancelNotification::new(session.session_id.clone()))?;
                                ss.lock().unwrap()["cancelEnqueued"]=json!(true);
                                acted_at=Some(Instant::now());
                            } else if mode=="permission" && !ss.lock().unwrap()["permissionDecisions"].as_array().unwrap().is_empty() {acted_at=Some(Instant::now());}
                        }
                        if acted_at.is_some_and(|t|t.elapsed()>=post_timeout) {ss.lock().unwrap()["stage"]=json!("no_terminal_timeout");si.lock().unwrap().active=false;break;}
                    }
                }
            }
            Ok(())
        });
    let deadline = if fake.is_some() {
        Duration::from_secs(4)
    } else {
        Duration::from_secs(150)
    };
    let outcome = tokio::time::timeout(deadline, operation).await;
    let error = match outcome {
        Ok(Ok(())) => None,
        Ok(Err(_)) => Some("SDK_ERROR_DETAILS_OMITTED"),
        Err(_) => Some("SCENARIO_TIMEOUT"),
    };
    identity.lock().unwrap().active = false;
    let first_wait = tokio::time::timeout(Duration::from_millis(300), child.wait())
        .await
        .ok();
    let cleanup = cleanup_child(&mut child, &mut stderr_task, first_wait).await;
    let after_result = manifest(&cwd);
    let after = after_result.as_ref().ok();
    let rows = safe_rows(&wire);
    let prompt_exchange = exchange(&rows, "session/prompt");
    let terminal_sequence = prompt_exchange["response"]["sequence"].as_u64();
    let updates=rows.iter().filter(|r|r["message"]["method"]=="session/update").map(|r|json!({"sequence":r["sequence"],"params":r["message"]["params"],"afterTerminal":terminal_sequence.is_some_and(|t|r["sequence"].as_u64().unwrap_or(0)>t)})).collect::<Vec<_>>();
    let typed = state.lock().unwrap().clone();
    let cancel = rows
        .iter()
        .find(|r| r["direction"] == "request" && r["message"]["method"] == "session/cancel");
    let mut report = json!({"scenario":mode,"fake":fake,"argv":std::iter::once(exe.to_string_lossy().to_string()).chain(argv).collect::<Vec<_>>(),"cwd":cwd,"pid":pid,"conversationRequestId":correlation,"identityDurableBeforePrompt":true,"protocol":typed,"prompt":prompt_exchange,"cancel":cancel,"wire":rows,"updates":updates,"before":before,"after":after,"delta":after.map(|a|delta(&before,a)),"manifestError":after_result.as_ref().err().map(|_|"MANIFEST_FAILED"),"markerRetained":marker_ready(&cwd),"error":error,"cleanup":cleanup,"elapsedMs":started.elapsed().as_millis(),"postActionTimeoutMs":post_timeout.as_millis(),"providerTerminalReceived":typed.get("terminal").is_some(),"runtimeTerminationEvidence":cleanup["directChildReaped"],"claimReleaseAuthorizedByCancel":false});
    report["sessionNew"] = exchange(&rows, "session/new");
    report["modeChange"] = exchange(&rows, "session/set_mode");
    report["sessionId"] = typed["sessionNew"]["sessionId"].clone();
    let deleted = workspace.close();
    report["workspaceDeleted"] = json!(deleted.is_ok());
    report["workspaceDeleteError"] = json!(deleted.err().map(|_| "DELETE_FAILED"));
    Ok(report)
}

/// Fake peer 实际读取 SDK wire；测试 cancel 后响应、无响应与 late update。
fn fake_peer(mode: &str) {
    use std::io::{BufRead, Write};
    let mut prompt_id = Value::Null;
    let mut correlation = Value::Null;
    for line in std::io::stdin().lock().lines() {
        let r: Value = serde_json::from_str(&line.unwrap()).unwrap();
        let method = r["method"].as_str().unwrap_or("");
        let result = match method {
            "initialize" => json!({"protocolVersion":1,"agentCapabilities":{}}),
            "session/new" => {
                if mode == "new-unknown" {
                    continue;
                }
                if mode == "new-failed" {
                    println!(
                        "{}",
                        json!({"jsonrpc":"2.0","id":r["id"],"error":{"code":-32603,"message":"fixture failure"}})
                    );
                    std::io::stdout().flush().unwrap();
                    continue;
                }
                // notification 穿插在 request/response 中间，客户端只能按 RPC id 配对。
                println!(
                    "{}",
                    json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"fake-session","update":{"sessionUpdate":"current_mode_update","currentModeId":"default"}}})
                );
                json!({"sessionId":"fake-session","modes":{"currentModeId":"default","availableModes":if mode=="missing-auto"{json!([{"id":"default","name":"Always Ask"}])}else{json!([{"id":"default","name":"Always Ask"},{"id":"auto","name":"Auto"}])}}})
            }
            "session/set_mode" => {
                assert_eq!(r["params"]["sessionId"], "fake-session");
                assert_eq!(r["params"]["modeId"], "auto");
                json!({})
            }
            "session/prompt" => {
                prompt_id = r["id"].clone();
                if mode == "mode-order" {
                    std::fs::write("before-cancel.txt", MARKER).unwrap();
                }
                correlation = r["params"]["_meta"][META_KEY].clone();
                if mode.starts_with("permission") {
                    println!(
                        "{}",
                        json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"fake-session","update":{"sessionUpdate":"tool_call","toolCallId":"fake-tool","title":"fixture","status":"pending","_meta":{META_KEY:correlation}}}})
                    );
                    println!(
                        "{}",
                        json!({"jsonrpc":"2.0","id":"permission-rpc","method":"session/request_permission","params":{"sessionId":"fake-session","toolCall":{"toolCallId":"fake-tool"},"options":[{"optionId":"allow","name":"Deny","kind":"allow_always"},{"optionId":"reject-advertised","name":"Allow","kind":"reject_once"}]}})
                    );
                }
                println!(
                    "{}",
                    json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"fake-session","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"active"},"_meta":{META_KEY:correlation}}}})
                );
                std::io::stdout().flush().unwrap();
                continue;
            }
            "session/cancel" => {
                if mode == "timeout" {
                    continue;
                }
                println!(
                    "{}",
                    json!({"jsonrpc":"2.0","id":prompt_id,"result":{"stopReason":"cancelled"}})
                );
                println!(
                    "{}",
                    json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"fake-session","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"late"},"_meta":{META_KEY:correlation}}}})
                );
                std::io::stdout().flush().unwrap();
                continue;
            }
            "" if r["id"] == "permission-rpc" => {
                assert_eq!(r["result"]["outcome"]["optionId"], "reject-advertised");
                if mode == "permission-terminal" {
                    println!(
                        "{}",
                        json!({"jsonrpc":"2.0","id":prompt_id,"result":{"stopReason":"end_turn"}})
                    );
                    std::io::stdout().flush().unwrap();
                }
                continue;
            }
            _ => continue,
        };
        println!("{}", json!({"jsonrpc":"2.0","id":r["id"],"result":result}));
        std::io::stdout().flush().unwrap();
    }
}

/// 命令只提供固定探针与 fake；结果只写调用方提供的 task-local 路径。
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().collect::<Vec<_>>();
    if args.get(1).map(String::as_str) == Some("--fake") {
        fake_peer(&args[2]);
        return Ok(());
    }
    let mode = &args[1];
    assert!(["before", "after", "permission", "diagnostic"].contains(&mode.as_str()));
    let output = Path::new(&args[2]);
    let report = probe(mode, output, args.get(3).map(String::as_str)).await?;
    std::fs::write(output, serde_json::to_string_pretty(&report)?)?;
    if report["cleanup"]["succeeded"] != true || report["workspaceDeleted"] != true {
        std::process::exit(2);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    /// 真实 launch 始终只包含 canonical entry 和 --acp。
    #[test]
    fn real_argv_is_canonical_baseline() {
        let argv = canonical_argv();
        assert_eq!(argv.len(), 2);
        assert_eq!(argv[1], "--acp");
        assert!(
            !argv
                .iter()
                .any(|s| ["--tools", "--settings", "--permission-mode"].contains(&s.as_str()))
        );
    }
    /// 不把字符串 auto 硬编码成未广告的能力；实际 catalog 缺失就停。
    #[test]
    fn auto_mode_must_come_from_response_catalog() {
        let mut s = NewSessionResponse::new("s");
        assert!(advertised_auto(&s).is_none());
        s.modes = Some(SessionModeState::new(
            "default",
            vec![SessionMode::new("other", "auto")],
        ));
        assert!(advertised_auto(&s).is_none());
        s.modes = Some(SessionModeState::new(
            "default",
            vec![SessionMode::new("auto", "Actual auto mode")],
        ));
        assert_eq!(advertised_auto(&s).unwrap().to_string(), "auto");
    }
    /// 构造真正 typed 权限请求，不依赖标签文字。
    fn request() -> RequestPermissionRequest {
        RequestPermissionRequest::new(
            "s",
            ToolCallUpdate::new("tool", ToolCallUpdateFields::default()),
            vec![PermissionOption::new(
                "deny-id",
                "Allow always misleading label",
                PermissionOptionKind::RejectOnce,
            )],
        )
    }
    /// 只接受 active/current identity，错误身份绝不进入发送分支。
    #[test]
    fn cancel_identity_mismatch_does_not_send() {
        let i = Identity {
            session: "s".into(),
            correlation: "c".into(),
            active: true,
            ..Default::default()
        };
        let mut sent = 0;
        for (s, c) in [("wrong", "c"), ("s", "wrong")] {
            if i.can_cancel(s, c) {
                sent += 1;
            }
        }
        assert_eq!(sent, 0);
        assert!(i.can_cancel("s", "c"));
    }
    /// session 与 tool 分别必须匹配；标签不能驱动权限选择。
    #[test]
    fn permission_identity_and_typed_kind() {
        let mut i = Identity {
            session: "wrong".into(),
            active: true,
            ..Default::default()
        };
        let r = request();
        assert!(i.deny(&r).is_err());
        i.session = "s".into();
        assert!(i.deny(&r).is_err());
        i.tools.insert("tool".into());
        let response = serde_json::to_value(i.deny(&r).unwrap()).unwrap();
        assert_eq!(response["outcome"]["optionId"], "deny-id");
    }
    /// SDK responder 消耗所有权，额外 identity guard 阻止重复/late 业务响应。
    #[test]
    fn duplicate_and_late_responder_fail_closed() {
        let mut i = Identity {
            session: "s".into(),
            active: true,
            tools: BTreeSet::from(["tool".into()]),
            ..Default::default()
        };
        assert!(i.deny(&request()).is_ok());
        assert_eq!(i.deny(&request()).unwrap_err(), "DUPLICATE_RESPONDER");
        i.responded.clear();
        i.active = false;
        assert!(i.deny(&request()).is_err());
    }
    /// 外部 malformed schema 与空/重复选项 ID 拒绝；不生成猜测响应。
    #[test]
    fn malformed_permission_option_and_response() {
        assert!(
            serde_json::from_value::<PermissionOption>(
                json!({"optionId":"x","name":"x","kind":"bogus"})
            )
            .is_err()
        );
        assert!(
            serde_json::from_value::<RequestPermissionResponse>(
                json!({"outcome":{"outcome":"selected"}})
            )
            .is_err()
        );
        let mut i = Identity {
            session: "s".into(),
            active: true,
            tools: BTreeSet::from(["tool".into()]),
            ..Default::default()
        };
        let mut r = request();
        r.options[0].option_id = PermissionOptionId::new("");
        assert!(i.deny(&r).is_err());
        r = request();
        r.options.push(r.options[0].clone());
        assert!(i.deny(&r).is_err());
        r = request();
        r.options[0].kind = PermissionOptionKind::AllowAlways;
        assert!(i.deny(&r).is_err());
    }
    /// 时间经过、错误内容、部分写入都不能触发；只有磁盘 exact bytes/hash 有效。
    #[test]
    fn after_trigger_requires_actual_marker_and_hash() {
        let root = tempfile::tempdir().unwrap();
        assert!(!marker_ready(root.path()));
        std::fs::write(
            root.path().join("before-cancel.txt"),
            b"SERENA_CB5_004_SIDE_EFFECT",
        )
        .unwrap();
        assert!(!marker_ready(root.path()));
        std::fs::write(root.path().join("before-cancel.txt"), MARKER).unwrap();
        assert!(marker_ready(root.path()));
        assert_eq!(
            format!("{:x}", Sha256::digest(MARKER)),
            "613841732b16579f9417ffa2ffeb2a86dab63957b0c8381976403c6ee4960301"
        );
    }
    /// 隐藏文件和目录也纳入 manifest，不能只检查 marker。
    #[test]
    fn manifest_tracks_hidden_paths() {
        let root = tempfile::tempdir().unwrap();
        let before = manifest(root.path()).unwrap();
        std::fs::create_dir(root.path().join(".hidden")).unwrap();
        std::fs::write(root.path().join(".hidden/file"), b"x").unwrap();
        assert_eq!(
            delta(&before, &manifest(root.path()).unwrap()),
            vec![".hidden", ".hidden/file"]
        );
    }
    /// wait 注入错误仍需 kill/wait 与 stderr abort，外层验证期限。
    #[tokio::test]
    async fn cleanup_wait_failure_bounded() {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .arg("--list")
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let mut stderr = tokio::spawn(std::future::pending::<Vec<u8>>());
        let r = tokio::time::timeout(
            Duration::from_secs(9),
            cleanup_child(
                &mut child,
                &mut stderr,
                Some(Err(io::Error::other("injected wait"))),
            ),
        )
        .await
        .unwrap();
        assert_eq!(r["succeeded"], false);
        assert_eq!(r["directChildReaped"], true);
        assert_eq!(r["stderrJoined"], true);
    }
    /// kill 注入错误仍 wait；不得因错误直接跳过清理。
    #[tokio::test]
    async fn cleanup_kill_failure_bounded() {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .arg("--list")
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let mut stderr = tokio::spawn(async { Vec::new() });
        let r = tokio::time::timeout(
            Duration::from_secs(8),
            cleanup_with_kill(&mut child, &mut stderr, None, |_| {
                Err(io::Error::other("injected kill"))
            }),
        )
        .await
        .unwrap();
        assert_eq!(r["succeeded"], false);
        assert_eq!(r["directChildReaped"], true);
    }
    /// 永久 evidence 不得携带 prompt/source/command 文本。
    #[test]
    fn evidence_whitelist_redacts_content() {
        let r = safe_message(
            &json!({"params":{"sessionId":"s","prompt":"secret","toolCall":{"toolCallId":"t","rawInput":{"command":"secret"}},"options":[{"optionId":"d","kind":"reject_once","name":"secret"}]}}),
        );
        assert!(!r.to_string().contains("secret"));
        assert_eq!(r["params"]["options"][0]["optionId"], "d");
    }
}
