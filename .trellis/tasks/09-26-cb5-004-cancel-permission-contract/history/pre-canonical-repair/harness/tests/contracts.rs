//! Fake peer 与真实 SDK external streams 的合同回归。
use serde_json::Value;
use std::process::Command;

/// 子进程内部已有总期限，输出 evidence 用当前 test 临时目录隔离。
fn run(mode: &str) -> Value {
    let root = tempfile::tempdir().unwrap();
    let output = root.path().join("report.json");
    let result = Command::new(env!("CARGO_BIN_EXE_cb5-004-contract-probe"))
        .args([
            if mode.starts_with("permission") {
                "permission"
            } else {
                "before"
            },
            output.to_str().unwrap(),
            mode,
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    serde_json::from_slice(&std::fs::read(output).unwrap()).unwrap()
}
/// cancel notification 后只接受 exact prompt response，并保留 late update 次序。
#[test]
fn cancel_terminal_and_late_update_ordering() {
    let r = run("terminal");
    assert_eq!(r["providerTerminalReceived"], true);
    assert_eq!(r["protocol"]["terminal"]["stopReason"], "cancelled");
    assert!(r["cancel"]["sequence"].as_u64() < r["prompt"]["response"]["sequence"].as_u64());
    assert!(
        r["updates"]
            .as_array()
            .unwrap()
            .iter()
            .any(|u| u["afterTerminal"] == true)
    );
    assert_eq!(r["delta"], serde_json::json!([]));
}
/// 没有 terminal 时只能得到 timeout + runtime reaped，不能伪造 cancelled。
#[test]
fn cancel_no_terminal_timeout() {
    let r = run("timeout");
    assert_eq!(r["providerTerminalReceived"], false);
    assert_eq!(r["protocol"]["stage"], "no_terminal_timeout");
    assert!(r["protocol"].get("terminal").is_none());
    assert_eq!(r["runtimeTerminationEvidence"], true);
    assert_eq!(r["claimReleaseAuthorizedByCancel"], false);
    assert_eq!(r["workspaceDeleted"], true);
}

/// typed responder 只选择广告的 RejectOnce，deny 不覆盖真实 end_turn。
#[test]
fn permission_deny_then_exact_terminal() {
    let r = run("permission-terminal");
    assert_eq!(
        r["protocol"]["permissionDecisions"][0]["response"]["outcome"]["optionId"],
        "reject-advertised"
    );
    assert_eq!(r["protocol"]["terminal"]["stopReason"], "end_turn");
    assert_eq!(r["providerTerminalReceived"], true);
}

/// deny 自身不是 terminal；无响应仅清理 runtime。
#[test]
fn permission_deny_without_terminal_times_out() {
    let r = run("permission-timeout");
    assert_eq!(
        r["protocol"]["permissionDecisions"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(r["providerTerminalReceived"], false);
    assert_eq!(r["protocol"]["stage"], "no_terminal_timeout");
    assert_eq!(r["runtimeTerminationEvidence"], true);
}
