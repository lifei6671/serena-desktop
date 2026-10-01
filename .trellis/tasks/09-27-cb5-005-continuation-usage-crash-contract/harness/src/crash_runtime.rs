//! Stage C独立Host runner；R2只有typed load历史检查，绝不发送模型prompt。
use super::*;

/// 单runtime的生命周期，退出路径统一执行bounded kill/wait/drain。
pub async fn run_runtime(
    cwd: &Path,
    mode: &str,
    recovery: Option<&Value>,
    identity_path: &Path,
    launcher: &[String],
    deadline: Duration,
) -> io::Result<Value> {
    identity_guard("workspace", "workspace", cwd, cwd)?;
    let expected = recovery.map(|r| r["sessionId"].as_str().unwrap_or("").to_string());
    if expected.as_ref().is_some_and(|s| !valid_id(s)) {
        return Err(io::Error::other("INVALID_SESSION"));
    }
    let corr = recovery
        .map(|r| r["requestId"].as_str().unwrap_or("").to_string())
        .unwrap_or_else(|| uuid::Uuid::now_v7().simple().to_string());
    let runtime_id = uuid::Uuid::now_v7().to_string();
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
        let mut n = 0usize;
        while let Ok(count) = stderr.read(&mut b).await {
            if count == 0 {
                break;
            }
            n = n.saturating_add(count);
        }
        vec![0; n.min(8192)]
    });
    let state = Arc::new(Mutex::new(
        json!({"stage":"initialize","permissionRequests":0}),
    ));
    let (ss, ps, sw) = (state.clone(), state.clone(), wire.clone());
    let (scwd, path, request, recover) = (
        cwd.to_path_buf(),
        identity_path.to_path_buf(),
        corr.clone(),
        expected.clone(),
    );
    let operation=Client.builder().name("cb5-005-crash")
        .on_receive_notification(async move |_n:SessionNotification,_cx|{Ok(())},agent_client_protocol::on_receive_notification!())
        .on_receive_request(async move |r:RequestPermissionRequest,responder,_cx|{
            let mut s=ps.lock().unwrap();let count=s["permissionRequests"].as_u64().unwrap_or(0);s["permissionRequests"]=json!(count+1);drop(s);
            if let Some(o)=r.options.iter().find(|o|o.kind==PermissionOptionKind::RejectOnce){responder.respond(RequestPermissionResponse::new(RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(o.option_id.clone()))))?;}
            else{responder.respond_with_internal_error("READ_ONLY_PROBE")?;}Ok(())
        },agent_client_protocol::on_receive_request!())
        .connect_with(ByteStreams::new(outgoing,incoming),async move |cx|{
            let raw=cx.send_request(agent_client_protocol::UntypedMessage::new("initialize",json!({"protocolVersion":1,"clientCapabilities":{"elicitation":{"form":{}},"_meta":{"subagent-transcript":true,"parameterizedModelPicker":true}},"clientInfo":{"name":"serena-desktop-gold-band-repro","title":"SerenaDesktop Gold Band Repro","version":"0.1"}}))?).block_task().await?;
            let init:InitializeResponse=serde_json::from_value(raw)?;
            if init.protocol_version!=ProtocolVersion::V1{return Err(agent_client_protocol::Error::internal_error());}
            ss.lock().unwrap()["protocolVersion"]=json!(1);
            if let Some(sid)=recover {
                ss.lock().unwrap()["sessionId"]=json!(&sid);ss.lock().unwrap()["stage"]=json!("load");
                cx.send_request(LoadSessionRequest::new(sid.clone(),scwd.clone()).mcp_servers(vec![])).block_task().await?;
                // bounded late drain仍只读取历史，不发P2/其他prompt。
                tokio::time::sleep(Duration::from_millis(250)).await;
                let (rows,incomplete)=crash::snapshot(&sw).map_err(|_|agent_client_protocol::Error::internal_error())?;
                if incomplete{return Err(agent_client_protocol::Error::internal_error());}
                recovery_guard(&rows,"session/load",&sid,&scwd,&scwd).map_err(|_|agent_client_protocol::Error::internal_error())?;
                ss.lock().unwrap()["stage"]=json!("history_inspected");
            } else {
                let sid=cx.send_request(NewSessionRequest::new(scwd.clone()).mcp_servers(vec![])).block_task().await?.session_id.to_string();
                if !valid_id(&sid){return Err(agent_client_protocol::Error::internal_error());}
                // write-ahead身份在任何prompt字节之前durable；没有prompt/正文/terminal/result。
                durable(&path,&json!({"sessionId":sid,"conversationRequestId":request,"requestId":request,"promptState":"prepared","promptRpcId":null,"runtimeTerminationEvidenceProven":false,"claimReleasePermitted":false})).map_err(|_|agent_client_protocol::Error::internal_error())?;
                ss.lock().unwrap()["sessionId"]=json!(&sid);ss.lock().unwrap()["stage"]=json!("prompt");
                let prompt=PromptRequest::new(sid,vec![ContentBlock::Text(TextContent::new("Read-only probe. Do not use tools, run commands, or read or modify any files. Explain in approximately twenty sentences why different shapes have different properties."))]).meta(serde_json::Map::from_iter([(META.into(),json!(request)),(crash::REQUEST.into(),json!(request))]));
                cx.send_request(prompt).block_task().await?;
                ss.lock().unwrap()["stage"]=json!("terminal_received");
            }
            Ok(())
        });
    let monitor = async {
        loop {
            tokio::time::sleep(Duration::from_millis(2)).await;
            let s = state.lock().unwrap().clone();
            if let Some(sid) = s["sessionId"].as_str() {
                let (rows, _) = crash::snapshot(&wire)?;
                if let Some(decision) = crash::trigger(&rows, sid, &corr)? {
                    return Ok::<_, io::Error>(decision);
                }
            }
        }
    };
    // single-thread select后立即进入cleanup；drop SDK未来，禁止在trigger之后继续pump。
    let before = mode == "crash-before-terminal" && recovery.is_none();
    let outcome = tokio::time::timeout(deadline, async {
        tokio::select! {biased;
            r=operation=>if r.is_ok(){"OPERATION_FINISHED"}else{"SDK_OR_CONTRACT_ERROR"},
            r=monitor,if before=>r.unwrap_or("OBSERVATION_ERROR")
        }
    })
    .await
    .unwrap_or("RUNTIME_TIMEOUT");
    // after-terminal此处在写任何terminal/result证据之前终止R1。
    let cleanup = cleanup_child(&mut child, &mut drain, None).await;
    let state = state.lock().unwrap().clone();
    let sid = state["sessionId"].as_str().unwrap_or("");
    let mut report = json!({"sessionId":sid,"requestId":corr,"conversationRequestId":corr,"runtimeInstanceId":runtime_id,"pid":pid,"outcome":outcome,"state":state,"cleanup":cleanup,"windowsJobAtCreationProven":false,"runtimeTerminationEvidenceProven":false,"claimReleasePermitted":false});
    match crash::snapshot(&wire) {
        Ok((rows, incomplete)) => {
            report["incompleteTrailingFrame"] = json!(incomplete);
            report["projection"] = crash::project(
                &rows,
                sid,
                &corr,
                recovery.is_some(),
                recovery.map(|r| &r["projection"]),
            )?;
            let (q, a) = exchange(
                &rows,
                if recovery.is_some() {
                    "session/load"
                } else {
                    "session/new"
                },
            );
            report["sessionExchange"] = json!({"method":if recovery.is_some(){"session/load"}else{"session/new"},"safeParams":q.map(|q|json!({"sessionId":q["message"]["params"].get("sessionId"),"cwd":q["message"]["params"]["cwd"],"mcpServers":q["message"]["params"].get("mcpServers")})),"rpcId":q.map(|q|&q["message"]["id"]),"responseSequence":a.map(|a|&a["sequence"]),"responseSessionIdMatchedOrAbsent":a.map(|a|a["message"]["result"].get("sessionId").is_none_or(|v|v==sid)),"responseCatalog":a.map(|a|catalog(&a["message"]["result"])),"rpcErrorCode":a.and_then(|a|a["message"]["error"]["code"].as_i64())});
            report["promptRequestCount"] = json!(
                rows.iter()
                    .filter(|r| r["direction"] == "request"
                        && r["message"]["method"] == "session/prompt")
                    .count()
            );
            report["beforeWindowObserved"] = json!(
                before
                    && outcome == "ACTIVITY_WITHOUT_TERMINAL"
                    && exchange(&rows, "session/prompt").1.is_none()
            );
        }
        Err(_) => report["projectionError"] = json!("FRAME_ERROR"),
    }
    Ok(report)
}

/// 成功采样与Result completeness分离；cleanup/identity失败不允许PASS。
pub fn grade(mode: &str, r1: &Value, r2: &Value, workspace_ok: bool) -> &'static str {
    if r1["cleanup"]["succeeded"] != true || !workspace_ok {
        return "PARTIAL";
    }
    if mode == "crash-before-terminal" && r1["beforeWindowObserved"] != true {
        return if r1["projection"]["terminalSequence"].is_number() {
            "NOT_OBSERVED"
        } else {
            "PARTIAL"
        };
    }
    if r2["outcome"] != "OPERATION_FINISHED"
        || r2["state"]["stage"] != "history_inspected"
        || r2["cleanup"]["succeeded"] != true
        || r2["promptRequestCount"] != 0
    {
        return "PARTIAL";
    }
    for r in [r1, r2] {
        if r["projection"]["rejectedUpdates"] != 0
            || r["projection"]["toolActivityCount"] != 0
            || r["projection"]["materialContractDifference"] != false
            || r["projectionError"].is_string()
        {
            return "PARTIAL";
        }
    }
    if r2["projection"]["boundHistoryCount"].as_u64().unwrap_or(0) == 0 {
        return "PARTIAL";
    }
    if mode == "crash-after-terminal"
        && (r1["projection"]["terminalExactConversation"] != true
            || r1["projection"]["terminalStopReason"] != "end_turn"
            || r1["projection"]["answerLength"].as_u64().unwrap_or(0) == 0
            || r2["projection"]["answerMatchesLive"] != true
            || r2["projection"]["missingMessageIdCount"] != 0
            || r1["projection"]["missingMessageIdCount"] != 0
            || r2["projection"]["messageIdSetMatchesLive"] != true)
    {
        return "PARTIAL";
    }
    "PASS"
}

/// 每mode独立fresh root；只有R1有效窗口和reap完成才启动R2历史读取。
pub async fn scenario(
    mode: &str,
    root: &Path,
    launcher: &[String],
    deadline: Duration,
) -> io::Result<Value> {
    let cwd = tempfile::Builder::new()
        .prefix(&format!("cb5-005-{mode}-"))
        .tempdir()?
        .keep();
    identity_guard("workspace", "workspace", &cwd, &cwd)?;
    let before = manifest(&cwd)?;
    let mut report = json!({"scenario":mode,"status":"PARTIAL","cwd":cwd,"before":before,"r2Status":"NOT_RUN","resultCompleteness":"unknown","businessCompletedRecoverable":false,"windowsJobAtCreationProven":false,"runtimeTerminationEvidenceProven":false,"claimReleasePermitted":false,"productionExecutionResultPersisted":false});
    let r1 = run_runtime(
        &cwd,
        mode,
        None,
        &root.join(format!("{mode}.prompt-identity.json")),
        launcher,
        deadline,
    )
    .await;
    let middle = manifest(&cwd);
    if let Ok(r1) = r1 {
        let ready = r1["cleanup"]["succeeded"] == true
            && middle.as_ref().is_ok_and(|m| m == &before)
            && if mode == "crash-before-terminal" {
                r1["beforeWindowObserved"] == true
            } else {
                r1["outcome"] == "OPERATION_FINISHED"
                    && r1["projection"]["terminalExactConversation"] == true
                    && r1["projection"]["terminalStopReason"] == "end_turn"
            };
        if ready {
            match run_runtime(
                &cwd,
                mode,
                Some(&r1),
                Path::new("unused"),
                launcher,
                deadline,
            )
            .await
            {
                Ok(r2) => {
                    report["resultCompleteness"] = json!(
                        r2["projection"]["resultCompleteness"]
                            .as_str()
                            .unwrap_or("unknown")
                    );
                    report["r2"] = r2;
                    report["r2Status"] = json!("INSPECTED");
                }
                Err(_) => report["r2Error"] = json!("RUNTIME_FAILED"),
            }
        }
        report["r1"] = r1;
    } else {
        report["r1Error"] = json!("RUNTIME_FAILED");
    }
    let after = manifest(&cwd);
    let workspace_ok =
        middle.as_ref().is_ok_and(|m| m == &before) && after.as_ref().is_ok_and(|m| m == &before);
    report["afterR1"] = json!(middle.as_ref().ok());
    report["afterR2"] = json!(after.as_ref().ok());
    report["manifestComplete"] = json!(middle.is_ok() && after.is_ok());
    report["workspaceDelta"] = json!(after.as_ref().ok().map(|m| {
        before
            .keys()
            .chain(m.keys())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .filter(|k| before.get(*k) != m.get(*k))
            .collect::<Vec<_>>()
    }));
    // 只移除确认为空的目录，不递归清理任何异常内容或reparse。
    let deleted = after.is_ok_and(|m| m.is_empty()) && std::fs::remove_dir(&cwd).is_ok();
    report["workspaceDeleted"] = json!(deleted);
    report["status"] = json!(grade(
        mode,
        &report["r1"],
        &report["r2"],
        workspace_ok && deleted
    ));
    Ok(report)
}

/// 编译时固定证据路径，不接受output重定向。
pub fn output_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("evidence/crash")
}

/// 唯一真实入口；父Host后续每mode各一次，本轮禁止调用。
pub async fn host_run(mode: &str) -> i32 {
    let root = output_root();
    let run=async {
        crash::reserve(&root,mode)?;
        let launcher=vec![NODE.into(),SCRIPT.into(),"--acp".into()];
        let report=scenario(mode,&root,&launcher,Duration::from_secs(120)).await.unwrap_or_else(|_|json!({"scenario":mode,"status":"PARTIAL","error":"SCENARIO_SETUP_FAILED","runtimeTerminationEvidenceProven":false,"claimReleasePermitted":false}));
        durable(&root.join(format!("{mode}.result.json")),&report)?;
        println!("{}",json!({"scenario":mode,"status":report["status"],"resultCompleteness":report["resultCompleteness"]}));
        Ok::<_,io::Error>(report["status"]=="PASS")
    }.await;
    match run {
        Ok(true) => 0,
        Ok(false) => 2,
        Err(_) => {
            eprintln!("CRASH_SENTINEL_OR_EVIDENCE_FAILED_NO_RETRY");
            2
        }
    }
}
