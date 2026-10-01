//! 单个 owned child 的有界生命周期，复用冻结 initialize/new dispatcher。
use super::*;

/// CB5-004 已证明 shape；仅 initialize 用 SDK 公共 UntypedMessage 精确保留扩展。
fn initialize() -> Value {
    json!({"protocolVersion":1,"clientCapabilities":{"elicitation":{"form":{}},"_meta":{"subagent-transcript":true,"parameterizedModelPicker":true}},"clientInfo":{"name":"serena-desktop-gold-band-repro","title":"SerenaDesktop Gold Band Repro","version":"0.1"}})
}

/// 每个 runtime 只能 new 或指定 recovery 一次；测试仅通过内部参数注入 fake peer。
pub async fn runtime(
    cwd: &Path,
    recovery: Option<(&str, &str)>,
    token: &str,
    launcher: &[String],
    timeout: Duration,
) -> io::Result<Value> {
    no_links(cwd)?;
    let mut child = Command::new(&launcher[0])
        .args(&launcher[1..])
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let pid = child.id();
    let wire: Wire = Arc::default();
    let outgoing = Tee {
        inner: child.stdin.take().unwrap().compat_write(),
        direction: "request",
        wire: wire.clone(),
        line_bytes: 0,
        frame_count: 0,
    };
    let incoming = Tee {
        inner: child.stdout.take().unwrap().compat(),
        direction: "response",
        wire: wire.clone(),
        line_bytes: 0,
        frame_count: 0,
    };
    let mut stderr = child.stderr.take().unwrap();
    let mut drain = tokio::spawn(async move {
        let mut b = [0; 8192];
        let mut count = 0usize;
        while let Ok(n) = stderr.read(&mut b).await {
            if n == 0 {
                break;
            }
            count = count.saturating_add(n);
        }
        vec![0; count.min(8192)]
    });
    let state = Arc::new(Mutex::new(json!({"stage":"initialize"})));
    let ss = state.clone();
    let sw = wire.clone();
    let scwd = cwd.to_path_buf();
    let recovery = recovery.map(|(m, s)| (m.to_owned(), s.to_owned()));
    let method = recovery
        .as_ref()
        .map(|r| format!("session/{}", r.0))
        .unwrap_or("session/new".into());
    let expected = recovery.as_ref().map(|r| r.1.clone()).unwrap_or_default();
    let method_for_run = method.clone();
    let token_owned = token.to_owned();
    let corr = uuid::Uuid::now_v7().simple().to_string();
    let correlation = corr.clone();
    let operation=Client.builder().name("cb5-005-continuation")
        .on_receive_notification(async move |_n:SessionNotification,_cx| {Ok(())},agent_client_protocol::on_receive_notification!())
        .on_receive_request(async move |r:RequestPermissionRequest,responder,_cx| {
            // 只读场景不授权工具；typed reject_once 必须来自 Provider 实际选项。
            if let Some(o)=r.options.iter().find(|o|o.kind==PermissionOptionKind::RejectOnce) {
                responder.respond(RequestPermissionResponse::new(RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(o.option_id.clone()))))?;
            } else {responder.respond_with_internal_error("READ_ONLY_PROBE")?;}
            Ok(())
        },agent_client_protocol::on_receive_request!())
        .connect_with(ByteStreams::new(outgoing,incoming),async move |cx| {
            let raw=cx.send_request(agent_client_protocol::UntypedMessage::new("initialize",initialize())?).block_task().await?;
            let init:InitializeResponse=serde_json::from_value(raw.clone())?;
            ss.lock().unwrap()["initialize"]=json!({"protocolVersion":raw["protocolVersion"].as_u64(),"loadSession":raw["agentCapabilities"]["loadSession"].as_bool()});
            if init.protocol_version!=ProtocolVersion::V1 {ss.lock().unwrap()["stage"]=json!("UNSUPPORTED_PROTOCOL");return Ok(());}
            ss.lock().unwrap()["stage"]=json!(&method_for_run);
            let sid=if let Some((m,s))=&recovery {
                identity_guard(s,s,&scwd,&scwd).map_err(|_|agent_client_protocol::Error::internal_error())?;
                match m.as_str() {
                    "resume"=>{cx.send_request(ResumeSessionRequest::new(s.clone(),scwd.clone()).mcp_servers(vec![])).block_task().await?;}
                    "load"=>{cx.send_request(LoadSessionRequest::new(s.clone(),scwd.clone()).mcp_servers(vec![])).block_task().await?;}
                    _=>{return Err(agent_client_protocol::Error::internal_error());}
                } s.clone()
            } else {
                let response=cx.send_request(NewSessionRequest::new(scwd.clone()).mcp_servers(vec![])).block_task().await?;
                response.session_id.to_string()
            };
            // 外部身份先校验，再进入任何可持久化状态；错误 ID 可能携带正文。
            if !valid_id(&sid) {
                ss.lock().unwrap()["stage"]=json!("INVALID_SESSION_ID");
                return Err(agent_client_protocol::Error::internal_error());
            }
            ss.lock().unwrap()["sessionId"]=json!(&sid);
            // 有界恢复后窗口；无关联的迟到 replay 不计 P2 answer。
            tokio::time::sleep(Duration::from_millis(250)).await;
            let rows=frames(&sw).map_err(|_|agent_client_protocol::Error::internal_error())?;
            if recovery.is_some() {recovery_guard(&rows,&method_for_run,&sid,&scwd,&scwd).map_err(|_|agent_client_protocol::Error::internal_error())?;}
            else {
                identity_guard(&sid,&sid,&scwd,&scwd).map_err(|_|agent_client_protocol::Error::internal_error())?;
                if rows.iter().any(|r|r["message"]["method"]=="session/update" && r["message"]["params"]["sessionId"]!=sid) {return Err(agent_client_protocol::Error::internal_error());}
            }
            let text=if recovery.is_none() {format!("Read-only memory test. Do not use tools or read/write files. Remember this exact memory token in this conversation only: {token_owned}. Reply exactly STORED.")}
                else {"Read-only memory test. Do not use tools or read/write files. Return only the exact memory token I asked you to remember earlier in this conversation. Do not invent or replace it.".into()};
            ss.lock().unwrap()["stage"]=json!("prompt");
            let request=PromptRequest::new(sid,vec![ContentBlock::Text(TextContent::new(text))])
                .meta(serde_json::Map::from_iter([(META.into(),json!(correlation))]));
            let terminal=cx.send_request(request).block_task().await?;
            ss.lock().unwrap()["stage"]=json!("terminal");
            ss.lock().unwrap()["stopReason"]=serde_json::to_value(terminal.stop_reason)?;
            tokio::time::sleep(Duration::from_millis(250)).await; Ok(())
        });
    let error = match tokio::time::timeout(timeout, operation).await {
        Ok(Ok(())) => None,
        Ok(Err(_)) => Some("SDK_OR_CONTRACT_ERROR"),
        Err(_) => Some("RUNTIME_TIMEOUT"),
    };
    // 所有路径先 kill/reap，再分析；绝不把 child 回收当 Job/tree evidence。
    let cleanup = cleanup_child(&mut child, &mut drain, None).await;
    let state = state.lock().unwrap().clone();
    let sid = state["sessionId"]
        .as_str()
        .or_else(|| valid_id(&expected).then_some(expected.as_str()))
        .unwrap_or("");
    let mut report = match frames(&wire) {
        Ok(rows) => summarize(&rows, &method, sid, &corr, token),
        Err(_) => json!({"frameError":true}),
    };
    report["runtimeInstanceId"] = json!(uuid::Uuid::now_v7().to_string());
    report["pid"] = json!(pid);
    report["cleanup"] = cleanup;
    report["state"] = state;
    report["error"] = json!(error);
    Ok(report)
}
