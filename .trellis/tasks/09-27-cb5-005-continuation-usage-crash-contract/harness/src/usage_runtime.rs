//! Stage B 自有生命周期；Stage A runtime/transport/evidence保持冻结。
use super::*;

/// 与Host已冻结初始化参数保持一致，仅使用SDK扩展请求保留形状。
fn initialize_params() -> Value {
    json!({"protocolVersion":1,"clientCapabilities":{"elicitation":{"form":{}},"_meta":{"subagent-transcript":true,"parameterizedModelPicker":true}},"clientInfo":{"name":"serena-desktop-gold-band-repro","title":"SerenaDesktop Gold Band Repro","version":"0.1"}})
}

/// 逐turn只读输入；文本仅内存/pipe，不进入任何报告。
fn prompt_text(turn: usize) -> &'static str {
    match turn {
        1 => {
            "Read-only probe. Do not use any tools or access or modify files. In one short sentence, explain why the sky appears blue."
        }
        2 => {
            "Read-only probe. Do not use any tools or access or modify files. In three short sentences, explain the difference between a triangle and a square."
        }
        _ => {
            "Read-only probe. Do not use any tools or access or modify files. In two short sentences, describe the difference between a circle and a triangle."
        }
    }
}

/// raw updates必须typed且属于精确S1；失败不进入下一个prompt。
fn verify_updates(rows: &[Value], sid: &str) -> Result<(), agent_client_protocol::Error> {
    for row in rows {
        if row["message"]["method"] == "session/update" {
            let n: SessionNotification = serde_json::from_value(row["message"]["params"].clone())?;
            if n.session_id.to_string() != sid {
                return Err(agent_client_protocol::Error::internal_error());
            }
        }
    }
    Ok(())
}

/// 单runtime：R1连续P1/P2，R2只typed resume/P3，绝无load分支。
pub async fn run_runtime(
    cwd: &Path,
    session: Option<&str>,
    launcher: &[String],
    deadline: Duration,
) -> io::Result<Value> {
    no_links(cwd)?;
    if let Some(sid) = session {
        identity_guard(sid, sid, cwd, cwd)?;
    }
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
    let state = Arc::new(Mutex::new(json!({"stage":"initialize","manifests":[]})));
    let ss = state.clone();
    let sw = wire.clone();
    let scwd = cwd.to_path_buf();
    let expected = session.map(String::from);
    let recover = expected.clone();
    let runtime_index = if session.is_some() { 2 } else { 1 };
    let first_turn = if session.is_some() { 3 } else { 1 };
    let operation=Client.builder().name("cb5-005-usage")
        .on_receive_notification(async move |_n:SessionNotification,_cx|{Ok(())},agent_client_protocol::on_receive_notification!())
        .on_receive_request(async move |r:RequestPermissionRequest,responder,_cx|{
            // 只读probe不授予任何工具权限，选择typed广告reject_once。
            if let Some(o)=r.options.iter().find(|o|o.kind==PermissionOptionKind::RejectOnce){responder.respond(RequestPermissionResponse::new(RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(o.option_id.clone()))))?;}
            else{responder.respond_with_internal_error("READ_ONLY_PROBE")?;}Ok(())
        },agent_client_protocol::on_receive_request!())
        .connect_with(ByteStreams::new(outgoing,incoming),async move |cx|{
            let raw=cx.send_request(agent_client_protocol::UntypedMessage::new("initialize",initialize_params())?).block_task().await?;
            let init:InitializeResponse=serde_json::from_value(raw.clone())?;
            ss.lock().unwrap()["initialize"]=json!({"protocolVersion":raw["protocolVersion"].as_u64(),"loadSession":raw["agentCapabilities"]["loadSession"].as_bool()});
            if init.protocol_version!=ProtocolVersion::V1{ss.lock().unwrap()["stage"]=json!("UNSUPPORTED_PROTOCOL");return Err(agent_client_protocol::Error::internal_error());}
            ss.lock().unwrap()["stage"]=json!(if recover.is_some(){"resume"}else{"new"});
            let sid=if let Some(sid)=recover.as_ref(){
                cx.send_request(ResumeSessionRequest::new(sid.clone(),scwd.clone()).mcp_servers(vec![])).block_task().await?;sid.clone()
            }else{cx.send_request(NewSessionRequest::new(scwd.clone()).mcp_servers(vec![])).block_task().await?.session_id.to_string()};
            if !valid_id(&sid){ss.lock().unwrap()["stage"]=json!("INVALID_SESSION_ID");return Err(agent_client_protocol::Error::internal_error());}
            ss.lock().unwrap()["sessionId"]=json!(&sid);
            tokio::time::sleep(Duration::from_millis(250)).await;
            let rows=frames(&sw).map_err(|_|agent_client_protocol::Error::internal_error())?;
            verify_updates(&rows,&sid)?;
            if recover.is_some(){recovery_guard(&rows,"session/resume",&sid,&scwd,&scwd).map_err(|_|agent_client_protocol::Error::internal_error())?;}
            for turn in if recover.is_some(){3..=3}else{1..=2}{
                identity_guard(&sid,&sid,&scwd,&scwd).map_err(|_|agent_client_protocol::Error::internal_error())?;
                let before=manifest(&scwd).map_err(|_|agent_client_protocol::Error::internal_error())?;
                if !before.is_empty(){ss.lock().unwrap()["stage"]=json!("WORKSPACE_CHANGED");return Err(agent_client_protocol::Error::internal_error());}
                ss.lock().unwrap()["stage"]=json!(format!("P{turn}"));
                let corr=uuid::Uuid::now_v7().simple().to_string();
                let request=PromptRequest::new(sid.clone(),vec![ContentBlock::Text(TextContent::new(prompt_text(turn)))]).meta(serde_json::Map::from_iter([(META.into(),json!(corr))]));
                let terminal=cx.send_request(request).block_task().await?;
                if serde_json::to_value(terminal.stop_reason)?!="end_turn" {ss.lock().unwrap()["stage"]=json!("NON_END_TURN");return Err(agent_client_protocol::Error::internal_error());}
                // late帧继续由dispatcher接收；无correlation的窗口归属不升级为token归因。
                tokio::time::sleep(Duration::from_millis(250)).await;
                let rows=frames(&sw).map_err(|_|agent_client_protocol::Error::internal_error())?;
                verify_updates(&rows,&sid)?;
                let collection=usage::collect(&rows,&sid,runtime_index,first_turn).map_err(|_|agent_client_protocol::Error::internal_error())?;
                if collection["turns"].as_array().unwrap().iter().any(|t|t["typedTerminal"]!=true||t["terminalCorrelationMatchedOrAbsent"]!=true){return Err(agent_client_protocol::Error::internal_error());}
                let after=manifest(&scwd).map_err(|_|agent_client_protocol::Error::internal_error())?;
                ss.lock().unwrap()["manifests"].as_array_mut().unwrap().push(json!({"turn":format!("P{turn}"),"entries":after}));
                if !after.is_empty(){ss.lock().unwrap()["stage"]=json!("WORKSPACE_CHANGED");return Err(agent_client_protocol::Error::internal_error());}
            }
            ss.lock().unwrap()["stage"]=json!("terminal");Ok(())
        });
    let error = match tokio::time::timeout(deadline, operation).await {
        Ok(Ok(())) => None,
        Ok(Err(_)) => Some("SDK_OR_CONTRACT_ERROR"),
        Err(_) => Some("RUNTIME_TIMEOUT"),
    };
    let cleanup = cleanup_child(&mut child, &mut drain, None).await;
    let state = state.lock().unwrap().clone();
    let sid = state["sessionId"]
        .as_str()
        .or(expected.as_deref())
        .filter(|s| valid_id(s))
        .unwrap_or("");
    let mut report = json!({"runtime":runtime_index,"runtimeInstanceId":uuid::Uuid::now_v7().to_string(),"sessionId":sid,"cwd":cwd,"pid":pid,"state":state,"error":error,"cleanup":cleanup});
    match frames(&wire) {
        Ok(rows) => {
            match usage::collect(&rows, sid, runtime_index, first_turn) {
                Ok(c) => report["collection"] = c,
                Err(_) => report["collectionError"] = json!("PROJECTION_FAILED"),
            }
            let method = if session.is_some() {
                "session/resume"
            } else {
                "session/new"
            };
            let (q, a) = exchange(&rows, method);
            report["sessionExchange"] = json!({"method":method,"rpcId":q.map(|r|&r["message"]["id"]),"responseSequence":a.map(|r|&r["sequence"]),"safeParams":q.map(|r|json!({"sessionId":r["message"]["params"].get("sessionId"),"cwd":r["message"]["params"]["cwd"],"mcpServersPresent":r["message"]["params"].get("mcpServers").is_some()})),"rpcErrorCode":a.and_then(|r|r["message"]["error"]["code"].as_i64())});
        }
        Err(_) => report["collectionError"] = json!("FRAME_ERROR"),
    }
    Ok(report)
}

/// scenario PASS仅表示执行/采样完成，不意味着public token_usage可开启。
fn runtime_pass(r: &Value, count: usize) -> bool {
    r["error"].is_null()
        && r["state"]["stage"] == "terminal"
        && r["cleanup"]["succeeded"] == true
        && r["collectionError"].is_null()
        && r["collection"]["rejectedEnvelopes"] == 0
        && r["collection"]["turns"].as_array().is_some_and(|ts| {
            ts.len() == count
                && ts.iter().all(|t| {
                    t["stopReason"] == "end_turn"
                        && t["typedTerminal"] == true
                        && t["terminalCorrelationMatchedOrAbsent"] == true
                })
        })
}

/// 两个runtime顺序执行，共用exact cwd/S1，R1失败不启动R2。
pub async fn scenario(
    launcher: &[String],
    r1_timeout: Duration,
    r2_timeout: Duration,
) -> io::Result<Value> {
    let cwd = tempfile::Builder::new()
        .prefix("cb5-005-usage-")
        .tempdir()?
        .keep();
    identity_guard("workspace", "workspace", &cwd, &cwd)?;
    let before = manifest(&cwd)?;
    let mut report = json!({"scenario":"usage","status":"PARTIAL","cwd":cwd,"before":before,"recoveryMethod":"session/resume","windowsJobAtCreationProven":false,"p3Status":"NOT_RUN"});
    match run_runtime(&cwd, None, launcher, r1_timeout).await {
        Ok(r1) => {
            let sid = r1["sessionId"].as_str().unwrap_or("").to_owned();
            let ready = runtime_pass(&r1, 2);
            report["r1"] = r1;
            if ready {
                match run_runtime(&cwd, Some(&sid), launcher, r2_timeout).await {
                    Ok(r2) => {
                        if runtime_pass(&r2, 1) {
                            report["status"] = json!("PASS");
                        }
                        if r2["collection"]["turns"]
                            .as_array()
                            .is_some_and(|t| !t.is_empty())
                        {
                            report["p3Status"] = json!("SENT");
                        }
                        report["r2"] = r2;
                    }
                    Err(_) => report["r2Error"] = json!("RUNTIME_SETUP_FAILED"),
                }
            }
        }
        Err(_) => report["r1Error"] = json!("RUNTIME_SETUP_FAILED"),
    }
    let after = manifest(&cwd);
    report["after"] = json!(after.as_ref().ok());
    report["manifestComplete"] = json!(after.is_ok());
    report["workspaceDelta"] = json!(after.as_ref().ok().map(|m| {
        before
            .keys()
            .chain(m.keys())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .filter(|k| before.get(*k) != m.get(*k))
            .collect::<Vec<_>>()
    }));
    if !after.as_ref().is_ok_and(|m| m == &before) {
        report["status"] = json!("PARTIAL");
    }
    report["workspaceDeleted"] =
        json!(after.is_ok_and(|m| m.is_empty()) && std::fs::remove_dir(&cwd).is_ok());
    if report["workspaceDeleted"] != true {
        report["status"] = json!("PARTIAL");
    }
    Ok(report)
}

/// 固定output，不接受caller指定路径；复用durable create_new/fsync。
pub fn output_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("evidence/usage")
}

/// 真实Host唯一入口；只在明确usage参数时启动direct installed CLI。
pub async fn host_run() -> i32 {
    let root = output_root();
    let run=async{
        no_links(&root)?;
        durable(&root.join("usage.attempt-started.json"),&json!({"scenario":"usage","attemptId":uuid::Uuid::now_v7().to_string(),"replayAllowed":false}))?;
        let launcher=vec![NODE.into(),SCRIPT.into(),"--acp".into()];
        let report=scenario(&launcher,Duration::from_secs(240),Duration::from_secs(120)).await.unwrap_or_else(|_|json!({"scenario":"usage","status":"PARTIAL","error":"SCENARIO_SETUP_FAILED"}));
        let analysis=usage::analyze(&report);
        durable(&root.join("usage.result.json"),&report)?;durable(&root.join("usage-analysis.json"),&analysis)?;
        println!("{}",json!({"scenario":"usage","status":report["status"],"result":root.join("usage.result.json"),"analysis":root.join("usage-analysis.json")}));
        Ok::<_,io::Error>(report["status"]=="PASS")
    }.await;
    match run {
        Ok(true) => 0,
        Ok(false) => 2,
        Err(_) => {
            eprintln!("USAGE_SENTINEL_OR_EVIDENCE_FAILED_NO_RETRY");
            2
        }
    }
}

/// 只在Host确认原attempt外层超时且无结果时，占用唯一fresh repair attempt。
pub fn reserve_repair(root: &Path) -> io::Result<()> {
    use std::io::Read;
    no_links(root)?;
    for name in ["usage.attempt-started.json", "usage.host-timeout.json"] {
        let path = root.join(name);
        no_links(&path)?;
        if !std::fs::symlink_metadata(path)?.is_file() {
            return Err(io::Error::other("REPAIR_PREREQUISITE_NOT_FILE"));
        }
    }
    // 任何既有结果/repair证据（含dangling link）都阻止启动；访问错误不当作缺失。
    for name in [
        "usage.result.json",
        "usage-repair.attempt-started.json",
        "usage-repair.result.json",
        "usage-repair-analysis.json",
    ] {
        match std::fs::symlink_metadata(root.join(name)) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            _ => {
                return Err(io::Error::other(
                    "REPAIR_ALREADY_ATTEMPTED_OR_RESULT_EXISTS",
                ));
            }
        }
    }
    let mut bytes = Vec::new();
    std::fs::File::open(root.join("usage.host-timeout.json"))?
        .take(65537)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 65536 {
        return Err(io::Error::other("HOST_TIMEOUT_EVIDENCE_LIMIT"));
    }
    let evidence: Value = serde_json::from_slice(&bytes)
        .map_err(|_| io::Error::other("INVALID_HOST_TIMEOUT_EVIDENCE"))?;
    if evidence["classification"] != "HOST_COMMAND_TIMEOUT" {
        return Err(io::Error::other("NOT_HOST_COMMAND_TIMEOUT"));
    }
    durable(
        &root.join("usage-repair.attempt-started.json"),
        &json!({"scenario":"usage-repair","attemptId":uuid::Uuid::now_v7().to_string(),"replayAllowed":false,"reason":"HOST_COMMAND_TIMEOUT","originalAttemptReplayed":false}),
    )
}

/// 只改变Host orchestration的gate/证据名；复用原scenario的新workspace/new Session。
pub async fn host_run_repair() -> i32 {
    let root = output_root();
    let run = async {
        reserve_repair(&root)?;
        let launcher = vec![NODE.into(), SCRIPT.into(), "--acp".into()];
        let mut report = scenario(&launcher, Duration::from_secs(240), Duration::from_secs(120))
            .await.unwrap_or_else(|_|json!({"status":"PARTIAL","error":"SCENARIO_SETUP_FAILED"}));
        report["scenario"] = json!("usage-repair");
        let mut analysis = usage::analyze(&report);
        analysis["scenario"] = json!("usage-repair");
        durable(&root.join("usage-repair.result.json"), &report)?;
        durable(&root.join("usage-repair-analysis.json"), &analysis)?;
        println!("{}", json!({"scenario":"usage-repair","status":report["status"],"result":root.join("usage-repair.result.json"),"analysis":root.join("usage-repair-analysis.json")}));
        Ok::<_,io::Error>(report["status"] == "PASS")
    }.await;
    match run {
        Ok(true) => 0,
        Ok(false) => 2,
        Err(_) => {
            eprintln!("USAGE_REPAIR_GATE_OR_EVIDENCE_FAILED_NO_RETRY");
            2
        }
    }
}
