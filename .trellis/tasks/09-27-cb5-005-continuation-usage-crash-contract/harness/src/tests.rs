//! Deterministic tests 只使用本地 Python fake child，绝不启动真实 launcher。
use super::*;
const TOKEN: &str = "01900000-0000-7000-8000-000000000001";

/// 构建隔离 fake launcher，log 在被测 workspace 之外。
fn fake(mode: &str, method: &str, log: &Path) -> Vec<String> {
    vec![
        r"C:\Users\lifei\AppData\Local\Programs\Python\Python312\python.exe".into(),
        "-B".into(),
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fake_peer.py")
            .to_string_lossy()
            .into(),
        mode.into(),
        method.into(),
        log.to_string_lossy().into(),
    ]
}

/// SDK typed serialization 的实际差异要按官方类型断言。
#[test]
fn typed_load_resume_wire() {
    let cwd = std::env::temp_dir();
    let load = serde_json::to_value(LoadSessionRequest::new("S1", cwd.clone()).mcp_servers(vec![]))
        .unwrap();
    let resume =
        serde_json::to_value(ResumeSessionRequest::new("S1", cwd.clone()).mcp_servers(vec![]))
            .unwrap();
    assert_eq!(load["mcpServers"], json!([]));
    assert!(resume.get("mcpServers").is_none());
    for v in [load, resume] {
        assert_eq!(v["sessionId"], "S1");
        assert_eq!(v["cwd"], json!(cwd));
        assert!(v.get("mcpServers").is_none() || v["mcpServers"] == json!([]));
    }
}

/// cwd/session 不匹配一律拒绝。
#[test]
fn wrong_session_and_cwd() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    assert!(identity_guard("S1", "wrong", a.path(), a.path()).is_err());
    assert!(identity_guard("S1", "S1", a.path(), b.path()).is_err());
    assert!(identity_guard("S1", "S1", a.path(), a.path()).is_ok());
    let mut rows = vec![
        json!({"direction":"request","message":{"id":3,"method":"session/resume","params":{"sessionId":"S1","cwd":b.path()}}}),
        json!({"direction":"response","message":{"id":3,"result":{}}}),
    ];
    assert!(recovery_guard(&rows, "session/resume", "S1", a.path(), a.path()).is_err());
    rows[0]["message"]["params"]["cwd"] = json!(a.path());
    assert!(recovery_guard(&rows, "session/resume", "S1", a.path(), a.path()).is_ok());
    rows[0]["message"]["params"]["sessionId"] = json!("S2");
    assert!(recovery_guard(&rows, "session/resume", "S1", a.path(), a.path()).is_err());
}

/// 首次落盘后无论成功或失败，都不能重放 sentinel。
#[test]
fn sentinel_no_replay() {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join("resume.attempt-started.json");
    durable(&p, &json!({"attempt":1})).unwrap();
    assert!(durable(&p, &json!({"attempt":2})).is_err());
    assert_eq!(
        serde_json::from_slice::<Value>(&std::fs::read(p).unwrap()).unwrap()["attempt"],
        1
    );
}

/// 隐藏文件目录也进入完整 manifest，内容只留 SHA256。
#[test]
fn complete_manifest_no_content() {
    let t = tempfile::tempdir().unwrap();
    assert!(manifest(t.path()).unwrap().is_empty());
    std::fs::create_dir(t.path().join(".hidden")).unwrap();
    std::fs::write(t.path().join(".hidden/file"), b"PRIVATE_SOURCE").unwrap();
    let m = manifest(t.path()).unwrap();
    assert_eq!(m.len(), 2);
    assert!(!json!(m).to_string().contains("PRIVATE_SOURCE"));
}

/// 独立恢复 methods 均运行 exact S1；early replay 不混入 live hash。
#[tokio::test]
async fn independent_methods_early_replay_live_and_privacy() {
    for method in ["resume", "load"] {
        let t = tempfile::tempdir().unwrap();
        let logs = tempfile::tempdir().unwrap();
        let log = logs.path().join("log");
        let r = runtime(
            t.path(),
            Some((method, "S1")),
            TOKEN,
            &fake("ok", method, &log),
            Duration::from_secs(8),
        )
        .await
        .unwrap();
        assert_eq!(r["error"], Value::Null, "{r}");
        assert_eq!(r["matched"], true, "{r}");
        assert_eq!(r["finalAnswerSha256"], hash(TOKEN.as_bytes()));
        assert_eq!(r["finalAnswerLength"], TOKEN.len());
        assert_eq!(r["recoveryUpdateCount"], 2);
        assert_eq!(r["cleanup"]["succeeded"], true);
        let text = r.to_string();
        for secret in [TOKEN, "PRIVATE_REPLAY", "PRIVATE_LATE", "STDERR_SECRET"] {
            assert!(!text.contains(secret));
        }
        let calls = std::fs::read_to_string(log).unwrap();
        assert!(calls.contains(&format!("session/{method}")));
        assert!(!calls.contains(if method == "load" {
            "session/resume"
        } else {
            "session/load"
        }));
        assert!(
            r["updates"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v["phase"] == "late")
        );
    }
}

/// 不支持/错误身份必须在发送 P2 前失败，且不能 fallback。
#[tokio::test]
async fn failed_recovery_no_prompt_no_fallback() {
    for mode in ["unsupported", "wrong-update", "wrong-response"] {
        for method in ["resume", "load"] {
            let t = tempfile::tempdir().unwrap();
            let logs = tempfile::tempdir().unwrap();
            let log = logs.path().join("log");
            let r = runtime(
                t.path(),
                Some((method, "S1")),
                TOKEN,
                &fake(mode, method, &log),
                Duration::from_secs(8),
            )
            .await
            .unwrap();
            assert!(!r["error"].is_null(), "{r}");
            assert!(r["promptRpcId"].is_null());
            assert_eq!(r["cleanup"]["directChildReaped"], true);
            let calls = std::fs::read_to_string(log).unwrap();
            assert!(!calls.contains("session/prompt"));
            assert!(!calls.contains(if method == "load" {
                "session/resume"
            } else {
                "session/load"
            }));
            if mode == "unsupported" {
                assert_eq!(r["rpcErrorCode"], -32601);
            }
        }
    }
}

/// 恢复 ACK 不代表记忆保留，忘记 token 不可 PASS。
#[tokio::test]
async fn memory_not_retained() {
    let t = tempfile::tempdir().unwrap();
    let logs = tempfile::tempdir().unwrap();
    let r = runtime(
        t.path(),
        Some(("resume", "S1")),
        TOKEN,
        &fake("forgot", "resume", &logs.path().join("log")),
        Duration::from_secs(8),
    )
    .await
    .unwrap();
    assert_eq!(r["terminalReceived"], true);
    assert_eq!(r["matched"], false);
}

/// 超时与超大帧都必须有界回收 owned child。
#[tokio::test]
async fn timeout_and_frame_limit_cleanup() {
    for mode in ["timeout", "large-frame"] {
        let t = tempfile::tempdir().unwrap();
        let logs = tempfile::tempdir().unwrap();
        let r = runtime(
            t.path(),
            None,
            TOKEN,
            &fake(mode, "resume", &logs.path().join("log")),
            Duration::from_millis(500),
        )
        .await
        .unwrap();
        assert!(!r["error"].is_null());
        assert_eq!(r["cleanup"]["directChildReaped"], true, "{r}");
        assert_eq!(r["cleanup"]["windowsJobAtCreationProven"], false);
    }
}

/// fresh new 期间的 early notifications 不能误配为 new 响应。
#[tokio::test]
async fn new_dispatch_and_terminal_identity() {
    let t = tempfile::tempdir().unwrap();
    let logs = tempfile::tempdir().unwrap();
    let r = runtime(
        t.path(),
        None,
        TOKEN,
        &fake("ok", "resume", &logs.path().join("log")),
        Duration::from_secs(8),
    )
    .await
    .unwrap();
    assert_eq!(r["sessionId"], "S1");
    assert_eq!(r["terminalStopReason"], "end_turn");
    assert!(!r["promptRpcId"].is_null());
    assert_ne!(r["promptRpcId"], r["recoveryRpcId"]);
}

/// 完整双 runtime 串联必须同 S1/cwd；随机 token 未回忆不得虚报 PASS。
#[tokio::test]
async fn full_scenario_same_workspace_and_partial_memory() {
    for method in ["resume", "load"] {
        let logs = tempfile::tempdir().unwrap();
        let log = logs.path().join("log");
        let r = scenario(method, &fake("ok", method, &log), Duration::from_secs(8))
            .await
            .unwrap();
        assert_eq!(r["status"], "PARTIAL", "{r}");
        assert_eq!(r["r2"]["matched"], false);
        assert_eq!(r["r1"]["sessionId"], r["r2"]["sessionId"]);
        assert_eq!(r["workspaceDelta"], json!([]));
        assert_eq!(r["workspaceDeleted"], true);
        assert_eq!(r["r1"]["cleanup"]["succeeded"], true);
        assert_eq!(r["r2"]["cleanup"]["succeeded"], true);
        assert_ne!(r["r1"]["runtimeInstanceId"], r["r2"]["runtimeInstanceId"]);
        let rows: Vec<Value> = std::fs::read_to_string(log)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        let cwd: Vec<_> = rows.iter().filter_map(|v| v["cwd"].as_str()).collect();
        assert_eq!(cwd.len(), 2);
        assert_eq!(cwd[0], cwd[1]);
        assert_eq!(
            rows.iter()
                .filter(|v| v["method"] == "session/prompt")
                .count(),
            2
        );
    }
}

/// late replay 无当前 correlation 时不计入结果，并阻止完全通过。
#[tokio::test]
async fn ambiguous_live_never_counted_as_answer() {
    let t = tempfile::tempdir().unwrap();
    let logs = tempfile::tempdir().unwrap();
    let r = runtime(
        t.path(),
        Some(("load", "S1")),
        TOKEN,
        &fake("ambiguous", "load", &logs.path().join("log")),
        Duration::from_secs(8),
    )
    .await
    .unwrap();
    assert_eq!(r["unattributedAnswerChunks"], 1);
    assert_eq!(r["finalAnswerSha256"], hash(TOKEN.as_bytes()));
    assert!(!r.to_string().contains("PRIVATE_LATE_REPLAY"));
}

/// 参数不能换输出路径/force/retry，root 必须固定在本任务。
#[test]
fn fixed_output_and_no_force_parameters() {
    assert_eq!(
        parse_scenario(&["exe".into(), "resume".into()]),
        Some("resume")
    );
    for args in [
        vec!["exe", "load", "other.json"],
        vec!["exe", "resume", "--force"],
        vec!["exe", "crash"],
        vec!["exe"],
    ] {
        assert!(parse_scenario(&args.into_iter().map(String::from).collect::<Vec<_>>()).is_none());
    }
    assert!(evidence_root().is_absolute());
    assert!(
        evidence_root()
            .ends_with("09-27-cb5-005-continuation-usage-crash-contract/evidence/continuation")
    );
}

/// 巨量小 frame 的队列边界也必须生效。
#[test]
fn frame_count_is_bounded() {
    let wire: Wire = Arc::default();
    wire.lock()
        .unwrap()
        .push(("response".into(), b"{}\n".repeat(4097)));
    assert!(frames(&wire).is_err());
}

/// Windows junction 不可穿越；清理只删除已知 link 本身。
#[cfg(windows)]
#[test]
fn junction_manifest_fails_closed() {
    use std::os::windows::process::CommandExt;
    let root = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let link = root.path().join("junction");
    let status = std::process::Command::new(r"C:\Windows\System32\cmd.exe")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(&link)
        .arg(target.path())
        .creation_flags(0x08000000)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(status.success());
    let rejected = manifest(root.path()).is_err();
    std::fs::remove_dir(&link).unwrap();
    assert!(rejected);
}

/// kill 报错仍尝试 wait，失败不能伪装成 cleanup 成功。
#[tokio::test]
async fn cleanup_kill_error_still_waits() {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .arg("--list")
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let mut stderr = tokio::spawn(async { Vec::new() });
    let r = cleanup_with_kill(&mut child, &mut stderr, None, |_| {
        Err(io::Error::other("INJECTED"))
    })
    .await;
    assert_eq!(r["directChildReaped"], true);
    assert_eq!(r["succeeded"], false);
}

/// 畸形 Provider 身份不得进入报告，失败后不发 P1。
#[tokio::test]
async fn invalid_new_identity_never_persisted() {
    let t = tempfile::tempdir().unwrap();
    let logs = tempfile::tempdir().unwrap();
    let log = logs.path().join("log");
    let r = runtime(
        t.path(),
        None,
        TOKEN,
        &fake("invalid-new-id", "resume", &log),
        Duration::from_secs(8),
    )
    .await
    .unwrap();
    assert_eq!(r["state"]["stage"], "INVALID_SESSION_ID");
    assert!(r["promptRpcId"].is_null());
    assert!(!r.to_string().contains("PRIVATE_INVALID_ID"));
    assert!(!r.to_string().contains("secret text"));
    assert_eq!(r["sessionId"], "");
    assert_eq!(r["response"]["sessionIdPresent"], true);
    assert!(r["response"]["sessionId"].is_null());
    assert!(
        !std::fs::read_to_string(log)
            .unwrap()
            .contains("session/prompt")
    );
    assert_eq!(r["cleanup"]["succeeded"], true);
}
