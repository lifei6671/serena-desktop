//! 内存 wire 的安全投影与完整 workspace 检查，不保存任何正文。
use super::*;

/// 证据只持久化摘要，不保存文本。
pub fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// 所有路径分量的 symlink/reparse 都拒绝，包含 root 与父目录。
pub fn no_links(path: &Path) -> io::Result<()> {
    for part in path.ancestors() {
        let m = std::fs::symlink_metadata(part)?;
        #[cfg(windows)]
        let link = {
            use std::os::windows::fs::MetadataExt;
            m.file_attributes() & 0x400 != 0
        };
        #[cfg(not(windows))]
        let link = false;
        if link || m.file_type().is_symlink() {
            return Err(io::Error::other("REPARSE_REJECTED"));
        }
    }
    Ok(())
}

/// 包括隐藏项的全量 manifest；路径名也 hash，超限不产生部分通过。
pub fn manifest(root: &Path) -> io::Result<BTreeMap<String, Value>> {
    /// 递归检查所有条目，限制文件总量与总字节。
    fn visit(
        root: &Path,
        dir: &Path,
        out: &mut BTreeMap<String, Value>,
        total: &mut u64,
    ) -> io::Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let path = entry?.path();
            no_links(&path)?;
            let m = std::fs::symlink_metadata(&path)?;
            if out.len() >= 4096 {
                return Err(io::Error::other("MANIFEST_LIMIT"));
            }
            let id = hash(
                path.strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .as_bytes(),
            );
            if m.is_dir() {
                out.insert(id, json!({"kind":"directory"}));
                visit(root, &path, out, total)?;
            } else if m.is_file() {
                *total += m.len();
                if *total > 32 * 1024 * 1024 {
                    return Err(io::Error::other("MANIFEST_BYTES_LIMIT"));
                }
                let bytes = std::fs::read(&path)?;
                out.insert(
                    id,
                    json!({"kind":"file","length":bytes.len(),"sha256":hash(&bytes)}),
                );
            } else {
                return Err(io::Error::other("SPECIAL_FILE_REJECTED"));
            }
        }
        Ok(())
    }
    no_links(root)?;
    let mut out = BTreeMap::new();
    visit(root, root, &mut out, &mut 0)?;
    Ok(out)
}

/// create_new + fsync，不覆盖旧证据或 sentinel。
pub fn durable(path: &Path, value: &Value) -> io::Result<()> {
    no_links(path.parent().unwrap())?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(serde_json::to_string_pretty(value)?.as_bytes())?;
    file.sync_all()
}

/// 有界身份字符集，不把任意 provider 正文当 session ID。
pub fn valid_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 200
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.:".contains(&b))
}

/// exact cwd 来自同一个 fresh root；错误 cwd 不自动重写。
pub fn identity_guard(expected: &str, actual: &str, cwd: &Path, frozen: &Path) -> io::Result<()> {
    if !valid_id(actual)
        || expected != actual
        || cwd != frozen
        || !cwd.is_absolute()
        || cwd.to_string_lossy().starts_with(r"\\?\")
    {
        return Err(io::Error::other("IDENTITY_MISMATCH"));
    }
    no_links(cwd)
}

/// catalog 保留字段结构，所有字符串（包括 label/value）只留 hash。
pub fn catalog(v: &Value) -> Value {
    match v {
        Value::Object(m) => Value::Object(
            m.iter()
                .filter(|(k, _)| {
                    [
                        "modes",
                        "configOptions",
                        "currentModeId",
                        "availableModes",
                        "id",
                        "category",
                        "type",
                        "options",
                        "currentValue",
                        "value",
                        "group",
                        "name",
                        "description",
                    ]
                    .contains(&k.as_str())
                })
                .map(|(k, v)| (k.clone(), catalog(v)))
                .collect(),
        ),
        Value::Array(a) => json!(a.iter().map(catalog).collect::<Vec<_>>()),
        Value::String(s) => json!({"sha256":hash(s.as_bytes()),"length":s.len()}),
        _ => v.clone(),
    }
}

/// 从实际 Tee 字节恢复 NDJSON；原始内容只在内存中使用。
pub fn frames(wire: &Wire) -> io::Result<Vec<Value>> {
    let (mut tx, mut rx, mut rows) = (Vec::new(), Vec::new(), Vec::new());
    for (direction, bytes) in wire.lock().unwrap().iter() {
        let buf = if direction == "request" {
            &mut tx
        } else {
            &mut rx
        };
        buf.extend(bytes);
        while let Some(end) = buf.iter().position(|b| *b == b'\n') {
            if end > 1_048_576 || rows.len() >= 4096 {
                return Err(io::Error::other("FRAME_LIMIT"));
            }
            let line: Vec<u8> = buf.drain(..=end).collect();
            let message: Value =
                serde_json::from_slice(&line).map_err(|_| io::Error::other("INVALID_JSON"))?;
            rows.push(json!({"sequence":rows.len()+1,"direction":direction,"message":message}));
        }
    }
    if !tx.is_empty() || !rx.is_empty() {
        return Err(io::Error::other("INCOMPLETE_FRAME"));
    }
    Ok(rows)
}

/// 仅按 exact RPC id 配对；early notification 不可冒充 response。
pub fn exchange<'a>(rows: &'a [Value], method: &str) -> (Option<&'a Value>, Option<&'a Value>) {
    let q = rows
        .iter()
        .find(|r| r["direction"] == "request" && r["message"]["method"] == method);
    let a = q.and_then(|q| {
        rows.iter().find(|r| {
            r["direction"] == "response"
                && r["message"].get("method").is_none()
                && r["message"].get("id") == q["message"].get("id")
        })
    });
    (q, a)
}

/// 不补造不存在的 response sessionId。
fn response_summary(r: &Value) -> Value {
    let sid = r
        .get("sessionId")
        .and_then(Value::as_str)
        .filter(|s| valid_id(s));
    json!({"sessionId":sid,"sessionIdPresent":r.get("sessionId").is_some(),"catalog":catalog(r)})
}

/// 请求、返回回显和全部 early updates 必须属于 S1，否则禁止 P2。
pub fn recovery_guard(
    rows: &[Value],
    method: &str,
    sid: &str,
    cwd: &Path,
    frozen: &Path,
) -> io::Result<()> {
    identity_guard(sid, sid, cwd, frozen)?;
    let (q, a) = exchange(rows, method);
    let q = q.ok_or_else(|| io::Error::other("NO_RECOVERY_REQUEST"))?;
    let a = a.ok_or_else(|| io::Error::other("NO_RECOVERY_RESPONSE"))?;
    if a["message"].get("error").is_some()
        || a["message"].get("result").is_none()
        || q["message"]["params"]["sessionId"] != sid
        || q["message"]["params"]["cwd"] != json!(cwd)
        || a["message"]["result"]
            .get("sessionId")
            .is_some_and(|s| s != sid)
        || rows.iter().any(|r| {
            r["message"]["method"] == "session/update" && r["message"]["params"]["sessionId"] != sid
        })
    {
        return Err(io::Error::other("RECOVERY_IDENTITY_FAILED"));
    }
    Ok(())
}

/// replay/live/late 分离；未关联正文绝不计入 P2 最终答案。
pub fn summarize(rows: &[Value], method: &str, sid: &str, corr: &str, token: &str) -> Value {
    let (pq, pa) = exchange(rows, "session/prompt");
    let prompt_seq = pq.and_then(|v| v["sequence"].as_u64()).unwrap_or(u64::MAX);
    let terminal_seq = pa.and_then(|v| v["sequence"].as_u64()).unwrap_or(u64::MAX);
    let (rq, ra) = exchange(rows, method);
    let recovery_seq = rq.and_then(|v| v["sequence"].as_u64()).unwrap_or(u64::MAX);
    let mut updates = Vec::new();
    let mut answer = String::new();
    let mut ambiguous = 0;
    let mut wrong_session = false;
    for row in rows {
        let p = &row["message"]["params"];
        if row["message"]["method"] != "session/update" {
            continue;
        }
        let seq = row["sequence"].as_u64().unwrap();
        let u = &p["update"];
        let kind = u["sessionUpdate"].as_str().unwrap_or("unknown");
        let safe_kind = if [
            "user_message_chunk",
            "agent_message_chunk",
            "agent_thought_chunk",
            "tool_call",
            "tool_call_update",
            "plan",
            "available_commands_update",
            "current_mode_update",
            "config_option_update",
            "session_info_update",
            "usage_update",
        ]
        .contains(&kind)
        {
            kind
        } else {
            "unknown"
        };
        let phase = if seq < prompt_seq {
            if seq >= recovery_seq {
                if method == "session/new" {
                    "session_new"
                } else {
                    "recovery"
                }
            } else {
                "initialize"
            }
        } else if seq > terminal_seq {
            "late"
        } else {
            "live"
        };
        let matched = p["sessionId"] == sid;
        wrong_session |= !matched;
        let correlated = u["_meta"][META] == corr || p["_meta"][META] == corr;
        updates.push(json!({"sequence":seq,"phase":phase,"type":safe_kind,"sessionMatched":matched,"currentPromptCorrelated":correlated}));
        if phase == "live" && kind == "agent_message_chunk" {
            if matched && correlated {
                if let Some(text) = u["content"]["text"].as_str() {
                    answer.push_str(text);
                }
            } else {
                ambiguous += 1;
            }
        }
    }
    let reason = pa.and_then(|a| a["message"]["result"]["stopReason"].as_str());
    let stop = reason.filter(|s| {
        [
            "end_turn",
            "max_tokens",
            "max_turn_requests",
            "refusal",
            "cancelled",
        ]
        .contains(s)
    });
    let replay: Vec<_> = updates
        .iter()
        .filter(|u| u["phase"] == "recovery")
        .cloned()
        .collect();
    let history: Vec<_> = replay
        .iter()
        .filter(|u| {
            [
                "user_message_chunk",
                "agent_message_chunk",
                "agent_thought_chunk",
                "tool_call",
                "tool_call_update",
                "plan",
            ]
            .iter()
            .any(|t| u["type"] == *t)
        })
        .cloned()
        .collect();
    let terminal_corr = pa
        .and_then(|r| r["message"]["result"]["_meta"].get(META))
        .map(|v| v == corr);
    json!({"sessionId":sid,"conversationRequestId":corr,"promptRpcId":pq.map(|r|&r["message"]["id"]),
        "promptSequence":pq.map(|r|&r["sequence"]),"terminalSequence":pa.map(|r|&r["sequence"]),"terminalStopReason":stop,
        "terminalRpcId":pa.map(|r|&r["message"]["id"]),"terminalConversationIdMatched":terminal_corr,
        "terminalReceived":stop.is_some(),"wrongSession":wrong_session,"unattributedAnswerChunks":ambiguous,
        "finalAnswerSha256":hash(answer.as_bytes()),"finalAnswerLength":answer.len(),"matched":answer.trim()==token,
        "updates":updates,"recoveryUpdateCount":replay.len(),"recoveryUpdateOrder":replay,
        "historyReplayUpdateCount":history.len(),"historyReplayUpdateOrder":history,"tokenMatchPolicy":"trimmed-exact",
        "recoveryMethod":method,"recoveryRpcId":rq.map(|r|&r["message"]["id"]),
        "safeParams":rq.map(|r|json!({"sessionId":r["message"]["params"]["sessionId"],"cwd":r["message"]["params"]["cwd"],"mcpServers":r["message"]["params"].get("mcpServers"),"mcpServersPresent":r["message"]["params"].get("mcpServers").is_some()})),
        "response":ra.map(|r|response_summary(&r["message"]["result"])),
        "rpcErrorCode":ra.and_then(|r|r["message"]["error"]["code"].as_i64())})
}
