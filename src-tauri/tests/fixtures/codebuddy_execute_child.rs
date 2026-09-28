//! CB7-005 隔离 native peer：control 在 cwd 之外，Workspace 只写指定 output。
use serde_json::{Value, json};
use std::{
    fs,
    io::{self, BufRead, Write},
    path::PathBuf,
};
/// 使用实际 SDK 请求 id 回复，不创建自己的 RPC identity。
fn reply(id: &Value, result: Value) {
    println!("{}", json!({"jsonrpc":"2.0","id":id,"result":result}));
    io::stdout().flush().unwrap();
}
/// 全部测试控制与证据写外部 temp control，避免污染 Workspace delta。
fn main() {
    let control: PathBuf = std::env::current_exe().unwrap().parent().unwrap().into();
    let db = rusqlite::Connection::open(control.join("agent-state.db")).unwrap();
    let mut pending_prompt: Option<Value> = None;
    for line in io::stdin().lock().lines() {
        let raw: Value = serde_json::from_str(&line.unwrap()).unwrap();
        match raw["method"].as_str().unwrap() {
            "initialize" => reply(&raw["id"], json!({"protocolVersion":1})),
            "session/new" => {
                if fs::read_to_string(control.join("mode")).unwrap() == "closed-input" {
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
                reply(&raw["id"], json!({"sessionId":"exact-session"}));
            }
            "session/prompt" => {
                assert!(control.join("accepted").exists());
                let (dispatch, intent): (String, String) = db.query_row("SELECT dispatch_state,prompt_state FROM executions JOIN codebuddy_execution_state ON executions.id=codebuddy_execution_state.execution_id", [], |r| Ok((r.get(0)?,r.get(1)?))).unwrap();
                assert!(matches!(dispatch.as_str(), "dispatching" | "dispatched"));
                assert_eq!(intent, "sent");
                fs::write(control.join("prompt.json"), raw.to_string()).unwrap();
                let mode = fs::read_to_string(control.join("mode")).unwrap();
                if mode.starts_with("cancel-") {
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
                reply(
                    &raw["id"],
                    json!({"stopReason":"end_turn","_meta":{"codebuddy.ai/conversationRequestId":conversation}}),
                );
            }
            "session/cancel" => {
                assert_eq!(raw, json!({"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":"exact-session"}}));
                assert!(!control.join("cancel.json").exists());
                fs::write(control.join("cancel.json"), raw.to_string()).unwrap();
                let mode = fs::read_to_string(control.join("mode")).unwrap();
                if mode == "cancel-timeout" { continue; }
                let prompt = pending_prompt.take().expect("prompt before cancel");
                reply(&prompt["id"], json!({"stopReason":if mode == "cancel-end-turn" { "end_turn" } else { "cancelled" },"_meta":{"codebuddy.ai/conversationRequestId":prompt["params"]["_meta"]["codebuddy.ai/conversationRequestId"]}}));
            }
            _ => panic!("unexpected request"),
        }
    }
}
