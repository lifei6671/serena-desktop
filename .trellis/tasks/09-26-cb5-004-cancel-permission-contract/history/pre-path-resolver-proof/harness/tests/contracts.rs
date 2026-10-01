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
            } else if ["mode-order", "missing-auto", "new-failed", "new-unknown"].contains(&mode) {
                "after"
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

/// new 成功响应后才发送 mode，mode ACK 后才发送 prompt；通知穿插不影响关联。
#[test]
fn new_response_precedes_catalog_mode_then_prompt() {
    let r = run("mode-order");
    let new = &r["sessionNew"];
    let mode = &r["modeChange"];
    assert_eq!(
        new["request"]["message"]["params"]["mcpServers"],
        serde_json::json!([])
    );
    assert_eq!(new["request"]["message"]["params"]["cwd"], r["cwd"]);
    assert_eq!(
        new["request"]["message"]["id"],
        new["response"]["message"]["id"]
    );
    assert!(new["response"]["sequence"].as_u64() < mode["request"]["sequence"].as_u64());
    assert!(mode["response"]["sequence"].as_u64() < r["prompt"]["request"]["sequence"].as_u64());
    assert_eq!(mode["request"]["message"]["params"]["modeId"], "auto");
    assert_eq!(r["protocol"]["setModeAcknowledged"], true);
}

/// 目录没有 auto 就不能猜测 mode，更不能继续 prompt。
#[test]
fn missing_auto_catalog_does_not_send_mode_or_prompt() {
    let r = run("missing-auto");
    assert!(r["modeChange"]["request"].is_null());
    assert!(r["prompt"]["request"].is_null());
    assert_eq!(r["protocol"]["stage"], "auto_mode_not_advertised");
}

/// failed/unknown session/new 绝不继续发 mode/config/prompt。
#[test]
fn failed_or_unknown_new_stops_dependent_requests() {
    for mode in ["new-failed", "new-unknown"] {
        let r = run(mode);
        assert!(r["modeChange"]["request"].is_null());
        assert!(r["prompt"]["request"].is_null());
        assert!(
            !r["wire"]
                .as_array()
                .unwrap()
                .iter()
                .any(|x| x["message"]["method"] == "session/set_config_option")
        );
        assert_eq!(r["providerTerminalReceived"], false);
    }
}
