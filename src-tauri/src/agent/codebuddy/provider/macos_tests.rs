//! macOS 配置目录通过正式 ACP Runtime 验证，fixture 不依赖已安装 CodeBuddy。

use super::*;
use std::{fs, path::Path, time::Duration};

/// 只响应 initialize/session/new 的原生 peer；禁止 prompt，记录自身 session/group 供清理断言。
const CATALOG_PEER: &str = r#"
use std::{fs, io::{self, BufRead, Write}};
unsafe extern "C" { fn getpgrp() -> i32; fn getsid(pid: i32) -> i32; }
/// 每次只回显请求 ID，不构造产品协议身份。
fn main() {
    let pid = std::process::id();
    // SAFETY: 无指针参数，仅查询当前进程所属组与 session。
    assert_eq!(unsafe { getpgrp() }, pid as i32);
    assert_eq!(unsafe { getsid(0) }, pid as i32);
    fs::write("peer-pid", pid.to_string()).unwrap();
    for line in io::stdin().lock().lines() {
        let line = line.unwrap();
        let id = line.split("\"id\":").nth(1).unwrap().split([',','}']).next().unwrap();
        let (key, body) = if line.contains("\"method\":\"initialize\"") {
            ("result", "{\"protocolVersion\":1}".to_string())
        } else if line.contains("\"method\":\"session/new\"") {
            if std::path::Path::new("fail-new").exists() {
                ("error", "{\"code\":-32603,\"message\":\"fixture failure\"}".to_string())
            } else {
                ("result", fs::read_to_string("new.json").unwrap())
            }
        } else { panic!("catalog must not issue prompt or other methods") };
        println!("{{\"jsonrpc\":\"2.0\",\"id\":{id},\"{key}\":{body}}}");
        io::stdout().flush().unwrap();
    }
}
"#;

/// 配置读取只创建临时 Runtime，不能落产品 Execution、Claim 或 runtime_instances。
fn assert_no_product_rows(data: &Path) {
    let connection = rusqlite::Connection::open(data.join("agent-state.db")).unwrap();
    for table in ["executions", "workspace_claims", "runtime_instances"] {
        let count: i64 = connection
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 0, "{table}");
    }
}

/// 成功与远端失败均走正式受管 Runtime，并在返回前确认原 process group 已空。
#[tokio::test]
async fn configuration_catalog_managed_group_converges_without_product_side_effects() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("catalog_peer.rs");
    let executable = directory.path().join("catalog-peer");
    fs::write(&source, CATALOG_PEER).unwrap();
    let compilation = std::process::Command::new("rustc")
        .args(["--edition=2024", "--crate-name", "codebuddy_catalog_peer"])
        .arg(source)
        .arg("-o")
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        compilation.status.success(),
        "{}",
        String::from_utf8_lossy(&compilation.stderr)
    );
    for fail in [false, true] {
        let workspace = directory.path().join(format!("workspace-{fail}"));
        fs::create_dir(&workspace).unwrap();
        if fail {
            fs::write(workspace.join("fail-new"), "1").unwrap();
        }
        fs::write(workspace.join("new.json"), serde_json::json!({
            "sessionId":"catalog-session",
            "modes":{"currentModeId":"default","availableModes":[{"id":"default","name":"Default"},{"id":"auto","name":"Auto"}]},
            "configOptions":[
                {"id":"model","name":"Model","category":"model","type":"select","currentValue":"model-a","options":[{"value":"model-a","name":"Model A"}]},
                {"id":"thought_level","name":"Reasoning","category":"thought_level","type":"select","currentValue":"high","options":[{"value":"high","name":"High"}]}
            ],
            "models":{"currentModelId":"model-a","availableModels":[{"modelId":"model-a","name":"Model A","_meta":{"supportsReasoning":true}}]}
        }).to_string()).unwrap();
        let data = directory.path().join(format!("data-{fail}"));
        let store = crate::agent::store::StateStore::open(data.clone())
            .await
            .unwrap();
        let provider = CodeBuddyProvider::from_discovery(
            store,
            "catalog-fixture-host".into(),
            Ok(DiscoveryResult::direct_for_test(&executable)),
        );
        let result = tokio::time::timeout(
            Duration::from_secs(20),
            provider.configuration_catalog(ProviderConfigurationCatalogContext {
                cwd: workspace
                    .canonicalize()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
            }),
        )
        .await
        .expect("managed catalog must finish within fixture deadline");
        if fail {
            assert_eq!(
                result.unwrap_err().code,
                ProviderErrorCode::AgentProviderOperationFailed
            );
        } else {
            let catalog = result.unwrap();
            assert_eq!(catalog.models[0].id, "model-a");
            assert_eq!(catalog.current_reasoning.as_deref(), Some("high"));
        }
        let pgid = fs::read_to_string(workspace.join("peer-pid"))
            .unwrap()
            .parse::<i32>()
            .unwrap();
        assert!(
            crate::agent::codex::macos_launcher::process_group_members(pgid)
                .unwrap()
                .is_empty()
        );
        assert_no_product_rows(&data);
    }
}

/// 显式本机 smoke：只执行受管 initialize/session/new，不发模型 prompt，不打印目录或凭据。
#[tokio::test]
#[ignore = "requires explicitly selected installed native CodeBuddy and its local authentication"]
async fn real_codebuddy_managed_configuration_catalog_smoke() {
    let directory = tempfile::tempdir().unwrap();
    let workspace = directory.path().join("workspace");
    let data = directory.path().join("data");
    fs::create_dir(&workspace).unwrap();
    let discovery = crate::agent::codebuddy::discover().expect("native CodeBuddy discovery");
    let store = crate::agent::store::StateStore::open(data.clone())
        .await
        .unwrap();
    let provider =
        CodeBuddyProvider::from_discovery(store, "real-catalog-smoke".into(), Ok(discovery));
    let catalog = provider
        .configuration_catalog(ProviderConfigurationCatalogContext {
            cwd: workspace
                .canonicalize()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
        })
        .await
        .expect("real managed CodeBuddy ACP catalog");
    assert!(
        !catalog.models.is_empty(),
        "real catalog must expose selectable models"
    );
    assert_no_product_rows(&data);
}

/// 显式验证真实 CLI 的 initialize 与整组清理，将握手和 session/new 故障分开报告。
#[tokio::test]
#[ignore = "requires explicitly selected installed native CodeBuddy"]
async fn real_codebuddy_managed_initialize_smoke() {
    use crate::agent::codebuddy::{
        platform_launcher::{LaunchRequest, UncCurrentDirectoryPolicy},
        protocol::Limits,
        runtime::Runtime,
    };
    let directory = tempfile::tempdir().unwrap();
    let discovery = crate::agent::codebuddy::discover().expect("native CodeBuddy discovery");
    let request = LaunchRequest::from_resolved(
        &discovery.launch_spec,
        &directory.path().canonicalize().unwrap(),
        UncCurrentDirectoryPolicy::Unsupported,
        "codebuddy-real-initialize-smoke".into(),
    )
    .unwrap();
    let (runtime, handshake) = Runtime::start(request, Limits::default())
        .await
        .unwrap_or_else(|failure| panic!("managed initialize: {}", failure.code()));
    assert_eq!(
        handshake.response.protocol_version,
        agent_client_protocol::schema::ProtocolVersion::V1
    );
    runtime
        .shutdown()
        .await
        .expect("real CodeBuddy process group cleanup");
}

/// 固定版本真实 Usage 合同：生产 discovery/Runtime/SDK，不输出正文或原始 wire。
#[tokio::test]
#[ignore = "requires installed CodeBuddy 2.160.0 and local authentication; sends three bounded prompts"]
async fn real_codebuddy_managed_end_turn_usage_contract() {
    use crate::agent::codebuddy::{
        platform_launcher::{LaunchRequest, UncCurrentDirectoryPolicy},
        protocol::Limits,
        runtime::Runtime,
    };
    use agent_client_protocol::schema::v1::{
        ContentBlock, LoadSessionRequest, NewSessionRequest, PromptRequest, TextContent,
    };
    use futures::FutureExt;
    let directory = tempfile::tempdir().unwrap();
    let workspace = directory.path().join("workspace");
    fs::create_dir(&workspace).unwrap();
    let cwd = workspace.canonicalize().unwrap();
    let discovery = crate::agent::codebuddy::discover().expect("managed native discovery");
    let version = std::process::Command::new(&discovery.launch_spec.executable)
        .arg("--version")
        .output()
        .unwrap();
    assert_eq!(String::from_utf8(version.stdout).unwrap().trim(), "2.160.0");
    let mut session: Option<String> = None;
    let mut all_usage_present = true;
    for runtime_index in 0..2 {
        let request = LaunchRequest::from_resolved(
            &discovery.launch_spec,
            &cwd,
            UncCurrentDirectoryPolicy::Unsupported,
            format!("usage-contract-{runtime_index}"),
        )
        .unwrap();
        let (runtime, handshake) = Runtime::start(request, Limits::default()).await.unwrap();
        assert_eq!(handshake.response.protocol_version.as_u16(), 1);
        let result = std::panic::AssertUnwindSafe(tokio::time::timeout(Duration::from_secs(180), async {
            let requests = &runtime.client.as_ref().unwrap().requests;
            let sid = if let Some(sid) = session.as_ref() {
                requests.shared.register_route(sid).unwrap();
                requests.request(LoadSessionRequest::new(sid.clone(), cwd.clone())).await.unwrap();
                requests.shared.take_session_load_extensions(sid).unwrap();
                requests.shared.take_continuation_replay(sid).unwrap();
                sid.clone()
            } else {
                let sid = requests.request(NewSessionRequest::new(cwd.clone()))
                    .await.unwrap().session_id.to_string();
                requests.shared.take_session_new_extensions(&sid).unwrap();
                requests.shared.register_route(&sid).unwrap();
                sid
            };
            let mut usage_present = true;
            for turn in if runtime_index == 0 { 1..=2 } else { 3..=3 } {
                let conversation = crate::agent::codebuddy::store::new_conversation_id().unwrap();
                let prompt = PromptRequest::new(sid.clone(), vec![
                    ContentBlock::Text(TextContent::new("Do not use tools or access files. Reply with OK only.")),
                ]).meta(serde_json::Map::from_iter([(
                    "codebuddy.ai/conversationRequestId".into(), serde_json::json!(conversation),
                )]));
                let pending = requests.request(prompt);
                tokio::pin!(pending);
                let mut drain = tokio::time::interval(Duration::from_millis(20));
                // 和生产 Prompt 相同，持续消费受管有界 session queue，防止长模型请求 TTL 失效。
                let response = loop {
                    tokio::select! {
                        result = &mut pending => break result.expect("managed typed PromptResponse"),
                        _ = drain.tick() => { requests.shared.take_session(&sid).unwrap(); }
                    }
                };
                assert_eq!(response.meta.as_ref().unwrap().get("codebuddy.ai/conversationRequestId")
                    .and_then(serde_json::Value::as_str), Some(conversation.as_str()));
                assert_eq!(response.stop_reason, agent_client_protocol::schema::v1::StopReason::EndTurn);
                // 缺失也继续采集 second/load；只输出身份与 typed 数字，不输出正文/meta 原文。
                if let Some(usage) = response.usage {
                    println!("managed Usage runtime={runtime_index} turn={turn} session={sid} conversation={conversation} total={} input={} output={} thought={:?} cache_read={:?} cache_write={:?}",
                        usage.total_tokens, usage.input_tokens, usage.output_tokens,
                        usage.thought_tokens, usage.cached_read_tokens, usage.cached_write_tokens);
                } else {
                    usage_present = false;
                    println!("managed Usage runtime={runtime_index} turn={turn} session={sid} conversation={conversation} stop=end_turn conversation_matched=true usage=None");
                }
                requests.shared.take_session(&sid).unwrap();
            }
            (sid, usage_present)
        })).catch_unwind().await;
        runtime.shutdown().await.expect("managed group cleanup");
        let (sid, usage_present) = result
            .expect("managed Usage assertions after cleanup")
            .expect("bounded real Usage contract");
        all_usage_present &= usage_present;
        session = Some(sid);
    }
    assert!(
        all_usage_present,
        "BLOCKED: CodeBuddy 2.160.0 PromptResponse.usage absent"
    );
}
