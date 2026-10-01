//! Usage 安全数字投影、wire phase/correlation 与只读分析；不推算累计 token。
use super::*;

/// 只正规化协议键用于匹配，不输出未经白名单处理的字段名。
fn normalized(key: &str) -> String {
    key.chars()
        .filter(|c| *c != '_' && *c != '-')
        .flat_map(char::to_lowercase)
        .collect()
}

/// 敏感子树整体拒绝，不能因父层是usage而把credential数字保存下来。
fn denied(key: &str) -> bool {
    let k = normalized(key);
    [
        "secret",
        "password",
        "credential",
        "authorization",
        "apikey",
        "accesstoken",
        "refreshtoken",
        "sessiontoken",
        "bearer",
        "cookie",
        "auth",
    ]
    .iter()
    .any(|s| k.contains(s))
        || [
            "providerdata",
            "env",
            "environment",
            "content",
            "text",
            "thought",
            "prompt",
            "message",
            "source",
            "token",
        ]
        .contains(&k.as_str())
}

/// 明确语义键的固定allowlist；其他含token名称只是观察，不推断breakdown。
fn semantic(key: &str) -> Option<&'static str> {
    match normalized(key).as_str() {
        "used" => Some("used"),
        "size" => Some("size"),
        "inputtokens" | "prompttokens" => Some("inputTokens"),
        "outputtokens" | "completiontokens" => Some("outputTokens"),
        "cachedreadtokens" | "cachereadtokens" | "cachereadinputtokens" => Some("cachedReadTokens"),
        "cachedwritetokens" | "cachewritetokens" | "cachecreationinputtokens" => {
            Some("cachedWriteTokens")
        }
        "cachedtokens" => Some("cachedTokens"),
        "totaltokens" => Some("totalTokens"),
        "cost" | "totalcost" | "inputcost" | "outputcost" => Some("cost"),
        _ => None,
    }
}

/// 已知键保持可读路径，扩展键用hash表示字段存在，避免键名本身携带正文。
fn safe_key(key: &str) -> String {
    let known = [
        "sessionUpdate",
        "_meta",
        "usage",
        "tokenUsage",
        "token_usage",
        "tokens",
        "context",
        "contextWindow",
        "context_window",
        "cost",
        "amount",
        "currency",
        "used",
        "size",
        "inputTokens",
        "input_tokens",
        "promptTokens",
        "prompt_tokens",
        "outputTokens",
        "output_tokens",
        "completionTokens",
        "completion_tokens",
        "cachedReadTokens",
        "cached_read_tokens",
        "cacheReadTokens",
        "cache_read_tokens",
        "cache_read_input_tokens",
        "cacheReadInputTokens",
        "cachedWriteTokens",
        "cached_write_tokens",
        "cacheWriteTokens",
        "cache_write_tokens",
        "cache_creation_input_tokens",
        "cacheCreationInputTokens",
        "cachedTokens",
        "cached_tokens",
        "totalTokens",
        "total_tokens",
        "totalCost",
        "total_cost",
        "inputCost",
        "outputCost",
    ];
    if known.contains(&key) {
        key.into()
    } else {
        format!("#{}", hash(key.as_bytes()))
    }
}

/// session_info / result 只关注命名相关的数值；保留_meta等容器以寻找嵌套字段。
fn relevant(key: &str) -> bool {
    let key = normalized(key);
    ["usage", "token", "context", "cost"]
        .iter()
        .any(|word| key.contains(word))
}

/// 所有持久字段是明确的number/bool/null或结构，无字符串/数组内容。
pub fn project(value: &Value, all_numeric: bool, budget: &mut usize) -> io::Result<Vec<Value>> {
    /// 每一层有界递归；被拒字段只留hash路径与redacted结构，不保存其值。
    fn visit(
        v: &Value,
        path: &str,
        selected: bool,
        key: &str,
        depth: usize,
        out: &mut Vec<Value>,
        budget: &mut usize,
    ) -> io::Result<()> {
        if depth > 24 || *budget == 0 {
            return Err(io::Error::other("PROJECTION_LIMIT"));
        }
        if denied(key) {
            *budget -= 1;
            out.push(json!({"path":path,"kind":"redacted"}));
            return Ok(());
        }
        match v {
            Value::Object(m) => {
                if !path.is_empty() {
                    *budget -= 1;
                    out.push(json!({"path":path,"kind":"object"}));
                }
                for (k, v) in m {
                    visit(
                        v,
                        &format!("{path}/{}", safe_key(k)),
                        selected || relevant(k),
                        k,
                        depth + 1,
                        out,
                        budget,
                    )?;
                }
            }
            Value::Array(a) => {
                if selected {
                    *budget -= 1;
                    out.push(json!({"path":path,"kind":"array","elements":a.len()}));
                }
            }
            Value::String(_) => {
                if selected {
                    *budget -= 1;
                    out.push(json!({"path":path,"kind":"string"}));
                }
            }
            _ => {
                if selected {
                    *budget -= 1;
                    let kind = if v.is_number() {
                        "number"
                    } else if v.is_boolean() {
                        "bool"
                    } else {
                        "null"
                    };
                    out.push(json!({"path":path,"kind":kind,"value":v,"semantic":semantic(key)}));
                }
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    visit(value, "", all_numeric, "", 0, &mut out, budget)?;
    Ok(out)
}

/// 从请求wire提取turn identity，不能按相邻帧假定terminal。
fn turns(rows: &[Value], first_turn: usize) -> Vec<Value> {
    rows.iter().filter(|r|r["direction"]=="request"&&r["message"]["method"]=="session/prompt").enumerate().map(|(i,q)|{
        let a=rows.iter().find(|r|r["direction"]=="response"&&r["message"].get("method").is_none()&&r["message"].get("id")==q["message"].get("id"));
        let corr=q["message"]["params"]["_meta"][META].as_str().filter(|s|valid_id(s)).unwrap_or("");
        let end=a.and_then(|r|r["sequence"].as_u64()).unwrap_or(u64::MAX);let start=q["sequence"].as_u64().unwrap();
        let mut answer=String::new();let mut unattributed=0;
        for r in rows {
            let seq=r["sequence"].as_u64().unwrap();let p=&r["message"]["params"];let u=&p["update"];
            if seq>start && seq<end && r["message"]["method"]=="session/update" && u["sessionUpdate"]=="agent_message_chunk" {
                if p["sessionId"]==q["message"]["params"]["sessionId"] && (u["_meta"][META]==corr||p["_meta"][META]==corr) {
                    if let Some(text)=u["content"]["text"].as_str(){answer.push_str(text);}
                }else{unattributed+=1;}
            }
        }
        let stop=a.and_then(|r|r["message"]["result"]["stopReason"].as_str()).filter(|s|["end_turn","cancelled","max_tokens","max_turn_requests","refusal"].contains(s));
        let typed=a.is_some_and(|r|serde_json::from_value::<PromptResponse>(r["message"]["result"].clone()).is_ok());
        let correlation_ok=a.and_then(|r|r["message"]["result"]["_meta"].get(META)).is_none_or(|v|v==corr);
        json!({"turn":format!("P{}",first_turn+i),"conversationRequestId":corr,"promptRpcId":q["message"]["id"],"requestSequence":start,"terminalSequence":a.map(|r|&r["sequence"]),"terminalRpcId":a.map(|r|&r["message"]["id"]),"stopReason":stop,"typedTerminal":typed,"terminalCorrelationMatchedOrAbsent":correlation_ok,"finalAnswerSha256":hash(answer.as_bytes()),"finalAnswerLength":answer.len(),"unattributedAnswerChunks":unattributed})
    }).collect()
}

/// phase只表达接收窗口；旧correlation即便穿插下一turn也标late，不冒充当前turn。
fn placement(seq: u64, corr: Option<&str>, turns: &[Value], base: &str) -> Value {
    let current = turns
        .iter()
        .rev()
        .find(|t| t["requestSequence"].as_u64().unwrap() < seq);
    let phase = current
        .map(|t| {
            if t["terminalSequence"].as_u64().is_some_and(|end| seq > end) {
                "late"
            } else {
                t["turn"].as_str().unwrap()
            }
        })
        .unwrap_or(base);
    let correlated = corr.and_then(|c| turns.iter().find(|t| t["conversationRequestId"] == c));
    let late =
        correlated.is_some_and(|t| t["terminalSequence"].as_u64().is_some_and(|end| seq > end));
    json!({"phase":if late{"late"}else{phase},"phaseAtReceipt":phase,
        "windowTurn":current.map(|t|&t["turn"]),"attributedTurn":correlated.map(|t|&t["turn"]),
        "attribution":if correlated.is_some(){"exact_conversation"}else if corr.is_some(){"foreign_conversation"}else{"window_only"},
        "lateFor":if late {correlated.map(|t|&t["turn"])}else if phase=="late"{current.map(|t|&t["turn"])}else{None}})
}

/// 身份与typed envelope通过后才投影extension数字；错误帧只计数不留正文。
pub fn collect(rows: &[Value], sid: &str, runtime: usize, first_turn: usize) -> io::Result<Value> {
    let turns = turns(rows, first_turn);
    let base = if runtime == 1 { "new" } else { "resume" };
    let mut samples = Vec::new();
    let mut rejected = 0;
    let mut budget = 8192;
    for r in rows {
        let seq = r["sequence"].as_u64().unwrap();
        let message = &r["message"];
        if message["method"] == "session/update" {
            let p = &message["params"];
            let u = &p["update"];
            let parsed = serde_json::from_value::<SessionNotification>(p.clone());
            if !parsed
                .as_ref()
                .is_ok_and(|n| n.session_id.to_string() == sid)
            {
                rejected += 1;
                continue;
            }
            let source = match u["sessionUpdate"].as_str() {
                Some("usage_update") => "usage_update",
                Some("session_info_update") => "session_info_update",
                _ => continue,
            };
            let typed_usage = matches!(parsed.unwrap().update, SessionUpdate::UsageUpdate(_));
            let corr = u["_meta"][META]
                .as_str()
                .or_else(|| p["_meta"][META].as_str());
            let fields = project(u, source == "usage_update", &mut budget)?;
            samples.push(json!({"runtime":runtime,"sequence":seq,"source":source,"placement":placement(seq,corr,&turns,base),"typedEnvelope":true,"typedUsageUpdate":typed_usage,"fields":fields}));
        } else if message.get("method").is_none() && r["direction"] == "response" {
            if let Some(t) = turns
                .iter()
                .find(|t| t["terminalSequence"] == r["sequence"])
            {
                if t["typedTerminal"] != true || t["terminalCorrelationMatchedOrAbsent"] != true {
                    rejected += 1;
                    continue;
                }
                samples.push(json!({"runtime":runtime,"sequence":seq,"source":"prompt_result","placement":{"phase":t["turn"],"phaseAtReceipt":t["turn"],"windowTurn":t["turn"],"attributedTurn":t["turn"],"attribution":"exact_rpc_id","lateFor":null},"typedEnvelope":true,"typedUsageUpdate":false,"fields":project(&message["result"],false,&mut budget)?}));
            }
        }
    }
    Ok(
        json!({"samples":samples,"turns":turns,"rejectedEnvelopes":rejected,"projectionComplete":true}),
    )
}

/// 返回原始路径匹配的数字，bool/null不会误作计数。
fn field_number(sample: &Value, path: &str) -> Option<u64> {
    sample["fields"]
        .as_array()?
        .iter()
        .find(|f| f["path"] == path && f["kind"] == "number")?["value"]
        .as_u64()
}

/// 快照只做相等/大小关系观察，绝不生成delta或累计token。
fn relation(a: Option<u64>, b: Option<u64>) -> &'static str {
    match (a, b) {
        (Some(a), Some(b)) if a == b => "equal",
        (Some(a), Some(b)) if a > b && b == 0 => "zero_after_positive",
        (Some(a), Some(b)) if a > b => "decreased",
        (Some(_), Some(_)) => "increased",
        _ => "unknown",
    }
}

/// 将实测与解释分开；任何reset都保留，不自动覆盖最后正值。
pub fn analyze(report: &Value) -> Value {
    let mut samples = Vec::new();
    let mut turns = Vec::new();
    for runtime in ["r1", "r2"] {
        if let Some(a) = report[runtime]["collection"]["samples"].as_array() {
            samples.extend(a.iter().cloned());
        }
        if let Some(a) = report[runtime]["collection"]["turns"].as_array() {
            for t in a {
                let mut t = t.clone();
                t["runtime"] = json!(if runtime == "r1" { 1 } else { 2 });
                turns.push(t);
            }
        }
    }
    let snapshots: Vec<_> = samples
        .iter()
        .filter(|s| s["source"] == "usage_update" && s["typedUsageUpdate"] == true)
        .cloned()
        .collect();
    let mut changes = Vec::new();
    for w in snapshots.windows(2) {
        changes.push(json!({"from":{"runtime":w[0]["runtime"],"sequence":w[0]["sequence"]},"to":{"runtime":w[1]["runtime"],"sequence":w[1]["sequence"]},"usedRelation":relation(field_number(&w[0],"/used"),field_number(&w[1],"/used")),"sizeRelation":relation(field_number(&w[0],"/size"),field_number(&w[1],"/size"))}));
    }
    let size_values: Vec<_> = snapshots
        .iter()
        .filter_map(|s| field_number(s, "/size"))
        .collect();
    let r1 = snapshots.iter().rev().find(|s| s["runtime"] == 1);
    let r2 = snapshots.iter().find(|s| s["runtime"] == 2);
    let breakdown:Vec<_>=samples.iter().flat_map(|s|s["fields"].as_array().into_iter().flatten().filter(|f|f["kind"]=="number" && ["inputTokens","outputTokens","cachedReadTokens","cachedWriteTokens","cachedTokens","totalTokens"].iter().any(|k|f["semantic"]==*k)).map(|f|json!({"runtime":s["runtime"],"sequence":s["sequence"],"source":s["source"],"placement":s["placement"],"field":f}))).collect();
    let costs:Vec<_>=samples.iter().flat_map(|s|s["fields"].as_array().into_iter().flatten().filter(|f|f["kind"]=="number"&&(f["semantic"]=="cost"||f["path"].as_str().is_some_and(|p|p.split('/').any(|k|semantic(k)==Some("cost"))))).map(|f|json!({"runtime":s["runtime"],"sequence":s["sequence"],"source":s["source"],"placement":s["placement"],"field":f}))).collect();
    let ordering:Vec<_>=turns.iter().map(|t|{
        let before=snapshots.iter().rev().find(|s|s["runtime"]==t["runtime"]&&s["sequence"].as_u64()>t["requestSequence"].as_u64()&&s["sequence"].as_u64()<t["terminalSequence"].as_u64()&&s["placement"]["attribution"]!="foreign_conversation"&&(s["placement"]["attributedTurn"].is_null()||s["placement"]["attributedTurn"]==t["turn"]));
        let late:Vec<_>=snapshots.iter().filter(|s|s["runtime"]==t["runtime"]&&s["placement"]["lateFor"]==t["turn"]).cloned().collect();
        json!({"runtime":t["runtime"],"turn":t["turn"],"terminalSequence":t["terminalSequence"],"lastUsageBeforeTerminal":before,"lateUsage":late})
    }).collect();
    let mut size_by_runtime = Vec::new();
    for runtime in [1, 2] {
        let vals: Vec<_> = snapshots
            .iter()
            .filter(|s| s["runtime"] == runtime)
            .filter_map(|s| field_number(s, "/size"))
            .collect();
        size_by_runtime.push(json!({"runtime":runtime,"observedValues":vals,"stable":if vals.len()>=2{Some(vals.windows(2).all(|w|w[0]==w[1]))}else{None}}));
    }
    json!({"scenarioStatus":report["status"],"observations":{"samples":samples,"usageUpdateSnapshots":snapshots,"turns":turns,"transitions":changes,"sizeAcrossSessionStable":if size_values.len()>=2{Some(size_values.windows(2).all(|w|w[0]==w[1]))}else{None},"sizeByRuntime":size_by_runtime,"explicitBreakdownFields":breakdown,"costFields":costs,"terminalOrdering":ordering,"restart":{"r1LastSnapshot":r1,"r2FirstSnapshot":r2,"usedRelation":relation(r1.and_then(|s|field_number(s,"/used")),r2.and_then(|s|field_number(s,"/used"))),"sizeRelation":relation(r1.and_then(|s|field_number(s,"/size")),r2.and_then(|s|field_number(s,"/size")))},"latestSnapshot":snapshots.last(),"lastPositiveSnapshot":snapshots.iter().rev().find(|s|field_number(s,"/used").is_some_and(|n|n>0))},"interpretation":{"usedSize":if snapshots.is_empty(){"UNKNOWN"}else{"context_occupancy_gauge"},"basis":"official_typed_UsageUpdate_context_window_semantics_plus_observed_snapshots","counterSemanticsProven":false,"sizeMeaning":"typed_total_context_window_tokens; provider_behavior_in_observations","zeroResetPolicy":"record_only_do_not_replace_last_positive","costScope":"typed_cost_is_session_cumulative; extension_scope_unknown; no_token_conversion","token_usage":false,"publicUsage":"unknown","reason":if breakdown.is_empty(){"NO_EXPLICIT_TOKEN_BREAKDOWN"}else{"BREAKDOWN_OBSERVED_BUT_CROSS_TURN_RESTART_SEMANTICS_REQUIRE_HOST_FREEZE"},"derivedTokenTotals":null,"lateWindowMs":250}})
}
