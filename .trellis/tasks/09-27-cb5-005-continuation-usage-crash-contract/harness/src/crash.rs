//! Stage C 只投影身份等值、摘要与顺序；不从历史正文推导业务终态。
use super::*;
pub const REQUEST: &str = "codebuddy.ai/requestId";
pub const MESSAGE: &str = "codebuddy.ai/messageId";

/// 在线观察允许最后一帧尚未读全，但完整坏帧不能被忽略。
pub fn snapshot(wire: &Wire) -> io::Result<(Vec<Value>, bool)> {
    let (mut tx, mut rx, mut rows) = (Vec::new(), Vec::new(), Vec::new());
    for (direction, bytes) in wire.lock().unwrap().iter() {
        let buffer = if direction == "request" {
            &mut tx
        } else {
            &mut rx
        };
        buffer.extend(bytes);
        while let Some(end) = buffer.iter().position(|b| *b == b'\n') {
            if end > 1_048_576 || rows.len() >= 4096 {
                return Err(io::Error::other("FRAME_LIMIT"));
            }
            let line: Vec<u8> = buffer.drain(..=end).collect();
            let message: Value =
                serde_json::from_slice(&line).map_err(|_| io::Error::other("INVALID_JSON"))?;
            rows.push(json!({"sequence":rows.len()+1,"direction":direction,"message":message}));
        }
    }
    Ok((rows, !tx.is_empty() || !rx.is_empty()))
}

/// 两层同名meta冲突时拒绝；不以另一种identity键替代requestId。
pub fn meta<'a>(params: &'a Value, key: &str) -> Option<&'a str> {
    let a = params["update"]["_meta"].get(key);
    let b = params["_meta"].get(key);
    if a.is_some() && b.is_some() && a != b {
        return None;
    }
    a.or(b).and_then(Value::as_str)
}

/// terminal拥有优先权：同一已接收batch里先activity后terminal也不能宣称窗口成功。
pub fn trigger(rows: &[Value], sid: &str, request: &str) -> io::Result<Option<&'static str>> {
    let (prompt, terminal) = exchange(rows, "session/prompt");
    let Some(prompt) = prompt else {
        return Ok(None);
    };
    if let Some(terminal) = terminal {
        return Ok(Some(
            if serde_json::from_value::<PromptResponse>(terminal["message"]["result"].clone())
                .is_ok()
            {
                "TERMINAL_FIRST"
            } else {
                "PROMPT_RESPONSE_ERROR"
            },
        ));
    }
    for row in rows
        .iter()
        .filter(|r| r["sequence"].as_u64() > prompt["sequence"].as_u64())
    {
        let p = &row["message"]["params"];
        if row["direction"] != "response" || row["message"]["method"] != "session/update" {
            continue;
        }
        let _: SessionNotification = serde_json::from_value(p.clone())?;
        if p["sessionId"] != sid {
            return Err(io::Error::other("WRONG_SESSION"));
        }
        if meta(p, REQUEST) == Some(request)
            && matches!(
                p["update"]["sessionUpdate"].as_str(),
                Some("agent_message_chunk" | "agent_thought_chunk" | "usage_update")
            )
        {
            return Ok(Some("ACTIVITY_WITHOUT_TERMINAL"));
        }
    }
    Ok(None)
}

/// 安全类型标签来自固定集合，不落未知provider字符串。
fn safe_kind(kind: &str) -> &str {
    match kind {
        "user_message_chunk"
        | "agent_message_chunk"
        | "agent_thought_chunk"
        | "usage_update"
        | "session_info_update"
        | "tool_call"
        | "tool_call_update"
        | "plan"
        | "available_commands_update"
        | "current_mode_update"
        | "config_option_update" => kind,
        _ => "unknown",
    }
}

/// 从live/replay生成摘要；live不去重chunk，replay按message identity重组诊断文本。
pub fn project(
    rows: &[Value],
    sid: &str,
    request: &str,
    replay: bool,
    live: Option<&Value>,
) -> io::Result<Value> {
    let (q, a) = exchange(
        rows,
        if replay {
            "session/load"
        } else {
            "session/prompt"
        },
    );
    let start = q.and_then(|r| r["sequence"].as_u64()).unwrap_or(u64::MAX);
    let end = if replay {
        u64::MAX
    } else {
        a.and_then(|r| r["sequence"].as_u64()).unwrap_or(u64::MAX)
    };
    let mut updates = Vec::new();
    let mut answer = String::new();
    let mut groups: Vec<(String, String, BTreeSet<String>)> = Vec::new();
    let (mut bound, mut rejected, mut missing_message, mut duplicate, mut tools) = (0, 0, 0, 0, 0);
    let mut material = false;
    let mut old_terminal = false;
    let mut terminal_observations = Vec::new();
    let mut ids = BTreeSet::new();
    for row in rows {
        if row["direction"] != "response" || row["sequence"].as_u64().unwrap_or(0) <= start {
            continue;
        }
        let m = &row["message"];
        // 历史检查没有发prompt：任何带stopReason的response/扩展只作为差异，不作为terminal授权。
        if replay
            && (m["result"].get("stopReason").is_some()
                || m["params"]["update"].get("stopReason").is_some())
        {
            material = true;
            let target_rpc = live.is_some_and(|l| {
                !l["promptRpcId"].is_null() && m.get("id") == l.get("promptRpcId")
            });
            let target_request =
                m["result"]["_meta"][META] == request || m["result"]["_meta"][REQUEST] == request;
            let typed_terminal =
                serde_json::from_value::<PromptResponse>(m["result"].clone()).is_ok();
            let exact = target_rpc && target_request && typed_terminal;
            old_terminal |= exact;
            terminal_observations.push(json!({"sequence":row["sequence"],"exactOldRpcIdMatched":target_rpc,"targetRequestMatched":target_request,"typedPromptResponse":typed_terminal,"exactOldTerminalCandidate":exact}));
        }
        if m["method"] != "session/update" {
            continue;
        }
        let p = &m["params"];
        let u = &p["update"];
        let typed = serde_json::from_value::<SessionNotification>(p.clone()).is_ok();
        let session_match = p["sessionId"] == sid;
        let req = meta(p, REQUEST);
        let message = meta(p, MESSAGE);
        let matched = typed && session_match && req == Some(request);
        let kind = safe_kind(u["sessionUpdate"].as_str().unwrap_or("unknown"));
        let history = matches!(
            kind,
            "user_message_chunk"
                | "agent_message_chunk"
                | "agent_thought_chunk"
                | "tool_call"
                | "tool_call_update"
                | "plan"
        );
        if !typed || !session_match {
            rejected += 1;
        }
        if history && !matched {
            rejected += 1;
        }
        if matched && history {
            bound += 1;
        }
        if matches!(kind, "tool_call" | "tool_call_update") {
            tools += 1;
        }
        let message_hash = message.map(|s| hash(s.as_bytes()));
        let known_live = message_hash.as_ref().map(|h| {
            live.is_some_and(|l| {
                l["messageIdHashes"]
                    .as_array()
                    .is_some_and(|ids| ids.contains(&json!(h)))
            })
        });
        updates.push(json!({"sequence":row["sequence"],"type":kind,"typedEnvelope":typed,"sessionMatched":session_match,"oldRequestIdMatched":matched,"requestIdSha256":req.map(|s|hash(s.as_bytes())),"messageIdSha256":message_hash,"messageIdMatchesLive":known_live}));
        if matched && kind == "agent_message_chunk" && row["sequence"].as_u64().unwrap_or(0) < end {
            if let Some(text) = u["content"]["text"].as_str() {
                if let Some(mid) = message {
                    let h = hash(mid.as_bytes());
                    ids.insert(h.clone());
                    if replay {
                        let index = groups.iter().position(|g| g.0 == h).unwrap_or_else(|| {
                            groups.push((h, String::new(), BTreeSet::new()));
                            groups.len() - 1
                        });
                        if groups[index].2.insert(hash(text.as_bytes())) {
                            groups[index].1.push_str(text);
                        } else {
                            duplicate += 1;
                        }
                    } else {
                        answer.push_str(text);
                    }
                } else {
                    missing_message += 1;
                    if !replay {
                        answer.push_str(text);
                    }
                }
            }
        }
    }
    if replay {
        for (_, text, _) in &groups {
            answer.push_str(text);
        }
    }
    let terminal = a.filter(|a| a["message"].get("result").is_some());
    let terminal_typed = terminal.is_some_and(|a| {
        serde_json::from_value::<PromptResponse>(a["message"]["result"].clone()).is_ok()
    });
    let terminal_exact = !replay
        && terminal_typed
        && terminal.is_some_and(|a| a["message"]["result"]["_meta"][META] == request);
    let stop = terminal
        .filter(|_| terminal_exact)
        .and_then(|a| a["message"]["result"]["stopReason"].as_str())
        .filter(|s| {
            matches!(
                *s,
                "end_turn" | "cancelled" | "max_tokens" | "max_turn_requests" | "refusal"
            )
        });
    let digest = hash(answer.as_bytes());
    Ok(
        json!({"updates":updates,"boundHistoryCount":bound,"rejectedUpdates":rejected,"missingMessageIdCount":missing_message,"duplicateReplayFragments":duplicate,"toolActivityCount":tools,"answerGroups":groups.iter().map(|(id,text,fragments)|json!({"messageIdSha256":id,"answerSha256":hash(text.as_bytes()),"answerLength":text.len(),"distinctFragmentCount":fragments.len()})).collect::<Vec<_>>(),"messageIdHashes":ids,"messageIdSetMatchesLive":live.map(|l| l["messageIdHashes"]==json!(ids)),"terminalWireObservations":terminal_observations,"answerSha256":digest,"answerLength":answer.len(),"answerMatchesLive":live.map(|l|l["answerSha256"]==digest&&l["answerLength"]==answer.len()),"promptRpcId":if replay{Value::Null}else{q.map(|q|q["message"]["id"].clone()).unwrap_or(Value::Null)},"terminalSequence":if replay||!terminal_typed{Value::Null}else{a.map(|r|r["sequence"].clone()).unwrap_or(Value::Null)},"terminalExactConversation":terminal_exact,"terminalStopReason":stop,"oldPromptTerminalWireEvidence":old_terminal,"materialContractDifference":material,"resultCompleteness":if bound>0{"partial"}else{"unknown"},"businessCompletedRecoverable":false,"windowsJobAtCreationProven":false,"runtimeTerminationEvidenceProven":false,"claimReleasePermitted":false}),
    )
}

/// 单次固定模式占位，任一既有产物都阻止重放。
pub fn reserve(root: &Path, mode: &str) -> io::Result<()> {
    if !matches!(mode, "crash-before-terminal" | "crash-after-terminal") {
        return Err(io::Error::other("INVALID_MODE"));
    }
    no_links(root)?;
    for suffix in [
        "attempt-started.json",
        "prompt-identity.json",
        "result.json",
    ] {
        match std::fs::symlink_metadata(root.join(format!("{mode}.{suffix}"))) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            _ => return Err(io::Error::other("NO_REPLAY")),
        }
    }
    durable(
        &root.join(format!("{mode}.attempt-started.json")),
        &json!({"scenario":mode,"attemptId":uuid::Uuid::now_v7().to_string(),"replayAllowed":false}),
    )
}
