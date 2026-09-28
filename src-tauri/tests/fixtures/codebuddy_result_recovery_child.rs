//! CB8-004 native fake：只接受 initialize/session/load，并把原始请求写到 Workspace 外的 control。

use serde_json::{Value, json};
use std::{
    fs::{self, OpenOptions},
    io::{self, BufRead, Write},
    path::PathBuf,
};

/// 使用 SDK 生成的 exact request id 回复，不创建额外协议方法。
fn reply(id: &Value, result: Value) {
    println!("{}", json!({"jsonrpc":"2.0","id":id,"result":result}));
    io::stdout().flush().unwrap();
}

/// 所有控制文件位于 executable 目录；R2 cwd 仅用于验证 exact Workspace projection。
fn main() {
    let control: PathBuf = std::env::current_exe().unwrap().parent().unwrap().into();
    let mode = fs::read_to_string(control.join("mode")).unwrap();
    let conversation = fs::read_to_string(control.join("conversation")).unwrap();
    for line in io::stdin().lock().lines() {
        let line = line.unwrap();
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
        match raw["method"].as_str().unwrap_or("forbidden") {
            "initialize" => reply(
                &raw["id"],
                json!({"protocolVersion":1,"agentCapabilities":{"loadSession":mode != "no-capability"}}),
            ),
            "session/load" => {
                assert_eq!(raw["params"]["sessionId"], "exact-session");
                assert_eq!(raw["params"]["cwd"], json!(std::env::current_dir().unwrap()));
                assert_eq!(raw["params"]["mcpServers"], json!([]));
                match mode.as_str() {
                    "exact" => println!(
                        "{}",
                        json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"exact-session","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"recovered partial"},"_meta":{"codebuddy.ai/conversationRequestId":conversation}}}})
                    ),
                    "foreign" => println!(
                        "{}",
                        json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"exact-session","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"foreign secret"},"_meta":{"codebuddy.ai/conversationRequestId":"foreign"}}}})
                    ),
                    "wrong-session" => println!(
                        "{}",
                        json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"wrong","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"wrong session"},"_meta":{"codebuddy.ai/conversationRequestId":conversation}}}})
                    ),
                    "load-error" => {
                        println!("{}", json!({"jsonrpc":"2.0","id":raw["id"],"error":{"code":-32000,"message":"fake failure"}}));
                        io::stdout().flush().unwrap();
                        continue;
                    }
                    "load-mismatch" | "empty" | "no-capability" => {}
                    other => panic!("unexpected mode {other}"),
                }
                io::stdout().flush().unwrap();
                if mode == "load-mismatch" {
                    reply(&raw["id"], json!({"sessionId":"wrong"}));
                } else {
                    reply(&raw["id"], json!({}));
                }
            }
            method => {
                fs::write(control.join("forbidden-method"), method).unwrap();
                panic!("forbidden Result Recovery method {method}");
            }
        }
    }
}
