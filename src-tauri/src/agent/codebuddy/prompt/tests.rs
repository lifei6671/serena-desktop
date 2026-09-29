//! 真实 Store + Windows Job + SDK fake wire，覆盖内部 Prompt authority。
use super::*;

#[path = "activity_tests.rs"]
mod activity_tests;

struct NoopTelemetry;
impl AgentEventSink for NoopTelemetry {}
use crate::agent::codebuddy::{
    fresh::{
        DesiredConfiguration, prepare,
        tests::{cleanup_evidence, fixture, wait_wire, wire},
    },
    protocol::{Limits, Shared},
};
use std::{
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

/// 同步 acceptance 读取已经提交的 SQLite 快照，并为 peer 留物理顺序标记。
struct Sink {
    directory: PathBuf,
    calls: AtomicUsize,
}
impl ProviderAcceptanceSink for Sink {
    /// 回调必须先于物理 Prompt；durable send-intent 必须已经可被独立连接读取。
    fn accepted(&self) {
        assert_eq!(self.calls.fetch_add(1, Ordering::SeqCst), 0);
        let database = std::fs::read_dir(&self.directory)
            .unwrap()
            .find_map(|entry| {
                let path = entry.unwrap().path();
                (path.extension().and_then(|v| v.to_str()) == Some("db")).then_some(path)
            })
            .expect("store db");
        let db = rusqlite::Connection::open(database).unwrap();
        let state: String = db
            .query_row(
                "SELECT prompt_state FROM codebuddy_execution_state",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(state, "sent");
        assert!(
            !wire(&self.directory)
                .iter()
                .any(|v| v["method"] == "session/prompt")
        );
        std::fs::write(self.directory.join("accepted"), "").unwrap();
    }
}

/// 无额外依赖的 native peer，每组测试仅编译一次。
fn build(directory: &Path) -> PathBuf {
    let executable = directory.join("prompt-peer.exe");
    let output = std::process::Command::new("rustc")
        .args(["--edition=2024", "--crate-name", "codebuddy_prompt_peer"])
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/codebuddy_prompt_child.rs"))
        .arg("-o")
        .arg(&executable)
        .creation_flags(0x0800_0000)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    executable
}

/// 只通过真实 fresh prepare 创建 R1/private identity，响应默认精确回显。
async fn setup(
    base: &Path,
    limits: Limits,
) -> (
    tempfile::TempDir,
    StateStore,
    PreparedFreshSession,
    Arc<Sink>,
) {
    let (dir, store, id, resolved) = fixture(base, "prompt", false).await;
    let session = prepare(
        store.clone(),
        "host".into(),
        id,
        &resolved,
        DesiredConfiguration::default(),
        limits,
    )
    .await
    .unwrap();
    std::fs::write(dir.path().join("response.json"), json!({"stopReason":"end_turn","_meta":{CONVERSATION:session.private.conversation_request_id,PROVIDER_REQUEST:"independent-provider-request"}}).to_string()).unwrap();
    let sink = Arc::new(Sink {
        directory: dir.path().into(),
        calls: AtomicUsize::new(0),
    });
    (dir, store, session, sink)
}

/// wire helper 保留 update._meta 的 Host shape，不把 meta 移到 envelope。
fn chunk(session: &str, conversation: &str, kind: &str, text: &str) -> Value {
    json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":session,"update":{
        "sessionUpdate":kind,"content":{"type":"text","text":text},"_meta":{CONVERSATION:conversation}}}})
}

/// 输出原序 JSONL，不为无关帧补 correlation。
fn write_frames(dir: &Path, file: &str, frames: &[Value]) {
    std::fs::write(
        dir.join(file),
        frames.iter().map(|v| format!("{v}\n")).collect::<String>(),
    )
    .unwrap();
}

#[tokio::test]
/// 单一 frozen text、exact meta、send/accept/request 顺序和 terminal 后原 Runtime/Claim 均保留。
async fn exact_wire_order_terminal_result_and_late_freeze() {
    let bin = tempfile::tempdir().unwrap();
    let base = build(bin.path());
    let (dir, store, session, sink) = setup(&base, Limits::default()).await;
    let id = session.private.execution_id.clone();
    let runtime = session.private.runtime_instance_id.clone().unwrap();
    let conversation = session.private.conversation_request_id.clone();
    let revision = session.private.revision;
    write_frames(
        dir.path(),
        "updates.jsonl",
        &[
            chunk(
                "exact-session",
                &conversation,
                "agent_message_chunk",
                "hello ",
            ),
            chunk(
                "foreign-session",
                &conversation,
                "agent_message_chunk",
                "wrong session",
            ),
            chunk(
                "exact-session",
                "foreign-conversation",
                "agent_message_chunk",
                "wrong conversation",
            ),
            chunk(
                "exact-session",
                &conversation,
                "agent_thought_chunk",
                "thought",
            ),
            chunk("exact-session", &conversation, "tool_call", "rawOutput"),
            chunk("exact-session", &conversation, "usage_update", "usage"),
            chunk(
                "exact-session",
                &conversation,
                "agent_message_chunk",
                "world",
            ),
            json!({"jsonrpc":"2.0","id":"unknown-response-id","result":{"stopReason":"cancelled"}}),
        ],
    );
    write_frames(
        dir.path(),
        "late.jsonl",
        &[chunk(
            "exact-session",
            &conversation,
            "agent_message_chunk",
            "late",
        )],
    );
    std::fs::write(dir.path().join("behavior"), "duplicate").unwrap();
    let completed = prompt(
        session,
        store.clone(),
        sink.clone(),
        Arc::new(NoopTelemetry),
    )
    .await
    .unwrap();
    let result = completed.result.as_ref().unwrap();
    assert_eq!(
        serde_json::to_value(result).unwrap(),
        json!({"executionId":id,"outcome":"completed","result":{"text":"hello world"},"resultCompleteness":"complete","diagnosticCode":null})
    );
    assert_eq!(
        completed.session.private.prompt_state,
        PromptState::TerminalObserved
    );
    assert_eq!(
        completed.session.private.terminal_stop_reason,
        Some(StopReason::EndTurn)
    );
    assert_eq!(
        completed.session.private.provider_request_id.as_deref(),
        Some("independent-provider-request")
    );
    // MarkSent 一次，provider request identity 与 terminal 合并为一次原子提交。
    assert_eq!(completed.session.private.revision, revision + 2);
    assert_eq!(
        completed.session.runtime.runtime_id(),
        Some(runtime.as_str())
    );
    assert_eq!(
        store
            .runtime(runtime)
            .await
            .unwrap()
            .unwrap()
            .termination_evidence_state,
        "unknown"
    );
    let rows = wire(dir.path());
    let prompts: Vec<_> = rows
        .iter()
        .filter(|v| v["method"] == "session/prompt")
        .collect();
    assert_eq!(prompts.len(), 1);
    assert_eq!(
        prompts[0]["params"],
        json!({"sessionId":"exact-session","prompt":[{"type":"text","text":"not sent"}],"_meta":{CONVERSATION:conversation}})
    );
    assert_eq!(sink.calls.load(Ordering::SeqCst), 1);
    let frozen = result.clone();
    std::fs::write(dir.path().join("release-late"), "").unwrap();
    let end = tokio::time::Instant::now() + Duration::from_secs(3);
    while !dir.path().join("late-sent").exists() {
        assert!(tokio::time::Instant::now() < end);
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert_eq!(completed.result.as_ref().unwrap(), &frozen);
    assert!(
        completed
            .session
            .runtime
            .client
            .as_ref()
            .unwrap()
            .requests
            .shared
            .diagnostics()
            .unmatched_response_id
            >= 1
    );
    assert_eq!(
        CodeBuddyStore(store.clone())
            .read(id.clone())
            .await
            .unwrap()
            .prompt_state,
        PromptState::TerminalObserved
    );
    completed.session.shutdown().await.unwrap();
    cleanup_evidence(&store, &id).await;
}

#[tokio::test]
/// 长 Prompt 超过 early TTL，持续 drain 不产生假的 QueueExpired；结果内存预算超限会降级。
async fn continuously_drains_bounded_collector() {
    let bin = tempfile::tempdir().unwrap();
    let base = build(bin.path());
    let limits = Limits {
        queue_ttl: Duration::from_millis(100),
        queue_count: 8,
        queue_bytes: 1024,
        ..Limits::default()
    };
    let (dir, store, session, sink) = setup(&base, limits).await;
    let conversation = &session.private.conversation_request_id;
    write_frames(
        dir.path(),
        "updates.jsonl",
        &(0..20)
            .map(|_| {
                chunk(
                    "exact-session",
                    conversation,
                    "agent_message_chunk",
                    &"x".repeat(100),
                )
            })
            .collect::<Vec<_>>(),
    );
    std::fs::write(dir.path().join("behavior"), "stream").unwrap();
    let completed = prompt(session, store, sink, Arc::new(NoopTelemetry))
        .await
        .unwrap();
    let result = completed.result.unwrap();
    assert_eq!(
        result.result_completeness,
        ProviderResultCompleteness::Partial
    );
    assert_eq!(result.result.unwrap()["text"].as_str().unwrap().len(), 1000);
    assert_eq!(
        result.diagnostic_code.as_deref(),
        Some("CODEBUDDY_PROMPT_RESULT_INCOMPLETE")
    );
    completed.session.shutdown().await.unwrap();
}

#[tokio::test]
/// 所有发送后错误均单次请求、无 terminal、durable Uncertain；future drop 仍清理整 Job。
async fn failures_and_future_drop_are_uncertain_without_retry() {
    let bin = tempfile::tempdir().unwrap();
    let base = build(bin.path());
    for behavior in [
        "eof",
        "error",
        "timeout",
        "missing-meta",
        "wrong-meta",
        "malformed-meta",
        "unknown-stop",
        "drop",
    ] {
        let limits = Limits {
            prompt_timeout: Duration::from_secs(2),
            ..Limits::default()
        };
        let (dir, store, session, sink) = setup(&base, limits).await;
        let id = session.private.execution_id.clone();
        let body = match behavior {
            "missing-meta" => Some(json!({"stopReason":"end_turn"})),
            "wrong-meta" => Some(json!({"stopReason":"end_turn","_meta":{CONVERSATION:"wrong"}})),
            "malformed-meta" => Some(json!({"stopReason":"end_turn","_meta":[1]})),
            "unknown-stop" => Some(
                json!({"stopReason":"future_reason","_meta":{CONVERSATION:session.private.conversation_request_id}}),
            ),
            _ => None,
        };
        if let Some(body) = body {
            std::fs::write(dir.path().join("response.json"), body.to_string()).unwrap();
        }
        std::fs::write(
            dir.path().join("behavior"),
            if behavior == "drop" {
                "timeout"
            } else {
                behavior
            },
        )
        .unwrap();
        if behavior == "drop" {
            let task = tokio::spawn(prompt(
                session,
                store.clone(),
                sink,
                Arc::new(NoopTelemetry),
            ));
            wait_wire(dir.path(), 3).await;
            task.abort();
            let _ = task.await;
            cleanup_evidence(&store, &id).await;
        } else {
            let completed = prompt(session, store.clone(), sink, Arc::new(NoopTelemetry))
                .await
                .unwrap();
            assert!(completed.result.is_err(), "{behavior}");
            assert_eq!(
                completed.session.private.prompt_state,
                PromptState::Uncertain,
                "{behavior}"
            );
            completed.session.shutdown().await.unwrap();
        }
        let private = CodeBuddyStore(store.clone()).read(id).await.unwrap();
        assert_eq!(private.prompt_state, PromptState::Uncertain, "{behavior}");
        assert!(private.terminal_stop_reason.is_none());
        assert_eq!(
            wire(dir.path())
                .iter()
                .filter(|v| v["method"] == "session/prompt")
                .count(),
            1
        );
    }
}

#[test]
/// Host CB5-003 write seq176 与 CB5-004 cancellation-after seq47 的真实 meta 可 typed 保留。
fn host_fixtures_and_all_typed_stop_mappings() {
    let frames: Vec<Value> =
        include_str!("../../../../tests/fixtures/codebuddy_prompt_end_turn.jsonl")
            .lines()
            .map(|v| serde_json::from_str(v).unwrap())
            .collect();
    let response: agent_client_protocol::schema::v1::PromptResponse =
        serde_json::from_value(frames.last().unwrap()["result"].clone()).unwrap();
    let conversation = response.meta.as_ref().unwrap()[CONVERSATION]
        .as_str()
        .unwrap();
    let session = frames[0]["params"]["sessionId"].as_str().unwrap();
    let shared = Shared::new(Limits::default());
    shared.register_route(session).unwrap();
    for frame in &frames[..3] {
        shared
            .notification("session/update".into(), frame["params"].clone())
            .unwrap();
    }
    let mut collector = Collector::default();
    collector.drain(
        shared.take_session(session).unwrap(),
        session,
        conversation,
        1024,
    );
    let result = collector.finish("e".into(), response.stop_reason).unwrap();
    // 仅三段 Host 摘录的合并快照，不宣称这等于 Host 完整回答。
    assert_eq!(
        result.result,
        Some(json!({"text":"Created `output.txt` in the workspace containing `"}))
    );
    assert_eq!(result.outcome, ProviderOutcome::Completed);
    let cancelled: Value = serde_json::from_str(
        include_str!("../../../../tests/fixtures/codebuddy_prompt_cancelled.jsonl").trim(),
    )
    .unwrap();
    let response: agent_client_protocol::schema::v1::PromptResponse =
        serde_json::from_value(cancelled["result"].clone()).unwrap();
    assert_eq!(response.stop_reason, StopReason::Cancelled);
    assert!(response.meta.unwrap()[CONVERSATION].is_string());
    for (stop, outcome, diagnostic) in [
        (StopReason::EndTurn, ProviderOutcome::Completed, None),
        (StopReason::Cancelled, ProviderOutcome::Cancelled, None),
        (
            StopReason::Refusal,
            ProviderOutcome::Failed,
            Some("CODEBUDDY_PROMPT_REFUSED"),
        ),
        (
            StopReason::MaxTokens,
            ProviderOutcome::Interrupted,
            Some("CODEBUDDY_PROMPT_MAX_TOKENS"),
        ),
        (
            StopReason::MaxTurnRequests,
            ProviderOutcome::Interrupted,
            Some("CODEBUDDY_PROMPT_MAX_TURN_REQUESTS"),
        ),
    ] {
        for (text, tainted) in [("", false), ("answer", false), ("", true), ("answer", true)] {
            let result = Collector {
                text: text.into(),
                tainted,
            }
            .finish("e".into(), stop)
            .unwrap();
            assert_eq!(result.outcome, outcome);
            assert_eq!(
                result.result_completeness,
                if !tainted && matches!(stop, StopReason::EndTurn | StopReason::Refusal) {
                    ProviderResultCompleteness::Complete
                } else if text.is_empty() {
                    ProviderResultCompleteness::Unknown
                } else {
                    ProviderResultCompleteness::Partial
                }
            );
            assert_eq!(
                result.diagnostic_code.as_deref(),
                diagnostic.or(tainted.then_some("CODEBUDDY_PROMPT_RESULT_INCOMPLETE"))
            );
        }
    }
}

#[test]
/// 缺失 identity、畸形 text 与非 text 都污染完整性；foreign prompt 单独忽略。
fn malformed_chunks_taint_but_foreign_identity_does_not() {
    for mode in [
        "missing",
        "null",
        "empty",
        "malformed-text",
        "non-text",
        "foreign",
    ] {
        let shared = Shared::new(Limits::default());
        shared.register_route("s").unwrap();
        let mut frame = chunk("s", "c", "agent_message_chunk", "must not appear");
        let update = &mut frame["params"]["update"];
        match mode {
            "missing" => {
                update.as_object_mut().unwrap().remove("_meta");
            }
            "null" => update["_meta"][CONVERSATION] = Value::Null,
            "empty" => update["_meta"][CONVERSATION] = json!(""),
            "malformed-text" => update["content"]["text"] = json!(5),
            "non-text" => {
                update["content"] = json!({"type":"image","data":"x","mimeType":"image/png"})
            }
            "foreign" => update["_meta"][CONVERSATION] = json!("foreign"),
            _ => unreachable!(),
        }
        shared
            .notification("session/update".into(), frame["params"].clone())
            .unwrap();
        let mut collector = Collector::default();
        collector.drain(shared.take_session("s").unwrap(), "s", "c", 1024);
        let result = collector.finish("e".into(), StopReason::EndTurn).unwrap();
        assert!(result.result.is_none());
        assert_eq!(
            result.result_completeness,
            if mode == "foreign" {
                ProviderResultCompleteness::Complete
            } else {
                ProviderResultCompleteness::Unknown
            }
        );
    }
}

#[tokio::test]
/// exact cancelled/refusal/token limit 均写 typed terminal；deny 单独不产生 terminal。
async fn permission_is_not_terminal_and_typed_responses_persist() {
    let bin = tempfile::tempdir().unwrap();
    let base = build(bin.path());
    for (stop, outcome) in [
        (StopReason::Cancelled, ProviderOutcome::Cancelled),
        (StopReason::Refusal, ProviderOutcome::Failed),
        (StopReason::MaxTokens, ProviderOutcome::Interrupted),
        (StopReason::MaxTurnRequests, ProviderOutcome::Interrupted),
    ] {
        let (dir, store, session, sink) = setup(&base, Limits::default()).await;
        let id = session.private.execution_id.clone();
        std::fs::write(dir.path().join("response.json"), json!({"stopReason":stop,"_meta":{CONVERSATION:session.private.conversation_request_id}}).to_string()).unwrap();
        std::fs::write(dir.path().join("behavior"), "gate").unwrap();
        write_frames(
            dir.path(),
            "permission.jsonl",
            &[
                json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":session.private.session_id,"update":{"sessionUpdate":"tool_call","toolCallId":"permission-tool","title":"private command","status":"pending","_meta":{CONVERSATION:session.private.conversation_request_id}}}}),
                json!({"jsonrpc":"2.0","id":"permission","method":"session/request_permission","params":{"sessionId":session.private.session_id,"toolCall":{"toolCallId":"permission-tool"},"options":[{"optionId":"deny-from-request","name":"deny","kind":"reject_once"}]}}),
            ],
        );
        let task = tokio::spawn(prompt(
            session,
            store.clone(),
            sink,
            Arc::new(NoopTelemetry),
        ));
        let rows = wait_wire(dir.path(), 4).await;
        assert_eq!(
            rows[3]["result"],
            json!({"outcome":{"outcome":"selected","optionId":"deny-from-request"}})
        );
        let private = CodeBuddyStore(store.clone())
            .read(id.clone())
            .await
            .unwrap();
        assert_eq!(private.prompt_state, PromptState::Sent);
        assert!(private.terminal_stop_reason.is_none());
        assert!(!task.is_finished());
        std::fs::write(dir.path().join("release-prompt"), "").unwrap();
        let mut completed = task.await.unwrap().unwrap();
        assert_eq!(completed.result.as_ref().unwrap().outcome, outcome);
        assert_eq!(completed.session.private.terminal_stop_reason, Some(stop));
        assert!(completed.session.private.provider_request_id.is_none());
        assert!(completed.session.check_acceptance_ready().is_err());
        // Drop terminal ownership 同样必须留下 Job evidence，不释放 Claim。
        drop(completed);
        cleanup_evidence(&store, &id).await;
    }
}

#[tokio::test]
/// catalog/private/R1/protocol/过期 revision 任一不一致都不发送；frame preflight 不写 Sent。
async fn rejected_identity_preflight_and_stale_revision_send_nothing() {
    let bin = tempfile::tempdir().unwrap();
    let base = build(bin.path());
    for case in [
        "catalog",
        "private-session",
        "runtime",
        "protocol",
        "stale",
        "frame",
        "accepted",
    ] {
        let (dir, store, mut session, sink) = setup(&base, Limits::default()).await;
        let id = session.private.execution_id.clone();
        match case {
            "catalog" => session.catalog.response.session_id = "other".into(),
            "private-session" => session.private.session_id = Some("other".into()),
            "runtime" => session.private.runtime_instance_id = Some("other".into()),
            "protocol" => session.private.acp_protocol_version = Some(2),
            "stale" => {
                let row = store.execution(id.clone()).await.unwrap().unwrap();
                CodeBuddyStore(store.clone())
                    .mutate(
                        id.clone(),
                        Ownership {
                            execution_revision: row.revision,
                            runtime_instance_id: row.runtime_instance_id,
                        },
                        session.private.revision,
                        Mutation::ExactProviderRequest("external".into()),
                    )
                    .await
                    .unwrap();
            }
            "frame" => {
                // frozen prompt 仅在测试数据库准备阶段变大；生产仍只读 Execution.prompt。
                let db = rusqlite::Connection::open(dir.path().join("agent-state.db")).unwrap();
                db.execute("UPDATE executions SET prompt=?1", ["x".repeat(300 * 1024)])
                    .unwrap();
            }
            "accepted" => {
                struct Noop;
                impl ProviderAcceptanceSink for Noop {
                    fn accepted(&self) {}
                }
                session = session.accept(&Noop).unwrap();
            }
            _ => unreachable!(),
        }
        let completed = prompt(
            session,
            store.clone(),
            sink.clone(),
            Arc::new(NoopTelemetry),
        )
        .await
        .unwrap();
        assert!(completed.result.is_err(), "{case}");
        assert_eq!(sink.calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            CodeBuddyStore(store.clone())
                .read(id)
                .await
                .unwrap()
                .prompt_state,
            PromptState::Prepared
        );
        assert!(
            !wire(dir.path())
                .iter()
                .any(|v| v["method"] == "session/prompt")
        );
        completed.session.shutdown().await.unwrap();
    }
}

#[tokio::test]
/// OCC 冲突不能重试 terminal 或覆盖已存在 terminal；普通 Sent 冲突只收敛 Uncertain。
async fn occ_conflicts_fail_closed_without_terminal_overwrite() {
    let bin = tempfile::tempdir().unwrap();
    let base = build(bin.path());
    for existing_terminal in [false, true] {
        let (dir, store, session, sink) = setup(&base, Limits::default()).await;
        let id = session.private.execution_id.clone();
        std::fs::write(dir.path().join("behavior"), "gate").unwrap();
        let task = tokio::spawn(prompt(
            session,
            store.clone(),
            sink,
            Arc::new(NoopTelemetry),
        ));
        wait_wire(dir.path(), 3).await;
        let private_store = CodeBuddyStore(store.clone());
        let current = private_store.read(id.clone()).await.unwrap();
        let row = store.execution(id.clone()).await.unwrap().unwrap();
        let mutation = if existing_terminal {
            Mutation::ObserveTerminal {
                session_id: current.session_id.clone().unwrap(),
                conversation_request_id: current.conversation_request_id.clone(),
                stop_reason: StopReason::Cancelled,
                observed_at: 123,
            }
        } else {
            Mutation::ExactProviderRequest("external-conflicting-provider-id".into())
        };
        let changed = private_store
            .mutate(
                id.clone(),
                Ownership {
                    execution_revision: row.revision,
                    runtime_instance_id: row.runtime_instance_id,
                },
                current.revision,
                mutation,
            )
            .await
            .unwrap();
        std::fs::write(dir.path().join("release-prompt"), "").unwrap();
        let completed = task.await.unwrap().unwrap();
        assert_eq!(completed.result, Err(Failure::State));
        let current = private_store.read(id).await.unwrap();
        if existing_terminal {
            assert_eq!(current, changed);
        } else {
            assert_eq!(current.prompt_state, PromptState::Uncertain);
            assert!(current.terminal_stop_reason.is_none());
        }
        completed.session.shutdown().await.unwrap();
    }
}

/// CB8：exact response 已到达、terminal 事务前首次 cancel 不能丢失正文或自然终态。
#[tokio::test]
async fn cancel_between_exact_response_and_atomic_terminal_preserves_result() {
    use crate::agent::{
        codebuddy::{discovery::DiscoveryError, provider::CodeBuddyProvider},
        execution::state::{ResultCompleteness, Status},
        provider::{ProviderCancelContext, port::AgentProvider},
    };
    let bin = tempfile::tempdir().unwrap();
    let base = build(bin.path());
    for has_provider_id in [false, true] {
        let (dir, store, session, sink) = setup(&base, Limits::default()).await;
        let id = session.private.execution_id.clone();
        let conversation = session.private.conversation_request_id.clone();
        if !has_provider_id {
            std::fs::write(
                dir.path().join("response.json"),
                json!({"stopReason":"end_turn","_meta":{CONVERSATION:conversation}}).to_string(),
            )
            .unwrap();
        }
        write_frames(
            dir.path(),
            "updates.jsonl",
            &[chunk(
                "exact-session",
                &conversation,
                "agent_message_chunk",
                "retained exact text",
            )],
        );
        let ready = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let scope = (ready.clone(), release.clone());
        let state = store.clone();
        let (keep, cancelled) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(BEFORE_TERMINAL_COMMIT.scope(
            scope,
            run(session, state, sink, Arc::new(NoopTelemetry), cancelled),
        ));
        tokio::time::timeout(Duration::from_secs(5), ready.notified())
            .await
            .unwrap();
        let before = store.execution(id.clone()).await.unwrap().unwrap();
        let private_before = CodeBuddyStore(store.clone())
            .read(id.clone())
            .await
            .unwrap();
        let provider = CodeBuddyProvider::from_discovery(
            store.clone(),
            "host".into(),
            Err(DiscoveryError::not_found(false)),
        );
        provider
            .cancel(ProviderCancelContext {
                execution_id: id.clone(),
            })
            .await
            .unwrap();
        let cancelled = store.execution(id.clone()).await.unwrap().unwrap();
        assert!(cancelled.revision > before.revision);
        assert_eq!(cancelled.status, "cancel_requested");
        assert_eq!(
            CodeBuddyStore(store.clone())
                .read(id.clone())
                .await
                .unwrap(),
            private_before
        );
        release.notify_one();
        let completed = task.await.unwrap();
        drop(keep);
        let result = completed.result.as_ref().unwrap();
        assert_eq!(result.outcome, ProviderOutcome::Completed);
        assert_eq!(result.result, Some(json!({"text":"retained exact text"})));
        assert_eq!(
            completed.session.private.terminal_stop_reason,
            Some(StopReason::EndTurn)
        );
        assert_eq!(
            completed.session.private.provider_request_id.as_deref(),
            has_provider_id.then_some("independent-provider-request")
        );
        assert!(
            !wire(dir.path())
                .iter()
                .any(|frame| frame["method"] == "session/cancel")
        );
        assert!(
            store
                .workspace_claim(before.canonical_workspace_root.clone())
                .await
                .unwrap()
                .is_some()
        );
        store
            .provider_event(
                id.clone(),
                Transition::ProviderTerminalResult {
                    runtime_id: before.runtime_instance_id.unwrap(),
                    status: Status::Completed,
                    result: result.result.clone(),
                    completeness: ResultCompleteness::Complete,
                },
                now(),
            )
            .await
            .unwrap();
        assert!(
            store
                .workspace_claim(before.canonical_workspace_root.clone())
                .await
                .unwrap()
                .is_some()
        );
        completed.session.shutdown().await.unwrap();
        crate::agent::codebuddy::recovery::reconcile_execution(&store, &id, false)
            .await
            .unwrap();
        let done = store.execution(id).await.unwrap().unwrap();
        assert_eq!(done.status, "completed");
        assert_eq!(
            done.final_result_json.as_deref(),
            Some("{\"text\":\"retained exact text\"}")
        );
        assert!(
            store
                .workspace_claim(before.canonical_workspace_root)
                .await
                .unwrap()
                .is_none()
        );
    }
}
