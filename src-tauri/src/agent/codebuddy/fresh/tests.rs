//! Fresh preparation 使用实际 Store 与 native fake ACP peer 验证。
#[path = "boundary_tests.rs"]
mod boundary;
use super::*;
use crate::agent::execution::{CreateExecutionInput, canonicalize_request};
use serde_json::json;
use std::{
    os::windows::process::CommandExt,
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

/// 真实 acceptance sink 仅记录可观察调用，不触发 prompt 或生命周期伪完成。
#[derive(Default)]
struct Sink(AtomicBool);
impl ProviderAcceptanceSink for Sink {
    /// 接收真正的 acceptance 回调，并禁止重复接受。
    fn accepted(&self) {
        assert!(!self.0.swap(true, Ordering::SeqCst));
    }
}

/// 假 binary 无外部依赖，所有启动仍经正式 CB6-002 launcher。
fn build(directory: &Path) -> PathBuf {
    let target = directory.join("base.exe");
    let status = std::process::Command::new("rustc")
        .args(["--edition=2024", "--crate-name", "codebuddy_fresh_child"])
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/codebuddy_fresh_child.rs"))
        .arg("-o")
        .arg(&target)
        .creation_flags(0x0800_0000)
        .output()
        .unwrap();
    assert!(
        status.status.success(),
        "{}",
        String::from_utf8_lossy(&status.stderr)
    );
    target
}

/// 创建真实 provider=codebuddy 的 Execution/Claim；不用 SQL 伪造正常绑定。
pub(crate) async fn fixture(
    base: &Path,
    mode: &str,
    auto: bool,
) -> (tempfile::TempDir, StateStore, String, ResolvedLaunchSpec) {
    let dir = tempfile::tempdir().unwrap();
    let executable = dir.path().join(format!("cb7-fresh-{mode}.exe"));
    std::fs::copy(base, &executable).unwrap();
    let store = StateStore::open(dir.path().into()).await.unwrap();
    let root = crate::config::canonicalize_workspace_root(dir.path()).unwrap();
    let input: CreateExecutionInput = serde_json::from_value(json!({
        "agent_id":"a","request_key":"k","prompt":"not sent","execution_profile":{},
        "workspace_id":"w","canonical_workspace_root":root,"mode":"read_only","provider":"codebuddy"
    }))
    .unwrap();
    let created = store
        .create_execution(
            "execution".into(),
            canonicalize_request(input).unwrap(),
            now(),
        )
        .await
        .unwrap();
    assert_eq!(created.execution.provider, "codebuddy");
    let mut modes = vec![json!({"id":"default","name":"Default"})];
    if auto {
        modes.push(json!({"id":"auto","name":"Auto"}));
    }
    std::fs::write(dir.path().join("new.json"), json!({
        "sessionId":"exact-session","modes":{"currentModeId":"default","availableModes":modes},
        "configOptions":[
            depth("low"),
            execution_option("model", "model", "model-a", &["model-a"]),
            execution_option("thought_level", "thought_level", "medium", &["low", "medium", "high"])
        ],
        "models":{"currentModelId":"model-a","availableModels":[{"modelId":"model-a","name":"Model A"}]}
    }).to_string()).unwrap();
    std::fs::write(
        dir.path().join("config.json"),
        json!({"configOptions":[
            depth("high"),
            execution_option("model", "model", "model-a", &["model-a"]),
            execution_option("thought_level", "thought_level", "medium", &["low", "medium", "high"])
        ]})
        .to_string(),
    )
    .unwrap();
    let resolved = ResolvedLaunchSpec {
        executable,
        args: vec!["--acp".into()],
        path_projection: vec![dir.path().into()],
    };
    (dir, store, created.execution_id, resolved)
}

/// 配置 id/value 来自本次目录，不依赖 Host 曾出现的 option 集合。
fn depth(current: &str) -> Value {
    json!({"id":"depth","name":"Depth","type":"select","currentValue":current,
        "options":[{"value":"low","name":"Low"},{"value":"high","name":"High"}]})
}

/// 构造 live ACP 已确认的 model/thought_level select。
fn execution_option(id: &str, category: &str, current: &str, values: &[&str]) -> Value {
    json!({"id":id,"name":id,"category":category,"type":"select","currentValue":current,
        "options":values.iter().map(|value| json!({"value":value,"name":value})).collect::<Vec<_>>()})
}

#[test]
/// effective profile 只接受 exact Session 的一致当前值，并保留 ACK 后实际档位。
fn effective_profile_uses_exact_current_configuration() {
    let catalog = SessionCatalog {
        response: serde_json::from_value(json!({
            "sessionId":"exact-session",
            "configOptions":[
                execution_option("model", "model", "model-b", &["model-a", "model-b"]),
                execution_option("thought_level", "thought_level", "high", &["low", "high"])
            ]
        }))
        .unwrap(),
        models: Some(json!({
            "currentModelId":"model-b",
            "availableModels":[{"modelId":"model-a"},{"modelId":"model-b","_meta":{}}]
        })),
    };

    assert_eq!(
        catalog.effective_execution_profile().unwrap(),
        crate::agent::execution::ExecutionProfile {
            model: Some("model-b".into()),
            reasoning: Some("high".into()),
        }
    );
}

#[test]
/// 不支持 reasoning 的实际模型必须忽略 Session 残留 thought_level，而不是记录伪事实。
fn effective_profile_ignores_stale_reasoning_for_nonreasoning_model() {
    let catalog = SessionCatalog {
        response: serde_json::from_value(json!({
            "sessionId":"exact-session",
            "configOptions":[
                execution_option("model", "model", "model-plain", &["model-plain"]),
                execution_option("thought_level", "thought_level", "stale-high", &["stale-high"])
            ]
        }))
        .unwrap(),
        models: Some(json!({
            "currentModelId":"model-plain",
            "availableModels":[{
                "modelId":"model-plain",
                "_meta":{"supportsReasoning":false}
            }]
        })),
    };

    assert_eq!(
        catalog.effective_execution_profile().unwrap(),
        crate::agent::execution::ExecutionProfile {
            model: Some("model-plain".into()),
            reasoning: None,
        }
    );
}

#[test]
/// 缺少完整 current authority 或 raw/model option 冲突时必须 fail closed。
fn effective_profile_rejects_missing_or_conflicting_authority() {
    let response = |config_options: Vec<Value>| {
        serde_json::from_value(json!({
            "sessionId":"exact-session",
            "configOptions":config_options
        }))
        .unwrap()
    };
    let model = execution_option("model", "model", "model-a", &["model-a", "model-b"]);
    let reasoning = execution_option("thought_level", "thought_level", "high", &["high"]);
    let missing_reasoning = SessionCatalog {
        response: response(vec![model.clone()]),
        models: Some(json!({
            "currentModelId":"model-a",
            "availableModels":[{"modelId":"model-a"}]
        })),
    };
    assert_eq!(
        missing_reasoning.effective_execution_profile(),
        Err(Failure::Configuration)
    );

    let missing_raw_models = SessionCatalog {
        response: response(vec![model.clone(), reasoning.clone()]),
        models: None,
    };
    assert_eq!(
        missing_raw_models.effective_execution_profile(),
        Err(Failure::Configuration)
    );

    let malformed_reasoning_metadata = SessionCatalog {
        response: response(vec![model.clone(), reasoning.clone()]),
        models: Some(json!({
            "currentModelId":"model-a",
            "availableModels":[{
                "modelId":"model-a",
                "_meta":{"supportsReasoning":"false"}
            }]
        })),
    };
    assert_eq!(
        malformed_reasoning_metadata.effective_execution_profile(),
        Err(Failure::Configuration)
    );
    for metadata in [Value::Null, json!("invalid"), json!([])] {
        let malformed_metadata = SessionCatalog {
            response: response(vec![model.clone(), reasoning.clone()]),
            models: Some(json!({
                "currentModelId":"model-a",
                "availableModels":[{"modelId":"model-a","_meta":metadata}]
            })),
        };
        assert_eq!(
            malformed_metadata.effective_execution_profile(),
            Err(Failure::Configuration)
        );
    }

    let conflicting_model = SessionCatalog {
        response: response(vec![model, reasoning]),
        models: Some(json!({
            "currentModelId":"model-b",
            "availableModels":[{"modelId":"model-a"},{"modelId":"model-b"}]
        })),
    };
    assert_eq!(
        conflicting_model.effective_execution_profile(),
        Err(Failure::Configuration)
    );
}

/// 有界轮询只等待 fake peer 已记录的请求，不以固定 sleep 推断完成。
pub(crate) async fn wait_wire(dir: &Path, count: usize) -> Vec<Value> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(12);
    loop {
        let rows = wire(dir);
        if rows.len() >= count {
            return rows;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "peer request timeout"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// 日志是 fake peer 收到的 SDK wire，仅在本次 temp workspace 内。
pub(crate) fn wire(dir: &Path) -> Vec<Value> {
    std::fs::read_to_string(dir.join("wire.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

/// 验证持久化 cleanup 完成后 Claim 仍由现有 startup/finalization authority 管理。
pub(crate) async fn cleanup_evidence(store: &StateStore, id: &str) -> String {
    let row = store.execution(id.into()).await.unwrap().unwrap();
    let runtime = row.runtime_instance_id.unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(12);
    loop {
        let r = store.runtime(runtime.clone()).await.unwrap().unwrap();
        if r.termination_evidence_state == "complete" {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "evidence missing: {}",
            r.state
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(
        store
            .workspace_claim(row.canonical_workspace_root)
            .await
            .unwrap()
            .is_some()
    );
    runtime
}

#[tokio::test]
/// 按 peer gate 观察真实 SQLite 写顺序；在 accepted 之后本卡立即停止。
async fn durable_ordering_early_routing_and_acceptance_without_prompt() {
    let bin_dir = tempfile::tempdir().unwrap();
    let base = build(bin_dir.path());
    let (dir, store, id, resolved) = fixture(&base, "gated", true).await;
    let updates = [
        json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"wrong-session","update":{"sessionUpdate":"config_option_update","configOptions":[]}}}),
        json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"exact-session","update":{"sessionUpdate":"config_option_update","configOptions":[depth("high")]}}}),
        json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"exact-session","update":{"sessionUpdate":"config_option_update","configOptions":[depth("low")]}}}),
    ];
    std::fs::write(
        dir.path().join("early.jsonl"),
        updates.iter().map(|v| format!("{v}\n")).collect::<String>(),
    )
    .unwrap();
    let task_store = store.clone();
    let task_id = id.clone();
    let task = tokio::spawn(async move {
        prepare(
            task_store,
            "host".into(),
            task_id,
            &resolved,
            DesiredConfiguration {
                mode: Some("auto".into()),
                model: None,
                reasoning: None,
                option: None,
            },
            Limits::default(),
        )
        .await
    });
    let sink = Sink::default();
    let mut trace = Vec::new();
    let initial = wait_wire(dir.path(), 1).await;
    let row = store.execution(id.clone()).await.unwrap().unwrap();
    assert!(row.canonical_workspace_root.starts_with(r"\\?\"));
    assert_eq!(row.dispatch_state, "not_dispatched");
    let runtime = store
        .runtime(row.runtime_instance_id.clone().unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(runtime.state, "starting");
    assert!(runtime.job_policy_verified_at.is_some() && runtime.codex_pid.is_some());
    let private = store.read_codebuddy_state(id.clone()).await.unwrap();
    assert_eq!(private.runtime_instance_id, row.runtime_instance_id);
    assert!(private.acp_protocol_version.is_none());
    assert!(super::super::store::valid_conversation_id(
        &private.conversation_request_id
    ));
    let caps = &initial[0]["params"]["clientCapabilities"];
    assert_ne!(caps["fs"]["readTextFile"], true);
    assert_ne!(caps["fs"]["writeTextFile"], true);
    assert_ne!(caps["terminal"], true);
    assert_ne!(caps["elicitation"], true);
    trace.extend([
        "runtime prepare",
        "Job launch",
        "policy/process durable",
        "initialize",
    ]);
    std::fs::write(dir.path().join("release-initialize"), "").unwrap();
    let requests = wait_wire(dir.path(), 2).await;
    let runtime = store.runtime(runtime.id).await.unwrap().unwrap();
    assert_eq!(runtime.state, "running");
    let private = store.read_codebuddy_state(id.clone()).await.unwrap();
    assert_eq!(private.acp_protocol_version, Some(1));
    assert!(private.session_id.is_none());
    assert_eq!(
        requests[1]["params"],
        json!({"cwd":dir.path(),"mcpServers":[]})
    );
    trace.extend(["initialized/protocol durable", "session/new exactly once"]);
    std::fs::write(dir.path().join("release-new"), "").unwrap();
    let requests = wait_wire(dir.path(), 3).await;
    let private = store.read_codebuddy_state(id.clone()).await.unwrap();
    assert_eq!(private.session_id.as_deref(), Some("exact-session"));
    assert_eq!(requests[2]["method"], "session/set_mode");
    assert_eq!(
        requests[2]["params"],
        json!({"sessionId":"exact-session","modeId":"auto"})
    );
    assert!(!sink.0.load(Ordering::SeqCst));
    std::fs::write(dir.path().join("release-mode"), "").unwrap();
    let prepared = task.await.unwrap().unwrap();
    assert_eq!(prepared.early_frames.len(), 2);
    let state = &prepared.runtime.client.as_ref().unwrap().requests.shared;
    state.register_route("wrong-session").unwrap();
    assert_eq!(state.take_session("wrong-session").unwrap().len(), 1);
    assert_eq!(
        serde_json::to_value(&prepared.catalog.response.config_options).unwrap()[0]["currentValue"],
        "low"
    );
    assert_eq!(
        prepared.catalog.models.as_ref().unwrap()["currentModelId"],
        "model-a"
    );
    trace.extend([
        "session durable",
        "route register",
        "early replay",
        "config ACK",
        "acceptance_ready",
    ]);
    let prepared = prepared.accept(&sink).unwrap();
    assert!(sink.0.load(Ordering::SeqCst));
    trace.push("accepted");
    assert_eq!(wire(dir.path()).len(), 3);
    assert!(
        wire(dir.path())
            .iter()
            .all(|r| r["method"] != "session/prompt")
    );
    println!("CB7-002 ORDER: {} -> STOP", trace.join(" -> "));
    drop(prepared);
    cleanup_evidence(&store, &id).await;
    super::super::recovery::startup(&store, "next-host")
        .await
        .unwrap();
    assert_eq!(
        store.execution(id).await.unwrap().unwrap().status,
        "interrupted"
    );
}

#[tokio::test]
/// Provider 当前模式、显式配置和每个 pre-accept 故障均经生产 prepare；unknown new 不重放。
async fn preparation_success_and_failure_matrix() {
    let bin_dir = tempfile::tempdir().unwrap();
    let base = build(bin_dir.path());
    for (mode, desired_mode, desired_option, success, new_count, set_count) in [
        ("default", None, None, true, 1, 0),
        ("auto", Some("auto"), None, true, 1, 1),
        ("config", None, Some(("depth", "high")), true, 1, 1),
        ("closed-before-accept", None, None, true, 1, 0),
        ("absent", Some("auto"), None, false, 1, 0),
        (
            "missing-option",
            None,
            Some(("not-advertised", "high")),
            false,
            1,
            0,
        ),
        ("mismatch", None, None, false, 0, 0),
        ("initialize-error", None, None, false, 0, 0),
        ("new-error", None, None, false, 1, 0),
        ("new-eof", None, None, false, 1, 0),
        ("new-timeout", None, None, false, 1, 0),
        ("mode-error", Some("auto"), None, false, 1, 1),
        ("mode-timeout", Some("auto"), None, false, 1, 1),
        ("config-error", None, Some(("depth", "high")), false, 1, 1),
        ("config-timeout", None, Some(("depth", "high")), false, 1, 1),
        ("empty-session", None, None, false, 1, 0),
        ("missing-session", None, None, false, 1, 0),
        ("bad-model", None, None, false, 1, 0),
        ("bad-early", None, None, false, 1, 0),
        ("route-limit", None, None, false, 1, 0),
    ] {
        let (dir, store, id, resolved) = fixture(&base, mode, mode != "absent").await;
        let path = dir.path().join("new.json");
        let mut response: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        match mode {
            "empty-session" => response["sessionId"] = json!(""),
            "missing-session" => { response.as_object_mut().unwrap().remove("sessionId"); },
            "bad-model" => response["models"]["currentModelId"] = json!("not-advertised"),
            "bad-early" => std::fs::write(dir.path().join("early.jsonl"), format!("{}\n", json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"exact-session","update":{"sessionUpdate":"config_option_update","configOptions":[{"id":"broken"}]}}}))).unwrap(),
            _ => {},
        }
        std::fs::write(path, response.to_string()).unwrap();
        let sink = Sink::default();
        let result = prepare(
            store.clone(),
            "host".into(),
            id.clone(),
            &resolved,
            DesiredConfiguration {
                mode: desired_mode.map(str::to_owned),
                model: None,
                reasoning: None,
                option: desired_option.map(|(id, value)| (id.into(), value.into())),
            },
            Limits {
                request_timeout: Duration::from_millis(800),
                routes: if mode == "route-limit" { 0 } else { 64 },
                ..Limits::default()
            },
        )
        .await;
        assert_eq!(
            result.is_ok(),
            success,
            "scenario {mode}, error {:?}, wire {:?}, raw wire {:?}, peer {:?}",
            result.as_ref().err(),
            wire(dir.path()),
            std::fs::read_to_string(dir.path().join("wire.jsonl")),
            std::fs::read_to_string(dir.path().join("peer-panic.txt"))
        );
        assert!(!sink.0.load(Ordering::SeqCst));
        if let Ok(prepared) = result {
            match mode {
                "default" => drop(prepared),
                "closed-before-accept" => {
                    prepared
                        .runtime
                        .client
                        .as_ref()
                        .unwrap()
                        .requests
                        .shared
                        .fail(Failure::Closed);
                    assert!(matches!(prepared.accept(&sink), Err(Failure::Closed)));
                    assert!(!sink.0.load(Ordering::SeqCst));
                }
                _ => prepared.shutdown().await.unwrap(),
            }
        }
        cleanup_evidence(&store, &id).await;
        let rows = wire(dir.path());
        assert_eq!(
            rows.iter().filter(|r| r["method"] == "session/new").count(),
            new_count,
            "{mode}"
        );
        assert_eq!(
            rows.iter()
                .filter(|r| r["method"].as_str().unwrap().starts_with("session/set_"))
                .count(),
            set_count,
            "{mode}"
        );
        assert!(rows.iter().all(|r| r["method"] != "session/prompt"));
        let row = store.execution(id).await.unwrap().unwrap();
        assert_eq!(row.dispatch_state, "not_dispatched");
        assert!(row.thread_id.is_none() && row.turn_id.is_none());
    }
}

#[tokio::test]
/// mode option 的 typed ACK 无额外通知时也应同步目录并允许 acceptance。
async fn mode_option_ack_reconciles_legacy_modes_without_notification() {
    let bin_dir = tempfile::tempdir().unwrap();
    let base = build(bin_dir.path());
    let (dir, store, id, resolved) = fixture(&base, "mode-option", true).await;
    let path = dir.path().join("new.json");
    let mut response: Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let mut option = json!({"id":"actual-mode","name":"Mode","category":"mode","type":"select",
        "currentValue":"default","options":[{"value":"default","name":"Default"},{"value":"auto","name":"Auto"}]});
    response["configOptions"] = json!([option.clone()]);
    std::fs::write(path, response.to_string()).unwrap();
    option["currentValue"] = json!("auto");
    std::fs::write(
        dir.path().join("config.json"),
        json!({"configOptions":[option]}).to_string(),
    )
    .unwrap();
    let prepared = prepare(
        store.clone(),
        "host".into(),
        id.clone(),
        &resolved,
        DesiredConfiguration {
            mode: None,
            model: None,
            reasoning: None,
            option: Some(("actual-mode".into(), "auto".into())),
        },
        Limits::default(),
    )
    .await
    .unwrap();
    assert_eq!(
        prepared
            .catalog
            .response
            .modes
            .as_ref()
            .unwrap()
            .current_mode_id
            .to_string(),
        "auto"
    );
    assert!(prepared.early_frames.is_empty());
    let sink = Sink::default();
    let prepared = prepared.accept(&sink).unwrap();
    assert!(sink.0.load(Ordering::SeqCst));
    prepared.shutdown().await.unwrap();
    cleanup_evidence(&store, &id).await;
    let rows = wire(dir.path());
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[2]["method"], "session/set_config_option");
    assert_eq!(
        rows[2]["params"],
        json!({"sessionId":"exact-session","configId":"actual-mode","value":"auto"})
    );
    assert!(rows.iter().all(|r| r["method"] != "session/prompt"));
}

#[test]
/// 配置身份及 value 必须明确；禁止将 boolean 或未知 id 当作 select fallback。
fn catalog_options_reject_ambiguous_values_and_multitask_true() {
    let mut duplicate = depth("low");
    duplicate["options"]
        .as_array_mut()
        .unwrap()
        .push(json!({"value":"low","name":"Duplicate"}));
    let option: SessionConfigOption = serde_json::from_value(duplicate).unwrap();
    assert_eq!(validate_options(&[option]), Err(Failure::Configuration));
    let multitask: SessionConfigOption = serde_json::from_value(
        json!({"id":"multitask","name":"Multitask","type":"boolean","currentValue":false}),
    )
    .unwrap();
    assert!(!option_accepts(&multitask, &true.into()));
    assert!(option_accepts(&multitask, &false.into()));
    assert!(!option_accepts(&multitask, &"true".into()));
    let select: SessionConfigOption = serde_json::from_value(depth("low")).unwrap();
    assert!(!option_accepts(&select, &"guessed".into()));
    assert!(!option_accepts(&select, &true.into()));
    let response: NewSessionResponse = serde_json::from_value(json!({
        "sessionId":"s","modes":{"currentModeId":"default","availableModes":[{"id":"default","name":"Default"},{"id":"auto","name":"Auto"}]},
        "configOptions":[{"id":"actual-mode","name":"Mode","category":"mode","type":"select","currentValue":"auto","options":[{"value":"default","name":"Default"},{"value":"auto","name":"Auto"}]}]
    })).unwrap();
    assert_eq!(
        SessionCatalog {
            response,
            models: None
        }
        .validate(),
        Err(Failure::Configuration)
    );
}

#[tokio::test]
/// model ACK 必须先替换目录，reasoning 才能按新 options 校验并发送。
async fn model_then_reasoning_uses_sequential_ack_authority() {
    let bin_dir = tempfile::tempdir().unwrap();
    let base = build(bin_dir.path());
    let (dir, store, id, resolved) = fixture(&base, "profile", true).await;
    let models = json!({"currentModelId":"model-a","availableModels":[
        {"modelId":"model-a","name":"Model A","_meta":{"supportsReasoning":true}},
        {"modelId":"model-b","name":"Model B","_meta":{"supportsReasoning":true}},
        {"modelId":"model-c","name":"Model C","_meta":{"supportsReasoning":false}}
    ]});
    let model_a = execution_option("model", "model", "model-a", &["model-a", "model-b"]);
    let model_b = execution_option("model", "model", "model-b", &["model-a", "model-b"]);
    let low = execution_option("thought_level", "thought_level", "low", &["low", "high"]);
    let high = execution_option("thought_level", "thought_level", "high", &["low", "high"]);
    std::fs::write(
        dir.path().join("new.json"),
        json!({"sessionId":"exact-session","configOptions":[model_a],"models":models}).to_string(),
    )
    .unwrap();
    std::fs::write(
        dir.path().join("config-1.json"),
        json!({"configOptions":[model_b.clone(),low]}).to_string(),
    )
    .unwrap();
    std::fs::write(
        dir.path().join("config-2.json"),
        json!({"configOptions":[model_b,high]}).to_string(),
    )
    .unwrap();
    let prepared = prepare(
        store.clone(),
        "host".into(),
        id.clone(),
        &resolved,
        DesiredConfiguration {
            mode: None,
            model: Some("model-b".into()),
            reasoning: Some("high".into()),
            option: None,
        },
        Limits::default(),
    )
    .await
    .unwrap();
    let projected = prepared.catalog.configuration_catalog().unwrap();
    assert_eq!(projected.current_model.as_deref(), Some("model-b"));
    assert_eq!(projected.current_reasoning.as_deref(), Some("high"));
    assert_eq!(
        projected
            .models
            .iter()
            .map(|model| model.id.as_str())
            .collect::<Vec<_>>(),
        ["model-a", "model-b", "model-c"]
    );
    assert_eq!(
        projected
            .models
            .iter()
            .find(|model| model.id == "model-b")
            .unwrap()
            .reasoning_options
            .iter()
            .map(|option| option.id.as_str())
            .collect::<Vec<_>>(),
        ["low", "high"]
    );
    assert!(
        projected
            .models
            .iter()
            .find(|model| model.id == "model-c")
            .unwrap()
            .reasoning_options
            .is_empty()
    );
    assert_eq!(
        prepared.catalog.confirm_desired(&DesiredConfiguration {
            mode: None,
            model: Some("model-b".into()),
            reasoning: Some("not-advertised".into()),
            option: None,
        }),
        Err(Failure::Configuration)
    );
    prepared.shutdown().await.unwrap();
    cleanup_evidence(&store, &id).await;
    let rows = wire(dir.path());
    let config_ids: Vec<_> = rows
        .iter()
        .filter(|row| row["method"] == "session/set_config_option")
        .map(|row| row["params"]["configId"].as_str().unwrap())
        .collect();
    assert_eq!(config_ids, ["model", "thought_level"]);
}

#[tokio::test]
/// Provider catalog 必须逐模型读取 reasoning ACK；当前模型的档位不能复制给其它模型。
async fn provider_catalog_reads_model_specific_reasoning_options() {
    let bin_dir = tempfile::tempdir().unwrap();
    let base = build(bin_dir.path());
    let (dir, store, id, resolved) = fixture(&base, "profile", true).await;
    let models = json!({"currentModelId":"model-a","availableModels":[
        {"modelId":"model-a","name":"Model A","_meta":{"supportsReasoning":true}},
        {"modelId":"model-b","name":"Model B","_meta":{"supportsReasoning":true}},
        {"modelId":"model-c","name":"Model C","_meta":{"supportsReasoning":false}}
    ]});
    let model_a = execution_option("model", "model", "model-a", &["model-a", "model-b"]);
    let model_b = execution_option("model", "model", "model-b", &["model-a", "model-b"]);
    let thought_a = execution_option("thought_level", "thought_level", "high", &["high"]);
    let thought_b = execution_option(
        "thought_level",
        "thought_level",
        "low",
        &["low", "high", "xhigh"],
    );
    std::fs::write(
        dir.path().join("new.json"),
        json!({"sessionId":"exact-session","configOptions":[model_a,thought_a],"models":models})
            .to_string(),
    )
    .unwrap();
    std::fs::write(
        dir.path().join("config-1.json"),
        json!({"configOptions":[model_b,thought_b]}).to_string(),
    )
    .unwrap();

    let mut prepared = prepare(
        store.clone(),
        "host".into(),
        id.clone(),
        &resolved,
        DesiredConfiguration::default(),
        Limits::default(),
    )
    .await
    .unwrap();
    let requests = prepared.runtime.client.as_ref().unwrap().requests.clone();
    let projected = prepared
        .catalog
        .configuration_catalog_for_provider(&requests)
        .await
        .unwrap();

    let model = |id: &str| {
        projected
            .models
            .iter()
            .find(|model| model.id == id)
            .unwrap()
    };
    assert_eq!(
        model("model-a")
            .reasoning_options
            .iter()
            .map(|option| option.id.as_str())
            .collect::<Vec<_>>(),
        ["high"]
    );
    assert_eq!(
        model("model-b")
            .reasoning_options
            .iter()
            .map(|option| option.id.as_str())
            .collect::<Vec<_>>(),
        ["low", "high", "xhigh"]
    );
    assert!(model("model-c").reasoning_options.is_empty());
    assert_eq!(projected.current_model.as_deref(), Some("model-a"));
    assert_eq!(projected.current_reasoning.as_deref(), Some("high"));

    prepared.shutdown().await.unwrap();
    cleanup_evidence(&store, &id).await;
    let rows = wire(dir.path());
    let config: Vec<_> = rows
        .iter()
        .filter(|row| row["method"] == "session/set_config_option")
        .collect();
    assert_eq!(config.len(), 1);
    assert_eq!(config[0]["params"]["configId"], "model");
    assert_eq!(config[0]["params"]["value"], "model-b");
}

#[tokio::test]
/// new 已发出后 generic OCC 变化必须拒绝 session durable/acceptance，且不重放 new。
async fn session_identity_occ_failure_cleans_runtime_without_acceptance() {
    let bin_dir = tempfile::tempdir().unwrap();
    let base = build(bin_dir.path());
    let (dir, store, id, resolved) = fixture(&base, "gated", true).await;
    std::fs::write(dir.path().join("release-initialize"), "").unwrap();
    let task_store = store.clone();
    let task_id = id.clone();
    let task = tokio::spawn(async move {
        prepare(
            task_store,
            "host".into(),
            task_id,
            &resolved,
            DesiredConfiguration::default(),
            Limits::default(),
        )
        .await
    });
    wait_wire(dir.path(), 2).await;
    {
        let db = rusqlite::Connection::open(dir.path().join("agent-state.db")).unwrap();
        db.execute(
            "UPDATE executions SET revision=revision+1 WHERE id=?1",
            [&id],
        )
        .unwrap();
    }
    let sink = Sink::default();
    std::fs::write(dir.path().join("release-new"), "").unwrap();
    assert!(matches!(task.await.unwrap(), Err(Failure::State)));
    assert!(!sink.0.load(Ordering::SeqCst));
    cleanup_evidence(&store, &id).await;
    assert!(
        store
            .read_codebuddy_state(id)
            .await
            .unwrap()
            .session_id
            .is_none()
    );
    assert_eq!(wire(dir.path()).len(), 2);
}
