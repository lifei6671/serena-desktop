//! CB7-005 隔离 native peer：control 在 cwd 之外，Workspace 只写指定 output。
use serde_json::{Value, json};
use std::{
    fs::{self, OpenOptions},
    io::{self, BufRead, Write},
    path::PathBuf,
};
/// 使用实际 SDK 请求 id 回复，不创建自己的 RPC identity。
fn reply(id: &Value, result: Value) {
    println!("{}", json!({"jsonrpc":"2.0","id":id,"result":result}));
    io::stdout().flush().unwrap();
}
/// fake peer 返回 exact Session 已确认的完整实际模型与推理配置。
fn effective_catalog(mode: &str) -> Value {
    let mut catalog = json!({
        "configOptions":[
            {"id":"model","name":"Model","category":"model","type":"select","currentValue":"model-a","options":[{"value":"model-a","name":"Model A"}]},
            {"id":"thought_level","name":"Thought level","category":"thought_level","type":"select","currentValue":"medium","options":[{"value":"medium","name":"Medium"}]}
        ],
        "models":{"currentModelId":"model-a","availableModels":[{"modelId":"model-a","name":"Model A"}]}
    });
    if mode.ends_with("nonreasoning") {
        catalog["configOptions"] = json!([
            {"id":"model","name":"Model","category":"model","type":"select","currentValue":"model-plain","options":[{"value":"model-plain","name":"Model Plain"}]},
            {"id":"thought_level","name":"Thought level","category":"thought_level","type":"select","currentValue":"stale-high","options":[{"value":"stale-high","name":"Stale High"}]}
        ]);
        catalog["models"] = json!({
            "currentModelId":"model-plain",
            "availableModels":[{"modelId":"model-plain","name":"Model Plain","_meta":{"supportsReasoning":false}}]
        });
    } else if mode == "malformed-effective-meta" {
        catalog["models"]["availableModels"][0]["_meta"] = Value::Null;
    }
    catalog
}
/// 全部测试控制与证据写外部 temp control，避免污染 Workspace delta。
fn main() {
    let control: PathBuf = std::env::current_exe().unwrap().parent().unwrap().into();
    let db = rusqlite::Connection::open(control.join("agent-state.db")).unwrap();
    let mut pending_prompt: Option<Value> = None;
    for line in io::stdin().lock().lines() {
        let line = line.unwrap();
        // 原始出站序列是 Continue 唯一方法与禁止 fallback 的测试证据。
        writeln!(
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(control.join("requests.jsonl"))
                .unwrap(),
            "{line}"
        )
        .unwrap();
        let raw: Value = serde_json::from_str(&line).unwrap();
        match raw["method"].as_str().unwrap_or("permission-response") {
            "permission-response" => {
                assert_eq!(raw, json!({"jsonrpc":"2.0","id":0,"result":{"outcome":{"outcome":"selected","optionId":"advertised-deny-id"}}}));
                assert!(!control.join("permission-response.json").exists());
                fs::write(control.join("permission-response.json"), raw.to_string()).unwrap();
                let mode = fs::read_to_string(control.join("mode")).unwrap();
                let execution = if mode.starts_with("continue-") { "c" } else { "e" };
                // deny 仅是 client 决策；此刻原 Job/Claim 必须仍活着，无 terminal/release。
                let (terminal, evidence, claims): (Option<String>, String, i64) = db.query_row("SELECT provider_terminal_status,release_evidence_state,(SELECT count(*) FROM workspace_claims) FROM executions WHERE id=?1", [execution], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
                assert!(terminal.is_none());
                assert_ne!(evidence, "complete");
                assert_eq!(claims, 1);
                // 测试握手保证后续 Activity 真正发生在父进程已观察 deny 之后。
                if mode == "permission-cancel" {
                    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
                    while !control.join("next-activity").exists() {
                        assert!(std::time::Instant::now() < deadline);
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                    let conversation = &pending_prompt.as_ref().unwrap()["params"]["_meta"]["codebuddy.ai/conversationRequestId"];
                    println!("{}", json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"exact-session","update":{"sessionUpdate":"tool_call","toolCallId":"new-tool","title":"private new command","kind":"read","status":"pending","_meta":{"codebuddy.ai/conversationRequestId":conversation}}}}));
                    io::stdout().flush().unwrap();
                }
                if mode == "permission-eof" { return; }
                if mode == "permission-timeout" || mode == "permission-gated" || mode == "permission-cancel" { continue; }
                let prompt = pending_prompt.take().unwrap();
                reply(&prompt["id"], json!({"stopReason":if mode == "permission-end-turn" { "end_turn" } else { "cancelled" },"_meta":{"codebuddy.ai/conversationRequestId":prompt["params"]["_meta"]["codebuddy.ai/conversationRequestId"]}}));
            }
            "initialize" => {
                let mode = fs::read_to_string(control.join("mode")).unwrap();
                if mode == "initialize-eof" {
                    return;
                }
                if mode == "initialize-timeout" {
                    continue;
                }
                reply(
                    &raw["id"],
                    json!({"protocolVersion":if mode == "initialize-incompatible" { json!(2) } else if mode == "initialize-malformed" { json!("1") } else { json!(1) },"agentCapabilities":{"loadSession":mode != "continue-no-capability"}}),
                );
            }
            "session/new" => {
                let mode = fs::read_to_string(control.join("mode")).unwrap();
                if mode.starts_with("continue-") {
                    fs::write(control.join("forbidden-session-new"), raw.to_string()).unwrap();
                    panic!("continued execution must not create a fresh session");
                }
                if mode == "closed-input" {
                    // 精确注入真实 pipe write 失败，stdout 保留，因此不是 pre-accept EOF。
                    #[link(name = "kernel32")]
                    unsafe extern "system" {
                        fn GetStdHandle(which: u32) -> *mut std::ffi::c_void;
                        fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
                    }
                    #[link(name = "ucrt")]
                    unsafe extern "C" {
                        fn _close(fd: i32) -> i32;
                        fn _get_osfhandle(fd: i32) -> isize;
                    }
                    // CRT 与 Rust/Win32 标准输入可能各持一个 read handle，必须都关闭。
                    unsafe {
                        let handle = GetStdHandle(-10i32 as u32);
                        assert!(!handle.is_null() && handle as isize != -1);
                        let crt = _get_osfhandle(0);
                        if crt != -1 {
                            assert_eq!(_close(0), 0);
                        }
                        if crt != handle as isize {
                            assert_ne!(CloseHandle(handle), 0);
                        }
                    }
                    reply(&raw["id"], json!({"sessionId":"exact-session"}));
                    std::thread::sleep(std::time::Duration::from_secs(30));
                    return;
                }
                let mut result = effective_catalog(&mode);
                result["sessionId"] = json!("exact-session");
                reply(&raw["id"], result);
            }
            "session/load" => {
                let mode = fs::read_to_string(control.join("mode")).unwrap();
                assert!(mode.starts_with("continue-"));
                assert_eq!(raw["params"]["sessionId"], "exact-session");
                assert_eq!(raw["params"]["cwd"], json!(std::env::current_dir().unwrap()));
                assert_eq!(raw["params"]["mcpServers"], json!([]));
                fs::write(control.join("load.json"), raw.to_string()).unwrap();
                match mode.as_str() {
                    "continue-missing-history" => {}
                    "continue-unusable-history" => {
                        println!("{}", json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"exact-session","update":{"sessionUpdate":"config_option_update","configOptions":[]}}}));
                    }
                    "continue-empty-object" => {
                        println!("{}", json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"exact-session","update":{"sessionUpdate":"user_message_chunk","content":{}}}}));
                    }
                    "continue-empty-text" => {
                        println!("{}", json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"exact-session","update":{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":""}}}}));
                    }
                    "continue-whitespace-history" => {
                        println!("{}", json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"exact-session","update":{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":" \t\r\n"}}}}));
                    }
                    "continue-malformed-history" => {
                        println!("{}", json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"exact-session","update":{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":42}}}}));
                    }
                    "continue-wrong-session" => {
                        println!("{}", json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"wrong-session","update":{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"parent prompt sentinel"}}}}));
                    }
                    _ => {
                        // 冻结 replay 只用于证明 S1 历史存在；child prompt/result 不得消费这些正文。
                        println!("{}", json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"exact-session","update":{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"parent prompt sentinel"}}}}));
                        println!("{}", json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"exact-session","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"parent answer sentinel"}}}}));
                    }
                }
                io::stdout().flush().unwrap();
                if mode == "continue-load-mismatch" {
                    let mut result = effective_catalog(&mode);
                    result["sessionId"] = json!("other-session");
                    reply(&raw["id"], result);
                } else {
                    reply(&raw["id"], effective_catalog(&mode));
                }
            }
            "session/prompt" => {
                let mode = fs::read_to_string(control.join("mode")).unwrap();
                let execution = if mode.starts_with("continue-") {
                    "c"
                } else {
                    "e"
                };
                let acceptance = if mode.starts_with("continue-") {
                    "accepted-child"
                } else {
                    "accepted"
                };
                assert!(control.join(acceptance).exists());
                let (dispatch, intent, effective): (String, String, String) = db.query_row("SELECT dispatch_state,prompt_state,effective_execution_profile_json FROM executions JOIN codebuddy_execution_state ON executions.id=codebuddy_execution_state.execution_id WHERE executions.id=?1", [execution], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
                assert!(matches!(dispatch.as_str(), "dispatching" | "dispatched"));
                assert_eq!(intent, "sent");
                let effective: Value = serde_json::from_str(&effective).unwrap();
                if mode.ends_with("nonreasoning") {
                    assert_eq!(effective["model"], "model-plain");
                    assert!(effective["reasoning"].is_null());
                    assert_ne!(effective["reasoning"], "stale-high");
                } else {
                    assert_eq!(effective, json!({"model":"model-a","reasoning":"medium"}));
                }
                fs::write(control.join("prompt.json"), raw.to_string()).unwrap();
                if mode.starts_with("permission-") || mode == "continue-permission" {
                    if mode == "permission-cancel-first" {
                        pending_prompt = Some(raw);
                        fs::write(control.join("permission-ready"), "").unwrap();
                        continue;
                    }
                    if mode == "permission-write" { fs::write("marker.txt", b"CB8_PERMISSION_WRITE\n").unwrap(); }
                    let conversation = &raw["params"]["_meta"]["codebuddy.ai/conversationRequestId"];
                    println!("{}", json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"exact-session","update":{"sessionUpdate":"tool_call","toolCallId":"permission-tool","title":"private command","kind":"execute","status":"pending","_meta":{"codebuddy.ai/conversationRequestId":conversation}}}}));
                    let options = if mode == "permission-no-reject" { json!([{"optionId":"allow","kind":"allow_always","name":"private label"}]) }
                        else if mode == "permission-malformed" { json!([{"optionId":42,"kind":"reject_once","name":"private label"}]) }
                        else { json!([{"optionId":"allow","kind":"allow_once","name":"private label"},{"optionId":"advertised-deny-id","kind":"reject_once","name":"private label"}]) };
                    println!("{}", json!({"jsonrpc":"2.0","id":0,"method":"session/request_permission","params":{"sessionId":"exact-session","toolCall":{"toolCallId":"permission-tool","rawInput":{"command":"private command","env":"private env","argv":["private argv"]}},"options":options}}));
                    io::stdout().flush().unwrap();
                    pending_prompt = Some(raw);
                    fs::write(control.join("permission-ready"), "").unwrap();
                    continue;
                }
                if mode.starts_with("cancel-") || mode == "continue-cancel" {
                    if mode == "cancel-write" { fs::write("marker.txt", b"CB8_WRITE\n").unwrap(); }
                    if mode == "cancel-pipe" {
                        // 只关闭真实 stdin pipe，stdout 保持打开让 owner 的 cancel write 失败。
                        #[link(name = "kernel32")]
                        unsafe extern "system" {
                            fn GetStdHandle(which: u32) -> *mut std::ffi::c_void;
                            fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
                        }
                        #[link(name = "ucrt")]
                        unsafe extern "C" { fn _close(fd: i32) -> i32; fn _get_osfhandle(fd: i32) -> isize; }
                        // 测试进程独占标准输入；关闭 CRT/Win32 两种所有权来源。
                        unsafe {
                            let handle = GetStdHandle(-10i32 as u32);
                            let crt = _get_osfhandle(0);
                            if crt != -1 { assert_eq!(_close(0), 0); }
                            if crt != handle as isize { assert_ne!(CloseHandle(handle), 0); }
                        }
                        fs::write(control.join("cancel-ready"), "").unwrap();
                        std::thread::sleep(std::time::Duration::from_secs(30));
                        return;
                    }
                    pending_prompt = Some(raw);
                    fs::write(control.join("cancel-ready"), "").unwrap();
                    continue;
                }
                if mode == "eof" {
                    return;
                }
                if mode == "hold" {
                    std::thread::sleep(std::time::Duration::from_secs(30));
                    return;
                }
                if mode == "write" {
                    fs::write("output.txt", b"CB7_005_WRITE\n").unwrap();
                }
                let conversation = &raw["params"]["_meta"]["codebuddy.ai/conversationRequestId"];
                println!(
                    "{}",
                    json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"exact-session","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"safe result"},"_meta":{"codebuddy.ai/conversationRequestId":conversation}}}})
                );
                println!(
                    "{}",
                    json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"exact-session","update":{"sessionUpdate":"tool_call","toolCallId":"tool-1","title":"private command","kind":"read","status":"completed","_meta":{"codebuddy.ai/conversationRequestId":conversation}}}})
                );
                io::stdout().flush().unwrap();
                let mut meta = json!({"codebuddy.ai/conversationRequestId":conversation});
                if mode == "source-identity" {
                    meta["codebuddy.ai/requestId"] = json!("source-provider-request");
                }
                reply(&raw["id"], json!({"stopReason":"end_turn","_meta":meta}));
            }
            "session/cancel" => {
                assert_eq!(raw, json!({"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":"exact-session"}}));
                assert!(!control.join("cancel.json").exists());
                fs::write(control.join("cancel.json"), raw.to_string()).unwrap();
                let mode = fs::read_to_string(control.join("mode")).unwrap();
                if mode == "permission-cancel-first" {
                    let conversation = &pending_prompt.as_ref().unwrap()["params"]["_meta"]["codebuddy.ai/conversationRequestId"];
                    println!("{}", json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"exact-session","update":{"sessionUpdate":"tool_call","toolCallId":"permission-tool","title":"private command","status":"pending","_meta":{"codebuddy.ai/conversationRequestId":conversation}}}}));
                    println!("{}", json!({"jsonrpc":"2.0","id":0,"method":"session/request_permission","params":{"sessionId":"exact-session","toolCall":{"toolCallId":"permission-tool"},"options":[{"optionId":"advertised-deny-id","name":"Deny","kind":"reject_once"}]}}));
                    io::stdout().flush().unwrap();
                    continue;
                }
                if mode == "cancel-timeout" { continue; }
                let prompt = pending_prompt.take().expect("prompt before cancel");
                reply(&prompt["id"], json!({"stopReason":if mode == "cancel-end-turn" { "end_turn" } else { "cancelled" },"_meta":{"codebuddy.ai/conversationRequestId":prompt["params"]["_meta"]["codebuddy.ai/conversationRequestId"]}}));
            }
            "session/resume" => {
                fs::write(control.join("forbidden-session-resume"), raw.to_string()).unwrap();
                panic!("session/resume is not an allowed continuation method");
            }
            _ => panic!("unexpected request"),
        }
    }
}
