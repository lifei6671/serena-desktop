//! 确定性 subprocess fake peer 测试；每个场景有独立超时 watchdog。
use serde_json::Value;
use std::{
    process::{Command, Stdio},
    time::{Duration, Instant},
};

/// 运行本卡 fake peer 并在上限内回收 harness，避免测试本身挂起。
fn run(mode: &str) -> Value {
    let temp = tempfile::tempdir().unwrap();
    let output = temp.path().join("result.json");
    let mut child = Command::new(env!("CARGO_BIN_EXE_cb5-003-session-probe"))
        .args(["fixture", output.to_str().unwrap(), mode])
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        if start.elapsed() > Duration::from_secs(15) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("fixture watchdog timeout");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let result: Value = serde_json::from_slice(&std::fs::read(output).unwrap()).unwrap();
    assert_eq!(result["cleanup"]["directChildReaped"], true);
    assert_eq!(result["workspaceDeleted"], true);
    result
}
/// 缺 sessionId 不得继续发送 prompt。
#[test]
fn missing_identity() {
    let r = run("missing");
    assert_eq!(r["protocolSucceeded"], false);
    assert!(r["prompt"]["request"].is_null());
}
/// 空 sessionId 即使 SDK 接受仍 fail closed。
#[test]
fn empty_identity() {
    let r = run("empty");
    assert_eq!(r["sdkTyped"]["failure"], "EMPTY_SESSION_ID");
    assert!(r["prompt"]["request"].is_null());
}
/// 非字符串 sessionId 由官方模型拒绝。
#[test]
fn malformed_session() {
    let r = run("malformed");
    assert_eq!(r["protocolSucceeded"], false);
    assert!(r["prompt"]["request"].is_null());
}
/// prompt 期间 EOF 不得伪造 terminal。
#[test]
fn prompt_eof() {
    let r = run("eof");
    assert_eq!(r["protocolSucceeded"], false);
    assert_eq!(r["sdkTyped"]["stage"], "session/prompt");
    assert!(r["prompt"]["response"].is_null());
}
/// prompt 超时必须退出 SDK 并回收子进程。
#[test]
fn prompt_timeout() {
    let r = run("timeout");
    assert_eq!(r["error"], "RPC_TIMEOUT");
    assert!(r["prompt"]["response"].is_null());
}
/// update 必须在 raw terminal 之前；SDK 未建模 extension 仍保留。
#[test]
fn update_then_terminal_preserved() {
    let r = run("success");
    assert_eq!(r["canExecuteEvidence"], true);
    let u = r["updates"][0]["sequence"].as_u64().unwrap();
    let t = r["prompt"]["response"]["sequence"].as_u64().unwrap();
    assert!(u < t);
    assert_eq!(
        r["prompt"]["response"]["message"]["result"]["stopReason"],
        "end_turn"
    );
    assert_eq!(
        r["prompt"]["response"]["message"]["result"]["fixtureExtra"]["result"],
        true
    );
    assert!(r["sdkTyped"]["terminal"].get("fixtureExtra").is_none());
}
/// 错误 terminal JSON 类型被 SDK 拒绝，raw 仍作为反证保留。
#[test]
fn malformed_terminal_preserved() {
    let r = run("bad-terminal");
    assert_eq!(r["protocolSucceeded"], false);
    assert_eq!(
        r["prompt"]["response"]["message"]["result"]["stopReason"],
        5
    );
}
