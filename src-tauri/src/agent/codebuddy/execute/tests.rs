//! public Provider native vertical slice；真实 Job-at-creation、pipe 与 SQLite。
use super::*;
use crate::agent::codebuddy::{discovery::DiscoveryResult, provider::CodeBuddyProvider};
use crate::agent::{
    execution::{CreateExecutionInput, canonicalize_request},
    provider::{ProviderExecutionContext, ProviderStartupContext, port::AgentProvider},
};
use serde_json::{Value, json};
use std::{
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
};

struct Sink(PathBuf);

#[tokio::test]
/// user cancel 已物理发送后到来的 permission 仍只选 typed deny，不生成第二次 cancel。
async fn native_permission_after_user_cancel_remains_denied() {
    use crate::agent::provider::ProviderCancelContext;
    let bin = tempfile::tempdir().unwrap();
    let binary = build(bin.path());
    let (control, _workspace, store, provider) = setup(&binary, "permission-cancel-first").await;
    let provider = Arc::new(provider);
    let running = provider.clone();
    let sink = Arc::new(Sink(control.path().into()));
    let telemetry = Arc::new(
        crate::agent::telemetry_projector::ExecutionTelemetryProjector::new(
            store.clone(),
            "e".into(),
        ),
    );
    let task = tokio::spawn(async move {
        running
            .execute(
                ProviderExecutionContext {
                    execution_id: "e".into(),
                },
                sink,
                telemetry,
            )
            .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while !control.path().join("permission-ready").exists() {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    provider
        .cancel(ProviderCancelContext {
            execution_id: "e".into(),
        })
        .await
        .unwrap();
    let result = tokio::time::timeout(std::time::Duration::from_secs(10), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(result.outcome, ProviderOutcome::Cancelled);
    assert!(control.path().join("cancel.json").exists());
    assert!(control.path().join("permission-response.json").exists());
    let row = store.execution("e".into()).await.unwrap().unwrap();
    assert_eq!(row.provider_terminal_status.as_deref(), Some("cancelled"));
    assert_eq!(row.release_evidence_state, "complete");
}

/// 测试委托真实安全投影，同时捕获闭集事件；不以事件替代 Runtime evidence。
struct PermissionTelemetry {
    projector: crate::agent::telemetry_projector::ExecutionTelemetryProjector,
    events: Arc<std::sync::Mutex<Vec<crate::agent::provider::telemetry::AgentActivityEvent>>>,
}
impl AgentEventSink for PermissionTelemetry {
    /// 捕获固定语义并交给真实 projector，保持与生产同一事务路径。
    fn publish(
        &self,
        event: crate::agent::provider::telemetry::AgentTelemetryEvent,
    ) -> crate::agent::provider::port::ProviderFuture<'_, ()> {
        Box::pin(async move {
            self.projector.publish(event.clone()).await;
            if let crate::agent::provider::telemetry::AgentTelemetryEvent::Activity(activity) =
                event
            {
                assert!(!format!("{activity:?}").contains("private"));
                self.events.lock().unwrap().push(activity);
            }
        })
    }
}

#[tokio::test]
/// native before/after side effect、即时 cancelled/end_turn/EOF 与不兼容 options：只有真实 terminal 决定结果。
async fn native_permission_terminal_and_cleanup_matrix() {
    let bin = tempfile::tempdir().unwrap();
    let binary = build(bin.path());
    for mode in [
        "permission-read",
        "permission-write",
        "permission-end-turn",
        "permission-eof",
        "permission-no-reject",
        "permission-malformed",
        "permission-timeout",
    ] {
        for evidence_fault in [false, true] {
            if evidence_fault && mode != "permission-eof" {
                continue;
            }
            let (control, workspace, store, provider) = setup(&binary, mode).await;
            // 真实 SQLite 诊断写入轨迹证明即刻 terminal/EOF 也不丢弃 deny，且诊断无终态权限。
            let db = rusqlite::Connection::open(control.path().join("agent-state.db")).unwrap();
            db.execute_batch("CREATE TABLE permission_trace(message TEXT,terminal TEXT,evidence TEXT,claims INTEGER); CREATE TRIGGER permission_trace AFTER UPDATE OF error_code ON executions WHEN NEW.error_code='CODEBUDDY_PERMISSION_DENIED' AND OLD.error_code IS NOT NEW.error_code BEGIN INSERT INTO permission_trace VALUES(NEW.error_message,NEW.provider_terminal_status,NEW.release_evidence_state,(SELECT count(*) FROM workspace_claims)); END;").unwrap();
            if evidence_fault {
                rusqlite::Connection::open(control.path().join("agent-state.db")).unwrap().execute_batch("CREATE TRIGGER fail_evidence BEFORE UPDATE OF termination_evidence_state ON runtime_instances WHEN NEW.termination_evidence_state='complete' BEGIN SELECT RAISE(ABORT,'evidence fault'); END;").unwrap();
            }
            let events = Arc::new(std::sync::Mutex::new(Vec::new()));
            let telemetry = Arc::new(PermissionTelemetry {
                projector: crate::agent::telemetry_projector::ExecutionTelemetryProjector::new(
                    store.clone(),
                    "e".into(),
                ),
                events: events.clone(),
            });
            let result = tokio::time::timeout(
                std::time::Duration::from_secs(25),
                provider.execute(
                    ProviderExecutionContext {
                        execution_id: "e".into(),
                    },
                    Arc::new(Sink(control.path().into())),
                    telemetry,
                ),
            )
            .await
            .unwrap();
            assert_eq!(provider.admission_diagnostic(), None, "{mode}");
            let row = store.execution("e".into()).await.unwrap().unwrap();
            let terminal = match mode {
                "permission-read" | "permission-write" => Some("cancelled"),
                "permission-end-turn" => Some("completed"),
                _ => None,
            };
            assert_eq!(row.provider_terminal_status.as_deref(), terminal, "{mode}");
            assert_eq!(
                row.status,
                if evidence_fault {
                    "unknown"
                } else {
                    terminal.unwrap_or("interrupted")
                },
                "{mode}"
            );
            if let Some(terminal) = terminal {
                assert_eq!(
                    result.unwrap().outcome,
                    if terminal == "completed" {
                        ProviderOutcome::Completed
                    } else {
                        ProviderOutcome::Cancelled
                    }
                );
            }
            assert_eq!(
                store
                    .workspace_claim(row.canonical_workspace_root)
                    .await
                    .unwrap()
                    .is_some(),
                evidence_fault
            );
            let runtime = store
                .runtime(row.runtime_instance_id.unwrap())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                runtime.termination_evidence_state == "complete",
                !evidence_fault
            );
            assert_eq!(row.release_evidence_state == "complete", !evidence_fault);
            assert!(row.interrupt_requested_at.is_none());
            assert!(!control.path().join("cancel.json").exists());
            let valid = !matches!(mode, "permission-no-reject" | "permission-malformed");
            let history = store
                .execution_activity_history("e".into(), None, Some(100))
                .await
                .unwrap();
            assert_eq!(
                history
                    .events
                    .iter()
                    .filter(
                        |event| event.summary_code.as_deref() == Some("provider.permission_denied")
                    )
                    .count(),
                usize::from(valid)
            );
            assert_eq!(
                control.path().join("permission-response.json").exists(),
                valid,
                "{mode}"
            );
            let events = events.lock().unwrap();
            assert_eq!(
                events
                    .iter()
                    .filter(|event| event.phase() == crate::agent::activity::ActivityPhase::Provider)
                    .count(),
                0,
                "permission Activity is written directly, not a plain processing sink event: {mode}"
            );
            let trace: Vec<(String, Option<String>, String, i64)> = db
                .prepare("SELECT * FROM permission_trace")
                .unwrap()
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
                .unwrap()
                .map(Result::unwrap)
                .collect();
            assert_eq!(trace.len(), usize::from(valid), "{mode}");
            for (message, terminal, evidence, claims) in trace {
                assert_eq!(message, "Provider permission denied");
                assert!(terminal.is_none());
                assert_ne!(evidence, "complete");
                assert_eq!(claims, 1);
            }
            if mode == "permission-write" {
                assert_eq!(
                    std::fs::read(workspace.path().join("marker.txt")).unwrap(),
                    b"CB8_PERMISSION_WRITE\n"
                );
                assert_eq!(std::fs::read_dir(workspace.path()).unwrap().count(), 1);
            } else {
                assert_eq!(std::fs::read_dir(workspace.path()).unwrap().count(), 0);
            }
        }
    }
}

#[tokio::test]
/// native deny 与 user cancel 独立，live Job 时 Activity 不写 terminal/release，错误 identity 不投影。
async fn native_permission_cancel_race_and_safe_projection() {
    use crate::agent::provider::ProviderCancelContext;
    let bin = tempfile::tempdir().unwrap();
    let binary = build(bin.path());
    let (control, workspace, store, provider) = setup(&binary, "permission-cancel").await;
    let provider = Arc::new(provider);
    let running = provider.clone();
    let events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let telemetry = Arc::new(PermissionTelemetry {
        projector: crate::agent::telemetry_projector::ExecutionTelemetryProjector::new(
            store.clone(),
            "e".into(),
        ),
        events: events.clone(),
    });
    let sink = Arc::new(Sink(control.path().into()));
    let task = tokio::spawn(async move {
        running
            .execute(
                ProviderExecutionContext {
                    execution_id: "e".into(),
                },
                sink,
                telemetry,
            )
            .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let row = store.execution("e".into()).await.unwrap().unwrap();
            if row.error_code.as_deref() == Some("CODEBUDDY_PERMISSION_DENIED")
                && row.activity_summary_code.as_deref() == Some("provider.permission_denied")
            {
                break;
            }
            assert!(!task.is_finished());
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let row = store.execution("e".into()).await.unwrap().unwrap();
    assert_eq!(
        row.error_message.as_deref(),
        Some("Provider permission denied")
    );
    assert!(row.provider_terminal_status.is_none());
    assert_ne!(row.release_evidence_state, "complete");
    assert!(row.interrupt_requested_at.is_none());
    assert!(
        store
            .workspace_claim(row.canonical_workspace_root.clone())
            .await
            .unwrap()
            .is_some()
    );
    let private = store.read_codebuddy_state("e".into()).await.unwrap();
    // MCP 与桌面共用的 Product operation 必须在 Running 期间投影真实拒绝摘要。
    let service = crate::agent::product::AgentProductService::new(store.clone());
    let view = service
        .operation(
            json!({"action":"observe","executionId":"e","waitMs":0}),
            None,
        )
        .await;
    assert_eq!(view["ok"], true, "{view}");
    assert_eq!(
        view["data"]["progress"]["summaryCode"],
        "provider.permission_denied"
    );
    assert_eq!(view["data"]["progress"]["activityPhase"], "provider");
    assert!(view["data"]["progress"]["toolCategory"].is_null());
    assert!(view["data"].get("ownsClaim").is_none());
    assert!(view["data"]["providerTerminalStatus"].is_null());
    let history = store
        .execution_activity_history("e".into(), None, Some(100))
        .await
        .unwrap();
    let denied = history.events.last().unwrap();
    assert_eq!(
        denied.summary_code.as_deref(),
        Some("provider.permission_denied")
    );
    assert_eq!(denied.activity_revision, view["data"]["activityRevision"]);
    assert!(!view.to_string().contains("private command"));
    for identity in [
        (
            "wrong-runtime".into(),
            private.session_id.clone().unwrap(),
            private.conversation_request_id.clone(),
        ),
        (
            private.runtime_instance_id.clone().unwrap(),
            "wrong-session".into(),
            private.conversation_request_id.clone(),
        ),
        (
            private.runtime_instance_id.clone().unwrap(),
            private.session_id.clone().unwrap(),
            "wrong-prompt".into(),
        ),
    ] {
        assert!(
            store
                .project_codebuddy_permission_denied("e".into(), identity.clone(), now())
                .await
                .is_err()
        );
        // 私有事件已在旧 context flush 后才遇到 durable identity 变化：不得向普通 sink 发 Activity。
        let shared = crate::agent::codebuddy::protocol::Shared::new(Default::default());
        let (_lease, mut pending) = shared
            .register_permission(
                identity.0,
                "e".into(),
                identity.1.clone(),
                identity.2.clone(),
                None,
            )
            .unwrap();
        shared.activate_permission();
        shared.notification("session/update".into(), json!({"sessionId":identity.1,"update":{"sessionUpdate":"tool_call","toolCallId":"t","title":"private command","status":"pending","_meta":{"codebuddy.ai/conversationRequestId":identity.2}}})).unwrap();
        shared.permission_response(&serde_json::from_value(json!({"sessionId":identity.1,"toolCall":{"toolCallId":"t"},"options":[{"kind":"reject_once","optionId":"advertised","name":"Deny"}]})).unwrap(), json!(0)).unwrap();
        shared.permission_flushed().unwrap();
        let before = events.lock().unwrap().len();
        pending.try_recv().unwrap().project(&store).await;
        assert_eq!(events.lock().unwrap().len(), before);
    }
    assert_eq!(
        store.execution("e".into()).await.unwrap().unwrap().revision,
        row.revision
    );
    // 真实 wire 新 ToolCall 在 deny 后才能产生，序号过滤不得抑制它。
    std::fs::write(control.path().join("next-activity"), "").unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while store
            .execution("e".into())
            .await
            .unwrap()
            .unwrap()
            .activity_summary_code
            .as_deref()
            != Some("tool.read")
        {
            assert!(!task.is_finished());
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let ordered = store
        .execution_activity_history("e".into(), None, Some(100))
        .await
        .unwrap();
    assert_eq!(
        ordered
            .events
            .iter()
            .rev()
            .take(2)
            .map(|event| event.summary_code.as_deref())
            .collect::<Vec<_>>(),
        vec![Some("tool.read"), Some("provider.permission_denied")]
    );
    // 后续真正的新普通活动重置 processing，但历史中的 deny 不能被覆盖。
    store
        .project_execution_activity(
            "e".into(),
            crate::agent::activity::ActivityPhase::Provider,
            None,
            now(),
        )
        .await
        .unwrap();
    let next = service
        .operation(
            json!({"action":"observe","executionId":"e","waitMs":0}),
            None,
        )
        .await;
    assert_eq!(
        next["data"]["progress"]["summaryCode"],
        "provider.processing"
    );
    assert_ne!(
        next["data"]["activityRevision"],
        view["data"]["activityRevision"]
    );
    let history = store
        .execution_activity_history("e".into(), None, Some(100))
        .await
        .unwrap();
    assert_eq!(
        history
            .events
            .iter()
            .rev()
            .take(2)
            .map(|event| event.summary_code.as_deref())
            .collect::<Vec<_>>(),
        vec![Some("provider.processing"), Some("tool.read")]
    );
    provider
        .cancel(ProviderCancelContext {
            execution_id: "e".into(),
        })
        .await
        .unwrap();
    let result = tokio::time::timeout(std::time::Duration::from_secs(10), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(result.outcome, ProviderOutcome::Cancelled);
    assert!(control.path().join("cancel.json").exists());
    assert!(control.path().join("permission-response.json").exists());
    let final_row = store.execution("e".into()).await.unwrap().unwrap();
    assert_eq!(
        final_row.provider_terminal_status.as_deref(),
        Some("cancelled")
    );
    assert_eq!(final_row.release_evidence_state, "complete");
    assert!(
        store
            .workspace_claim(final_row.canonical_workspace_root)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(std::fs::read_dir(workspace.path()).unwrap().count(), 0);
}
impl ProviderAcceptanceSink for Sink {
    /// acceptance 必须在 private Sent 后、generic Dispatching 与物理 Prompt 前。
    fn accepted(&self) {
        let db = rusqlite::Connection::open(self.0.join("agent-state.db")).unwrap();
        let (dispatch, private): (String,String) = db.query_row("SELECT dispatch_state,prompt_state FROM executions JOIN codebuddy_execution_state ON executions.id=codebuddy_execution_state.execution_id", [], |r| Ok((r.get(0)?,r.get(1)?))).unwrap();
        assert_eq!(dispatch, "not_dispatched");
        assert_eq!(private, "sent");
        assert!(!self.0.join("prompt.json").exists());
        std::fs::write(self.0.join("accepted"), "").unwrap();
    }
}
struct SlowTelemetry;
impl AgentEventSink for SlowTelemetry {
    /// 永不完成的 activity consumer 不能阻塞 terminal/finalization。
    fn publish<'a>(
        &'a self,
        event: crate::agent::provider::telemetry::AgentTelemetryEvent,
    ) -> crate::agent::provider::port::ProviderFuture<'a, ()> {
        let safe = format!("{event:?}");
        assert!(!safe.contains("private command"));
        Box::pin(std::future::pending())
    }
}

/// 复用当前 Cargo 已构建依赖；fake child 本身独立 native executable。
fn build(dir: &Path) -> PathBuf {
    let deps = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_owned();
    let mut command = std::process::Command::new("rustc");
    command
        .args(["--edition=2024", "--crate-name", "codebuddy_execute_child"])
        .arg(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/codebuddy_execute_child.rs"),
        )
        .arg("-L")
        .arg(format!("dependency={}", deps.display()));
    for name in ["rusqlite", "serde_json"] {
        let library = std::fs::read_dir(&deps)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with(&format!("lib{name}-"))
                    && e.path().extension().is_some_and(|v| v == "rlib")
            })
            .max_by_key(|e| e.metadata().unwrap().modified().unwrap())
            .expect("built dependency")
            .path();
        command
            .arg("--extern")
            .arg(format!("{name}={}", library.display()));
    }
    let binary = dir.join("peer.exe");
    let output = command
        .arg("-o")
        .arg(&binary)
        .creation_flags(0x0800_0000)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    binary
}

/// Workspace 与 Store/control 分离，所有真实文件 delta 可完整核对。
async fn setup(
    binary: &Path,
    mode: &str,
) -> (
    tempfile::TempDir,
    tempfile::TempDir,
    StateStore,
    CodeBuddyProvider,
) {
    let control = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let store = StateStore::open(control.path().into()).await.unwrap();
    let root = crate::config::canonicalize_workspace_root(workspace.path()).unwrap();
    let input: CreateExecutionInput = serde_json::from_value(json!({"agent_id":"a","request_key":"k","prompt":"fixed fake input","execution_profile":{},"workspace_id":"w","canonical_workspace_root":root,"mode":if matches!(mode,"write" | "source-identity") {"workspace_write"} else {"read_only"},"provider":"codebuddy"})).unwrap();
    store
        .create_execution("e".into(), canonicalize_request(input).unwrap(), now())
        .await
        .unwrap();
    let db = rusqlite::Connection::open(control.path().join("agent-state.db")).unwrap();
    db.execute_batch("CREATE TABLE cb7_trace(status TEXT,dispatch TEXT,claims INTEGER,runtime_state TEXT,evidence TEXT); CREATE TRIGGER cb7_trace AFTER UPDATE OF status,dispatch_state ON executions BEGIN INSERT INTO cb7_trace SELECT NEW.status,NEW.dispatch_state,(SELECT count(*) FROM workspace_claims),state,termination_evidence_state FROM runtime_instances WHERE id=NEW.runtime_instance_id; END;").unwrap();
    std::fs::write(control.path().join("mode"), mode).unwrap();
    let peer = control.path().join("peer.exe");
    std::fs::copy(binary, &peer).unwrap();
    let discovery = DiscoveryResult::direct_for_test(peer);
    let mut provider =
        CodeBuddyProvider::from_discovery(store.clone(), "host".into(), Ok(discovery));
    if mode == "permission-timeout" {
        provider = provider.with_limits_for_test(Limits {
            prompt_timeout: std::time::Duration::from_millis(300),
            ..Limits::default()
        });
    }
    (control, workspace, store, provider)
}

/// Continue acceptance 只能发生在 child 自己的 load/recovery 与 durable prompt intent 之后。
struct ContinuationSink(PathBuf);
impl ProviderAcceptanceSink for ContinuationSink {
    /// 同步观察 child 的 Runtime/Claim；source 已终止，不能充当本次释放证据。
    fn accepted(&self) {
        let db = rusqlite::Connection::open(self.0.join("agent-state.db")).unwrap();
        let (dispatch, prompt, recovery, runtime, claims): (String, String, String, String, i64) = db
            .query_row(
                "SELECT e.dispatch_state,s.prompt_state,s.recovery_state,r.state,(SELECT count(*) FROM workspace_claims) FROM executions e JOIN codebuddy_execution_state s ON s.execution_id=e.id JOIN runtime_instances r ON r.id=e.runtime_instance_id WHERE e.id='c'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
            )
            .unwrap();
        assert_eq!(dispatch, "not_dispatched");
        assert_eq!(prompt, "sent");
        assert_eq!(recovery, "partial");
        assert_eq!(runtime, "running");
        assert_eq!(claims, 1);
        assert!(!self.0.join("prompt.json").exists());
        std::fs::write(self.0.join("accepted-child"), "").unwrap();
    }
}

/// 先完成真实 R1，再由通用 product transaction 创建 child；只替换 peer 的下一次响应模式。
async fn setup_continuation(
    binary: &Path,
    mode: &str,
) -> (
    tempfile::TempDir,
    tempfile::TempDir,
    StateStore,
    Arc<CodeBuddyProvider>,
) {
    use crate::agent::provider::port::{ProviderContinuationContext, ProviderContinuationDecision};
    // 通用 Continue 契约只允许 workspace_write source；这里先走完整 Fresh 生产路径。
    let (control, workspace, store, provider) = setup(binary, "source-identity").await;
    let provider = Arc::new(provider);
    let source = provider
        .execute(
            ProviderExecutionContext {
                execution_id: "e".into(),
            },
            Arc::new(Sink(control.path().into())),
            Arc::new(SlowTelemetry),
        )
        .await
        .unwrap();
    assert_eq!(source.outcome, ProviderOutcome::Completed);
    assert_eq!(
        store
            .read_codebuddy_state("e".into())
            .await
            .unwrap()
            .provider_request_id
            .as_deref(),
        Some("source-provider-request")
    );
    assert_eq!(
        provider
            .validate_continuation(ProviderContinuationContext {
                source_execution_id: "e".into(),
            })
            .await
            .unwrap(),
        ProviderContinuationDecision::Eligible
    );
    for evidence in ["accepted", "prompt.json", "requests.jsonl"] {
        std::fs::remove_file(control.path().join(evidence)).unwrap();
    }
    std::fs::write(control.path().join("mode"), mode).unwrap();
    let created = store
        .product_create_continuation(
            "c".into(),
            "e".into(),
            format!("child-{mode}"),
            "child prompt".into(),
            now(),
        )
        .await
        .unwrap();
    assert!(created.created);
    assert_eq!(created.execution.parent_execution_id.as_deref(), Some("e"));
    assert!(created.execution.runtime_instance_id.is_none());
    (control, workspace, store, provider)
}

/// 读取 fake peer 捕获的完整请求方法序列。
fn request_methods(control: &Path) -> Vec<String> {
    std::fs::read_to_string(control.join("requests.jsonl"))
        .unwrap_or_default()
        .lines()
        .map(|line| {
            serde_json::from_str::<Value>(line).unwrap()["method"]
                .as_str()
                .unwrap_or("permission-response")
                .to_owned()
        })
        .collect()
}

#[tokio::test]
/// Fresh 的 non-reasoning 模型仍可派发，且不得把残留 thought_level 写成历史事实。
async fn native_fresh_nonreasoning_model_persists_no_stale_reasoning_before_prompt() {
    let bin = tempfile::tempdir().unwrap();
    let binary = build(bin.path());
    let (control, _workspace, store, provider) = setup(&binary, "nonreasoning").await;
    let result = provider
        .execute(
            ProviderExecutionContext {
                execution_id: "e".into(),
            },
            Arc::new(Sink(control.path().into())),
            Arc::new(SlowTelemetry),
        )
        .await
        .unwrap();

    assert_eq!(result.outcome, ProviderOutcome::Completed);
    let row = store.execution("e".into()).await.unwrap().unwrap();
    let effective = crate::agent::execution::ExecutionProfile::from_json(
        row.effective_execution_profile_json.as_deref().unwrap(),
    )
    .unwrap();
    assert_eq!(effective.model.as_deref(), Some("model-plain"));
    assert!(effective.reasoning.is_none());
    assert!(control.path().join("prompt.json").exists());
}

#[tokio::test]
/// malformed model metadata 在 public Provider 路径 fail closed，不能持久化或发送 Prompt。
async fn native_fresh_malformed_effective_meta_sends_no_prompt() {
    let bin = tempfile::tempdir().unwrap();
    let binary = build(bin.path());
    let (control, _workspace, store, provider) = setup(&binary, "malformed-effective-meta").await;
    let result = provider
        .execute(
            ProviderExecutionContext {
                execution_id: "e".into(),
            },
            Arc::new(Sink(control.path().into())),
            Arc::new(SlowTelemetry),
        )
        .await;

    assert!(result.is_err());
    let row = store.execution("e".into()).await.unwrap().unwrap();
    assert!(row.effective_execution_profile_json.is_none());
    assert!(!control.path().join("accepted").exists());
    assert!(!control.path().join("prompt.json").exists());
}

#[tokio::test]
/// R1→R2 只执行 initialize/load/prompt；历史只验证 S1，不进入 child result。
async fn native_continuation_uses_new_runtime_and_exact_load_only() {
    let bin = tempfile::tempdir().unwrap();
    let binary = build(bin.path());
    let (control, _workspace, store, provider) =
        setup_continuation(&binary, "continue-success").await;
    let source_row = store.execution("e".into()).await.unwrap().unwrap();
    let source_private = store.read_codebuddy_state("e".into()).await.unwrap();
    let result = provider
        .execute(
            ProviderExecutionContext {
                execution_id: "c".into(),
            },
            Arc::new(ContinuationSink(control.path().into())),
            Arc::new(
                crate::agent::telemetry_projector::ExecutionTelemetryProjector::new(
                    store.clone(),
                    "c".into(),
                ),
            ),
        )
        .await
        .unwrap();
    assert_eq!(result.outcome, ProviderOutcome::Completed);
    assert_eq!(result.result, Some(json!({"text":"safe result"})));
    assert_eq!(
        request_methods(control.path()),
        vec!["initialize", "session/load", "session/prompt"]
    );
    assert!(!control.path().join("forbidden-session-new").exists());
    assert!(!control.path().join("forbidden-session-resume").exists());
    let load: Value =
        serde_json::from_str(&std::fs::read_to_string(control.path().join("load.json")).unwrap())
            .unwrap();
    assert_eq!(load["params"]["sessionId"], "exact-session");
    assert_eq!(load["params"]["mcpServers"], json!([]));
    let prompt = std::fs::read_to_string(control.path().join("prompt.json")).unwrap();
    assert!(prompt.contains("child prompt"));
    assert!(!prompt.contains("parent prompt sentinel"));
    let child_row = store.execution("c".into()).await.unwrap().unwrap();
    let child_private = store.read_codebuddy_state("c".into()).await.unwrap();
    assert_eq!(child_row.parent_execution_id.as_deref(), Some("e"));
    assert_ne!(
        child_row.runtime_instance_id,
        source_row.runtime_instance_id
    );
    assert_eq!(child_private.session_id, source_private.session_id);
    assert_ne!(
        child_private.conversation_request_id,
        source_private.conversation_request_id
    );
    assert!(child_private.provider_request_id.is_none());
    assert_eq!(
        child_private.recovery_method.as_deref(),
        Some("session/load")
    );
    assert_eq!(
        child_private.recovery_state,
        crate::agent::codebuddy::store::RecoveryState::Partial
    );
    for row in [source_row, child_row] {
        let runtime = store
            .runtime(row.runtime_instance_id.unwrap())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(runtime.state, "terminated");
        assert_eq!(runtime.termination_evidence_state, "complete");
    }
}

#[tokio::test]
/// Continue child 必须采用 exact load 的 non-reasoning 配置，不能继承 source 的 reasoning。
async fn native_continuation_nonreasoning_model_uses_child_session_fact() {
    let bin = tempfile::tempdir().unwrap();
    let binary = build(bin.path());
    let (control, _workspace, store, provider) =
        setup_continuation(&binary, "continue-nonreasoning").await;
    let result = provider
        .execute(
            ProviderExecutionContext {
                execution_id: "c".into(),
            },
            Arc::new(ContinuationSink(control.path().into())),
            Arc::new(SlowTelemetry),
        )
        .await
        .unwrap();

    assert_eq!(result.outcome, ProviderOutcome::Completed);
    let source = store.execution("e".into()).await.unwrap().unwrap();
    let child = store.execution("c".into()).await.unwrap().unwrap();
    let source = crate::agent::execution::ExecutionProfile::from_json(
        source.effective_execution_profile_json.as_deref().unwrap(),
    )
    .unwrap();
    let child = crate::agent::execution::ExecutionProfile::from_json(
        child.effective_execution_profile_json.as_deref().unwrap(),
    )
    .unwrap();
    assert_eq!(source.reasoning.as_deref(), Some("medium"));
    assert_eq!(child.model.as_deref(), Some("model-plain"));
    assert!(child.reasoning.is_none());
    assert_eq!(
        request_methods(control.path()),
        vec!["initialize", "session/load", "session/prompt"]
    );
}

#[tokio::test]
/// 错 Session、response mismatch、缺失/空洞历史与缺 capability 均在 acceptance 前关闭。
async fn native_continuation_rejects_unproven_load_matrix() {
    let bin = tempfile::tempdir().unwrap();
    let binary = build(bin.path());
    for mode in [
        "continue-wrong-session",
        "continue-load-mismatch",
        "continue-missing-history",
        "continue-unusable-history",
        "continue-empty-object",
        "continue-empty-text",
        "continue-whitespace-history",
        "continue-malformed-history",
        "continue-no-capability",
    ] {
        let (control, _workspace, store, provider) = setup_continuation(&binary, mode).await;
        let error = provider
            .execute(
                ProviderExecutionContext {
                    execution_id: "c".into(),
                },
                Arc::new(ContinuationSink(control.path().into())),
                Arc::new(SlowTelemetry),
            )
            .await
            .unwrap_err();
        assert_eq!(
            error,
            ProviderExecutionFailure::State("CODEBUDDY_CONTINUATION_VALIDATION_FAILED".into()),
            "{mode}"
        );
        assert!(!control.path().join("accepted-child").exists(), "{mode}");
        assert!(!control.path().join("prompt.json").exists(), "{mode}");
        let methods = request_methods(control.path());
        assert_eq!(methods.first().map(String::as_str), Some("initialize"));
        assert!(!methods.iter().any(|method| matches!(
            method.as_str(),
            "session/new" | "session/resume" | "session/prompt"
        )));
        let row = store.execution("c".into()).await.unwrap().unwrap();
        let runtime = store
            .runtime(row.runtime_instance_id.unwrap())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(runtime.state, "terminated");
        assert_eq!(runtime.termination_evidence_state, "complete");
        assert_eq!(row.release_evidence_state, "complete");
    }
}

#[tokio::test]
/// child/source 的 parent、cwd 与 generation 任一漂移都必须在创建 Runtime 前失败。
async fn native_continuation_rejects_lineage_drift_before_runtime() {
    let bin = tempfile::tempdir().unwrap();
    let binary = build(bin.path());
    for statement in [
        "UPDATE executions SET parent_execution_id='c' WHERE id='c'",
        "UPDATE executions SET canonical_workspace_root='C:/wrong-workspace' WHERE id='c'",
        "UPDATE executions SET workspace_generation=workspace_generation+1 WHERE id='c'",
    ] {
        let (control, _workspace, store, provider) =
            setup_continuation(&binary, "continue-success").await;
        let connection = rusqlite::Connection::open(control.path().join("agent-state.db")).unwrap();
        // 故意构造通用事务不可能产生的损坏快照，验证 Provider 二次校验仍失败关闭。
        connection.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
        connection.execute(statement, []).unwrap();
        let error = provider
            .execute(
                ProviderExecutionContext {
                    execution_id: "c".into(),
                },
                Arc::new(ContinuationSink(control.path().into())),
                Arc::new(SlowTelemetry),
            )
            .await
            .unwrap_err();
        assert_eq!(
            error,
            ProviderExecutionFailure::State("CODEBUDDY_CONTINUATION_VALIDATION_FAILED".into())
        );
        assert!(request_methods(control.path()).is_empty());
        assert!(!control.path().join("accepted-child").exists());
        assert!(
            store
                .execution("c".into())
                .await
                .unwrap()
                .unwrap()
                .runtime_instance_id
                .is_none()
        );
    }
}

#[tokio::test]
/// continued prompt 的 Cancel、Permission 与 Activity 仍经过已有生产路径。
async fn native_continuation_reuses_cancel_permission_and_activity_paths() {
    use crate::agent::provider::ProviderCancelContext;
    let bin = tempfile::tempdir().unwrap();
    let binary = build(bin.path());

    let (control, _workspace, store, provider) =
        setup_continuation(&binary, "continue-cancel").await;
    let running = provider.clone();
    let child_control = control.path().to_owned();
    let task = tokio::spawn(async move {
        running
            .execute(
                ProviderExecutionContext {
                    execution_id: "c".into(),
                },
                Arc::new(ContinuationSink(child_control)),
                Arc::new(SlowTelemetry),
            )
            .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while !control.path().join("cancel-ready").exists() {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    provider
        .cancel(ProviderCancelContext {
            execution_id: "c".into(),
        })
        .await
        .unwrap();
    assert_eq!(
        task.await.unwrap().unwrap().outcome,
        ProviderOutcome::Cancelled
    );
    assert_eq!(
        store
            .execution("c".into())
            .await
            .unwrap()
            .unwrap()
            .release_evidence_state,
        "complete"
    );

    let (control, _workspace, store, provider) =
        setup_continuation(&binary, "continue-permission").await;
    let result = provider
        .execute(
            ProviderExecutionContext {
                execution_id: "c".into(),
            },
            Arc::new(ContinuationSink(control.path().into())),
            Arc::new(
                crate::agent::telemetry_projector::ExecutionTelemetryProjector::new(
                    store.clone(),
                    "c".into(),
                ),
            ),
        )
        .await
        .unwrap();
    assert_eq!(result.outcome, ProviderOutcome::Cancelled);
    assert!(control.path().join("permission-response.json").exists());
    assert!(
        store
            .execution_activity_history("c".into(), None, Some(100))
            .await
            .unwrap()
            .events
            .iter()
            .any(|event| event.summary_code.as_deref() == Some("provider.permission_denied"))
    );
}

#[tokio::test]
/// 真正 public execute 完成 isolated write/read-only；slow Activity 不改变结果。
async fn native_public_execute_write_and_readonly_atomic_release() {
    let bin = tempfile::tempdir().unwrap();
    let binary = build(bin.path());
    for mode in ["write", "read"] {
        let (control, workspace, store, provider) = setup(&binary, mode).await;
        let result = provider
            .execute(
                ProviderExecutionContext {
                    execution_id: "e".into(),
                },
                Arc::new(Sink(control.path().into())),
                Arc::new(SlowTelemetry),
            )
            .await
            .unwrap();
        assert_eq!(result.outcome, ProviderOutcome::Completed);
        assert_eq!(result.result, Some(json!({"text":"safe result"})));
        let row = store.execution("e".into()).await.unwrap().unwrap();
        assert_eq!(row.status, "completed");
        assert_eq!(row.provider_terminal_status.as_deref(), Some("completed"));
        assert_eq!(
            row.final_result_json,
            Some(serde_json::to_string(&result.result).unwrap())
        );
        assert_eq!(row.result_completeness, "complete");
        assert_eq!(row.release_evidence_state, "complete");
        assert_eq!(
            row.release_evidence_kind.as_deref(),
            Some("runtime_terminated")
        );
        let runtime = store
            .runtime(row.runtime_instance_id.clone().unwrap())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(runtime.state, "terminated");
        assert_eq!(runtime.termination_evidence_state, "complete");
        let release: Value =
            serde_json::from_str(row.release_evidence_json.as_ref().unwrap()).unwrap();
        assert_eq!(release["runtime_instance_id"], runtime.id);
        assert_eq!(
            release["evidence_at"],
            runtime.termination_evidence_at.unwrap()
        );
        assert!(
            store
                .workspace_claim(row.canonical_workspace_root)
                .await
                .unwrap()
                .is_none()
        );
        let db = rusqlite::Connection::open(control.path().join("agent-state.db")).unwrap();
        let trace: Vec<(String, String, i64, String, String)> = db
            .prepare("SELECT * FROM cb7_trace")
            .unwrap()
            .query_map([], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert!(
            trace
                .iter()
                .any(|r| r.0 == "finalizing" && r.2 == 1 && r.3 == "running" && r.4 != "complete")
        );
        let dispatching = trace.iter().position(|r| r.1 == "dispatching").unwrap();
        let dispatched = trace.iter().position(|r| r.1 == "dispatched").unwrap();
        let running = trace.iter().position(|r| r.0 == "running").unwrap();
        let terminal = trace.iter().position(|r| r.0 == "finalizing").unwrap();
        assert!(dispatching < dispatched && dispatched < running && running < terminal);
        let files: Vec<_> = std::fs::read_dir(workspace.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        if mode == "write" {
            assert_eq!(files, vec![std::ffi::OsString::from("output.txt")]);
            assert_eq!(
                std::fs::read(workspace.path().join("output.txt")).unwrap(),
                b"CB7_005_WRITE\n"
            );
        } else {
            assert!(files.is_empty());
        }
    }
}

#[tokio::test]
/// 无 Provider terminal 时安全终止只能 Interrupted；证据持久化失败必须 Unknown + Claim。
async fn native_execute_eof_and_evidence_persistence_failure() {
    let bin = tempfile::tempdir().unwrap();
    let binary = build(bin.path());
    for failure in [false, true] {
        let (control, _workspace, store, provider) =
            setup(&binary, if failure { "read" } else { "eof" }).await;
        if failure {
            rusqlite::Connection::open(control.path().join("agent-state.db")).unwrap().execute_batch("CREATE TRIGGER fail_evidence BEFORE UPDATE OF termination_evidence_state ON runtime_instances WHEN NEW.termination_evidence_state='complete' BEGIN SELECT RAISE(ABORT,'evidence fault'); END;").unwrap();
        }
        assert!(
            provider
                .execute(
                    ProviderExecutionContext {
                        execution_id: "e".into()
                    },
                    Arc::new(Sink(control.path().into())),
                    Arc::new(SlowTelemetry)
                )
                .await
                .is_err()
        );
        assert_eq!(provider.admission_diagnostic(), None);
        let row = store.execution("e".into()).await.unwrap().unwrap();
        assert_eq!(row.status, if failure { "unknown" } else { "interrupted" });
        assert_eq!(
            store
                .workspace_claim(row.canonical_workspace_root)
                .await
                .unwrap()
                .is_some(),
            failure
        );
        if failure {
            assert_eq!(row.provider_terminal_status.as_deref(), Some("completed"));
            assert_eq!(
                row.final_result_json.as_deref(),
                Some("{\"text\":\"safe result\"}")
            );
            rusqlite::Connection::open(control.path().join("agent-state.db"))
                .unwrap()
                .execute_batch("DROP TRIGGER fail_evidence")
                .unwrap();
            provider
                .startup_reconcile(ProviderStartupContext {})
                .await
                .unwrap();
            let resumed = store.execution("e".into()).await.unwrap().unwrap();
            assert_eq!(resumed.status, "completed");
            assert_eq!(resumed.final_result_json, row.final_result_json);
        } else {
            assert!(row.provider_terminal_status.is_none());
            assert_eq!(row.result_completeness, "unknown");
        }
    }
}

#[tokio::test]
/// initialize mismatch 只污染同一 adapter 的未来 admission；refresh 替换 adapter 后清除且不启动 ACP。
async fn native_initialize_incompatibility_wires_registry_catalog_and_refresh() {
    use crate::{
        agent::{
            product::AgentProductService,
            provider::{
                ProviderErrorCode, ProviderId,
                registry::{ProviderHealth, ProviderRegistry},
            },
        },
        config::{AgentProviderPolicy, AppPaths, ManagerConfig},
        serena::SupervisorState,
    };

    let bin = tempfile::tempdir().unwrap();
    let binary = build(bin.path());
    let (control, _workspace, store, provider) = setup(&binary, "initialize-incompatible").await;
    let provider = Arc::new(provider);
    let id = ProviderId::new("codebuddy".into()).unwrap();
    let mut registry = ProviderRegistry::new();
    registry
        .register(provider.clone(), ProviderHealth::Available)
        .unwrap();

    let error = provider
        .execute(
            ProviderExecutionContext {
                execution_id: "e".into(),
            },
            Arc::new(Sink(control.path().into())),
            Arc::new(SlowTelemetry),
        )
        .await
        .unwrap_err();
    assert_eq!(
        error,
        ProviderExecutionFailure::State("CODEBUDDY_ACP_INCOMPATIBLE".into())
    );
    assert_eq!(
        registry.diagnostic_code(&id).unwrap().as_deref(),
        Some("CODEBUDDY_ACP_INCOMPATIBLE")
    );
    assert_eq!(registry.health(&id).unwrap(), ProviderHealth::Unavailable);
    match registry.get(&id) {
        Err(error) => assert_eq!(error.code, ProviderErrorCode::AgentProviderUnavailable),
        Ok(_) => panic!("incompatible adapter admitted a new execution"),
    }
    let erased: Arc<dyn AgentProvider> = provider.clone();
    assert!(Arc::ptr_eq(&erased, &registry.get_registered(&id).unwrap()));

    let mut service = AgentProductService::new(store);
    service.use_registry_for_test(registry);
    let paths = AppPaths {
        runtime_directory: control.path().join("runtime"),
        config_file: control.path().join("config.json"),
        log_directory: control.path().join("logs"),
        app_log: control.path().join("logs/app.log"),
        serena_log: control.path().join("logs/serena.log"),
    };
    let mut config = ManagerConfig {
        agent_enabled: true,
        ..Default::default()
    };
    config
        .agent_providers
        .providers
        .insert("codebuddy".into(), AgentProviderPolicy { enabled: true });
    crate::config::save(&paths.config_file, &config).unwrap();
    let supervisor = SupervisorState::new(paths).unwrap();
    let catalog = service.provider_catalog(&supervisor).unwrap();
    let entry = catalog
        .providers
        .iter()
        .find(|entry| entry.id == id)
        .unwrap();
    assert_eq!(entry.health, ProviderHealth::Unavailable);
    assert!(!entry.available_for_new_execution);
    assert_eq!(
        entry.diagnostic_code.as_deref(),
        Some("CODEBUDDY_ACP_INCOMPATIBLE")
    );

    let requests_before = std::fs::read(control.path().join("requests.jsonl")).unwrap();
    crate::agent::codebuddy::TEST_DISCOVERY
        .scope(
            Ok(DiscoveryResult::direct_for_test(
                control.path().join("peer.exe"),
            )),
            service.refresh_provider_health(id.clone()),
        )
        .await
        .unwrap();
    assert_eq!(
        std::fs::read(control.path().join("requests.jsonl")).unwrap(),
        requests_before
    );
    let refreshed = service.provider_catalog(&supervisor).unwrap();
    let entry = refreshed
        .providers
        .iter()
        .find(|entry| entry.id == id)
        .unwrap();
    assert_eq!(entry.health, ProviderHealth::Available);
    assert_eq!(entry.diagnostic_code, None);
    assert!(entry.available_for_new_execution);
}

#[tokio::test]
/// initialize 的 EOF、timeout 与 malformed 都是单次 execution 失败，不污染未来 admission。
async fn native_initialize_transient_failures_do_not_pollute_registry() {
    use crate::agent::provider::{
        ProviderId,
        registry::{ProviderHealth, ProviderRegistry},
    };

    let bin = tempfile::tempdir().unwrap();
    let binary = build(bin.path());
    for (mode, code) in [
        ("initialize-eof", "CODEBUDDY_ACP_EOF"),
        ("initialize-timeout", "CODEBUDDY_ACP_TIMEOUT"),
        ("initialize-malformed", "CODEBUDDY_ACP_MALFORMED"),
    ] {
        let (control, _workspace, _store, provider) = setup(&binary, mode).await;
        let provider = Arc::new(provider);
        let id = ProviderId::new("codebuddy".into()).unwrap();
        let mut registry = ProviderRegistry::new();
        registry
            .register(provider.clone(), ProviderHealth::Available)
            .unwrap();

        let error = tokio::time::timeout(
            std::time::Duration::from_secs(25),
            provider.execute(
                ProviderExecutionContext {
                    execution_id: "e".into(),
                },
                Arc::new(Sink(control.path().into())),
                Arc::new(SlowTelemetry),
            ),
        )
        .await
        .unwrap()
        .unwrap_err();
        assert_eq!(
            error,
            ProviderExecutionFailure::State(code.into()),
            "{mode}"
        );
        assert_eq!(
            registry.health(&id).unwrap(),
            ProviderHealth::Available,
            "{mode}"
        );
        assert_eq!(registry.diagnostic_code(&id).unwrap(), None, "{mode}");
        let resolved = registry.get(&id).unwrap();
        let registered = registry.get_registered(&id).unwrap();
        assert!(Arc::ptr_eq(&resolved, &registered), "{mode}");
    }
}

#[tokio::test]
/// host crash window：已暂存 exact terminal 时 Job 尚活，startup 终止后保留安全结果。
async fn native_staged_live_job_startup_preserves_terminal_and_result() {
    let bin = tempfile::tempdir().unwrap();
    let binary = build(bin.path());
    let (control, _workspace, store, provider) = setup(&binary, "read").await;
    let session = provider
        .prepare_fresh("e".into(), DesiredConfiguration::default())
        .await
        .unwrap();
    let completion = crate::agent::codebuddy::prompt::prompt(
        session,
        store.clone(),
        Arc::new(Sink(control.path().into())),
        Arc::new(SlowTelemetry),
    )
    .await
    .unwrap();
    let result = completion.result.unwrap();
    // exact natural terminal 已到达后才提交用户意图：不得追加 cancel wire 或改成 Cancelled。
    provider
        .cancel(crate::agent::provider::ProviderCancelContext {
            execution_id: "e".into(),
        })
        .await
        .unwrap();
    assert!(!control.path().join("cancel.json").exists());
    let runtime = completion
        .session
        .private
        .runtime_instance_id
        .clone()
        .unwrap();
    store
        .provider_event(
            "e".into(),
            Transition::ProviderTerminalResult {
                runtime_id: runtime.clone(),
                status: Status::Completed,
                result: result.result.clone(),
                completeness: ResultCompleteness::Complete,
            },
            now(),
        )
        .await
        .unwrap();
    let staged = store.execution("e".into()).await.unwrap().unwrap();
    assert_eq!(staged.status, "finalizing");
    assert_eq!(staged.release_evidence_state, "incomplete");
    assert!(
        store
            .workspace_claim(staged.canonical_workspace_root.clone())
            .await
            .unwrap()
            .is_some()
    );
    let live = store.runtime(runtime.clone()).await.unwrap().unwrap();
    assert_eq!(live.state, "running");
    assert_ne!(live.termination_evidence_state, "complete");
    assert!(active_job_processes(live.job_name.as_deref().unwrap()) > 0);
    // CLI 缺失的 registered skeleton 仍恢复当前 owned Runtime；不重新 launch。
    let missing = CodeBuddyProvider::from_discovery(
        store.clone(),
        "new-host".into(),
        Err(crate::agent::codebuddy::discovery::DiscoveryError::not_found(false)),
    );
    missing
        .startup_reconcile(ProviderStartupContext {})
        .await
        .unwrap();
    let done = store.execution("e".into()).await.unwrap().unwrap();
    assert_eq!(done.status, "completed");
    assert_eq!(done.final_result_json, staged.final_result_json);
    assert_eq!(
        done.provider_terminal_status,
        staged.provider_terminal_status
    );
    assert!(
        store
            .workspace_claim(done.canonical_workspace_root)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        store
            .runtime(runtime)
            .await
            .unwrap()
            .unwrap()
            .termination_evidence_state,
        "complete"
    );
    drop(completion.session);
}

#[tokio::test]
/// worker caller drop 不丢失 Runtime owner，也不能伪造 Provider terminal。
async fn native_caller_drop_after_flush_converges_without_replay() {
    let bin = tempfile::tempdir().unwrap();
    let binary = build(bin.path());
    let (control, _workspace, store, provider) = setup(&binary, "hold").await;
    let sink = Arc::new(Sink(control.path().into()));
    let task = tokio::spawn(async move {
        provider
            .execute(
                ProviderExecutionContext {
                    execution_id: "e".into(),
                },
                sink,
                Arc::new(SlowTelemetry),
            )
            .await
    });
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(15);
    while !control.path().join("prompt.json").exists() {
        assert!(tokio::time::Instant::now() < deadline);
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    task.abort();
    let _ = task.await;
    loop {
        let row = store.execution("e".into()).await.unwrap().unwrap();
        if row.status == "interrupted" {
            assert!(row.provider_terminal_status.is_none());
            assert_eq!(row.result_completeness, "unknown");
            assert!(
                store
                    .workspace_claim(row.canonical_workspace_root)
                    .await
                    .unwrap()
                    .is_none()
            );
            break;
        }
        assert!(tokio::time::Instant::now() < deadline, "{}", row.status);
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert_eq!(
        store
            .read_codebuddy_state("e".into())
            .await
            .unwrap()
            .prompt_state,
        crate::agent::codebuddy::store::PromptState::Uncertain
    );
}

/// 真实 QueryInformationJobObject 仅用于 native crash-window 断言，不制造持久化证据。
fn active_job_processes(name: &str) -> u32 {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::System::{JobObjects::*, SystemServices::JOB_OBJECT_QUERY};
    let name: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
    // SAFETY: NUL 结尾名称、owned handle 与匹配的 Win32 输出缓冲区。
    unsafe {
        let handle = OpenJobObjectW(JOB_OBJECT_QUERY, 0, name.as_ptr());
        assert!(!handle.is_null());
        let handle = OwnedHandle::from_raw_handle(handle);
        let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = std::mem::zeroed();
        assert_ne!(
            QueryInformationJobObject(
                handle.as_raw_handle(),
                JobObjectBasicAccountingInformation,
                (&mut info as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                std::mem::size_of_val(&info) as u32,
                std::ptr::null_mut()
            ),
            0
        );
        info.ActiveProcesses
    }
}

#[tokio::test]
/// 真实 pipe 在 acceptance 前关闭输入，但 stdout 保持；物理 flush 失败收敛 Uncertain。
async fn native_post_accept_preflush_failure_is_uncertain_then_interrupted() {
    let bin = tempfile::tempdir().unwrap();
    let binary = build(bin.path());
    let (control, _workspace, store, provider) = setup(&binary, "closed-input").await;
    assert!(
        provider
            .execute(
                ProviderExecutionContext {
                    execution_id: "e".into()
                },
                Arc::new(Sink(control.path().into())),
                Arc::new(SlowTelemetry)
            )
            .await
            .is_err()
    );
    assert!(control.path().join("accepted").exists());
    assert!(!control.path().join("prompt.json").exists());
    let row = store.execution("e".into()).await.unwrap().unwrap();
    assert_eq!(row.status, "interrupted");
    assert_eq!(row.dispatch_state, "uncertain");
    assert!(row.provider_terminal_status.is_none());
    assert_eq!(
        store
            .read_codebuddy_state("e".into())
            .await
            .unwrap()
            .prompt_state,
        crate::agent::codebuddy::store::PromptState::Uncertain
    );
    assert!(
        store
            .workspace_claim(row.canonical_workspace_root)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
/// 两个 bypass execute 竞争同一 Execution，输家不得收敛或停止赢家的 R1。
async fn native_competing_execute_only_cleans_its_own_attempt() {
    let bin = tempfile::tempdir().unwrap();
    let binary = build(bin.path());
    let (control, _workspace, store, provider) = setup(&binary, "read").await;
    let sink = Arc::new(Sink(control.path().into()));
    let context = ProviderExecutionContext {
        execution_id: "e".into(),
    };
    let (a, b) = tokio::join!(
        provider.execute(context.clone(), sink.clone(), Arc::new(SlowTelemetry)),
        provider.execute(context, sink, Arc::new(SlowTelemetry))
    );
    assert_ne!(a.is_ok(), b.is_ok(), "{a:?} {b:?}");
    let row = store.execution("e".into()).await.unwrap().unwrap();
    assert_eq!(row.status, "completed");
    assert!(
        store
            .workspace_claim(row.canonical_workspace_root)
            .await
            .unwrap()
            .is_none()
    );
    let count: i64 = rusqlite::Connection::open(control.path().join("agent-state.db"))
        .unwrap()
        .query_row("SELECT count(*) FROM runtime_instances", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
/// registered adapter 持有当前生命周期；accepted 后 disable 只阻止后续 admission。
async fn native_disable_after_acceptance_preserves_owned_execute() {
    use crate::agent::provider::{
        ProviderErrorCode, ProviderId,
        control::{ProviderAdmissionCapability, ProviderAdmissionPolicy},
        registry::{ProviderHealth, ProviderRegistry},
    };
    struct DisableSink {
        sink: Sink,
        policy: ProviderAdmissionPolicy,
    }
    impl ProviderAcceptanceSink for DisableSink {
        /// 回调时改变实际 shared policy，当前 execution 不得重读它进行取消。
        fn accepted(&self) {
            self.sink.accepted();
            self.policy.set_enabled_for_test("codebuddy", false);
        }
    }
    let bin = tempfile::tempdir().unwrap();
    let binary = build(bin.path());
    let (control, _workspace, store, provider) = setup(&binary, "read").await;
    let mut registry = ProviderRegistry::new();
    registry
        .register(Arc::new(provider), ProviderHealth::Available)
        .unwrap();
    let policy = ProviderAdmissionPolicy::new(Default::default());
    policy.set_enabled_for_test("codebuddy", true);
    let id = ProviderId::new("codebuddy".into()).unwrap();
    let provider = policy
        .admit(&registry, &id, ProviderAdmissionCapability::Execute)
        .unwrap();
    provider
        .execute(
            ProviderExecutionContext {
                execution_id: "e".into(),
            },
            Arc::new(DisableSink {
                sink: Sink(control.path().into()),
                policy: policy.clone(),
            }),
            Arc::new(SlowTelemetry),
        )
        .await
        .unwrap();
    assert_eq!(
        store.execution("e".into()).await.unwrap().unwrap().status,
        "completed"
    );
    match policy.admit(&registry, &id, ProviderAdmissionCapability::Execute) {
        Err(error) => assert_eq!(error.code, ProviderErrorCode::AgentProviderDisabled),
        Ok(_) => panic!("disabled provider admitted new execution"),
    }
}

/// CB8 native：同一 owner 收到重复 durable intent 只发一次，真实写入不回滚。
#[tokio::test]
async fn native_cancel_exact_terminal_and_workspace_matrix() {
    use crate::agent::provider::ProviderCancelContext;
    use crate::agent::provider::{
        ProviderId,
        control::{ProviderAdmissionCapability, ProviderAdmissionPolicy},
        registry::ProviderRegistry,
    };
    let bin = tempfile::tempdir().unwrap();
    let binary = build(bin.path());
    for mode in ["cancel-read", "cancel-write", "cancel-end-turn"] {
        let (control, workspace, store, provider) = setup(&binary, mode).await;
        let provider = Arc::new(provider);
        let running = provider.clone();
        let sink = Arc::new(Sink(control.path().into()));
        let task = tokio::spawn(async move {
            running
                .execute(
                    ProviderExecutionContext {
                        execution_id: "e".into(),
                    },
                    sink,
                    Arc::new(SlowTelemetry),
                )
                .await
        });
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            while !control.path().join("cancel-ready").exists() {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        // acceptance 后 disable 且 discovery unavailable：历史 control 仍通过 registered adapter。
        let mut registry = ProviderRegistry::new();
        crate::agent::codebuddy::provider::register_codebuddy_provider_with_discovery(
            &mut registry,
            store.clone(),
            "host".into(),
            Err(crate::agent::codebuddy::discovery::DiscoveryError::not_found(false)),
        )
        .unwrap();
        let id = ProviderId::new("codebuddy".into()).unwrap();
        let policy = ProviderAdmissionPolicy::new(Default::default());
        policy.set_enabled_for_test("codebuddy", false);
        assert!(
            policy
                .admit(&registry, &id, ProviderAdmissionCapability::Execute)
                .is_err()
        );
        let control_provider = registry.get_registered(&id).unwrap();
        control_provider
            .cancel(ProviderCancelContext {
                execution_id: "e".into(),
            })
            .await
            .unwrap();
        control_provider
            .cancel(ProviderCancelContext {
                execution_id: "e".into(),
            })
            .await
            .unwrap();
        let result = tokio::time::timeout(std::time::Duration::from_secs(10), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(
            result.outcome,
            if mode == "cancel-end-turn" {
                ProviderOutcome::Completed
            } else {
                ProviderOutcome::Cancelled
            }
        );
        let row = store.execution("e".into()).await.unwrap().unwrap();
        assert_eq!(
            row.status,
            if mode == "cancel-end-turn" {
                "completed"
            } else {
                "cancelled"
            }
        );
        assert_eq!(
            row.release_evidence_kind.as_deref(),
            Some("runtime_terminated")
        );
        assert!(row.interrupt_requested_at.is_some());
        assert!(control.path().join("cancel.json").exists());
        assert!(
            store
                .workspace_claim(row.canonical_workspace_root)
                .await
                .unwrap()
                .is_none()
        );
        let private = crate::agent::codebuddy::store::CodeBuddyStore(store.clone())
            .read("e".into())
            .await
            .unwrap();
        assert_eq!(
            private.prompt_state,
            crate::agent::codebuddy::store::PromptState::TerminalObserved
        );
        if mode == "cancel-write" {
            assert_eq!(
                std::fs::read(workspace.path().join("marker.txt")).unwrap(),
                b"CB8_WRITE\n"
            );
            assert_eq!(std::fs::read_dir(workspace.path()).unwrap().count(), 1);
        } else {
            assert_eq!(std::fs::read_dir(workspace.path()).unwrap().count(), 0);
        }
        // 已冻结 exact terminal 后的重复 cancel 不产生新的通知或更改结果。
        provider
            .cancel(ProviderCancelContext {
                execution_id: "e".into(),
            })
            .await
            .unwrap();
        assert_eq!(
            store.execution("e".into()).await.unwrap().unwrap().status,
            row.status
        );
    }
}

/// CB8 native：未准备没有 child；准备后的 cancel 不发送 Prompt，也不伪造 Cancelled。
#[tokio::test]
async fn native_cancel_before_prompt_retains_evidence_authority() {
    use crate::agent::provider::ProviderCancelContext;
    let bin = tempfile::tempdir().unwrap();
    let binary = build(bin.path());
    for prepared in [false, true] {
        for evidence_fault in [false, true] {
            let (control, workspace, store, provider) = setup(&binary, "read").await;
            let session = if prepared {
                Some(
                    provider
                        .prepare_fresh("e".into(), DesiredConfiguration::default())
                        .await
                        .unwrap(),
                )
            } else {
                None
            };
            provider
                .cancel(ProviderCancelContext {
                    execution_id: "e".into(),
                })
                .await
                .unwrap();
            if let Some(session) = session {
                let before = store.execution("e".into()).await.unwrap().unwrap();
                assert_eq!(before.status, "dispatch_pending");
                assert!(before.interrupt_requested_at.is_some());
                assert!(
                    store
                        .workspace_claim(before.canonical_workspace_root)
                        .await
                        .unwrap()
                        .is_some()
                );
                if evidence_fault {
                    rusqlite::Connection::open(control.path().join("agent-state.db")).unwrap().execute_batch("CREATE TRIGGER fail_evidence BEFORE UPDATE OF termination_evidence_state ON runtime_instances WHEN NEW.termination_evidence_state='complete' BEGIN SELECT RAISE(ABORT,'evidence fault'); END;").unwrap();
                }
                let (_keep, cancelled) = oneshot::channel();
                let completion = crate::agent::codebuddy::prompt::run(
                    session,
                    store.clone(),
                    Arc::new(Sink(control.path().into())),
                    Arc::new(SlowTelemetry),
                    cancelled,
                )
                .await;
                assert!(completion.result.is_err());
                let _ = completion.session.shutdown().await;
                let result =
                    crate::agent::codebuddy::recovery::reconcile_execution(&store, "e", false)
                        .await;
                if result.is_err() {
                    crate::agent::codebuddy::recovery::mark_unknown(&store, "e")
                        .await
                        .unwrap();
                }
            } else {
                assert!(
                    provider
                        .execute(
                            ProviderExecutionContext {
                                execution_id: "e".into()
                            },
                            Arc::new(Sink(control.path().into())),
                            Arc::new(SlowTelemetry)
                        )
                        .await
                        .is_err()
                );
            }
            let row = store.execution("e".into()).await.unwrap().unwrap();
            assert_eq!(
                row.status,
                if !prepared {
                    "cancelled"
                } else if evidence_fault {
                    "unknown"
                } else {
                    "interrupted"
                }
            );
            assert_eq!(
                store
                    .workspace_claim(row.canonical_workspace_root)
                    .await
                    .unwrap()
                    .is_some(),
                prepared && evidence_fault
            );
            assert!(!control.path().join("prompt.json").exists());
            assert!(!control.path().join("cancel.json").exists());
            assert_eq!(std::fs::read_dir(workspace.path()).unwrap().count(), 0);
        }
    }
}

/// CB8 native：绝对 cancel deadline 与 pipe 失败都只能收敛 Interrupted；cancel 不释放 Claim。
#[tokio::test]
async fn native_cancel_timeout_pipe_failure_and_staged_claim() {
    use crate::agent::codebuddy::{fresh, protocol::Limits};
    use crate::agent::provider::ProviderCancelContext;
    let bin = tempfile::tempdir().unwrap();
    let binary = build(bin.path());
    for mode in ["cancel-timeout", "cancel-pipe", "cancel-read"] {
        let (control, _workspace, store, provider) = setup(&binary, mode).await;
        let session = fresh::prepare(
            store.clone(),
            "host".into(),
            "e".into(),
            provider.resolved_launch_spec().unwrap(),
            DesiredConfiguration::default(),
            Limits {
                request_timeout: std::time::Duration::from_secs(2),
                prompt_timeout: std::time::Duration::from_secs(60 * 60),
                ..Limits::default()
            },
        )
        .await
        .unwrap();
        let (keep, cancelled) = oneshot::channel();
        let state = store.clone();
        let sink = Arc::new(Sink(control.path().into()));
        let task = tokio::spawn(async move {
            crate::agent::codebuddy::prompt::run(
                session,
                state,
                sink,
                Arc::new(SlowTelemetry),
                cancelled,
            )
            .await
        });
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while !control.path().join("cancel-ready").exists() {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        provider
            .cancel(ProviderCancelContext {
                execution_id: "e".into(),
            })
            .await
            .unwrap();
        let completion = tokio::time::timeout(std::time::Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap();
        drop(keep);
        let row = store.execution("e".into()).await.unwrap().unwrap();
        assert!(
            store
                .workspace_claim(row.canonical_workspace_root.clone())
                .await
                .unwrap()
                .is_some()
        );
        if mode == "cancel-read" {
            assert_eq!(
                completion.result.as_ref().unwrap().outcome,
                ProviderOutcome::Cancelled
            );
            store
                .provider_event(
                    "e".into(),
                    Transition::ProviderTerminalResult {
                        runtime_id: row.runtime_instance_id.clone().unwrap(),
                        status: Status::Cancelled,
                        result: completion.result.as_ref().unwrap().result.clone(),
                        completeness: ResultCompleteness::Complete,
                    },
                    now(),
                )
                .await
                .unwrap();
            let staged = store.execution("e".into()).await.unwrap().unwrap();
            assert_eq!(staged.status, "finalizing");
            assert!(
                store
                    .workspace_claim(staged.canonical_workspace_root)
                    .await
                    .unwrap()
                    .is_some()
            );
            let runtime = store
                .runtime(row.runtime_instance_id.clone().unwrap())
                .await
                .unwrap()
                .unwrap();
            assert!(active_job_processes(runtime.job_name.as_deref().unwrap()) > 0);
        } else {
            assert!(completion.result.is_err());
            assert!(row.provider_terminal_status.is_none());
            assert!(row.interrupt_timeout_at.is_some());
            assert_eq!(
                row.interrupt_diagnostic.as_deref(),
                Some(if mode == "cancel-timeout" {
                    "CODEBUDDY_CANCEL_TIMEOUT"
                } else {
                    "CODEBUDDY_CANCEL_SEND_FAILED"
                })
            );
            assert_eq!(
                control.path().join("cancel.json").exists(),
                mode == "cancel-timeout"
            );
        }
        completion.session.shutdown().await.unwrap();
        crate::agent::codebuddy::recovery::reconcile_execution(&store, "e", false)
            .await
            .unwrap();
        let done = store.execution("e".into()).await.unwrap().unwrap();
        assert_eq!(
            done.status,
            if mode == "cancel-read" {
                "cancelled"
            } else {
                "interrupted"
            }
        );
        assert!(
            store
                .workspace_claim(done.canonical_workspace_root)
                .await
                .unwrap()
                .is_none()
        );
    }
}

/// CB8：exact Cancelled 已暂存但 Job evidence 失败时 Unknown + Claim；startup 后才释放。
#[tokio::test]
async fn native_cancel_terminal_evidence_failure_and_startup() {
    use crate::agent::provider::ProviderCancelContext;
    let bin = tempfile::tempdir().unwrap();
    let binary = build(bin.path());
    let (control, _workspace, store, provider) = setup(&binary, "cancel-read").await;
    let db = rusqlite::Connection::open(control.path().join("agent-state.db")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_evidence BEFORE UPDATE OF termination_evidence_state ON runtime_instances WHEN NEW.termination_evidence_state='complete' BEGIN SELECT RAISE(ABORT,'evidence fault'); END;").unwrap();
    let provider = Arc::new(provider);
    let running = provider.clone();
    let sink = Arc::new(Sink(control.path().into()));
    let task = tokio::spawn(async move {
        running
            .execute(
                ProviderExecutionContext {
                    execution_id: "e".into(),
                },
                sink,
                Arc::new(SlowTelemetry),
            )
            .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while !control.path().join("cancel-ready").exists() {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    provider
        .cancel(ProviderCancelContext {
            execution_id: "e".into(),
        })
        .await
        .unwrap();
    assert!(task.await.unwrap().is_err());
    let row = store.execution("e".into()).await.unwrap().unwrap();
    assert_eq!(row.status, "unknown");
    assert_eq!(row.provider_terminal_status.as_deref(), Some("cancelled"));
    assert!(
        store
            .workspace_claim(row.canonical_workspace_root.clone())
            .await
            .unwrap()
            .is_some()
    );
    db.execute_batch("DROP TRIGGER fail_evidence").unwrap();
    provider
        .startup_reconcile(ProviderStartupContext {})
        .await
        .unwrap();
    let done = store.execution("e".into()).await.unwrap().unwrap();
    assert_eq!(done.status, "cancelled");
    assert_eq!(done.final_result_json, row.final_result_json);
    assert!(
        store
            .workspace_claim(done.canonical_workspace_root)
            .await
            .unwrap()
            .is_none()
    );
}

/// CB8：acceptance 后立即持久化 cancel，Prompt 必须先 flush；flush 失败时没有 cancel wire。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn native_cancel_accepted_before_prompt_flush() {
    use crate::agent::provider::ProviderCancelContext;
    struct PausedAcceptance {
        sink: Sink,
        ready: Arc<tokio::sync::Notify>,
        release: std::sync::Mutex<std::sync::mpsc::Receiver<()>>,
    }
    impl ProviderAcceptanceSink for PausedAcceptance {
        /// 仅测试同步边界，用握手而非 sleep 排定 durable cancel 在物理 Prompt 之前。
        fn accepted(&self) {
            self.sink.accepted();
            self.ready.notify_one();
            self.release
                .lock()
                .unwrap()
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap();
        }
    }
    let bin = tempfile::tempdir().unwrap();
    let binary = build(bin.path());
    for mode in ["cancel-read", "closed-input"] {
        let (control, _workspace, store, provider) = setup(&binary, mode).await;
        let provider = Arc::new(provider);
        let ready = Arc::new(tokio::sync::Notify::new());
        let (release, receiver) = std::sync::mpsc::channel();
        let sink = Arc::new(PausedAcceptance {
            sink: Sink(control.path().into()),
            ready: ready.clone(),
            release: std::sync::Mutex::new(receiver),
        });
        let executing = provider.clone();
        let task = tokio::spawn(async move {
            executing
                .execute(
                    ProviderExecutionContext {
                        execution_id: "e".into(),
                    },
                    sink,
                    Arc::new(SlowTelemetry),
                )
                .await
        });
        tokio::time::timeout(std::time::Duration::from_secs(5), ready.notified())
            .await
            .unwrap();
        provider
            .cancel(ProviderCancelContext {
                execution_id: "e".into(),
            })
            .await
            .unwrap();
        assert!(!control.path().join("prompt.json").exists());
        assert!(!control.path().join("cancel.json").exists());
        let row = store.execution("e".into()).await.unwrap().unwrap();
        assert!(row.interrupt_requested_at.is_some());
        assert!(
            store
                .workspace_claim(row.canonical_workspace_root)
                .await
                .unwrap()
                .is_some()
        );
        release.send(()).unwrap();
        let result = tokio::time::timeout(std::time::Duration::from_secs(10), task)
            .await
            .unwrap()
            .unwrap();
        let row = store.execution("e".into()).await.unwrap().unwrap();
        if mode == "cancel-read" {
            assert_eq!(result.unwrap().outcome, ProviderOutcome::Cancelled);
            assert!(row.interrupt_ack_at.is_some());
            assert!(control.path().join("cancel.json").exists());
        } else {
            assert!(result.is_err());
            assert_eq!(row.status, "interrupted");
            assert!(row.provider_terminal_status.is_none());
            assert!(!control.path().join("cancel.json").exists());
        }
    }
}

/// CB8 native crash-window：intent / 已发送但无 terminal 的恢复均只能 Interrupted。
#[tokio::test]
async fn native_cancel_startup_after_intent_or_send() {
    use crate::agent::provider::ProviderCancelContext;
    let bin = tempfile::tempdir().unwrap();
    let binary = build(bin.path());
    for sent in [false, true] {
        let (control, _workspace, store, provider) = setup(&binary, "cancel-timeout").await;
        let session = provider
            .prepare_fresh("e".into(), DesiredConfiguration::default())
            .await
            .unwrap();
        let (keep, cancelled) = oneshot::channel();
        let mut prepared = Some(session);
        let task = if sent {
            let session = prepared.take().unwrap();
            let state = store.clone();
            let sink = Arc::new(Sink(control.path().into()));
            Some(tokio::spawn(async move {
                crate::agent::codebuddy::prompt::run(
                    session,
                    state,
                    sink,
                    Arc::new(SlowTelemetry),
                    cancelled,
                )
                .await
            }))
        } else {
            None
        };
        if sent {
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                while !control.path().join("cancel-ready").exists() {
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                }
            })
            .await
            .unwrap();
        }
        provider
            .cancel(ProviderCancelContext {
                execution_id: "e".into(),
            })
            .await
            .unwrap();
        if sent {
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                while store
                    .execution("e".into())
                    .await
                    .unwrap()
                    .unwrap()
                    .interrupt_ack_at
                    .is_none()
                {
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                }
            })
            .await
            .unwrap();
            assert!(control.path().join("cancel.json").exists());
        }
        let missing = CodeBuddyProvider::from_discovery(
            store.clone(),
            "new-host".into(),
            Err(crate::agent::codebuddy::discovery::DiscoveryError::not_found(false)),
        );
        missing
            .startup_reconcile(ProviderStartupContext {})
            .await
            .unwrap();
        let row = store.execution("e".into()).await.unwrap().unwrap();
        assert_eq!(row.status, "interrupted");
        assert!(row.provider_terminal_status.is_none());
        assert!(
            store
                .workspace_claim(row.canonical_workspace_root)
                .await
                .unwrap()
                .is_none()
        );
        if let Some(task) = task {
            let completion = task.await.unwrap();
            assert!(completion.result.is_err());
            let _ = completion.session.shutdown().await;
        }
        if let Some(session) = prepared {
            let _ = session.shutdown().await;
        }
        drop(keep);
    }
}
