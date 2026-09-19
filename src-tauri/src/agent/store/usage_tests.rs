//! P4-004 的 Store Usage epoch 回归测试，所有 fixture 都显式建立 runtime/thread/turn 身份。

use super::*;
use crate::agent::{
    execution::{CreateExecutionInput, canonicalize_request},
    provider::{ProviderId, telemetry::UsageEvent},
};
use serde_json::{Value, json};

/// 创建最小 Codex Execution，避免测试借用 Product 或 MCP 路径。
async fn execution(store: &StateStore, id: &str, root: &std::path::Path) {
    let execution_root = root.join(id);
    std::fs::create_dir_all(&execution_root).unwrap();
    let input: CreateExecutionInput = serde_json::from_value(json!({
        "agent_id": format!("usage-{id}"), "request_key": id, "prompt": "usage",
        "execution_profile": {}, "workspace_id": id,
        "canonical_workspace_root": execution_root, "mode": "read_only"
    }))
    .unwrap();
    store
        .create_execution(id.into(), canonicalize_request(input).unwrap(), 1)
        .await
        .unwrap();
}

/// 直接构造已由 provider 生命周期验证过的 Thread bind 前置状态。
fn bind_thread_fixture(store: &StateStore, id: &str, runtime: &str, thread: &str) {
    let connection = store.connection.lock().unwrap();
    connection
        .execute(
            "INSERT OR IGNORE INTO runtime_instances(id,owner_host_instance_id,state,created_at,updated_at)
             VALUES(?1,'usage-test','unknown',1,1)",
            [runtime],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE executions SET runtime_instance_id=?2,thread_id=?3 WHERE id=?1",
            rusqlite::params![id, runtime, thread],
        )
        .unwrap();
}

/// 将 private state 同步到当前 Turn；这模拟 `bind_protocol_identity` 已完成的正常 event 路径。
fn bind_turn_fixture(store: &StateStore, id: &str, turn: &str) {
    let connection = store.connection.lock().unwrap();
    connection
        .execute(
            "UPDATE executions SET turn_id=?2 WHERE id=?1",
            rusqlite::params![id, turn],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE codex_execution_usage_state SET turn_id=?2 WHERE execution_id=?1",
            rusqlite::params![id, turn],
        )
        .unwrap();
}

/// 构造 P4-003 已绑定的安全累计 Usage event；breakdown 仅用于验证其不泄漏进 public delta。
fn event(id: &str, total: i64, context: Option<i64>, at: i64) -> UsageEvent {
    UsageEvent::cumulative(
        id.into(),
        ProviderId::new("codex".into()).unwrap(),
        total,
        Some(9),
        Some(8),
        Some(0),
        Some(7),
        Some(6),
        context,
        at,
    )
}

/// 读取 private state，断言测试不把 private identity 暴露到 public DTO。
fn state(store: &StateStore, id: &str) -> usage::CodexExecutionUsageStateRecord {
    let connection = store.connection.lock().unwrap();
    usage::codex_execution_usage_state_record(&connection, id)
        .unwrap()
        .unwrap()
}

/// Codex Usage 的 baseline 只允许由已绑定的 runtime/thread 证据建立，不得引入 account Usage checkpoint。
#[test]
fn codex_usage_baseline_never_routes_through_account_usage_read() {
    let provider_source = include_str!("../codex/provider.rs");
    let protocol_source = include_str!("../codex/protocol.rs");
    assert!(!provider_source.contains("account/usage/read"));
    assert!(!protocol_source.contains("account/usage/read"));
}

/// 建立已 bind 且已有 fresh-zero public Usage 的 grace fixture。
async fn grace_fixture(store: &StateStore, root: &std::path::Path, id: &str, runtime: &str) {
    execution(store, id, root).await;
    let thread = format!("thread-{id}");
    let turn = format!("turn-{id}");
    bind_thread_fixture(store, id, runtime, &thread);
    store
        .prepare_codex_usage_baseline(
            id.into(),
            runtime.into(),
            thread,
            CodexUsageBaselineIntent::FreshZero,
            1,
        )
        .await
        .unwrap();
    bind_turn_fixture(store, id, &turn);
    store
        .project_execution_usage(event(id, 100, Some(10), 10))
        .await
        .unwrap();
}

#[tokio::test]
async fn terminal_grace_fixes_first_terminal_time_and_accepts_inclusive_deadline() {
    let directory = tempfile::tempdir().unwrap();
    let store = StateStore::open(directory.path().into()).await.unwrap();
    grace_fixture(&store, directory.path(), "grace", "runtime").await;

    store
        .enter_codex_usage_terminal_grace(
            "grace".into(),
            "runtime".into(),
            "thread-grace".into(),
            "turn-grace".into(),
            1_000,
        )
        .await
        .unwrap();
    store
        .enter_codex_usage_terminal_grace(
            "grace".into(),
            "runtime".into(),
            "thread-grace".into(),
            "turn-grace".into(),
            9_999,
        )
        .await
        .unwrap();
    assert_eq!(
        (
            state(&store, "grace").telemetry_state,
            state(&store, "grace").terminal_at
        ),
        ("terminal_grace".into(), Some(1_000))
    );

    store
        .project_execution_usage(event("grace", 150, Some(10), 2_000))
        .await
        .unwrap();
    store
        .project_execution_usage(event("grace", 160, Some(11), 3_000))
        .await
        .unwrap();
    let public = store
        .execution_usage("grace".into())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (public.total_tokens, public.completeness, public.revision),
        (
            Some(160),
            crate::agent::usage::UsageCompleteness::Partial,
            3
        )
    );
    assert_eq!(state(&store, "grace").telemetry_state, "terminal_grace");
}

#[tokio::test]
async fn expired_and_frozen_usage_are_atomic_noops_without_codex_complete() {
    let directory = tempfile::tempdir().unwrap();
    let store = StateStore::open(directory.path().into()).await.unwrap();
    grace_fixture(&store, directory.path(), "expired", "runtime").await;
    store
        .enter_codex_usage_terminal_grace(
            "expired".into(),
            "runtime".into(),
            "thread-expired".into(),
            "turn-expired".into(),
            1_000,
        )
        .await
        .unwrap();
    let public_before = store
        .execution_usage("expired".into())
        .await
        .unwrap()
        .unwrap();
    let private_before = state(&store, "expired");
    assert_eq!(
        store
            .project_execution_usage(event("expired", 200, Some(20), 3_001))
            .await
            .unwrap_err(),
        usage::USAGE_TELEMETRY_FROZEN
    );
    let frozen = state(&store, "expired");
    assert_eq!(
        (frozen.telemetry_state.as_str(), frozen.freeze_at),
        ("frozen", Some(3_000))
    );
    assert_eq!(
        frozen.latest_cumulative_json,
        private_before.latest_cumulative_json
    );
    assert_eq!(
        store
            .execution_usage("expired".into())
            .await
            .unwrap()
            .unwrap(),
        public_before
    );
    assert_eq!(
        store
            .project_execution_usage(event("expired", 250, Some(21), 3_002))
            .await
            .unwrap_err(),
        usage::USAGE_TELEMETRY_FROZEN
    );
    assert_eq!(
        store
            .execution_usage("expired".into())
            .await
            .unwrap()
            .unwrap()
            .completeness,
        crate::agent::usage::UsageCompleteness::Partial
    );
}

#[tokio::test]
async fn freeze_and_runtime_termination_preserve_public_usage_and_isolate_runtimes() {
    let directory = tempfile::tempdir().unwrap();
    let store = StateStore::open(directory.path().into()).await.unwrap();
    grace_fixture(&store, directory.path(), "terminal-grace", "runtime-a").await;
    grace_fixture(&store, directory.path(), "accepting", "runtime-a").await;
    grace_fixture(&store, directory.path(), "other-runtime", "runtime-b").await;
    store
        .enter_codex_usage_terminal_grace(
            "terminal-grace".into(),
            "runtime-a".into(),
            "thread-terminal-grace".into(),
            "turn-terminal-grace".into(),
            1_000,
        )
        .await
        .unwrap();
    let public_before = store
        .execution_usage("terminal-grace".into())
        .await
        .unwrap()
        .unwrap();
    store
        .freeze_codex_usage("terminal-grace".into(), 1_500)
        .await
        .unwrap();
    assert_eq!(state(&store, "terminal-grace").freeze_at, Some(1_500));
    assert_eq!(
        store
            .execution_usage("terminal-grace".into())
            .await
            .unwrap()
            .unwrap(),
        public_before
    );

    store
        .freeze_codex_usage("terminal-grace".into(), 1_600)
        .await
        .unwrap();
    assert_eq!(state(&store, "terminal-grace").freeze_at, Some(1_500));
    store
        .complete_runtime(
            &crate::agent::codex::runtime::TerminationEvidence::for_test("runtime-a".into(), 2_000),
        )
        .unwrap();
    assert_eq!(state(&store, "accepting").telemetry_state, "frozen");
    assert_eq!(state(&store, "accepting").freeze_at, Some(2_000));
    assert_eq!(state(&store, "terminal-grace").freeze_at, Some(1_500));
    assert_eq!(state(&store, "other-runtime").telemetry_state, "accepting");
    assert_eq!(
        store
            .execution_usage("accepting".into())
            .await
            .unwrap()
            .unwrap()
            .completeness,
        crate::agent::usage::UsageCompleteness::Partial
    );
}

#[tokio::test]
async fn grace_identity_mismatch_does_not_change_private_or_execution_state() {
    let directory = tempfile::tempdir().unwrap();
    let store = StateStore::open(directory.path().into()).await.unwrap();
    grace_fixture(&store, directory.path(), "identity", "runtime").await;
    let execution_before = store.execution("identity".into()).await.unwrap().unwrap();
    assert_eq!(
        store
            .enter_codex_usage_terminal_grace(
                "identity".into(),
                "runtime".into(),
                "thread-identity".into(),
                "wrong-turn".into(),
                1_000,
            )
            .await
            .unwrap_err(),
        "USAGE_GRACE_IDENTITY_MISMATCH"
    );
    assert_eq!(state(&store, "identity").telemetry_state, "accepting");
    assert_eq!(
        store.execution("identity".into()).await.unwrap().unwrap(),
        execution_before
    );
}

#[tokio::test]
async fn baseline_intents_require_provenance_and_never_forge_fresh_zero() {
    let directory = tempfile::tempdir().unwrap();
    let store = StateStore::open(directory.path().into()).await.unwrap();
    execution(&store, "fresh", directory.path()).await;
    bind_thread_fixture(&store, "fresh", "runtime-1", "thread-1");
    store
        .prepare_codex_usage_baseline(
            "fresh".into(),
            "runtime-1".into(),
            "thread-1".into(),
            CodexUsageBaselineIntent::FreshZero,
            1,
        )
        .await
        .unwrap();
    assert_eq!(state(&store, "fresh").baseline_kind, "fresh_zero");
    assert_eq!(
        state(&store, "fresh").baseline_json.as_deref(),
        Some("{\"totalTokens\":0}")
    );

    execution(&store, "not-fresh", directory.path()).await;
    // 同 epoch 已有累计记录时，即使新的 Execution 自称 fresh 也不能伪造 zero。
    {
        let connection = store.connection.lock().unwrap();
        connection
            .execute(
                "INSERT INTO codex_thread_usage_epochs(runtime_instance_id,thread_id,latest_cumulative_json,latest_turn_id,captured_at)
                 VALUES('runtime-1','thread-1','{\"totalTokens\":0}',NULL,2)",
                [],
            )
            .unwrap();
        connection
            .execute(
                "UPDATE executions SET runtime_instance_id='runtime-1',thread_id='thread-1' WHERE id='not-fresh'",
                [],
            )
            .unwrap();
    }
    store
        .prepare_codex_usage_baseline(
            "not-fresh".into(),
            "runtime-1".into(),
            "thread-1".into(),
            CodexUsageBaselineIntent::FreshZero,
            2,
        )
        .await
        .unwrap();
    assert_eq!(state(&store, "not-fresh").baseline_kind, "unknown");

    {
        let connection = store.connection.lock().unwrap();
        connection
            .execute(
                "DELETE FROM codex_execution_usage_state WHERE execution_id='not-fresh'",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO codex_thread_usage_epochs(runtime_instance_id,thread_id,latest_cumulative_json,latest_turn_id,captured_at)
                 VALUES('runtime-1','warm-thread','{\"totalTokens\":100}',NULL,3)",
                [],
            )
            .unwrap();
    }
    execution(&store, "warm", directory.path()).await;
    {
        let connection = store.connection.lock().unwrap();
        connection.execute("UPDATE executions SET runtime_instance_id='runtime-1',thread_id='warm-thread' WHERE id='warm'", []).unwrap();
    }
    store
        .prepare_codex_usage_baseline(
            "warm".into(),
            "runtime-1".into(),
            "warm-thread".into(),
            CodexUsageBaselineIntent::WarmObservedSameEpoch,
            4,
        )
        .await
        .unwrap();
    assert_eq!(state(&store, "warm").baseline_kind, "observed_same_epoch");
    assert_eq!(
        state(&store, "warm").baseline_json.as_deref(),
        Some("{\"totalTokens\":100}")
    );

    execution(&store, "cold", directory.path()).await;
    bind_thread_fixture(&store, "cold", "runtime-2", "warm-thread");
    store
        .prepare_codex_usage_baseline(
            "cold".into(),
            "runtime-2".into(),
            "warm-thread".into(),
            CodexUsageBaselineIntent::Unknown,
            5,
        )
        .await
        .unwrap();
    assert_eq!(state(&store, "cold").baseline_kind, "unknown");

    execution(&store, "missing", directory.path()).await;
    bind_thread_fixture(&store, "missing", "runtime-3", "missing-thread");
    store
        .prepare_codex_usage_baseline(
            "missing".into(),
            "runtime-3".into(),
            "missing-thread".into(),
            CodexUsageBaselineIntent::WarmObservedSameEpoch,
            6,
        )
        .await
        .unwrap();
    assert_eq!(state(&store, "missing").baseline_kind, "unknown");

    execution(&store, "corrupt", directory.path()).await;
    bind_thread_fixture(&store, "corrupt", "runtime-3", "corrupt-thread");
    {
        let connection = store.connection.lock().unwrap();
        connection.execute("INSERT INTO codex_thread_usage_epochs(runtime_instance_id,thread_id,latest_cumulative_json,latest_turn_id,captured_at) VALUES('runtime-3','corrupt-thread','{bad',NULL,7)", []).unwrap();
    }
    store
        .prepare_codex_usage_baseline(
            "corrupt".into(),
            "runtime-3".into(),
            "corrupt-thread".into(),
            CodexUsageBaselineIntent::WarmObservedSameEpoch,
            7,
        )
        .await
        .unwrap();
    assert_eq!(state(&store, "corrupt").baseline_kind, "unknown");
}

#[tokio::test]
async fn baseline_refuses_a_bound_turn_and_real_bind_synchronizes_private_turn() {
    let directory = tempfile::tempdir().unwrap();
    let store = StateStore::open(directory.path().into()).await.unwrap();
    execution(&store, "bound", directory.path()).await;
    {
        let connection = store.connection.lock().unwrap();
        connection.execute("INSERT INTO runtime_instances(id,owner_host_instance_id,state,created_at,updated_at) VALUES('bind-runtime','usage-test','unknown',1,1)", []).unwrap();
        connection
            .execute(
                "UPDATE executions SET runtime_instance_id='bind-runtime' WHERE id='bound'",
                [],
            )
            .unwrap();
    }
    store
        .bind_protocol_identity(
            "bound".into(),
            0,
            "bind-runtime".into(),
            "bind-thread".into(),
            None,
            2,
        )
        .await
        .unwrap();
    store
        .prepare_codex_usage_baseline(
            "bound".into(),
            "bind-runtime".into(),
            "bind-thread".into(),
            CodexUsageBaselineIntent::FreshZero,
            3,
        )
        .await
        .unwrap();
    store
        .bind_protocol_identity(
            "bound".into(),
            1,
            "bind-runtime".into(),
            "bind-thread".into(),
            Some("bound-turn".into()),
            4,
        )
        .await
        .unwrap();
    assert_eq!(
        state(&store, "bound").turn_id.as_deref(),
        Some("bound-turn")
    );
    assert_eq!(
        store
            .prepare_codex_usage_baseline(
                "bound".into(),
                "bind-runtime".into(),
                "bind-thread".into(),
                CodexUsageBaselineIntent::FreshZero,
                5
            )
            .await
            .unwrap_err(),
        "USAGE_BASELINE_IDENTITY_UNAVAILABLE"
    );
}

#[tokio::test]
async fn fresh_and_warm_projection_are_total_only_and_revision_is_semantic() {
    let directory = tempfile::tempdir().unwrap();
    let store = StateStore::open(directory.path().into()).await.unwrap();
    execution(&store, "fresh", directory.path()).await;
    bind_thread_fixture(&store, "fresh", "runtime", "fresh-thread");
    store
        .prepare_codex_usage_baseline(
            "fresh".into(),
            "runtime".into(),
            "fresh-thread".into(),
            CodexUsageBaselineIntent::FreshZero,
            1,
        )
        .await
        .unwrap();
    bind_turn_fixture(&store, "fresh", "turn-fresh");
    store
        .project_execution_usage(event("fresh", 100, Some(258400), 10))
        .await
        .unwrap();
    let first = store
        .execution_usage("fresh".into())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first.total_tokens, Some(100));
    assert_eq!(
        first.completeness,
        crate::agent::usage::UsageCompleteness::Partial
    );
    assert_eq!(
        (
            first.input_tokens,
            first.cached_input_tokens,
            first.cache_write_input_tokens,
            first.output_tokens,
            first.reasoning_tokens
        ),
        (None, None, None, None, None)
    );
    assert_eq!(first.model_context_window, Some(258400));
    assert_eq!(first.revision, 1);
    store
        .project_execution_usage(event("fresh", 150, Some(258400), 11))
        .await
        .unwrap();
    let second = store
        .execution_usage("fresh".into())
        .await
        .unwrap()
        .unwrap();
    assert_eq!((second.total_tokens, second.revision), (Some(150), 2));
    store
        .project_execution_usage(event("fresh", 150, Some(258400), 99))
        .await
        .unwrap();
    assert_eq!(
        store
            .execution_usage("fresh".into())
            .await
            .unwrap()
            .unwrap()
            .revision,
        2
    );
    store
        .project_execution_usage(event("fresh", 150, Some(300000), 12))
        .await
        .unwrap();
    assert_eq!(
        store
            .execution_usage("fresh".into())
            .await
            .unwrap()
            .unwrap()
            .revision,
        3
    );

    execution(&store, "warm", directory.path()).await;
    bind_thread_fixture(&store, "warm", "runtime", "warm-thread");
    {
        let connection = store.connection.lock().unwrap();
        connection.execute("INSERT INTO codex_thread_usage_epochs(runtime_instance_id,thread_id,latest_cumulative_json,latest_turn_id,captured_at) VALUES('runtime','warm-thread','{\"totalTokens\":100}',NULL,1)", []).unwrap();
    }
    store
        .prepare_codex_usage_baseline(
            "warm".into(),
            "runtime".into(),
            "warm-thread".into(),
            CodexUsageBaselineIntent::WarmObservedSameEpoch,
            1,
        )
        .await
        .unwrap();
    bind_turn_fixture(&store, "warm", "turn-warm");
    store
        .project_execution_usage(event("warm", 150, None, 2))
        .await
        .unwrap();
    store
        .project_execution_usage(event("warm", 180, None, 3))
        .await
        .unwrap();
    assert_eq!(
        store
            .execution_usage("warm".into())
            .await
            .unwrap()
            .unwrap()
            .total_tokens,
        Some(80)
    );
}

#[tokio::test]
async fn unknown_cross_runtime_and_regression_are_fail_safe_and_atomic() {
    let directory = tempfile::tempdir().unwrap();
    let store = StateStore::open(directory.path().into()).await.unwrap();
    execution(&store, "old", directory.path()).await;
    bind_thread_fixture(&store, "old", "runtime-old", "thread");
    store
        .prepare_codex_usage_baseline(
            "old".into(),
            "runtime-old".into(),
            "thread".into(),
            CodexUsageBaselineIntent::FreshZero,
            1,
        )
        .await
        .unwrap();
    bind_turn_fixture(&store, "old", "turn-old");
    store
        .project_execution_usage(event("old", 150, None, 2))
        .await
        .unwrap();
    let before = store.execution_usage("old".into()).await.unwrap().unwrap();
    assert_eq!(
        store
            .project_execution_usage(event("old", 140, None, 3))
            .await
            .unwrap_err(),
        usage::USAGE_COUNTER_REGRESSION
    );
    assert_eq!(
        store.execution_usage("old".into()).await.unwrap().unwrap(),
        before
    );

    execution(&store, "restart", directory.path()).await;
    bind_thread_fixture(&store, "restart", "runtime-new", "thread");
    store
        .prepare_codex_usage_baseline(
            "restart".into(),
            "runtime-new".into(),
            "thread".into(),
            CodexUsageBaselineIntent::Unknown,
            1,
        )
        .await
        .unwrap();
    bind_turn_fixture(&store, "restart", "turn-new");
    store
        .project_execution_usage(event("restart", 50, None, 4))
        .await
        .unwrap();
    let restart = store
        .execution_usage("restart".into())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(restart.total_tokens, None);
    assert_eq!(
        restart.completeness,
        crate::agent::usage::UsageCompleteness::Unknown
    );
    let connection = store.connection.lock().unwrap();
    assert_eq!(
        usage::codex_thread_usage_epoch_record(&connection, "runtime-new", "thread")
            .unwrap()
            .unwrap()
            .latest_cumulative_json,
        safe_json_total(50)
    );
}

/// 复用 production safe shape 的 total 部分，避免测试接受旧 `total` 字段。
fn safe_json_total(total: i64) -> String {
    json!({"totalTokens": total, "inputTokens": 9, "cachedInputTokens": 8, "cacheWriteInputTokens": 0, "outputTokens": 7, "reasoningOutputTokens": 6, "modelContextWindow": Value::Null}).to_string()
}

#[tokio::test]
async fn invalidation_degrades_partial_without_touching_execution_lifecycle() {
    let directory = tempfile::tempdir().unwrap();
    let store = StateStore::open(directory.path().into()).await.unwrap();
    execution(&store, "polluted", directory.path()).await;
    bind_thread_fixture(&store, "polluted", "runtime", "thread");
    store
        .prepare_codex_usage_baseline(
            "polluted".into(),
            "runtime".into(),
            "thread".into(),
            CodexUsageBaselineIntent::FreshZero,
            1,
        )
        .await
        .unwrap();
    bind_turn_fixture(&store, "polluted", "current-turn");
    store
        .project_execution_usage(event("polluted", 100, Some(10), 2))
        .await
        .unwrap();
    let lifecycle = store.execution("polluted".into()).await.unwrap().unwrap();
    store
        .invalidate_codex_usage_baseline("polluted".into(), "runtime".into(), "thread".into(), 3)
        .await
        .unwrap();
    let public = store
        .execution_usage("polluted".into())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (public.total_tokens, public.completeness, public.revision),
        (None, crate::agent::usage::UsageCompleteness::Unknown, 2)
    );
    assert_eq!(state(&store, "polluted").baseline_kind, "unknown");
    assert_eq!(
        store
            .execution("polluted".into())
            .await
            .unwrap()
            .unwrap()
            .status,
        lifecycle.status
    );
}

#[tokio::test]
async fn invalidation_failure_leaves_execution_and_usage_untouched() {
    let directory = tempfile::tempdir().unwrap();
    let store = StateStore::open(directory.path().into()).await.unwrap();
    execution(&store, "invalidation-failure", directory.path()).await;
    bind_thread_fixture(&store, "invalidation-failure", "runtime", "thread");
    store
        .prepare_codex_usage_baseline(
            "invalidation-failure".into(),
            "runtime".into(),
            "thread".into(),
            CodexUsageBaselineIntent::FreshZero,
            1,
        )
        .await
        .unwrap();
    bind_turn_fixture(&store, "invalidation-failure", "turn");
    store
        .project_execution_usage(event("invalidation-failure", 100, None, 2))
        .await
        .unwrap();
    let execution_before = store
        .execution("invalidation-failure".into())
        .await
        .unwrap()
        .unwrap();
    let usage_before = store
        .execution_usage("invalidation-failure".into())
        .await
        .unwrap()
        .unwrap();
    store.inject_observability_failure(ObservabilityFault::UsageInvalidation);
    assert_eq!(
        store
            .invalidate_codex_usage_baseline(
                "invalidation-failure".into(),
                "runtime".into(),
                "thread".into(),
                77,
            )
            .await
            .unwrap_err(),
        "INJECTED_USAGE_INVALIDATION_FAILURE"
    );
    assert_eq!(
        store
            .execution("invalidation-failure".into())
            .await
            .unwrap()
            .unwrap(),
        execution_before
    );
    assert_eq!(
        store
            .execution_usage("invalidation-failure".into())
            .await
            .unwrap()
            .unwrap(),
        usage_before
    );
}

#[tokio::test]
async fn missing_private_state_projects_unknown_without_lifecycle_mutation() {
    let directory = tempfile::tempdir().unwrap();
    let store = StateStore::open(directory.path().into()).await.unwrap();
    execution(&store, "missing-state", directory.path()).await;
    bind_thread_fixture(&store, "missing-state", "runtime", "thread");
    {
        let connection = store.connection.lock().unwrap();
        connection
            .execute(
                "UPDATE executions SET turn_id='turn' WHERE id='missing-state'",
                [],
            )
            .unwrap();
    }
    let before = store
        .execution("missing-state".into())
        .await
        .unwrap()
        .unwrap();
    store
        .project_execution_usage(event("missing-state", 20, None, 2))
        .await
        .unwrap();
    let public = store
        .execution_usage("missing-state".into())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (public.total_tokens, public.completeness),
        (None, crate::agent::usage::UsageCompleteness::Unknown)
    );
    assert_eq!(state(&store, "missing-state").baseline_kind, "unknown");
    assert_eq!(
        store
            .execution("missing-state".into())
            .await
            .unwrap()
            .unwrap()
            .revision,
        before.revision
    );
}
