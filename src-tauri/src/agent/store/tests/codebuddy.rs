use super::*;
use crate::agent::codebuddy::store::{
    CodeBuddyStore, InspectionOutcome, Mutation, Ownership, PrivateState, PromptRpcId, PromptState,
    RecoveryState, valid_conversation_id,
};
use agent_client_protocol::schema::v1::StopReason;
use rusqlite::types::Value;

/// 用真实当前 schema 构造隔离父记录；SQL 仅存在于 store 私有测试内。
async fn fixture(bound: bool) -> (tempfile::TempDir, CodeBuddyStore) {
    let directory = tempfile::tempdir().unwrap();
    let store = StateStore::open(directory.path().into()).await.unwrap();
    {
        let mut c = store.connection.lock().unwrap();
        for (id, provider) in [("r1", "codebuddy"), ("r2", "codebuddy"), ("codex", "codex")] {
            runtime(&c, id);
            c.execute(
                "UPDATE runtime_instances SET provider=?2 WHERE id=?1",
                params![id, provider],
            )
            .unwrap();
        }
        insert(&mut c, "e1", "agent1", "C:/cb-private");
        c.execute(
            "UPDATE executions SET provider='codebuddy',runtime_instance_id=?1 WHERE id='e1'",
            [bound.then_some("r1")],
        )
        .unwrap();
        c.execute("INSERT INTO workspace_claims (canonical_workspace_root,execution_id,claim_type,acquired_at) VALUES ('C:/cb-private','e1','exclusive_execution',123)", []).unwrap();
    }
    (directory, CodeBuddyStore(store))
}

/// 正常调用持有的 generic ownership 快照。
fn owner(bound: bool) -> Ownership {
    Ownership {
        execution_revision: 0,
        runtime_instance_id: bound.then(|| "r1".into()),
    }
}

/// 比较全表所有列，包含 final_result、termination/release evidence 与 Claim authority。
fn snapshot(store: &CodeBuddyStore, tables: &[&str]) -> Vec<Vec<Vec<Value>>> {
    let c = store.0.connection.lock().unwrap();
    tables
        .iter()
        .map(|table| {
            let mut statement = c
                .prepare(&format!("SELECT * FROM {table} ORDER BY 1"))
                .unwrap();
            let columns = statement.column_count();
            statement
                .query_map([], |row| (0..columns).map(|i| row.get(i)).collect())
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap()
        })
        .collect()
}

const AUTHORITY: &[&str] = &["executions", "runtime_instances", "workspace_claims"];
const ALL: &[&str] = &[
    "executions",
    "runtime_instances",
    "workspace_claims",
    "codebuddy_execution_state",
];

/// 拒绝操作必须保持所有持久记录逐值不变。
async fn rejected(store: &CodeBuddyStore, ownership: Ownership, revision: i64, mutation: Mutation) {
    let before = snapshot(store, ALL);
    assert!(
        store
            .mutate("e1".into(), ownership, revision, mutation)
            .await
            .is_err()
    );
    assert_eq!(snapshot(store, ALL), before);
}

/// 按真实 typed API 准备完整 Session 身份，不直接写私有表。
async fn ready(store: &CodeBuddyStore) -> PrivateState {
    let state = store.create("e1".into(), owner(true)).await.unwrap();
    let state = store
        .mutate(
            "e1".into(),
            owner(true),
            state.revision,
            Mutation::NegotiatedProtocol(1),
        )
        .await
        .unwrap();
    store
        .mutate(
            "e1".into(),
            owner(true),
            state.revision,
            Mutation::ExactSession("session-private".into()),
        )
        .await
        .unwrap()
}

/// 精确 terminal 只使用当前已保存的 Session/Conversation pair。
fn terminal(state: &PrivateState) -> Mutation {
    Mutation::ObserveTerminal {
        session_id: state.session_id.clone().unwrap(),
        conversation_request_id: state.conversation_request_id.clone(),
        stop_reason: StopReason::EndTurn,
        observed_at: 456,
    }
}

/// create 原子预留 UUIDv7；重复 create 不覆盖，重启逐字段保持。
#[tokio::test]
async fn create_read_duplicate_and_restart_preserve_prepared_uuid() {
    let (directory, store) = fixture(false).await;
    let authority = snapshot(&store, AUTHORITY);
    let state = store.create("e1".into(), owner(false)).await.unwrap();
    assert_eq!(state.prompt_state, PromptState::Prepared);
    assert_eq!(state.revision, 0);
    assert!(valid_conversation_id(&state.conversation_request_id));
    assert_eq!(state.conversation_request_id.len(), 32);
    assert_eq!(state.runtime_instance_id, None);
    assert_eq!(state.acp_protocol_version, None);
    assert_eq!(state.session_id, None);
    assert_eq!(state.provider_request_id, None);
    assert_eq!(store.read("e1".into()).await.unwrap(), state);
    let before = snapshot(&store, ALL);
    assert!(store.create("e1".into(), owner(false)).await.is_err());
    assert_eq!(snapshot(&store, ALL), before);
    assert_eq!(snapshot(&store, AUTHORITY), authority);
    drop(store);
    let reopened = CodeBuddyStore(StateStore::open(directory.path().into()).await.unwrap());
    assert_eq!(reopened.read("e1".into()).await.unwrap(), state);
}

/// provider、runtime provider、generic binding/revision 和 private OCC 均失败关闭且零写入。
#[tokio::test]
async fn ownership_and_revisions_reject_without_writes() {
    for sql in [
        "UPDATE executions SET provider='codex' WHERE id='e1'",
        "UPDATE runtime_instances SET provider='codex' WHERE id='r1'",
    ] {
        let (_directory, store) = fixture(true).await;
        store.0.connection.lock().unwrap().execute(sql, []).unwrap();
        let before = snapshot(&store, ALL);
        assert!(store.create("e1".into(), owner(true)).await.is_err());
        assert_eq!(snapshot(&store, ALL), before);
    }
    let (_directory, store) = fixture(true).await;
    let state = ready(&store).await;
    for ownership in [
        owner(false),
        Ownership {
            execution_revision: 1,
            ..owner(true)
        },
        Ownership {
            runtime_instance_id: Some("r2".into()),
            ..owner(true)
        },
    ] {
        let before = snapshot(&store, ALL);
        assert!(store.create("e1".into(), ownership.clone()).await.is_err());
        assert_eq!(snapshot(&store, ALL), before);
        rejected(
            &store,
            ownership,
            state.revision,
            Mutation::MarkSent { rpc_id: None },
        )
        .await;
    }
    rejected(
        &store,
        owner(true),
        state.revision - 1,
        Mutation::MarkSent { rpc_id: None },
    )
    .await;
    rejected(&store, owner(true), -1, Mutation::MarkSent { rpc_id: None }).await;
    for sql in [
        "UPDATE executions SET provider='codex' WHERE id='e1'",
        "UPDATE runtime_instances SET provider='codex' WHERE id='r1'",
    ] {
        let (_directory, store) = fixture(true).await;
        let state = ready(&store).await;
        store.0.connection.lock().unwrap().execute(sql, []).unwrap();
        assert!(store.read("e1".into()).await.is_err());
        rejected(
            &store,
            owner(true),
            state.revision,
            Mutation::MarkSent { rpc_id: None },
        )
        .await;
    }
}

/// 三种 readiness 缺失分别拒绝发送，之后绑定 R1 可继续完成准备。
#[tokio::test]
async fn mark_sent_requires_each_readiness_field_and_preserves_revision() {
    for missing in ["runtime", "protocol", "session"] {
        let (_directory, store) = fixture(true).await;
        let state = ready(&store).await;
        {
            let c = store.0.connection.lock().unwrap();
            let column = match missing {
                "runtime" => "runtime_instance_id",
                "protocol" => "acp_protocol_version",
                _ => "session_id",
            };
            // prepared 允许局部缺失，隔离每一个 readiness 检查。
            c.execute(
                &format!(
                    "UPDATE codebuddy_execution_state SET {column}=NULL WHERE execution_id='e1'"
                ),
                [],
            )
            .unwrap();
        }
        rejected(
            &store,
            owner(true),
            state.revision,
            Mutation::MarkSent { rpc_id: None },
        )
        .await;
    }
    let (_directory, store) = fixture(false).await;
    let state = store.create("e1".into(), owner(false)).await.unwrap();
    rejected(
        &store,
        owner(false),
        state.revision,
        Mutation::MarkSent { rpc_id: None },
    )
    .await;
    rejected(&store, owner(false), state.revision, Mutation::BindRuntime).await;
    rejected(
        &store,
        owner(false),
        state.revision,
        Mutation::NegotiatedProtocol(1),
    )
    .await;
    rejected(
        &store,
        owner(false),
        state.revision,
        Mutation::ExactSession("early".into()),
    )
    .await;
    store
        .0
        .connection
        .lock()
        .unwrap()
        .execute(
            "UPDATE executions SET runtime_instance_id='r1' WHERE id='e1'",
            [],
        )
        .unwrap();
    let bound = store
        .mutate("e1".into(), owner(true), 0, Mutation::BindRuntime)
        .await
        .unwrap();
    assert_eq!(bound.runtime_instance_id.as_deref(), Some("r1"));
    assert_eq!(bound.conversation_request_id, state.conversation_request_id);
}

/// sent 与 uncertain 均可接受 exact terminal；terminal 后所有身份冻结。
#[tokio::test]
async fn exact_terminal_from_sent_and_uncertain_freezes_identity() {
    for uncertain in [false, true] {
        let (directory, store) = fixture(true).await;
        let mut state = ready(&store).await;
        let authority = snapshot(&store, AUTHORITY);
        rejected(
            &store,
            owner(true),
            state.revision,
            Mutation::NegotiatedProtocol(2),
        )
        .await;
        rejected(
            &store,
            owner(true),
            state.revision,
            Mutation::ExactSession("conflict".into()),
        )
        .await;
        rejected(
            &store,
            owner(true),
            state.revision,
            Mutation::ExactSession(String::new()),
        )
        .await;
        rejected(
            &store,
            owner(true),
            state.revision,
            Mutation::ExactProviderRequest(String::new()),
        )
        .await;
        rejected(&store, owner(true), state.revision, Mutation::MarkUncertain).await;
        rejected(&store, owner(true), state.revision, terminal(&state)).await;
        state = store
            .mutate(
                "e1".into(),
                owner(true),
                state.revision,
                Mutation::ExactProviderRequest("provider-private".into()),
            )
            .await
            .unwrap();
        rejected(
            &store,
            owner(true),
            state.revision,
            Mutation::ExactProviderRequest("conflict".into()),
        )
        .await;
        state = store
            .mutate(
                "e1".into(),
                owner(true),
                state.revision,
                Mutation::MarkSent {
                    rpc_id: Some(PromptRpcId::from_json("\"rpc-private\"").unwrap()),
                },
            )
            .await
            .unwrap();
        assert_eq!(state.prompt_state, PromptState::Sent);
        rejected(
            &store,
            owner(true),
            state.revision,
            Mutation::MarkSent { rpc_id: None },
        )
        .await;
        if uncertain {
            state = store
                .mutate(
                    "e1".into(),
                    owner(true),
                    state.revision,
                    Mutation::MarkUncertain,
                )
                .await
                .unwrap();
            assert_eq!(state.prompt_state, PromptState::Uncertain);
            rejected(&store, owner(true), state.revision, Mutation::MarkUncertain).await;
        }
        for (session, conversation) in [
            ("wrong".into(), state.conversation_request_id.clone()),
            (state.session_id.clone().unwrap(), "wrong".into()),
        ] {
            rejected(
                &store,
                owner(true),
                state.revision,
                Mutation::ObserveTerminal {
                    session_id: session,
                    conversation_request_id: conversation,
                    stop_reason: StopReason::EndTurn,
                    observed_at: 456,
                },
            )
            .await;
        }
        state = store
            .mutate("e1".into(), owner(true), state.revision, terminal(&state))
            .await
            .unwrap();
        assert_eq!(state.prompt_state, PromptState::TerminalObserved);
        assert_eq!(state.terminal_stop_reason, Some(StopReason::EndTurn));
        assert_eq!(
            state.provider_request_id_source.as_deref(),
            Some("exact_provider_observation")
        );
        for mutation in [
            Mutation::BindRuntime,
            Mutation::NegotiatedProtocol(1),
            Mutation::ExactSession("session-private".into()),
            Mutation::ExactProviderRequest("provider-private".into()),
            Mutation::MarkSent { rpc_id: None },
            Mutation::MarkUncertain,
            Mutation::ObserveTerminal {
                session_id: state.session_id.clone().unwrap(),
                conversation_request_id: state.conversation_request_id.clone(),
                stop_reason: StopReason::Cancelled,
                observed_at: 456,
            },
            Mutation::ObserveTerminal {
                session_id: state.session_id.clone().unwrap(),
                conversation_request_id: state.conversation_request_id.clone(),
                stop_reason: StopReason::EndTurn,
                observed_at: 457,
            },
        ] {
            rejected(&store, owner(true), state.revision, mutation).await;
        }
        let repeated = store
            .mutate("e1".into(), owner(true), state.revision, terminal(&state))
            .await
            .unwrap();
        assert_eq!(repeated.revision, state.revision + 1);
        assert_eq!(
            repeated.conversation_request_id,
            state.conversation_request_id
        );
        assert_eq!(snapshot(&store, AUTHORITY), authority);
        drop(store);
        let reopened = CodeBuddyStore(StateStore::open(directory.path().into()).await.unwrap());
        assert_eq!(reopened.read("e1".into()).await.unwrap(), repeated);
    }
}

/// SDK string/i64 JSON domain 保留原标量类型，其他形状一律拒绝。
#[test]
fn rpc_id_json_domain_roundtrip() {
    for value in [
        "\"rpc-中文\"",
        "\"42\"",
        "\"\"",
        "0",
        "-1",
        "9223372036854775807",
        "-9223372036854775808",
    ] {
        let id = PromptRpcId::from_json(value).unwrap();
        assert_eq!(id.to_json(), value);
        assert_eq!(PromptRpcId::from_json(&id.to_json()).unwrap(), id);
    }
    assert_ne!(
        PromptRpcId::from_json("42").unwrap(),
        PromptRpcId::from_json("\"42\"").unwrap()
    );
    for value in [
        "null",
        "1.5",
        "1.0",
        "true",
        "false",
        "{}",
        "[]",
        "9223372036854775808",
        "-9223372036854775809",
        "1e100",
        "invalid",
    ] {
        assert!(PromptRpcId::from_json(value).is_err(), "{value}");
    }
}

/// 数字与字符串 RPC 身份经过数据库和重启仍保留 JSON 标量类型。
#[tokio::test]
async fn rpc_id_scalar_type_survives_restart() {
    for value in ["\"42\"", "-9223372036854775808", "9223372036854775807"] {
        let (directory, store) = fixture(true).await;
        let state = ready(&store).await;
        let sent = store
            .mutate(
                "e1".into(),
                owner(true),
                state.revision,
                Mutation::MarkSent {
                    rpc_id: Some(PromptRpcId::from_json(value).unwrap()),
                },
            )
            .await
            .unwrap();
        drop(store);
        let reopened = CodeBuddyStore(StateStore::open(directory.path().into()).await.unwrap());
        let persisted = reopened.read("e1".into()).await.unwrap();
        assert_eq!(persisted, sent);
        assert_eq!(persisted.prompt_rpc_id.unwrap().to_json(), value);
    }
}

/// Continue load 保持同一 child Runtime authority，后续 Result Recovery 则切换到独立 R2。
#[tokio::test]
async fn continuation_load_reuses_v13_fields_without_terminal_or_release() {
    let (_directory, store) = fixture(true).await;
    let mut state = ready(&store).await;
    rejected(
        &store,
        owner(true),
        state.revision,
        Mutation::BeginContinuationLoad {
            session_id: "session-private".into(),
            recovery_runtime_instance_id: "r1".into(),
        },
    )
    .await;
    state = store
        .mutate(
            "e1".into(),
            owner(true),
            state.revision,
            Mutation::MarkSent { rpc_id: None },
        )
        .await
        .unwrap();
    let authority = snapshot(&store, AUTHORITY);
    for (session, runtime) in [("wrong-session", "r1"), ("session-private", "r2")] {
        rejected(
            &store,
            owner(true),
            state.revision,
            Mutation::BeginContinuationLoad {
                session_id: session.into(),
                recovery_runtime_instance_id: runtime.into(),
            },
        )
        .await;
    }
    state = store
        .mutate(
            "e1".into(),
            owner(true),
            state.revision,
            Mutation::BeginContinuationLoad {
                session_id: "session-private".into(),
                recovery_runtime_instance_id: "r1".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(state.recovery_state, RecoveryState::Inspecting);
    assert_eq!(
        state.recovery_runtime_instance_id,
        state.runtime_instance_id
    );
    assert_eq!(state.prompt_state, PromptState::Sent);
    state = store
        .mutate(
            "e1".into(),
            owner(true),
            state.revision,
            Mutation::FinishContinuationLoad,
        )
        .await
        .unwrap();
    assert_eq!(state.recovery_state, RecoveryState::Partial);
    assert_eq!(state.recovery_method.as_deref(), Some("session/load"));
    assert!(state.recovery_finished_at.is_some());
    assert!(state.terminal_stop_reason.is_none());
    assert_eq!(snapshot(&store, AUTHORITY), authority);
    assert_eq!(store.read("e1".into()).await.unwrap(), state);
    state = store
        .mutate(
            "e1".into(),
            owner(true),
            state.revision,
            Mutation::BeginInspection {
                recovery_runtime_instance_id: "r2".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(state.recovery_state, RecoveryState::Inspecting);
    assert_eq!(state.runtime_instance_id.as_deref(), Some("r1"));
    assert_eq!(state.recovery_runtime_instance_id.as_deref(), Some("r2"));
    assert_eq!(snapshot(&store, AUTHORITY), authority);
}

/// child private create 不接收 source 参数，因此非空 provider/RPC connection identity 也不能被复制。
#[tokio::test]
async fn continuation_child_private_identity_never_inherits_source_connection_ids() {
    let (_directory, store) = fixture(true).await;
    let mut source = ready(&store).await;
    source = store
        .mutate(
            "e1".into(),
            owner(true),
            source.revision,
            Mutation::ExactProviderRequest("source-provider-request".into()),
        )
        .await
        .unwrap();
    source = store
        .mutate(
            "e1".into(),
            owner(true),
            source.revision,
            Mutation::MarkSent {
                rpc_id: Some(PromptRpcId::from_json("\"source-rpc\"").unwrap()),
            },
        )
        .await
        .unwrap();
    {
        let mut connection = store.0.connection.lock().unwrap();
        insert(&mut connection, "e2", "agent2", "C:/cb-private-2");
        connection
            .execute(
                "UPDATE executions SET provider='codebuddy' WHERE id='e2'",
                [],
            )
            .unwrap();
    }
    let child = store.create("e2".into(), owner(false)).await.unwrap();
    assert_eq!(
        source.provider_request_id.as_deref(),
        Some("source-provider-request")
    );
    assert_eq!(source.prompt_rpc_id.unwrap().to_json(), "\"source-rpc\"");
    assert!(child.provider_request_id.is_none());
    assert!(child.prompt_rpc_id.is_none());
    assert!(child.session_id.is_none());
    assert_ne!(
        child.conversation_request_id,
        source.conversation_request_id
    );
}

/// R2 必须为独立 CodeBuddy runtime；三个检查结果都不改变 generic authority。
#[tokio::test]
async fn inspection_provenance_preserves_r1_and_generic_authority() {
    for (outcome, expected) in [
        (InspectionOutcome::Partial, RecoveryState::Partial),
        (InspectionOutcome::Unknown, RecoveryState::Unknown),
        (
            InspectionOutcome::MaterialDifference,
            RecoveryState::MaterialDifference,
        ),
    ] {
        let (_directory, store) = fixture(true).await;
        let mut state = ready(&store).await;
        rejected(
            &store,
            owner(true),
            state.revision,
            Mutation::BeginInspection {
                recovery_runtime_instance_id: "r2".into(),
            },
        )
        .await;
        state = store
            .mutate(
                "e1".into(),
                owner(true),
                state.revision,
                Mutation::MarkSent { rpc_id: None },
            )
            .await
            .unwrap();
        let authority = snapshot(&store, AUTHORITY);
        rejected(
            &store,
            owner(true),
            state.revision,
            Mutation::FinishInspection { outcome },
        )
        .await;
        for runtime in ["r1", "codex", "missing"] {
            rejected(
                &store,
                owner(true),
                state.revision,
                Mutation::BeginInspection {
                    recovery_runtime_instance_id: runtime.into(),
                },
            )
            .await;
        }
        state = store
            .mutate(
                "e1".into(),
                owner(true),
                state.revision,
                Mutation::BeginInspection {
                    recovery_runtime_instance_id: "r2".into(),
                },
            )
            .await
            .unwrap();
        assert_eq!(state.recovery_state, RecoveryState::Inspecting);
        assert_eq!(state.recovery_method.as_deref(), Some("session/load"));
        assert!(state.recovery_started_at.is_some());
        assert_eq!(state.recovery_finished_at, None);
        rejected(
            &store,
            owner(true),
            state.revision,
            Mutation::BeginInspection {
                recovery_runtime_instance_id: "r2".into(),
            },
        )
        .await;
        state = store
            .mutate(
                "e1".into(),
                owner(true),
                state.revision,
                Mutation::FinishInspection { outcome },
            )
            .await
            .unwrap();
        assert_eq!(state.recovery_state, expected);
        assert!(state.recovery_finished_at.is_some());
        assert_eq!(state.runtime_instance_id.as_deref(), Some("r1"));
        assert_eq!(state.recovery_runtime_instance_id.as_deref(), Some("r2"));
        assert_eq!(state.prompt_state, PromptState::Sent);
        assert_eq!(state.terminal_stop_reason, None);
        rejected(
            &store,
            owner(true),
            state.revision,
            Mutation::FinishInspection { outcome },
        )
        .await;
        assert_eq!(snapshot(&store, AUTHORITY), authority);
        // 已有 R2 provenance 的读写也必须持续校验其 Provider，不能只在 begin 检查。
        store
            .0
            .connection
            .lock()
            .unwrap()
            .execute(
                "UPDATE runtime_instances SET provider='codex' WHERE id='r2'",
                [],
            )
            .unwrap();
        assert!(store.read("e1".into()).await.is_err());
        rejected(&store, owner(true), state.revision, Mutation::MarkUncertain).await;
    }
}

/// partial unique 只限制已知 Session/Conversation pair，未知 Session 可独立预留。
#[tokio::test]
async fn session_conversation_pair_partial_unique() {
    let (_directory, store) = fixture(true).await;
    let state = ready(&store).await;
    {
        let mut c = store.0.connection.lock().unwrap();
        insert(&mut c, "e2", "agent2", "C:/cb-private-2");
        c.execute(
            "UPDATE executions SET provider='codebuddy',runtime_instance_id='r1' WHERE id='e2'",
            [],
        )
        .unwrap();
    }
    let second = store.create("e2".into(), owner(true)).await.unwrap();
    assert_ne!(
        state.conversation_request_id,
        second.conversation_request_id
    );
    {
        let c = store.0.connection.lock().unwrap();
        c.execute("UPDATE codebuddy_execution_state SET conversation_request_id=?1,acp_protocol_version=1 WHERE execution_id='e2'", [&state.conversation_request_id]).unwrap();
    }
    let before = snapshot(&store, ALL);
    assert!(
        store
            .mutate(
                "e2".into(),
                owner(true),
                0,
                Mutation::ExactSession("session-private".into())
            )
            .await
            .is_err()
    );
    assert_eq!(snapshot(&store, ALL), before);
    let other = store
        .mutate(
            "e2".into(),
            owner(true),
            0,
            Mutation::ExactSession("other-session".into()),
        )
        .await
        .unwrap();
    assert_eq!(other.conversation_request_id, state.conversation_request_id);
}

/// 历史缺失与绕过 SQL CHECK 的损坏记录不能被读写或静默修复。
#[tokio::test]
async fn missing_and_corrupt_private_state_fail_closed() {
    let (_directory, store) = fixture(true).await;
    assert!(store.read("e1".into()).await.is_err());
    rejected(&store, owner(true), 0, Mutation::MarkSent { rpc_id: None }).await;
    for assignment in [
        "conversation_request_id='bad'",
        "revision=-1",
        "prompt_state='unknown'",
        "prompt_rpc_id='null'",
        "prompt_rpc_id='9223372036854775808'",
        "session_id=''",
        "acp_protocol_version=65536",
        "provider_request_id='orphan'",
        "terminal_stop_reason='end_turn'",
        "recovery_state='partial'",
        "runtime_instance_id='r2'",
    ] {
        let (_directory, store) = fixture(true).await;
        let state = ready(&store).await;
        {
            let c = store.0.connection.lock().unwrap();
            c.pragma_update(None, "ignore_check_constraints", true)
                .unwrap();
            c.execute(
                &format!(
                    "UPDATE codebuddy_execution_state SET {assignment} WHERE execution_id='e1'"
                ),
                [],
            )
            .unwrap();
            c.pragma_update(None, "ignore_check_constraints", false)
                .unwrap();
        }
        assert!(store.read("e1".into()).await.is_err(), "{assignment}");
        rejected(
            &store,
            owner(true),
            state.revision,
            Mutation::MarkSent { rpc_id: None },
        )
        .await;
    }
}

/// CB8：原子 response 提交仅容忍同一 owner 的 generic cancel；private OCC/Runtime/terminal/SQL 失败仍回滚。
#[tokio::test]
async fn atomic_prompt_response_cancel_and_conflict_matrix() {
    use crate::agent::execution::state::{Status, Transition};
    for fault in [
        "cancel",
        "private",
        "runtime",
        "private_terminal",
        "generic_terminal",
        "reconciling",
        "sql",
    ] {
        let (_dir, store) = fixture(true).await;
        let prepared = ready(&store).await;
        let mut expected = store
            .mutate(
                "e1".into(),
                owner(true),
                prepared.revision,
                Mutation::MarkSent { rpc_id: None },
            )
            .await
            .unwrap();
        store
            .0
            .connection
            .lock()
            .unwrap()
            .execute(
                "UPDATE executions SET status='running',dispatch_state='dispatched' WHERE id='e1'",
                [],
            )
            .unwrap();
        match fault {
            "cancel" => {
                store.0.request_cancel("e1".into(), 400).await.unwrap();
            }
            "private" => {
                expected.session_id = Some("stale-session".into());
            }
            "runtime" => {
                // R1 在数据库中不可重绑；注入错误调用方快照而不移除安全 trigger。
                expected.runtime_instance_id = Some("r2".into());
            }
            "private_terminal" => {
                store
                    .mutate(
                        "e1".into(),
                        owner(true),
                        expected.revision,
                        terminal(&expected),
                    )
                    .await
                    .unwrap();
            }
            "generic_terminal" => {
                store
                    .0
                    .provider_event(
                        "e1".into(),
                        Transition::ProviderTerminal {
                            runtime_id: "r1".into(),
                            status: Status::Completed,
                        },
                        400,
                    )
                    .await
                    .unwrap();
            }
            "reconciling" => {
                store
                    .0
                    .provider_event("e1".into(), Transition::Reconcile, 400)
                    .await
                    .unwrap();
            }
            "sql" => {
                store.0.connection.lock().unwrap().execute_batch("CREATE TRIGGER reject_terminal BEFORE UPDATE ON codebuddy_execution_state WHEN NEW.prompt_state='terminal_observed' BEGIN SELECT RAISE(ABORT,'terminal fault'); END;").unwrap();
            }
            _ => unreachable!(),
        }
        let before = snapshot(&store, ALL);
        let authority = snapshot(&store, AUTHORITY);
        let result = store
            .observe_prompt_response(
                expected,
                Some("exact-provider-request".into()),
                StopReason::EndTurn,
                456,
            )
            .await;
        if fault == "cancel" {
            let state = result.unwrap();
            assert_eq!(state.prompt_state, PromptState::TerminalObserved);
            assert_eq!(state.terminal_stop_reason, Some(StopReason::EndTurn));
            assert_eq!(
                state.provider_request_id.as_deref(),
                Some("exact-provider-request")
            );
            assert_eq!(snapshot(&store, AUTHORITY), authority);
        } else {
            assert!(result.is_err(), "{fault}");
            assert_eq!(snapshot(&store, ALL), before, "{fault}");
        }
    }
}
