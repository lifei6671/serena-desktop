//! Stage C纯fake/unit测试；禁止调用真实mode/CodeBuddy。
use super::*;

/// 测试launcher仅指向Python合成peer，日志位于workspace之外。
fn fake(mode: &str, log: &Path) -> Vec<String> {
    vec![
        r"C:\Users\lifei\AppData\Local\Programs\Python\Python312\python.exe".into(),
        "-B".into(),
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fake_crash_peer.py")
            .to_string_lossy()
            .into(),
        mode.into(),
        log.to_string_lossy().into(),
    ]
}

/// 合成内存wire行；不需外部进程即可测试ordering。
fn row(sequence: u64, direction: &str, message: Value) -> Value {
    json!({"sequence":sequence,"direction":direction,"message":message})
}

/// 合成带exact Session/request/message的typed chunk。
fn chunk(sequence: u64, session: &str, request: &str, message: &str, text: &str) -> Value {
    row(
        sequence,
        "response",
        json!({"method":"session/update","params":{"sessionId":session,"update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":text},"_meta":{crash::REQUEST:request,crash::MESSAGE:message}}}}),
    )
}

/// trigger只看当前request的activity，并且terminal在同一snapshot中优先。
#[test]
fn crash_trigger_identity_terminal_priority() {
    let q = row(1, "request", json!({"id":"p","method":"session/prompt"}));
    let mut rows = vec![q, chunk(2, "S1", "wrong", "M1", "PRIVATE")];
    assert_eq!(crash::trigger(&rows, "S1", "C1").unwrap(), None);
    rows.push(chunk(3, "S1", "C1", "M1", "PRIVATE"));
    assert_eq!(
        crash::trigger(&rows, "S1", "C1").unwrap(),
        Some("ACTIVITY_WITHOUT_TERMINAL")
    );
    rows.push(row(
        4,
        "response",
        json!({"id":"p","result":{"stopReason":"end_turn"}}),
    ));
    assert_eq!(
        crash::trigger(&rows, "S1", "C1").unwrap(),
        Some("TERMINAL_FIRST")
    );
    rows.pop();
    rows.push(row(
        4,
        "response",
        json!({"id":"p","error":{"code":-32603}}),
    ));
    assert_eq!(
        crash::trigger(&rows, "S1", "C1").unwrap(),
        Some("PROMPT_RESPONSE_ERROR")
    );
    assert!(crash::project(&rows, "S1", "C1", false, None).unwrap()["terminalSequence"].is_null());
    rows.pop();
    rows[1] = chunk(2, "wrong", "C1", "M1", "PRIVATE");
    assert!(crash::trigger(&rows, "S1", "C1").is_err());
}

/// replay按message组装/去重；无terminal即使hash相等也只partial。
#[test]
fn crash_replay_grouping_identity_privacy_no_terminal() {
    let rows = vec![
        row(1, "request", json!({"id":"load","method":"session/load"})),
        chunk(2, "S1", "C1", "M1", "PRIVATE_"),
        chunk(3, "S1", "C1", "M1", "PRIVATE_"),
        chunk(4, "S1", "C1", "M1", "ANSWER"),
        chunk(5, "S1", "C1", "M2", "!"),
    ];
    let live = json!({"answerSha256":hash(b"PRIVATE_ANSWER!"),"answerLength":15,"messageIdHashes":[hash(b"M1"),hash(b"M2")]});
    let p = crash::project(&rows, "S1", "C1", true, Some(&live)).unwrap();
    assert_eq!(p["answerMatchesLive"], true);
    assert_eq!(p["duplicateReplayFragments"], 1);
    assert_eq!(p["messageIdHashes"].as_array().unwrap().len(), 2);
    assert_eq!(p["resultCompleteness"], "partial");
    assert_eq!(p["oldPromptTerminalWireEvidence"], false);
    for flag in [
        "businessCompletedRecoverable",
        "runtimeTerminationEvidenceProven",
        "claimReleasePermitted",
    ] {
        assert_eq!(p[flag], false);
    }
    assert!(!p.to_string().contains("PRIVATE"));
    assert!(!p.to_string().contains("C1"));
    let wrong = crash::project(&rows, "S2", "C1", true, None).unwrap();
    assert_eq!(wrong["resultCompleteness"], "unknown");
    assert_eq!(wrong["answerLength"], 0);
    let wrong = crash::project(&rows, "S1", "C2", true, None).unwrap();
    assert_eq!(wrong["resultCompleteness"], "unknown");
    assert_eq!(wrong["answerLength"], 0);
}

/// 即使未来扩展出现stopReason也只能Material Contract Difference。
#[test]
fn crash_terminal_extension_never_upgrades_completed() {
    let rows = vec![
        row(1, "request", json!({"id":"l","method":"session/load"})),
        chunk(2, "S1", "C1", "M1", "text"),
        row(
            3,
            "response",
            json!({"id":"l","result":{"stopReason":"end_turn","_meta":{crash::REQUEST:"C1"}}}),
        ),
    ];
    let p = crash::project(&rows, "S1", "C1", true, None).unwrap();
    assert_eq!(p["materialContractDifference"], true);
    assert_eq!(p["businessCompletedRecoverable"], false);
    assert_eq!(p["resultCompleteness"], "partial");

    // 明确旧RPC/target身份的未来wire也只记录差异，不改变business mapping。
    let mut exact_rows = rows.clone();
    exact_rows[2]["message"]["id"] = json!("old-prompt");
    let p = crash::project(
        &exact_rows,
        "S1",
        "C1",
        true,
        Some(&json!({"promptRpcId":"old-prompt"})),
    )
    .unwrap();
    assert_eq!(p["oldPromptTerminalWireEvidence"], true);
    assert_eq!(p["materialContractDifference"], true);
    assert_eq!(p["businessCompletedRecoverable"], false);
}

/// manifest包含隐藏项且拒绝超预算文件；链接边界复用既有junction回归。
#[test]
fn crash_manifest_hidden_and_byte_bound() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join(".hidden"), b"not-answer").unwrap();
    assert_eq!(manifest(root.path()).unwrap().len(), 1);
    let file = std::fs::File::create(root.path().join("oversized")).unwrap();
    file.set_len(33 * 1024 * 1024).unwrap();
    assert!(manifest(root.path()).is_err());
}

/// 每mode sentinel独立，已有任何产物不能重放；没有output参数。
#[test]
fn crash_sentinel_no_retry_and_fixed_modes() {
    let root = tempfile::tempdir().unwrap();
    for mode in ["crash-before-terminal", "crash-after-terminal"] {
        crash::reserve(root.path(), mode).unwrap();
        let path = root.path().join(format!("{mode}.attempt-started.json"));
        let bytes = std::fs::read(&path).unwrap();
        assert!(crash::reserve(root.path(), mode).is_err());
        assert_eq!(std::fs::read(path).unwrap(), bytes);
        assert_eq!(parse_scenario(&["exe".into(), mode.into()]), Some(mode));
        assert!(parse_scenario(&["exe".into(), mode.into(), "--force".into()]).is_none());
    }
    assert!(crash::reserve(root.path(), "crash-repair").is_err());
    assert!(crash_runtime::output_root().ends_with("evidence/crash"));
}

/// 在线半帧不触发；坏JSON仍拒绝，完整帧按全局顺序保留。
#[test]
fn crash_partial_frame_boundary() {
    let wire: Wire = Arc::default();
    wire.lock()
        .unwrap()
        .push(("response".into(), b"{\"id\":1".to_vec()));
    let (rows, incomplete) = crash::snapshot(&wire).unwrap();
    assert!(rows.is_empty() && incomplete);
    wire.lock()
        .unwrap()
        .push(("response".into(), b"}\n".to_vec()));
    assert!(!crash::snapshot(&wire).unwrap().1);
    wire.lock()
        .unwrap()
        .push(("response".into(), b"not-json\n".to_vec()));
    assert!(crash::snapshot(&wire).is_err());
}

/// 两个目标窗口真实跑fake child，验证reap、exact历史和write-ahead身份。
#[tokio::test(flavor = "current_thread")]
async fn crash_fake_both_windows() {
    for (mode, peer) in [
        ("crash-before-terminal", "before"),
        ("crash-after-terminal", "after"),
    ] {
        let root = tempfile::tempdir().unwrap();
        let log = root.path().join("calls.jsonl");
        let report =
            crash_runtime::scenario(mode, root.path(), &fake(peer, &log), Duration::from_secs(5))
                .await
                .unwrap();
        assert_eq!(report["status"], "PASS", "{report}");
        assert_eq!(report["resultCompleteness"], "partial");
        assert_eq!(report["workspaceDelta"], json!([]));
        assert_eq!(report["workspaceDeleted"], true);
        for name in ["r1", "r2"] {
            assert_eq!(report[name]["cleanup"]["directChildReaped"], true);
            assert_eq!(report[name]["runtimeTerminationEvidenceProven"], false);
        }
        assert_eq!(report["r2"]["promptRequestCount"], 0);
        assert_eq!(
            report["r2"]["projection"]["oldPromptTerminalWireEvidence"],
            false
        );
        if peer == "after" {
            assert_eq!(report["r2"]["projection"]["answerMatchesLive"], true);
        }
        let identity: Value = serde_json::from_slice(
            &std::fs::read(root.path().join(format!("{mode}.prompt-identity.json"))).unwrap(),
        )
        .unwrap();
        assert_eq!(identity["requestId"], report["r1"]["requestId"]);
        assert_eq!(identity["promptState"], "prepared");
        assert!(identity.get("terminalStopReason").is_none());
        let calls = std::fs::read_to_string(log).unwrap();
        assert_eq!(calls.matches("session/prompt").count(), 1);
        assert!(!calls.contains("session/resume"));
        for private in ["PRIVATE_ANSWER", "PRIVATE_PROMPT", "Read-only probe"] {
            assert!(!report.to_string().contains(private));
        }
    }
}

/// terminal先到只能NOT_OBSERVED，不启动第二次prompt或重试窗口。
#[tokio::test(flavor = "current_thread")]
async fn crash_terminal_first_no_retry() {
    let root = tempfile::tempdir().unwrap();
    let log = root.path().join("calls.jsonl");
    let r = crash_runtime::scenario(
        "crash-before-terminal",
        root.path(),
        &fake("terminal-first", &log),
        Duration::from_secs(5),
    )
    .await
    .unwrap();
    assert_eq!(r["status"], "NOT_OBSERVED", "{r}");
    assert_eq!(r["r2Status"], "NOT_RUN");
    assert_eq!(
        std::fs::read_to_string(log)
            .unwrap()
            .matches("session/prompt")
            .count(),
        1
    );
}

/// load失败/错identity不fallback且R2永远无prompt；错误terminal不能进入R2。
#[tokio::test(flavor = "current_thread")]
async fn crash_failed_load_and_identity_no_fallback() {
    for peer in [
        "failed-load",
        "wrong-request",
        "wrong-message",
        "wrong-session",
        "wrong-terminal",
        "terminal-extension",
    ] {
        let root = tempfile::tempdir().unwrap();
        let log = root.path().join("calls.jsonl");
        let r = crash_runtime::scenario(
            "crash-after-terminal",
            root.path(),
            &fake(peer, &log),
            Duration::from_secs(5),
        )
        .await
        .unwrap();
        assert_eq!(r["status"], "PARTIAL", "{peer}: {r}");
        assert_eq!(r["claimReleasePermitted"], false);
        let calls = std::fs::read_to_string(log).unwrap();
        assert_eq!(calls.matches("session/prompt").count(), 1);
        assert!(!calls.contains("session/resume"));
        if peer == "wrong-terminal" {
            assert_eq!(r["r2Status"], "NOT_RUN");
        }
    }
}

/// 超时触发owned child清理，但不能被升级为before-terminal成功或Job证据。
#[tokio::test(flavor = "current_thread")]
async fn crash_timeout_cleanup() {
    let root = tempfile::tempdir().unwrap();
    let log = root.path().join("calls.jsonl");
    let r = crash_runtime::scenario(
        "crash-before-terminal",
        root.path(),
        &fake("uncorrelated", &log),
        Duration::from_millis(400),
    )
    .await
    .unwrap();
    assert_eq!(r["status"], "PARTIAL");
    assert_eq!(r["r1"]["cleanup"]["succeeded"], true);
    assert_eq!(r["r2Status"], "NOT_RUN");
}

/// 完整fake结果注入cleanup/manifest失败后必须降级，结果恢复不授权释放。
#[tokio::test(flavor = "current_thread")]
async fn crash_cleanup_and_manifest_gate() {
    let root = tempfile::tempdir().unwrap();
    let log = root.path().join("calls.jsonl");
    let mode = "crash-after-terminal";
    let mut r = crash_runtime::scenario(
        mode,
        root.path(),
        &fake("after", &log),
        Duration::from_secs(5),
    )
    .await
    .unwrap();
    assert_eq!(
        crash_runtime::grade(mode, &r["r1"], &r["r2"], false),
        "PARTIAL"
    );
    r["r2"]["cleanup"]["succeeded"] = json!(false);
    assert_eq!(
        crash_runtime::grade(mode, &r["r1"], &r["r2"], true),
        "PARTIAL"
    );
}
