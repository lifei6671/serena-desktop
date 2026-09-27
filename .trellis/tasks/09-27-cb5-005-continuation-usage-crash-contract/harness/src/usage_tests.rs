//! Stage B 测试，只运行本地fake，不重跑Stage A真实场景。
use super::*;

/// 假进程的安全日志在workspace外，只含method/identity。
fn launcher(mode: &str, log: &Path) -> Vec<String> {
    vec![
        r"C:\Users\lifei\AppData\Local\Programs\Python\Python312\python.exe".into(),
        "-B".into(),
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fake_usage_peer.py")
            .to_string_lossy()
            .into(),
        mode.into(),
        log.to_string_lossy().into(),
    ]
}

/// 数字父级不豁免敏感子字段；字符串/array值以及未知键原文都不能落盘。
#[test]
fn usage_numeric_projection_privacy() {
    let v = json!({"used":12,"size":100,"usage":{"inputTokens":7,"outputTokens":"PRIVATE_NUMBER","accessToken":987654321,"secretCost":{"amount":87654321},"bool":false,"nil":null,"note":"PRIVATE_TEXT","array":[123456789,"PRIVATE_ARRAY"]},"PRIVATE_KEY":3});
    let p = usage::project(&v, true, &mut 100).unwrap();
    let text = json!(p).to_string();
    for forbidden in [
        "PRIVATE_NUMBER",
        "PRIVATE_TEXT",
        "PRIVATE_ARRAY",
        "PRIVATE_KEY",
        "987654321",
        "87654321",
        "123456789",
    ] {
        assert!(!text.contains(forbidden), "{text}");
    }
    assert!(
        p.iter()
            .any(|f| f["semantic"] == "inputTokens" && f["value"] == 7)
    );
    assert!(!p.iter().any(|f| f["semantic"] == "outputTokens"));
    assert!(p.iter().any(|f| f["kind"] == "bool" && f["value"] == false));
    assert!(p.iter().any(|f| f["kind"] == "null"));
}

/// 限制info/result为语义相关字段，secret-looking路径不因包含token变成breakdown。
#[test]
fn usage_semantic_allowlist_not_substring_guess() {
    let p=usage::project(&json!({"count":123456,"_meta":{"usage":{"input_tokens":5,"outputTokens":7,"myTokenCount":10,"authTokenCount":919191,"totalTokens":12},"unrelated":456789}}),false,&mut 100).unwrap();
    let text = json!(p).to_string();
    assert!(!text.contains("123456"));
    assert!(!text.contains("456789"));
    assert!(!text.contains("919191"));
    assert_eq!(p.iter().filter(|f| !f["semantic"].is_null()).count(), 3);
    assert!(
        p.iter()
            .any(|f| f["value"] == 10 && f["semantic"].is_null())
    );
}

/// 构建已经typed验证的数字快照，用于纯分析边界。
fn snapshot(runtime: usize, seq: usize, used: u64, size: u64) -> Value {
    json!({"runtime":runtime,"sequence":seq,"source":"usage_update","typedUsageUpdate":true,"placement":{"phase":"late","lateFor":"P1","attribution":"window_only"},"fields":usage::project(&json!({"used":used,"size":size}),true,&mut 100).unwrap()})
}

/// used下降/归零只是观察，不能求累计delta，也不能覆盖最后正值。
#[test]
fn usage_decrease_reset_no_counter_derivation() {
    let report = json!({"status":"PASS","r1":{"collection":{"samples":[snapshot(1,1,100,1000),snapshot(1,2,80,1000)]}},"r2":{"collection":{"samples":[snapshot(2,1,0,1000)]}}});
    let a = usage::analyze(&report);
    assert_eq!(
        a["observations"]["transitions"][0]["usedRelation"],
        "decreased"
    );
    assert_eq!(
        a["observations"]["restart"]["usedRelation"],
        "zero_after_positive"
    );
    assert_eq!(a["observations"]["lastPositiveSnapshot"]["sequence"], 2);
    assert_eq!(a["observations"]["latestSnapshot"]["runtime"], 2);
    assert_eq!(a["interpretation"]["token_usage"], false);
    assert!(a["interpretation"]["derivedTokenTotals"].is_null());
    assert_eq!(a["interpretation"]["usedSize"], "context_occupancy_gauge");
}

/// 稳定/改变仅按实际快照比较；保留大于f64精度范围的u64差异。
#[test]
fn usage_size_stability_and_integer_precision() {
    for (size, stable) in [(1000, true), (2000, false)] {
        let a = usage::analyze(
            &json!({"r1":{"collection":{"samples":[snapshot(1,1,9007199254740993,1000),snapshot(1,2,9007199254740992,size)]}}}),
        );
        assert_eq!(a["observations"]["sizeAcrossSessionStable"], stable);
        assert_eq!(
            a["observations"]["transitions"][0]["usedRelation"],
            "decreased"
        );
    }
}

/// 仅有gauge或完全无快照都不能开启public Usage。
#[test]
fn usage_missing_breakdown_is_unknown() {
    let a = usage::analyze(&json!({}));
    assert_eq!(a["interpretation"]["token_usage"], false);
    assert_eq!(a["interpretation"]["publicUsage"], "unknown");
    assert_eq!(a["interpretation"]["usedSize"], "UNKNOWN");
}

/// Phase/terminal/late：P1迟到帧即便在P2期间仍归P1，不能合并成P2。
#[tokio::test]
async fn usage_runtime_phase_terminal_late_and_privacy() {
    let cwd = tempfile::tempdir().unwrap();
    let logs = tempfile::tempdir().unwrap();
    let log = logs.path().join("log");
    let r = usage_runtime::run_runtime(
        cwd.path(),
        None,
        &launcher("ok", &log),
        Duration::from_secs(8),
    )
    .await
    .unwrap();
    assert!(r["error"].is_null(), "{r}");
    assert_eq!(r["cleanup"]["succeeded"], true);
    assert_eq!(r["collection"]["turns"].as_array().unwrap().len(), 2);
    let samples = r["collection"]["samples"].as_array().unwrap();
    assert!(samples.iter().any(|s| s["placement"]["phase"] == "new"));
    assert!(samples.iter().any(|s| s["placement"]["phase"] == "late"
        && s["placement"]["phaseAtReceipt"] == "P2"
        && s["placement"]["attributedTurn"] == "P1"));
    let a = usage::analyze(&json!({"r1":r}));
    assert!(!a["observations"]["terminalOrdering"][0]["lastUsageBeforeTerminal"].is_null());
    assert_eq!(
        a["observations"]["terminalOrdering"][0]["lateUsage"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    for secret in [
        "PRIVATE_ANSWER",
        "PRIVATE_USAGE",
        "PRIVATE_ARRAY",
        "PRIVATE_CURRENCY",
        "STDERR_SECRET",
        "987654321",
        "87654321",
    ] {
        assert!(!a.to_string().contains(secret));
    }
}

/// 完整scenario共用cwd/S1，R1两turn、R2只resume与P3。
#[tokio::test]
async fn usage_three_turns_exact_resume_and_cost() {
    let logs = tempfile::tempdir().unwrap();
    let log = logs.path().join("log");
    let r = usage_runtime::scenario(
        &launcher("ok", &log),
        Duration::from_secs(8),
        Duration::from_secs(8),
    )
    .await
    .unwrap();
    assert_eq!(r["status"], "PASS", "{r}");
    assert_eq!(r["workspaceDelta"], json!([]));
    assert_eq!(r["r1"]["sessionId"], r["r2"]["sessionId"]);
    assert_eq!(r["r1"]["cwd"], r["r2"]["cwd"]);
    let rows: Vec<Value> = std::fs::read_to_string(log)
        .unwrap()
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    assert_eq!(
        rows.iter()
            .filter(|r| r["method"] == "session/prompt")
            .count(),
        3
    );
    assert_eq!(
        rows.iter()
            .filter(|r| r["method"] == "session/resume")
            .count(),
        1
    );
    assert!(!rows.iter().any(|r| r["method"] == "session/load"));
    let a = usage::analyze(&r);
    assert!(
        !a["observations"]["costFields"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        a["observations"]["restart"]["r2FirstSnapshot"]["placement"]["phase"],
        "resume"
    );
    assert_eq!(a["interpretation"]["token_usage"], false);
}

/// 成功采样明确token字段仍不猜跨turn/restart累计语义。
#[tokio::test]
async fn usage_explicit_breakdown_observation_only() {
    let cwd = tempfile::tempdir().unwrap();
    let logs = tempfile::tempdir().unwrap();
    let r = usage_runtime::run_runtime(
        cwd.path(),
        None,
        &launcher("breakdown", &logs.path().join("log")),
        Duration::from_secs(8),
    )
    .await
    .unwrap();
    let a = usage::analyze(&json!({"r1":r}));
    let fields = a["observations"]["explicitBreakdownFields"]
        .as_array()
        .unwrap();
    assert_eq!(fields.len(), 10);
    assert!(
        fields
            .iter()
            .all(|f| f["placement"]["attribution"] == "exact_rpc_id")
    );
    assert_eq!(a["interpretation"]["token_usage"], false);
    assert_eq!(
        a["interpretation"]["reason"],
        "BREAKDOWN_OBSERVED_BUT_CROSS_TURN_RESTART_SEMANTICS_REQUIRE_HOST_FREEZE"
    );
}

/// failed resume或wrong identity绝不发P3，更不fallback load。
#[tokio::test]
async fn usage_failed_resume_no_p3() {
    for mode in ["failed-resume", "wrong-session", "wrong-response"] {
        let cwd = tempfile::tempdir().unwrap();
        let logs = tempfile::tempdir().unwrap();
        let log = logs.path().join("log");
        let r = usage_runtime::run_runtime(
            cwd.path(),
            Some("S1"),
            &launcher(mode, &log),
            Duration::from_secs(8),
        )
        .await
        .unwrap();
        assert!(!r["error"].is_null());
        assert_eq!(r["cleanup"]["directChildReaped"], true);
        assert!(r["collection"]["turns"].as_array().unwrap().is_empty());
        let text = std::fs::read_to_string(log).unwrap();
        assert!(!text.contains("session/prompt"));
        assert!(!text.contains("session/load"));
    }
}

/// 所有usage源只在typed envelope/exact S1通过后投影数字。
#[test]
fn usage_wrong_session_and_invalid_typed_envelope() {
    for (session, used) in [("wrong", json!(4)), ("S1", json!("secret"))] {
        let rows = vec![
            json!({"sequence":1,"direction":"response","message":{"method":"session/update","params":{"sessionId":session,"update":{"sessionUpdate":"usage_update","used":used,"size":100}}}}),
        ];
        let c = usage::collect(&rows, "S1", 1, 1).unwrap();
        assert_eq!(c["rejectedEnvelopes"], 1);
        assert!(c["samples"].as_array().unwrap().is_empty());
    }
}

/// 固定usage路径且sentinel只能首次create_new，失败也不能覆盖重试。
#[test]
fn usage_sentinel_and_output_fixed() {
    let cwd = tempfile::tempdir().unwrap();
    let p = cwd.path().join("usage.attempt-started.json");
    durable(&p, &json!({"attempt":1})).unwrap();
    assert!(durable(&p, &json!({"attempt":2})).is_err());
    assert!(usage_runtime::output_root().ends_with("evidence/usage"));
    assert!(usage_runtime::output_root().is_absolute());
    assert_eq!(
        parse_scenario(&["exe".into(), "usage".into()]),
        Some("usage")
    );
    assert!(parse_scenario(&["exe".into(), "usage".into(), "elsewhere".into()]).is_none());
}

/// 超时仍回收owned child与drain，不遗留真实Provider进程。
#[tokio::test]
async fn usage_timeout_cleanup() {
    let cwd = tempfile::tempdir().unwrap();
    let logs = tempfile::tempdir().unwrap();
    let r = usage_runtime::run_runtime(
        cwd.path(),
        None,
        &launcher("timeout", &logs.path().join("log")),
        Duration::from_millis(400),
    )
    .await
    .unwrap();
    assert_eq!(r["error"], "RUNTIME_TIMEOUT");
    assert_eq!(r["cleanup"]["succeeded"], true);
}

/// 投影预算不足必须整体失败，不把截断数据宣称完整采样。
#[test]
fn usage_projection_limit_fails_closed() {
    assert!(usage::project(&json!({"used":1,"size":2}), true, &mut 1).is_err());
}

/// runtime restart后的size变化据实观察，不把旧window常量带入新进程。
#[tokio::test]
async fn usage_size_change_across_turns() {
    let logs = tempfile::tempdir().unwrap();
    let r = usage_runtime::scenario(
        &launcher("size-change", &logs.path().join("log")),
        Duration::from_secs(8),
        Duration::from_secs(8),
    )
    .await
    .unwrap();
    assert_eq!(r["status"], "PASS");
    let a = usage::analyze(&r);
    assert_eq!(a["observations"]["sizeAcrossSessionStable"], false);
    assert_eq!(a["observations"]["sizeByRuntime"][0]["stable"], false);
    assert_eq!(a["observations"]["sizeByRuntime"][1]["stable"], false);
}

/// 创建合法旧Host超时fixture；函数不会调用任何Provider。
fn usage_repair_fixture(root: &Path) {
    durable(
        &root.join("usage.attempt-started.json"),
        &json!({"attemptId":"original-immutable"}),
    )
    .unwrap();
    durable(
        &root.join("usage.host-timeout.json"),
        &json!({"classification":"HOST_COMMAND_TIMEOUT"}),
    )
    .unwrap();
}

/// 原sentinel/超时证据缺失或分类不符，一律拒绝，不占repair sentinel。
#[test]
fn usage_repair_missing_or_invalid_timeout_rejected() {
    for missing in ["usage.attempt-started.json", "usage.host-timeout.json"] {
        let root = tempfile::tempdir().unwrap();
        let name = if missing == "usage.attempt-started.json" {
            "usage.host-timeout.json"
        } else {
            "usage.attempt-started.json"
        };
        durable(
            &root.path().join(name),
            &json!({"classification":"HOST_COMMAND_TIMEOUT"}),
        )
        .unwrap();
        assert!(usage_runtime::reserve_repair(root.path()).is_err());
        assert!(
            !root
                .path()
                .join("usage-repair.attempt-started.json")
                .exists()
        );
    }
    for bad in [json!({}), json!({"classification":"PROVIDER_TIMEOUT"})] {
        let root = tempfile::tempdir().unwrap();
        durable(&root.path().join("usage.attempt-started.json"), &json!({})).unwrap();
        durable(&root.path().join("usage.host-timeout.json"), &bad).unwrap();
        assert!(usage_runtime::reserve_repair(root.path()).is_err());
        assert!(
            !root
                .path()
                .join("usage-repair.attempt-started.json")
                .exists()
        );
    }
}

/// 原result存在或repair已经占用时都拒绝；不得覆盖任何旧字节。
#[test]
fn usage_repair_existing_evidence_rejected() {
    for name in [
        "usage.result.json",
        "usage-repair.attempt-started.json",
        "usage-repair.result.json",
        "usage-repair-analysis.json",
    ] {
        let root = tempfile::tempdir().unwrap();
        usage_repair_fixture(root.path());
        let path = root.path().join(name);
        durable(&path, &json!({"immutable":true})).unwrap();
        let before = manifest(root.path()).unwrap();
        assert!(usage_runtime::reserve_repair(root.path()).is_err());
        assert_eq!(before, manifest(root.path()).unwrap());
    }
}

/// 合法gate仅新增repair sentinel，旧sentinel/timeout原字节不动；不能再次占用。
#[test]
fn usage_repair_valid_gate_preserves_original_and_no_replay() {
    let root = tempfile::tempdir().unwrap();
    usage_repair_fixture(root.path());
    let old = std::fs::read(root.path().join("usage.attempt-started.json")).unwrap();
    let timeout = std::fs::read(root.path().join("usage.host-timeout.json")).unwrap();
    usage_runtime::reserve_repair(root.path()).unwrap();
    let p = root.path().join("usage-repair.attempt-started.json");
    let sentinel = std::fs::read(&p).unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&sentinel).unwrap()["scenario"],
        "usage-repair"
    );
    assert!(usage_runtime::reserve_repair(root.path()).is_err());
    assert_eq!(std::fs::read(&p).unwrap(), sentinel);
    assert_eq!(
        std::fs::read(root.path().join("usage.attempt-started.json")).unwrap(),
        old
    );
    assert_eq!(
        std::fs::read(root.path().join("usage.host-timeout.json")).unwrap(),
        timeout
    );
}

/// 只提供唯一repair模式，没有第三次repair或force/output参数。
#[test]
fn usage_repair_fixed_cli_no_third_attempt() {
    assert_eq!(
        parse_scenario(&["exe".into(), "usage-repair".into()]),
        Some("usage-repair")
    );
    for args in [
        vec!["exe", "usage-repair-2"],
        vec!["exe", "usage-repair", "--force"],
        vec!["exe", "usage-repair", "other.json"],
    ] {
        assert!(parse_scenario(&args.into_iter().map(String::from).collect::<Vec<_>>()).is_none());
    }
}
