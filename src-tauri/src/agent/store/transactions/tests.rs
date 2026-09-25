//! SQLite fixtures are simulated evidence, never Windows/Provider contract evidence.
use super::*;
use crate::agent::execution::{CreateExecutionInput, canonicalize_request};
use std::sync::{Arc, Barrier};

fn block<T>(future: impl std::future::Future<Output = T>) -> T {
    tauri::async_runtime::block_on(future)
}
fn open(path: &std::path::Path) -> StateStore {
    block(StateStore::open(path.to_owned())).unwrap()
}
fn request(agent: &str, key: &str, root: &str) -> CanonicalRequest {
    canonicalize_request(
        serde_json::from_value::<CreateExecutionInput>(json!({
            "agent_id":agent,"request_key":key,"prompt":"payload","execution_profile":{},
            "workspace_id":"w","canonical_workspace_root":root,"mode":"workspace_write"
        }))
        .unwrap(),
    )
    .unwrap()
}
fn create_one(s: &StateStore) {
    block(s.create_execution("e".into(), request("a", "k", "root"), 1)).unwrap();
}
fn fixture(s: &StateStore, status: Status, dispatch: DispatchState) {
    create_one(s);
    let c = s.connection.lock().unwrap();
    c.execute("INSERT INTO runtime_instances (id,owner_host_instance_id,state,created_at,updated_at) VALUES ('r','host','running',1,1)",[]).unwrap();
    c.execute(
        "UPDATE executions SET status=?1,dispatch_state=?2,runtime_instance_id='r' WHERE id='e'",
        params![status.as_str(), dispatch.as_str()],
    )
    .unwrap();
}
fn event(s: &StateStore, event: Transition, now: i64) -> Result<(), String> {
    let revision = block(s.execution("e".into())).unwrap().unwrap().revision;
    block(s.transition_execution("e".into(), revision, event, now))
}
fn status(s: &StateStore) -> ExecutionRecord {
    block(s.execution("e".into())).unwrap().unwrap()
}
/// 读取 Store 内部 history，用于断言权威持久化结果而非 Product wire。
fn history(s: &StateStore, after_sequence: Option<i64>, limit: Option<i64>) -> ActivityHistoryPage {
    block(s.execution_activity_history("e".into(), after_sequence, limit)).unwrap()
}
fn safe_cleanup(s: &StateStore) {
    event(
        s,
        Transition::ProviderTerminal {
            runtime_id: "r".into(),
            status: Status::Completed,
        },
        2,
    )
    .unwrap();
    event(
        s,
        Transition::CleanupEmpty {
            runtime_id: "r".into(),
        },
        3,
    )
    .unwrap();
}
fn finalization() -> Finalization {
    Finalization {
        terminal: Status::Completed,
        basis: ReleaseBasis::SameRuntimeCleanup,
        result: Some(json!({"answer":"done"})),
        completeness: ResultCompleteness::Complete,
    }
}
fn finish(s: &StateStore) -> Result<(), String> {
    block(s.finalize_and_release_execution("e".into(), status(s).revision, finalization(), 4))
}

/// 首次绑定必须在同一写事务读取双方 Provider；拒绝后不消耗 Claim 或 Runtime。
#[test]
fn first_bind_rejects_runtime_provider_mismatch_without_side_effects() {
    for (execution_provider, runtime_provider) in [("codex", "codebuddy"), ("codebuddy", "codex")] {
        let dir = tempfile::tempdir().unwrap();
        let store = open(dir.path());
        let input: CreateExecutionInput = serde_json::from_value(json!({
            "agent_id":"a","request_key":"k","prompt":"payload","execution_profile":{},
            "workspace_id":"w","canonical_workspace_root":"root","mode":"workspace_write",
            "provider":execution_provider
        }))
        .unwrap();
        block(store.create_execution("e".into(), canonicalize_request(input).unwrap(), 1)).unwrap();
        store.connection.lock().unwrap().execute(
            "INSERT INTO runtime_instances (id,owner_host_instance_id,provider,state,created_at,updated_at)
             VALUES ('r','host',?1,'running',1,1)",
            [runtime_provider],
        ).unwrap();
        let before = execution_snapshot(&store);
        assert_eq!(
            event(
                &store,
                Transition::Dispatch {
                    to: DispatchState::Dispatching,
                    runtime_id: Some("r".into()),
                },
                2
            )
            .unwrap_err(),
            "RUNTIME_PROVIDER_MISMATCH"
        );
        assert_eq!(execution_snapshot(&store), before);
        assert!(
            block(store.workspace_claim("root".into()))
                .unwrap()
                .is_some()
        );
        assert_eq!(
            store
                .connection
                .lock()
                .unwrap()
                .query_row(
                    "SELECT state FROM runtime_instances WHERE id='r'",
                    [],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            "running"
        );
    }
}

/// Provider terminal 与 cleanup 都只能引用同 Provider Runtime，拒绝时不写新证据。
#[test]
fn provider_terminal_and_cleanup_reject_provider_mismatch() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    fixture(&store, Status::Running, DispatchState::Dispatched);
    store
        .connection
        .lock()
        .unwrap()
        .execute(
            "UPDATE runtime_instances SET provider='codebuddy' WHERE id='r'",
            [],
        )
        .unwrap();
    let before = execution_snapshot(&store);
    assert_eq!(
        event(
            &store,
            Transition::ProviderTerminal {
                runtime_id: "r".into(),
                status: Status::Completed,
            },
            2
        )
        .unwrap_err(),
        "RUNTIME_PROVIDER_MISMATCH"
    );
    assert_eq!(execution_snapshot(&store), before);
    assert_eq!(
        status(&store).provider_terminal_evidence_runtime_instance_id,
        None
    );
    assert!(
        block(store.workspace_claim("root".into()))
            .unwrap()
            .is_some()
    );

    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    fixture(&store, Status::Running, DispatchState::Dispatched);
    event(
        &store,
        Transition::ProviderTerminal {
            runtime_id: "r".into(),
            status: Status::Completed,
        },
        2,
    )
    .unwrap();
    store
        .connection
        .lock()
        .unwrap()
        .execute(
            "UPDATE runtime_instances SET provider='codebuddy' WHERE id='r'",
            [],
        )
        .unwrap();
    let before = execution_snapshot(&store);
    assert_eq!(
        event(
            &store,
            Transition::CleanupEmpty {
                runtime_id: "r".into(),
            },
            3
        )
        .unwrap_err(),
        "RUNTIME_PROVIDER_MISMATCH"
    );
    assert_eq!(execution_snapshot(&store), before);
    assert!(
        block(store.workspace_claim("root".into()))
            .unwrap()
            .is_some()
    );
}

/// 恢复证据与两种 ReleaseBasis 均重新读取 Runtime provider，失配不能释放 Claim。
#[test]
fn recovery_and_finalization_reject_provider_mismatch_and_retain_claim() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    fixture(&store, Status::Unknown, DispatchState::Uncertain);
    store.connection.lock().unwrap().execute(
        "UPDATE runtime_instances SET provider='codebuddy',state='terminated',
         termination_evidence_state='complete',termination_evidence_type='job_active_processes_zero',
         termination_evidence_at=2 WHERE id='r'", []
    ).unwrap();
    let before = execution_snapshot(&store);
    assert_eq!(
        event(
            &store,
            Transition::ResumeRecovery(RecoveryBasis::RuntimeTermination {
                runtime_id: "r".into(),
                evidence_at: 2
            }),
            3
        )
        .unwrap_err(),
        "RUNTIME_PROVIDER_MISMATCH"
    );
    assert_eq!(execution_snapshot(&store), before);
    assert!(
        block(store.workspace_claim("root".into()))
            .unwrap()
            .is_some()
    );

    store
        .connection
        .lock()
        .unwrap()
        .execute(
            "UPDATE executions SET status='reconciling' WHERE id='e'",
            [],
        )
        .unwrap();
    let before = execution_snapshot(&store);
    let release = Finalization {
        terminal: Status::Interrupted,
        basis: ReleaseBasis::RuntimeTerminated,
        result: None,
        completeness: ResultCompleteness::Unknown,
    };
    assert_eq!(
        block(store.finalize_and_release_execution(
            "e".into(),
            status(&store).revision,
            release,
            4
        ))
        .unwrap_err(),
        "RUNTIME_PROVIDER_MISMATCH"
    );
    assert_eq!(execution_snapshot(&store), before);
    assert!(
        block(store.workspace_claim("root".into()))
            .unwrap()
            .is_some()
    );
}

/// 已存在同 Runtime cleanup 证据也不绕过 finalize 时的持久化 Provider 复核。
#[test]
fn same_runtime_cleanup_mismatch_cannot_release_claim() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    fixture(&store, Status::Running, DispatchState::Dispatched);
    safe_cleanup(&store);
    store
        .connection
        .lock()
        .unwrap()
        .execute(
            "UPDATE runtime_instances SET provider='codebuddy' WHERE id='r'",
            [],
        )
        .unwrap();
    let before = execution_snapshot(&store);
    assert_eq!(finish(&store).unwrap_err(), "RUNTIME_PROVIDER_MISMATCH");
    assert_eq!(execution_snapshot(&store), before);
    assert!(
        block(store.workspace_claim("root".into()))
            .unwrap()
            .is_some()
    );
    assert_eq!(status(&store).release_evidence_state, "incomplete");
}

/// 启动扫描先把跨 Provider 绑定降为 unknown；旧的 complete release 字段也不能删 Claim。
#[test]
fn startup_claim_scan_keeps_mismatched_runtime_and_claim_fail_closed() {
    for terminal in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let store = open(dir.path());
        fixture(
            &store,
            if terminal {
                Status::Completed
            } else {
                Status::Running
            },
            DispatchState::Dispatched,
        );
        store
            .connection
            .lock()
            .unwrap()
            .execute(
                "UPDATE runtime_instances SET provider='codebuddy' WHERE id='r'",
                [],
            )
            .unwrap();
        if terminal {
            store.connection.lock().unwrap().execute(
                "UPDATE executions SET release_evidence_state='complete',
                 release_evidence_kind='same_runtime_cleanup',release_evidence_json='{}' WHERE id='e'", []
            ).unwrap();
        }
        let outcome = block(store.recover_claims(3)).unwrap();
        assert!(matches!(
            outcome.as_slice(),
            [ClaimRecovery::Inconsistent {
                code: "RUNTIME_PROVIDER_MISMATCH",
                ..
            }]
        ));
        assert_eq!(
            status(&store).status,
            if terminal { "completed" } else { "unknown" }
        );
        assert!(
            block(store.workspace_claim("root".into()))
                .unwrap()
                .is_some()
        );
    }
}

const STATUSES: [Status; 11] = [
    Status::DispatchPending,
    Status::Running,
    Status::CancelRequested,
    Status::Cancelling,
    Status::Finalizing,
    Status::Reconciling,
    Status::Completed,
    Status::Failed,
    Status::Cancelled,
    Status::Interrupted,
    Status::Unknown,
];
const DISPATCH: [DispatchState; 4] = [
    DispatchState::NotDispatched,
    DispatchState::Dispatching,
    DispatchState::Dispatched,
    DispatchState::Uncertain,
];

#[test]
fn exact_11_by_11_matrix() {
    // Independent literal transcription of section 21, including the cancel exception.
    let edges = [
        (0, 1),
        (1, 2),
        (2, 3),
        (0, 4),
        (1, 4),
        (2, 4),
        (3, 4),
        (0, 5),
        (1, 5),
        (2, 5),
        (3, 5),
        (4, 5),
        (4, 6),
        (4, 7),
        (4, 8),
        (4, 9),
        (5, 6),
        (5, 7),
        (5, 8),
        (5, 9),
        (5, 10),
        (10, 5),
        (0, 8),
    ];
    let mut count = 0;
    for (i, from) in STATUSES.iter().enumerate() {
        for (j, to) in STATUSES.iter().enumerate() {
            let expected = edges.contains(&(i, j));
            assert_eq!(from.allows(*to), expected, "{from:?}->{to:?}");
            count += usize::from(expected);
        }
    }
    assert_eq!(count, 23);
}

#[test]
fn all_dispatch_edges_through_real_transactions() {
    for (i, from) in DISPATCH.iter().enumerate() {
        for (j, to) in DISPATCH.iter().enumerate() {
            let dir = tempfile::tempdir().unwrap();
            let s = open(dir.path());
            fixture(&s, Status::DispatchPending, *from);
            if *from == DispatchState::NotDispatched {
                s.connection
                    .lock()
                    .unwrap()
                    .execute("UPDATE executions SET runtime_instance_id=NULL", [])
                    .unwrap_err();
                // Binding is immutable even in fixtures: use a fresh unbound execution instead.
            }
            let dir2 = tempfile::tempdir().unwrap();
            let fresh;
            let store = if *from == DispatchState::NotDispatched {
                fresh = open(dir2.path());
                create_one(&fresh);
                fresh.connection.lock().unwrap().execute("INSERT INTO runtime_instances (id,owner_host_instance_id,state,created_at,updated_at) VALUES ('r','h','running',1,1)",[]).unwrap();
                &fresh
            } else {
                &s
            };
            let expected = [(0, 1), (1, 2), (1, 3)].contains(&(i, j));
            assert_eq!(from.allows(*to), expected);
            let result = event(
                store,
                Transition::Dispatch {
                    to: *to,
                    runtime_id: if *to == DispatchState::Dispatching {
                        Some("r".into())
                    } else {
                        None
                    },
                },
                2,
            );
            assert_eq!(result.is_ok(), expected, "{from:?}->{to:?}: {result:?}");
            assert_eq!(
                status(store).dispatch_state,
                if expected { to.as_str() } else { from.as_str() }
            );
        }
    }
}

#[test]
fn status_edges_execute_and_forbidden_edges_reject() {
    // Diagnostic ACK no-ops are not state transitions; verify all destination pairs
    // against both the graph and externally visible persisted status.
    for from in STATUSES {
        for to in STATUSES {
            let dir = tempfile::tempdir().unwrap();
            let s = open(dir.path());
            if from == Status::DispatchPending && to == Status::Cancelled {
                create_one(&s);
            } else {
                fixture(&s, from, DispatchState::Dispatched);
            }
            if matches!(
                to,
                Status::Completed | Status::Failed | Status::Cancelled | Status::Interrupted
            ) && from != Status::DispatchPending
            {
                let c = s.connection.lock().unwrap();
                c.execute("UPDATE executions SET provider_terminal_status='completed',provider_terminal_evidence_runtime_instance_id='r',provider_terminal_evidence_at=1,background_cleanup_state='empty',background_cleanup_runtime_instance_id='r',background_cleanup_evidence_at=1",[]).unwrap();
            }
            let result = match to {
                Status::DispatchPending => Err("NO_TRANSITION_TO_DISPATCH_PENDING".into()),
                Status::Running => event(&s, Transition::Running, 3),
                Status::CancelRequested => event(&s, Transition::RequestCancel, 3),
                Status::Cancelling => event(&s, Transition::InterruptAck, 3),
                Status::Finalizing => event(
                    &s,
                    Transition::ProviderTerminal {
                        runtime_id: "r".into(),
                        status: Status::Completed,
                    },
                    3,
                ),
                Status::Reconciling if from == Status::Unknown => event(
                    &s,
                    Transition::ResumeRecovery(RecoveryBasis::LocalResolve {
                        diagnostic: "local audit".into(),
                    }),
                    3,
                ),
                Status::Reconciling => event(&s, Transition::Reconcile, 3),
                Status::Unknown => event(&s, Transition::MarkUnknown, 3),
                Status::Cancelled if from == Status::DispatchPending => {
                    block(s.cancel_before_dispatch_and_release("e".into(), 0, 3))
                }
                terminal => block(s.finalize_and_release_execution(
                    "e".into(),
                    0,
                    Finalization {
                        terminal,
                        ..finalization()
                    },
                    3,
                )),
            };
            let changed = status(&s).status != from.as_str();
            assert_eq!(changed, from.allows(to), "{from:?}->{to:?}, {result:?}");
            if from.allows(to) {
                assert!(result.is_ok(), "{from:?}->{to:?}: {result:?}");
            }
        }
    }
}

#[test]
fn terminal_evidence_closes_running_stage_and_late_cancel_only_adds_diagnostics() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(dir.path());
    fixture(&s, Status::CancelRequested, DispatchState::Dispatched);
    safe_cleanup(&s);
    for e in [Transition::Running, Transition::RequestCancel] {
        assert!(event(&s, e, 4).is_err());
    }
    event(&s, Transition::InterruptAck, 5).unwrap();
    event(
        &s,
        Transition::InterruptTimeout {
            diagnostic: "late".into(),
        },
        6,
    )
    .unwrap();
    assert_eq!(status(&s).status, "finalizing");
    assert!(
        event(
            &s,
            Transition::ProviderTerminal {
                runtime_id: "r".into(),
                status: Status::Failed
            },
            7
        )
        .is_err()
    );
    finish(&s).unwrap();
    for e in [
        Transition::Running,
        Transition::RequestCancel,
        Transition::Reconcile,
        Transition::MarkUnknown,
    ] {
        assert!(event(&s, e, 8).is_err());
    }
    event(&s, Transition::InterruptAck, 9).unwrap();
    event(
        &s,
        Transition::InterruptTimeout {
            diagnostic: "later".into(),
        },
        10,
    )
    .unwrap();
    assert_eq!(status(&s).status, "completed");
    let c = s.connection.lock().unwrap();
    let data: (String, i64, i64) = c
        .query_row(
            "SELECT provider_terminal_status,interrupt_ack_at,interrupt_timeout_at FROM executions",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(data, ("completed".into(), 5, 6));
}

#[test]
fn provider_terminal_before_start_ack_and_cross_runtime_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(dir.path());
    fixture(&s, Status::DispatchPending, DispatchState::Dispatching);
    assert!(
        event(
            &s,
            Transition::ProviderTerminal {
                runtime_id: "other".into(),
                status: Status::Completed
            },
            2
        )
        .is_err()
    );
    safe_cleanup(&s);
    assert_eq!(status(&s).status, "finalizing");
    assert!(event(&s, Transition::Running, 4).is_err());
    assert!(
        event(
            &s,
            Transition::CleanupEmpty {
                runtime_id: "other".into()
            },
            4
        )
        .is_err()
    );
}

#[test]
fn unknown_needs_new_persisted_evidence_or_local_resolve_and_never_polling() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(dir.path());
    fixture(&s, Status::Unknown, DispatchState::Uncertain);
    assert!(event(&s, Transition::Reconcile, 2).is_err());
    assert!(
        event(
            &s,
            Transition::ResumeRecovery(RecoveryBasis::RuntimeTermination {
                runtime_id: "r".into(),
                evidence_at: 3
            }),
            3
        )
        .is_err()
    );
    s.connection.lock().unwrap().execute("UPDATE runtime_instances SET state='terminated',termination_evidence_state='complete',termination_evidence_type='job_active_processes_zero',termination_evidence_at=3",[]).unwrap();
    assert!(
        event(
            &s,
            Transition::ResumeRecovery(RecoveryBasis::RuntimeTermination {
                runtime_id: "r".into(),
                evidence_at: 2
            }),
            3
        )
        .is_err()
    );
    event(
        &s,
        Transition::ResumeRecovery(RecoveryBasis::RuntimeTermination {
            runtime_id: "r".into(),
            evidence_at: 3,
        }),
        4,
    )
    .unwrap();
    assert_eq!(status(&s).status, "reconciling");
    assert!(block(s.workspace_claim("root".into())).unwrap().is_some());
}

/// 验证 RuntimeTermination gate 只接受与平台元数据匹配的完整终止证据。
#[test]
fn runtime_termination_evidence_gate_is_platform_conditional() {
    let directory = tempfile::tempdir().unwrap();
    let store = open(directory.path());
    let mut connection = store.connection.lock().unwrap();
    connection
        .execute(
            "INSERT INTO runtime_instances(
                id,owner_host_instance_id,state,created_at,updated_at,
                runtime_platform,containment_type,process_identity_scheme,
                process_id,process_start_token,containment_process_group_id,
                containment_session_id,containment_verified_at,
                stopped_at,termination_evidence_type,termination_evidence_at,
                termination_evidence_state)
             VALUES('mac','host','terminated',1,8,'macos','macos_process_group',
                    'darwin_proc_bsd_start_v1',50,'darwin_proc_bsd_start_v1:1:2',50,50,2,
                    8,'macos_recovered_process_group_empty',8,'complete')",
            [],
        )
        .unwrap();
    let transaction = connection.transaction().unwrap();
    assert_eq!(terminated_runtime(&transaction, "mac", "codex").unwrap(), 8);
    transaction.rollback().unwrap();

    // 模拟损坏数据库，确认 release gate 不依赖 schema trigger 作为唯一防线。
    connection
        .execute_batch(
            "DROP TRIGGER runtime_instances_v10_validate_insert;
             DROP TRIGGER runtime_instances_v10_validate_update;
             UPDATE runtime_instances SET containment_type='windows_job' WHERE id='mac';",
        )
        .unwrap();
    let transaction = connection.transaction().unwrap();
    assert_eq!(
        terminated_runtime(&transaction, "mac", "codex").unwrap_err(),
        "RUNTIME_TERMINATION_EVIDENCE_REQUIRED"
    );
}

#[test]
fn idempotency_precedes_busy_and_different_payload_conflicts() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(dir.path());
    create_one(&s);
    let same =
        block(s.create_execution("ignored-new-id".into(), request("a", "k", "root"), 2)).unwrap();
    assert!(!same.created);
    assert_eq!(same.execution_id, "e");
    assert_eq!(same.execution.id, "e");
    let mut other = request("a", "k", "root").input().clone();
    other.prompt = "different".into();
    assert_eq!(
        block(s.create_execution("new".into(), canonicalize_request(other).unwrap(), 2))
            .unwrap_err(),
        "EXECUTION_REQUEST_KEY_CONFLICT"
    );
    assert_eq!(
        block(s.create_execution("new".into(), request("a", "k2", "other"), 2)).unwrap_err(),
        "AGENT_BUSY"
    );
    s.connection
        .lock()
        .unwrap()
        .execute("UPDATE executions SET status='unknown'", [])
        .unwrap();
    assert_eq!(
        block(s.create_execution("new".into(), request("a", "k2", "other"), 2)).unwrap_err(),
        "AGENT_BUSY"
    );
    assert!(
        !block(s.create_execution("new".into(), request("a", "k", "root"), 2))
            .unwrap()
            .created
    );
}

/// v2 历史行仅接受完整身份相同的 General 重试，且绝不改写持久化 hash。
#[test]
fn historical_v2_general_retry_is_bounded_and_preserves_hash() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    create_one(&store);
    let current = request("a", "k", "root");
    let historical = crate::agent::execution::legacy_v2_request_hash(current.input()).unwrap();
    store
        .connection
        .lock()
        .unwrap()
        .execute(
            "UPDATE executions SET request_hash=?1 WHERE id='e'",
            [&historical],
        )
        .unwrap();

    let retry = block(store.create_execution("unused".into(), current, 2)).unwrap();
    assert!(!retry.created);
    assert_eq!(retry.execution_id, "e");
    assert_eq!(retry.execution.request_hash, historical);

    let original = request("a", "k", "root").input().clone();
    let mut variants = Vec::new();
    let mut role = original.clone();
    role.task_role = crate::agent::execution::AgentTaskRole::Testing;
    variants.push(role);
    let mut provider = original.clone();
    provider.provider = crate::agent::provider::ProviderId::new("codebuddy".into()).unwrap();
    variants.push(provider);
    let mut generation = original.clone();
    generation.workspace_generation = 2;
    variants.push(generation);
    let mut mode = original.clone();
    mode.mode = crate::agent::execution::ExecutionMode::ReadOnly;
    variants.push(mode);
    let mut parent = original;
    parent.parent_execution_id = Some("different-parent".into());
    variants.push(parent);
    for input in variants {
        assert_eq!(
            block(store.create_execution("unused".into(), canonicalize_request(input).unwrap(), 3))
                .unwrap_err(),
            "EXECUTION_REQUEST_KEY_CONFLICT"
        );
    }
    let connection = store.connection.lock().unwrap();
    let (hash, count, status): (String, i64, String) = connection.query_row(
        "SELECT request_hash, (SELECT COUNT(*) FROM executions WHERE agent_id='a' AND request_key='k'), status FROM executions WHERE id='e'",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    ).unwrap();
    assert_eq!(hash, historical);
    assert_eq!(count, 1);
    assert_eq!(status, "dispatch_pending");

    connection
        .execute("UPDATE executions SET task_role='testing' WHERE id='e'", [])
        .unwrap();
    drop(connection);
    assert_eq!(
        block(store.create_execution("unused".into(), request("a", "k", "root"), 4)).unwrap_err(),
        "EXECUTION_REQUEST_KEY_CONFLICT"
    );
}

/// v3 exact hash 命中也不能掩盖持久化 Provider 或角色字段漂移。
#[test]
fn v3_retry_rejects_persisted_identity_drift() {
    for column in ["provider='codebuddy'", "task_role='testing'"] {
        let dir = tempfile::tempdir().unwrap();
        let store = open(dir.path());
        create_one(&store);
        store
            .connection
            .lock()
            .unwrap()
            .execute(&format!("UPDATE executions SET {column} WHERE id='e'"), [])
            .unwrap();
        assert_eq!(
            block(store.create_execution("unused".into(), request("a", "k", "root"), 2))
                .unwrap_err(),
            "EXECUTION_REQUEST_KEY_CONFLICT"
        );
        assert_eq!(status(&store).status, "dispatch_pending");
    }
}

/// v3 的每个冻结身份维度变化都必须让同一 requestKey 稳定冲突。
#[test]
fn v3_request_key_conflicts_on_every_frozen_identity_change() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    create_one(&store);
    let original = request("a", "k", "root");
    assert!(
        std::str::from_utf8(original.bytes())
            .unwrap()
            .starts_with("[\"execution-request-v3\"")
    );
    let retry = block(store.create_execution("unused".into(), original.clone(), 2)).unwrap();
    assert!(!retry.created);
    assert_eq!(retry.execution_id, "e");
    let mut variants = Vec::new();
    let mut input = original.input().clone();
    input.provider = crate::agent::provider::ProviderId::new("codebuddy".into()).unwrap();
    variants.push(input);
    let mut input = original.input().clone();
    input.task_role = crate::agent::execution::AgentTaskRole::Testing;
    variants.push(input);
    let mut input = original.input().clone();
    input.prompt = "changed".into();
    variants.push(input);
    let mut input = original.input().clone();
    input.mode = crate::agent::execution::ExecutionMode::ReadOnly;
    variants.push(input);
    let mut input = original.input().clone();
    input.workspace_id = "other-workspace".into();
    variants.push(input);
    let mut input = original.input().clone();
    input.canonical_workspace_root = "other-root".into();
    variants.push(input);
    let mut input = original.input().clone();
    input.workspace_generation = 2;
    variants.push(input);
    let mut input = original.input().clone();
    input.parent_execution_id = Some("other-parent".into());
    variants.push(input);
    let mut input = original.input().clone();
    input.execution_profile = json!({"different":true});
    variants.push(input);
    for input in variants {
        assert_eq!(
            block(store.create_execution("unused".into(), canonicalize_request(input).unwrap(), 3))
                .unwrap_err(),
            "EXECUTION_REQUEST_KEY_CONFLICT"
        );
    }
    assert_eq!(status(&store).request_hash, original.request_hash());
    assert_eq!(
        store
            .connection
            .lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM executions", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
}

/// unknown Agent 的序列化门槛不能被历史 v2 兼容路径绕过。
#[test]
fn historical_v2_retry_does_not_bypass_unknown_agent_serialization() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    create_one(&store);
    let original = request("a", "k", "root");
    let historical = crate::agent::execution::legacy_v2_request_hash(original.input()).unwrap();
    store
        .connection
        .lock()
        .unwrap()
        .execute(
            "UPDATE executions SET request_hash=?1,status='unknown' WHERE id='e'",
            [&historical],
        )
        .unwrap();
    let retry = block(store.create_execution("unused".into(), original, 2)).unwrap();
    assert!(!retry.created);
    assert_eq!(retry.execution_id, "e");
    assert_eq!(retry.execution.request_hash, historical);
    assert_eq!(
        block(store.create_execution("new".into(), request("a", "other-key", "root"), 3))
            .unwrap_err(),
        "AGENT_BUSY"
    );
    assert!(
        block(store.workspace_claim("root".into()))
            .unwrap()
            .is_some()
    );
}

#[test]
fn frozen_agent_snapshot_and_canonical_workspace_exclusivity() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(dir.path());
    create_one(&s);
    assert_eq!(
        block(s.create_execution("other".into(), request("b", "k", "root"), 2)).unwrap_err(),
        "WORKSPACE_CLAIM_CONFLICT"
    );
    block(s.cancel_before_dispatch_and_release("e".into(), 0, 2)).unwrap();
    for field in [
        "workspace_id",
        "canonical_workspace_root",
        "thread_id",
        "execution_profile",
        "mode",
    ] {
        let mut value = json!({"agent_id":"a","request_key":"next","prompt":"p","execution_profile":{},"workspace_id":"w","canonical_workspace_root":"root","mode":"workspace_write"});
        value[field] = if field == "execution_profile" {
            json!({"different":true})
        } else if field == "mode" {
            json!("read_only")
        } else {
            json!("different")
        };
        let req = canonicalize_request(serde_json::from_value(value).unwrap()).unwrap();
        assert_eq!(
            block(s.create_execution("next".into(), req, 3)).unwrap_err(),
            "AGENT_SNAPSHOT_CONFLICT"
        );
    }
    assert!(
        block(s.create_execution("next".into(), request("a", "next", "root"), 3))
            .unwrap()
            .created
    );
}

#[test]
fn concurrent_same_key_and_agent_and_workspace_use_separate_connections() {
    for scenario in 0..3 {
        let dir = tempfile::tempdir().unwrap();
        let a = open(dir.path());
        let b = open(dir.path());
        let barrier = Arc::new(Barrier::new(2));
        let handles: Vec<_> = [a, b]
            .into_iter()
            .enumerate()
            .map(|(i, s)| {
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    block(s.create_execution(
                        format!("e{i}"),
                        request(
                            if scenario == 2 && i == 1 { "b" } else { "a" },
                            if scenario == 1 && i == 1 { "k2" } else { "k" },
                            "root",
                        ),
                        1,
                    ))
                })
            })
            .collect();
        let outcomes: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        if scenario == 0 {
            assert_eq!(
                outcomes
                    .iter()
                    .filter(|r| r.as_ref().unwrap().created)
                    .count(),
                1
            );
            assert_eq!(
                outcomes[0].as_ref().unwrap().execution_id,
                outcomes[1].as_ref().unwrap().execution_id
            );
        } else {
            assert_eq!(outcomes.iter().filter(|r| r.is_ok()).count(), 1);
        }
        let s = open(dir.path());
        assert_eq!(
            s.connection
                .lock()
                .unwrap()
                .query_row("SELECT count(*) FROM executions", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }
}

#[test]
fn cancel_dispatch_race_preserves_winning_transaction() {
    for _ in 0..8 {
        let dir = tempfile::tempdir().unwrap();
        let s = open(dir.path());
        create_one(&s);
        s.connection.lock().unwrap().execute("INSERT INTO runtime_instances (id,owner_host_instance_id,state,created_at,updated_at) VALUES ('r','h','running',1,1)",[]).unwrap();
        let a = open(dir.path());
        let b = open(dir.path());
        let barrier = Arc::new(Barrier::new(2));
        let other = barrier.clone();
        let cancel = std::thread::spawn(move || {
            barrier.wait();
            block(a.cancel_before_dispatch_and_release("e".into(), 0, 2))
        });
        let dispatch = std::thread::spawn(move || {
            other.wait();
            block(b.transition_execution(
                "e".into(),
                0,
                Transition::Dispatch {
                    to: DispatchState::Dispatching,
                    runtime_id: Some("r".into()),
                },
                2,
            ))
        });
        let cancelled = cancel.join().unwrap().is_ok();
        let dispatched = dispatch.join().unwrap().is_ok();
        assert_ne!(cancelled, dispatched);
        let e = status(&s);
        assert_eq!(
            e.status,
            if cancelled {
                "cancelled"
            } else {
                "dispatch_pending"
            }
        );
        assert_eq!(
            block(s.workspace_claim("root".into())).unwrap().is_some(),
            dispatched
        );
        assert_eq!(e.runtime_instance_id.is_some(), dispatched);
    }
}

#[test]
fn failed_dispatch_does_not_bind_and_cancel_rejects_every_other_pair() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(dir.path());
    create_one(&s);
    assert!(
        event(
            &s,
            Transition::Dispatch {
                to: DispatchState::Dispatching,
                runtime_id: Some("missing".into())
            },
            2
        )
        .is_err()
    );
    assert_eq!(status(&s).runtime_instance_id, None);
    for state in STATUSES {
        for dispatch in DISPATCH {
            let dir = tempfile::tempdir().unwrap();
            let s = open(dir.path());
            create_one(&s);
            s.connection
                .lock()
                .unwrap()
                .execute(
                    "UPDATE executions SET status=?1,dispatch_state=?2",
                    params![state.as_str(), dispatch.as_str()],
                )
                .unwrap();
            let expected =
                state == Status::DispatchPending && dispatch == DispatchState::NotDispatched;
            assert_eq!(
                block(s.cancel_before_dispatch_and_release("e".into(), 0, 2)).is_ok(),
                expected
            );
            assert_eq!(
                block(s.workspace_claim("root".into())).unwrap().is_none(),
                expected
            );
        }
    }
}

#[test]
fn terminal_and_claim_delete_failures_rollback_every_field() {
    for trigger in [
        "CREATE TRIGGER fail_delete BEFORE DELETE ON workspace_claims BEGIN SELECT RAISE(ABORT,'fault'); END;",
        "CREATE TRIGGER ignore_delete BEFORE DELETE ON workspace_claims BEGIN SELECT RAISE(IGNORE); END;",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let s = open(dir.path());
        fixture(&s, Status::Running, DispatchState::Dispatched);
        safe_cleanup(&s);
        let before = status(&s);
        s.connection.lock().unwrap().execute_batch(trigger).unwrap();
        assert!(finish(&s).is_err());
        assert_eq!(status(&s), before);
        let c = s.connection.lock().unwrap();
        let result: Option<String> = c
            .query_row("SELECT final_result_json FROM executions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(result, None);
        drop(c);
        assert!(block(s.workspace_claim("root".into())).unwrap().is_some());
    }
}

#[test]
fn release_requires_same_runtime_or_persisted_job_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(dir.path());
    fixture(&s, Status::Reconciling, DispatchState::Uncertain);
    assert!(finish(&s).is_err());
    let release = Finalization {
        terminal: Status::Interrupted,
        basis: ReleaseBasis::RuntimeTerminated,
        result: None,
        completeness: ResultCompleteness::Unknown,
    };
    assert!(block(s.finalize_and_release_execution("e".into(), 0, release.clone(), 2)).is_err());
    let c = s.connection.lock().unwrap();
    c.execute("UPDATE runtime_instances SET state='terminated',termination_evidence_state='complete',termination_evidence_type='managed_job_destroyed',termination_evidence_at=2",[]).unwrap();
    drop(c);
    assert!(block(s.finalize_and_release_execution("e".into(), 0, release.clone(), 3)).is_err());
    s.connection
        .lock()
        .unwrap()
        .execute(
            "UPDATE runtime_instances SET termination_evidence_type='job_active_processes_zero'",
            [],
        )
        .unwrap();
    block(s.finalize_and_release_execution("e".into(), 0, release, 3)).unwrap();
    assert_eq!(status(&s).status, "interrupted");
    assert!(block(s.workspace_claim("root".into())).unwrap().is_none());
}

#[test]
fn recovery_scans_claims_retains_unknown_and_inconsistent_terminal_and_never_replays() {
    for (state, dispatch, complete) in [
        (Status::Unknown, DispatchState::Uncertain, false),
        (Status::Completed, DispatchState::Dispatched, false),
        (Status::Completed, DispatchState::Dispatched, true),
        (Status::Running, DispatchState::Dispatching, false),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let s = open(dir.path());
        fixture(&s, state, dispatch);
        if complete {
            s.connection.lock().unwrap().execute("UPDATE executions SET release_evidence_state='complete',release_evidence_kind='same_runtime_cleanup',release_evidence_json='{}'",[]).unwrap();
        }
        let outcomes = block(s.recover_claims(2)).unwrap();
        assert_eq!(outcomes.len(), 1);
        match state {
            Status::Unknown => {
                assert!(matches!(outcomes[0], ClaimRecovery::Unknown { .. }));
                assert_eq!(status(&s).status, "unknown");
            }
            Status::Completed if complete => {
                assert!(matches!(outcomes[0], ClaimRecovery::Released { .. }))
            }
            Status::Completed => assert!(matches!(
                outcomes[0],
                ClaimRecovery::Inconsistent {
                    code: "WORKSPACE_CLAIM_INCONSISTENT",
                    ..
                }
            )),
            _ => {
                assert_eq!(status(&s).status, "reconciling");
                assert_eq!(status(&s).dispatch_state, "uncertain");
                assert!(
                    event(
                        &s,
                        Transition::Dispatch {
                            to: DispatchState::Dispatching,
                            runtime_id: Some("r".into())
                        },
                        3
                    )
                    .is_err()
                );
            }
        }
        assert_eq!(
            block(s.workspace_claim("root".into())).unwrap().is_none(),
            complete
        );
    }
}

#[test]
fn crash_child() {
    let Ok(path) = std::env::var("TASK002_CRASH_DATABASE") else {
        return;
    };
    let s = open(std::path::Path::new(&path));
    finish(&s).unwrap();
    panic!("checkpoint did not terminate child");
}

#[test]
fn real_process_exit_before_and_after_commit_has_only_atomic_outcomes() {
    for point in ["after_terminal", "after_delete", "after_commit"] {
        let dir = tempfile::tempdir().unwrap();
        {
            let s = open(dir.path());
            fixture(&s, Status::Running, DispatchState::Dispatched);
            safe_cleanup(&s);
        }
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "agent::store::transactions::tests::crash_child",
                "--nocapture",
            ])
            .env("TASK002_CRASH_DATABASE", dir.path())
            .env("TASK002_CRASH_POINT", point)
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(91),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        let s = open(dir.path());
        let committed = point == "after_commit";
        assert_eq!(
            status(&s).status,
            if committed { "completed" } else { "finalizing" }
        );
        assert_eq!(
            block(s.workspace_claim("root".into())).unwrap().is_none(),
            committed
        );
        assert_eq!(
            status(&s).release_evidence_state,
            if committed { "complete" } else { "incomplete" }
        );
    }
}

#[test]
fn creation_claim_insert_failure_leaves_no_execution_or_idempotency_key() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(dir.path());
    s.connection.lock().unwrap().execute_batch("CREATE TRIGGER fault BEFORE INSERT ON workspace_claims BEGIN SELECT RAISE(ABORT,'fault'); END;").unwrap();
    assert!(block(s.create_execution("e".into(), request("a", "k", "root"), 1)).is_err());
    assert!(block(s.execution("e".into())).unwrap().is_none());
    s.connection
        .lock()
        .unwrap()
        .execute_batch("DROP TRIGGER fault;")
        .unwrap();
    assert!(
        block(s.create_execution("e".into(), request("a", "k", "root"), 2))
            .unwrap()
            .created
    );
}

#[test]
fn first_binding_requires_running_runtime_owned_claim_and_current_revision() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(dir.path());
    create_one(&s);
    s.connection.lock().unwrap().execute("INSERT INTO runtime_instances (id,owner_host_instance_id,state,created_at,updated_at) VALUES ('r','h','preparing',1,1)",[]).unwrap();
    let begin = Transition::Dispatch {
        to: DispatchState::Dispatching,
        runtime_id: Some("r".into()),
    };
    assert_eq!(
        event(&s, begin.clone(), 2).unwrap_err(),
        "RUNTIME_NOT_RUNNING"
    );
    s.connection
        .lock()
        .unwrap()
        .execute("UPDATE runtime_instances SET state='running'", [])
        .unwrap();
    s.connection
        .lock()
        .unwrap()
        .execute("DELETE FROM workspace_claims", [])
        .unwrap();
    assert_eq!(
        event(&s, begin.clone(), 2).unwrap_err(),
        "WORKSPACE_CLAIM_INCONSISTENT"
    );
    s.connection
        .lock()
        .unwrap()
        .execute(
            "INSERT INTO workspace_claims VALUES ('root','e','exclusive_execution',1)",
            [],
        )
        .unwrap();
    event(&s, begin.clone(), 2).unwrap();
    assert_eq!(
        block(s.transition_execution("e".into(), 0, Transition::Running, 3)).unwrap_err(),
        "EXECUTION_REVISION_CONFLICT"
    );
    assert!(event(&s, begin, 3).is_err());
    assert_eq!(status(&s).runtime_instance_id.as_deref(), Some("r"));
}

#[test]
fn dispatched_pending_keeps_cancel_intent_and_late_flush_does_not_replay_uncertain() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(dir.path());
    fixture(&s, Status::DispatchPending, DispatchState::Dispatching);
    event(&s, Transition::RequestCancel, 2).unwrap();
    assert_eq!(status(&s).status, "dispatch_pending");
    assert_eq!(
        s.connection
            .lock()
            .unwrap()
            .query_row("SELECT interrupt_requested_at FROM executions", [], |r| r
                .get::<_, i64>(
                0
            ))
            .unwrap(),
        2
    );
    block(s.recover_claims(3)).unwrap();
    assert_eq!(status(&s).dispatch_state, "uncertain");
    assert!(
        event(
            &s,
            Transition::Dispatch {
                to: DispatchState::Dispatched,
                runtime_id: None
            },
            4
        )
        .is_err()
    );
    assert_eq!(status(&s).dispatch_state, "uncertain");
    assert!(block(s.workspace_claim("root".into())).unwrap().is_some());
}

#[test]
fn unknown_rejects_stale_evidence_and_empty_local_resolve() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(dir.path());
    fixture(&s, Status::Unknown, DispatchState::Uncertain);
    s.connection.lock().unwrap().execute("UPDATE runtime_instances SET state='terminated',termination_evidence_state='complete',termination_evidence_type='job_active_processes_zero',termination_evidence_at=1",[]).unwrap();
    s.connection.lock().unwrap().execute("UPDATE executions SET runtime_termination_evidence_runtime_instance_id='r',runtime_termination_evidence_at=1",[]).unwrap();
    assert!(
        event(
            &s,
            Transition::ResumeRecovery(RecoveryBasis::RuntimeTermination {
                runtime_id: "r".into(),
                evidence_at: 1
            }),
            2
        )
        .is_err()
    );
    assert!(
        event(
            &s,
            Transition::ResumeRecovery(RecoveryBasis::LocalResolve {
                diagnostic: " ".into()
            }),
            2
        )
        .is_err()
    );
    assert_eq!(status(&s).status, "unknown");
}

#[test]
fn immutable_binding_trigger_remains_active_through_public_dispatch_api() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(dir.path());
    fixture(&s, Status::DispatchPending, DispatchState::Dispatching);
    assert!(
        event(
            &s,
            Transition::Dispatch {
                to: DispatchState::Dispatched,
                runtime_id: Some("different".into())
            },
            2
        )
        .is_err()
    );
    assert_eq!(status(&s).runtime_instance_id.as_deref(), Some("r"));
    event(
        &s,
        Transition::Dispatch {
            to: DispatchState::Dispatched,
            runtime_id: None,
        },
        2,
    )
    .unwrap();
    assert_eq!(status(&s).runtime_instance_id.as_deref(), Some("r"));
}

#[test]
fn permission_hint_cannot_replace_turn_or_provider_failure_diagnostic() {
    for authoritative in ["CODEX_TURN_ERROR", "CODEX_PROVIDER_FAILURE"] {
        let dir = tempfile::tempdir().unwrap();
        let store = open(dir.path());
        fixture(&store, Status::Running, DispatchState::Dispatched);
        block(store.execution_diagnostic(
            "e".into(),
            authoritative.into(),
            "authoritative".into(),
            2,
        ))
        .unwrap();
        let before = status(&store);
        block(store.execution_diagnostic(
            "e".into(),
            "CODEX_PERMISSION_DENIED".into(),
            "command".into(),
            3,
        ))
        .unwrap();
        let after = status(&store);
        assert_eq!(after.error_code.as_deref(), Some(authoritative));
        assert_eq!(after.error_message.as_deref(), Some("authoritative"));
        assert_eq!(after.revision, before.revision);
    }

    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    fixture(&store, Status::Running, DispatchState::Dispatched);
    block(store.execution_diagnostic(
        "e".into(),
        "CODEX_PERMISSION_DENIED".into(),
        "command".into(),
        2,
    ))
    .unwrap();
    block(store.execution_diagnostic("e".into(), "CODEX_TURN_ERROR".into(), "turn".into(), 3))
        .unwrap();
    let row = status(&store);
    assert_eq!(row.error_code.as_deref(), Some("CODEX_TURN_ERROR"));
    assert_eq!(row.error_message.as_deref(), Some("turn"));
}

#[test]
fn first_semantic_activity_and_heartbeats_keep_lifecycle_revision_and_updated_at_stable() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    fixture(&store, Status::Running, DispatchState::Dispatched);
    store
        .connection
        .lock()
        .unwrap()
        .execute(
            "UPDATE executions SET thread_id='ROOT',turn_id='TURN' WHERE id='e'",
            [],
        )
        .unwrap();
    let before_revision = status(&store).revision;
    let before_updated_at: i64 = store
        .connection
        .lock()
        .unwrap()
        .query_row(
            "SELECT updated_at FROM executions WHERE id='e'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    block(store.execution_activity(
        "e".into(),
        "ROOT".into(),
        "TURN".into(),
        ActivityPhase::Tool,
        Some(ToolCategory::Test),
        10,
    ))
    .unwrap();

    let first = status(&store);
    assert_eq!(first.last_activity_at, Some(10));
    assert_eq!(first.activity_phase.as_deref(), Some("tool"));
    assert_eq!(first.tool_category.as_deref(), Some("test"));
    assert_eq!(first.activity_summary_code.as_deref(), Some("tool.test"));
    assert_eq!(first.activity_sequence, 1);
    assert_eq!(first.revision, before_revision);
    assert_eq!(history(&store, None, None).events.len(), 1);

    block(store.execution_activity(
        "e".into(),
        "ROOT".into(),
        "TURN".into(),
        ActivityPhase::Tool,
        Some(ToolCategory::Test),
        20,
    ))
    .unwrap();
    block(store.execution_activity(
        "e".into(),
        "ROOT".into(),
        "TURN".into(),
        ActivityPhase::Tool,
        Some(ToolCategory::Test),
        15,
    ))
    .unwrap();

    let heartbeat = status(&store);
    let updated_at: i64 = store
        .connection
        .lock()
        .unwrap()
        .query_row(
            "SELECT updated_at FROM executions WHERE id='e'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(heartbeat.last_activity_at, Some(20));
    assert_eq!(heartbeat.activity_sequence, 1);
    assert_eq!(heartbeat.revision, before_revision);
    assert_eq!(updated_at, before_updated_at);
    assert_eq!(history(&store, None, None).events.len(), 1);
}

#[test]
fn semantic_activity_changes_append_exact_history_without_lifecycle_revision_cas() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    fixture(&store, Status::Running, DispatchState::Dispatched);
    let before = status(&store);

    block(store.project_execution_activity(
        "e".into(),
        ActivityPhase::Tool,
        Some(ToolCategory::Test),
        10,
    ))
    .unwrap();
    block(store.project_execution_activity(
        "e".into(),
        ActivityPhase::Tool,
        Some(ToolCategory::Command),
        11,
    ))
    .unwrap();
    let projected = status(&store);
    let page = history(&store, None, None);
    assert_eq!(projected.last_activity_at, Some(11));
    assert_eq!(projected.activity_phase.as_deref(), Some("tool"));
    assert_eq!(projected.tool_category.as_deref(), Some("command"));
    assert_eq!(
        projected.activity_summary_code.as_deref(),
        Some("tool.command")
    );
    assert_eq!(projected.activity_sequence, 2);
    assert_eq!(projected.revision, before.revision);
    assert_eq!(page.events.len(), 2);
    assert_eq!(page.events[0].sequence, 1);
    assert_eq!(
        page.events[0].activity_revision,
        "ded56df71bc1875c111bf734e82758961fa8858efc7a792da6ab6db4d1cbd176"
    );
    assert_eq!(page.events[1].sequence, 2);
    assert_eq!(page.events[1].summary_code.as_deref(), Some("tool.command"));

    assert_eq!(
        block(store.project_execution_activity(
            "e".into(),
            ActivityPhase::Provider,
            Some(ToolCategory::Test),
            11,
        ))
        .unwrap_err(),
        "INVALID_EXECUTION_ACTIVITY"
    );
    store
        .connection
        .lock()
        .unwrap()
        .execute("DELETE FROM workspace_claims WHERE execution_id='e'", [])
        .unwrap();
    assert_eq!(
        block(store.project_execution_activity("e".into(), ActivityPhase::Provider, None, 12,))
            .unwrap_err(),
        "WORKSPACE_CLAIM_INCONSISTENT"
    );

    store
        .connection
        .lock()
        .unwrap()
        .execute("UPDATE executions SET status='completed' WHERE id='e'", [])
        .unwrap();
    let terminal = status(&store);
    block(store.project_execution_activity("e".into(), ActivityPhase::Provider, None, 13)).unwrap();
    assert_eq!(status(&store), terminal);
}

/// 验证 Finalizing 覆盖底层 Activity，且相同 heartbeat 不生成重复 history。
#[test]
fn lifecycle_finalizing_changes_summary_and_heartbeat_does_not_repeat_it() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    fixture(&store, Status::Running, DispatchState::Dispatched);
    block(store.project_execution_activity(
        "e".into(),
        ActivityPhase::Tool,
        Some(ToolCategory::Test),
        1,
    ))
    .unwrap();
    let before_lifecycle = status(&store);

    event(
        &store,
        Transition::ProviderTerminal {
            runtime_id: "r".into(),
            status: Status::Completed,
        },
        2,
    )
    .unwrap();
    let finalizing = status(&store);
    let page = history(&store, None, None);
    assert_eq!(finalizing.status, "finalizing");
    assert_eq!(finalizing.revision, before_lifecycle.revision + 1);
    assert_eq!(
        finalizing.activity_summary_code.as_deref(),
        Some("execution.finalizing")
    );
    assert_eq!(finalizing.activity_sequence, 2);
    assert_ne!(
        page.events[0].activity_revision,
        page.events[1].activity_revision
    );

    block(store.project_execution_activity(
        "e".into(),
        ActivityPhase::Tool,
        Some(ToolCategory::Test),
        3,
    ))
    .unwrap();
    let heartbeat = status(&store);
    assert_eq!(heartbeat.last_activity_at, Some(3));
    assert_eq!(heartbeat.activity_sequence, 2);
    assert_eq!(heartbeat.revision, finalizing.revision);
    assert_eq!(history(&store, None, None).events.len(), 2);
}

/// 验证 Reconciling 也以进度摘要覆盖底层 Activity。
#[test]
fn lifecycle_reconciling_appends_its_overridden_summary() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    fixture(&store, Status::Running, DispatchState::Dispatched);
    block(store.project_execution_activity(
        "e".into(),
        ActivityPhase::Tool,
        Some(ToolCategory::Command),
        1,
    ))
    .unwrap();
    event(&store, Transition::Reconcile, 2).unwrap();
    let row = status(&store);
    let page = history(&store, None, None);
    assert_eq!(row.status, "reconciling");
    assert_eq!(
        row.activity_summary_code.as_deref(),
        Some("execution.reconciling")
    );
    assert_eq!(row.activity_sequence, 2);
    assert_eq!(
        page.events[1].summary_code.as_deref(),
        Some("execution.reconciling")
    );
}

/// 验证离开覆盖态会将无底层 Activity 的 null summary 原样写入 history。
#[test]
fn lifecycle_leaving_override_appends_nullable_summary_history() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    fixture(&store, Status::Finalizing, DispatchState::Dispatched);

    event(
        &store,
        Transition::ProviderTerminal {
            runtime_id: "r".into(),
            status: Status::Completed,
        },
        1,
    )
    .unwrap();
    event(
        &store,
        Transition::CleanupEmpty {
            runtime_id: "r".into(),
        },
        2,
    )
    .unwrap();
    finish(&store).unwrap();

    let row = status(&store);
    let page = history(&store, None, None);
    assert_eq!(row.status, "completed");
    assert_eq!(row.activity_summary_code, None);
    assert_eq!(row.activity_sequence, 2);
    assert_eq!(page.events.len(), 2);
    assert_eq!(
        page.events[0].summary_code.as_deref(),
        Some("execution.finalizing")
    );
    assert_eq!(page.events[1].summary_code, None);
    assert_eq!(
        page.events[1].activity_revision,
        crate::agent::activity::derive_activity_revision("e", None, None, None).unwrap()
    );
}

/// 验证同一 Store 串行事务可收敛并发 Activity 与生命周期，不依赖 sleep。
#[test]
fn concurrent_activity_and_lifecycle_preserve_both_semantic_history_events() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    fixture(&store, Status::Running, DispatchState::Dispatched);
    let barrier = Arc::new(Barrier::new(3));
    let activity_store = store.clone();
    let activity_barrier = barrier.clone();
    let activity = std::thread::spawn(move || {
        activity_barrier.wait();
        block(activity_store.project_execution_activity(
            "e".into(),
            ActivityPhase::Tool,
            Some(ToolCategory::Test),
            1,
        ))
    });
    let lifecycle_store = store.clone();
    let lifecycle_barrier = barrier.clone();
    let lifecycle = std::thread::spawn(move || {
        lifecycle_barrier.wait();
        block(lifecycle_store.transition_execution(
            "e".into(),
            0,
            Transition::ProviderTerminal {
                runtime_id: "r".into(),
                status: Status::Completed,
            },
            2,
        ))
    });
    barrier.wait();
    activity.join().unwrap().unwrap();
    lifecycle.join().unwrap().unwrap();

    let row = status(&store);
    let page = history(&store, None, None);
    assert_eq!(row.status, "finalizing");
    assert_eq!(row.activity_phase.as_deref(), Some("tool"));
    assert_eq!(row.tool_category.as_deref(), Some("test"));
    assert_eq!(
        row.activity_summary_code.as_deref(),
        Some("execution.finalizing")
    );
    assert_eq!(row.activity_sequence, 2);
    assert_eq!(row.revision, 1);
    assert_eq!(page.events.len(), 2);
    assert_eq!(page.events[0].sequence, 1);
    assert_eq!(page.events[1].sequence, 2);
}

/// 验证 history 查询强制上限、排他 cursor、稳定升序与 nullable roundtrip。
#[test]
fn activity_history_pagination_is_bounded_exclusive_and_preserves_null_summary() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    fixture(&store, Status::Finalizing, DispatchState::Dispatched);
    event(
        &store,
        Transition::ProviderTerminal {
            runtime_id: "r".into(),
            status: Status::Completed,
        },
        1,
    )
    .unwrap();
    event(
        &store,
        Transition::CleanupEmpty {
            runtime_id: "r".into(),
        },
        2,
    )
    .unwrap();
    finish(&store).unwrap();
    let first = history(&store, None, Some(1));
    assert_eq!(first.events.len(), 1);
    assert_eq!(first.events[0].sequence, 1);
    assert_eq!(first.next_cursor, Some(1));
    let second = history(&store, first.next_cursor, Some(1));
    assert_eq!(second.events.len(), 1);
    assert_eq!(second.events[0].sequence, 2);
    assert_eq!(second.events[0].summary_code, None);
    assert_eq!(second.next_cursor, None);
    assert!(history(&store, Some(2), Some(1)).events.is_empty());
    assert_eq!(history(&store, None, Some(10_000)).events.len(), 2);
    assert_eq!(
        block(store.execution_activity_history("e".into(), Some(-1), Some(1))).unwrap_err(),
        "INVALID_ACTIVITY_HISTORY_CURSOR"
    );
}

/// 验证 Activity 的失败路径不会留下 current 或 history 的半条写入。
#[test]
fn activity_rejections_and_history_insert_failure_are_atomic() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    fixture(&store, Status::Running, DispatchState::Dispatched);
    let before = status(&store);
    store.inject_observability_failure(ObservabilityFault::Activity);
    assert_eq!(
        block(store.project_execution_activity(
            "e".into(),
            ActivityPhase::Tool,
            Some(ToolCategory::Test),
            1,
        ))
        .unwrap_err(),
        "INJECTED_ACTIVITY_PERSISTENCE_FAILURE"
    );
    assert_eq!(status(&store), before);
    assert!(history(&store, None, None).events.is_empty());

    assert_eq!(
        block(store.execution_activity(
            "e".into(),
            "wrong-root".into(),
            "wrong-turn".into(),
            ActivityPhase::Tool,
            Some(ToolCategory::Test),
            1,
        ))
        .unwrap_err(),
        "EXECUTION_PROTOCOL_IDENTITY_MISMATCH"
    );
    assert_eq!(status(&store), before);
    assert!(history(&store, None, None).events.is_empty());

    store
        .connection
        .lock()
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER reject_activity_history BEFORE INSERT ON execution_activity_events
             BEGIN SELECT RAISE(ABORT,'history-fault'); END;",
        )
        .unwrap();
    assert!(
        block(store.project_execution_activity(
            "e".into(),
            ActivityPhase::Tool,
            Some(ToolCategory::Test),
            1,
        ))
        .is_err()
    );
    assert_eq!(status(&store), before);
    assert!(history(&store, None, None).events.is_empty());
}

#[test]
fn late_diagnostics_do_not_hide_new_runtime_evidence_and_consumption_is_atomic() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(dir.path());
    fixture(&s, Status::Unknown, DispatchState::Uncertain);
    // The late ACK requires terminal context even while status remains unknown.
    event(
        &s,
        Transition::ProviderTerminal {
            runtime_id: "r".into(),
            status: Status::Completed,
        },
        9,
    )
    .unwrap();
    s.connection
        .lock()
        .unwrap()
        .execute("UPDATE executions SET updated_at=10", [])
        .unwrap();
    s.connection.lock().unwrap().execute("UPDATE runtime_instances SET state='terminated',termination_evidence_state='complete',termination_evidence_type='job_active_processes_zero',termination_evidence_at=11",[]).unwrap();
    event(&s, Transition::InterruptAck, 12).unwrap();
    let resume = Transition::ResumeRecovery(RecoveryBasis::RuntimeTermination {
        runtime_id: "r".into(),
        evidence_at: 11,
    });
    event(&s, resume.clone(), 13).unwrap();
    assert_eq!(status(&s).status, "reconciling");
    event(&s, Transition::MarkUnknown, 14).unwrap();
    assert!(event(&s, resume, 15).is_err());
    assert_eq!(status(&s).status, "unknown");
    assert_eq!(
        s.connection
            .lock()
            .unwrap()
            .query_row(
                "SELECT runtime_termination_evidence_at FROM executions",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        11
    );
}

fn execution_snapshot(s: &StateStore) -> Vec<rusqlite::types::Value> {
    let c = s.connection.lock().unwrap();
    let mut statement = c.prepare("SELECT * FROM executions WHERE id='e'").unwrap();
    let count = statement.column_count();
    statement
        .query_row([], |row| (0..count).map(|index| row.get(index)).collect())
        .unwrap()
}

#[test]
fn runtime_termination_only_reconciling_to_interrupted_and_rejections_preserve_all_fields() {
    for (from, to, allowed) in [
        (Status::Reconciling, Status::Interrupted, true),
        (Status::Reconciling, Status::Completed, false),
        (Status::Reconciling, Status::Failed, false),
        (Status::Reconciling, Status::Cancelled, false),
        (Status::Finalizing, Status::Interrupted, false),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let s = open(dir.path());
        fixture(&s, from, DispatchState::Uncertain);
        s.connection.lock().unwrap().execute("UPDATE runtime_instances SET state='terminated',termination_evidence_state='complete',termination_evidence_type='job_active_processes_zero',termination_evidence_at=2",[]).unwrap();
        s.connection.lock().unwrap().execute("UPDATE executions SET final_result_json='\"prior result\"',result_completeness='partial'",[]).unwrap();
        let before = execution_snapshot(&s);
        let claim = block(s.workspace_claim("root".into())).unwrap();
        let result = block(s.finalize_and_release_execution(
            "e".into(),
            0,
            Finalization {
                terminal: to,
                basis: ReleaseBasis::RuntimeTerminated,
                result: Some(json!({"recovered":"result"})),
                completeness: ResultCompleteness::Complete,
            },
            3,
        ));
        if allowed {
            result.unwrap();
            assert_eq!(status(&s).status, "interrupted");
            assert!(block(s.workspace_claim("root".into())).unwrap().is_none());
            let c = s.connection.lock().unwrap();
            let (saved, at): (String, i64) = c
                .query_row(
                    "SELECT final_result_json,runtime_termination_evidence_at FROM executions",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap();
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&saved).unwrap(),
                json!({"recovered":"result"})
            );
            assert_eq!(at, 2);
        } else {
            assert_eq!(
                result.unwrap_err(),
                "RUNTIME_TERMINATION_REQUIRES_RECONCILING_TO_INTERRUPTED"
            );
            assert_eq!(execution_snapshot(&s), before, "{from:?}->{to:?}");
            assert_eq!(block(s.workspace_claim("root".into())).unwrap(), claim);
        }
    }
}

#[test]
fn interrupt_ack_validates_context_before_any_write() {
    for from in STATUSES {
        for terminal in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let s = open(dir.path());
            fixture(&s, from, DispatchState::Dispatched);
            if terminal {
                s.connection.lock().unwrap().execute("UPDATE executions SET provider_terminal_status='completed',provider_terminal_evidence_runtime_instance_id='r',provider_terminal_evidence_at=1",[]).unwrap();
            }
            s.connection
                .lock()
                .unwrap()
                .execute(
                    "UPDATE executions SET interrupt_diagnostic='existing diagnostic'",
                    [],
                )
                .unwrap();
            if from == Status::Cancelling && !terminal {
                s.connection
                    .lock()
                    .unwrap()
                    .execute("UPDATE executions SET interrupt_ack_at=1", [])
                    .unwrap();
            }
            let before = execution_snapshot(&s);
            let result = event(&s, Transition::InterruptAck, 2);
            let allowed = terminal
                || matches!(
                    from,
                    Status::CancelRequested
                        | Status::Finalizing
                        | Status::Completed
                        | Status::Failed
                        | Status::Cancelled
                        | Status::Interrupted
                );
            if allowed {
                result.unwrap();
                let e = status(&s);
                assert_eq!(e.revision, 1);
                assert_eq!(
                    e.status,
                    if !terminal && from == Status::CancelRequested {
                        "cancelling"
                    } else {
                        from.as_str()
                    }
                );
                let c = s.connection.lock().unwrap();
                let fields:(i64,Option<i64>,String)=c.query_row("SELECT interrupt_ack_at,interrupt_timeout_at,interrupt_diagnostic FROM executions",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
                assert_eq!(fields, (2, None, "existing diagnostic".into()));
            } else {
                assert_eq!(result.unwrap_err(), "INVALID_INTERRUPT_ACK_CONTEXT");
                assert_eq!(
                    execution_snapshot(&s),
                    before,
                    "ACK {from:?}, terminal={terminal}"
                );
            }
        }
    }
}

#[test]
fn interrupt_timeout_validates_context_before_any_write() {
    for from in STATUSES {
        for terminal in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let s = open(dir.path());
            fixture(&s, from, DispatchState::Dispatched);
            if terminal {
                s.connection.lock().unwrap().execute("UPDATE executions SET provider_terminal_status='completed',provider_terminal_evidence_runtime_instance_id='r',provider_terminal_evidence_at=1",[]).unwrap();
            }
            s.connection.lock().unwrap().execute("UPDATE executions SET interrupt_ack_at=1,interrupt_diagnostic='existing diagnostic'",[]).unwrap();
            let before = execution_snapshot(&s);
            let result = event(
                &s,
                Transition::InterruptTimeout {
                    diagnostic: "new timeout".into(),
                },
                2,
            );
            let allowed = terminal
                || matches!(
                    from,
                    Status::CancelRequested
                        | Status::Cancelling
                        | Status::Completed
                        | Status::Failed
                        | Status::Cancelled
                        | Status::Interrupted
                );
            if allowed {
                result.unwrap();
                let e = status(&s);
                assert_eq!(e.revision, 1);
                assert_eq!(
                    e.status,
                    if !terminal && matches!(from, Status::CancelRequested | Status::Cancelling) {
                        "reconciling"
                    } else {
                        from.as_str()
                    }
                );
                let c = s.connection.lock().unwrap();
                let fields:(i64,i64,String)=c.query_row("SELECT interrupt_ack_at,interrupt_timeout_at,interrupt_diagnostic FROM executions",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
                assert_eq!(fields, (1, 2, "new timeout".into()));
            } else {
                assert_eq!(result.unwrap_err(), "INVALID_INTERRUPT_TIMEOUT_CONTEXT");
                assert_eq!(
                    execution_snapshot(&s),
                    before,
                    "Timeout {from:?}, terminal={terminal}"
                );
            }
        }
    }
}

fn manual_resolution_fixture(store: &StateStore) {
    create_one(store);
    store
        .connection
        .lock()
        .unwrap()
        .execute(
            "UPDATE executions SET status='unknown',dispatch_state='not_dispatched',error_code='CODEX_PROVIDER_FAILURE',error_message='CODEX_PROTOCOL_INVALID_MESSAGE: fixture' WHERE id='e'",
            [],
        )
        .unwrap();
}

#[test]
fn manual_resolution_interrupts_and_releases_in_one_transaction() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    manual_resolution_fixture(&store);

    block(store.manual_resolve_and_release("e".into(), true, 9)).unwrap();
    let row = status(&store);
    assert_eq!(row.status, "interrupted");
    assert_eq!(row.result_completeness, "unknown");
    assert_eq!(row.error_code.as_deref(), Some("CODEX_PROVIDER_FAILURE"));
    assert_eq!(
        row.error_message.as_deref(),
        Some("CODEX_PROTOCOL_INVALID_MESSAGE: fixture")
    );
    assert_eq!(row.release_evidence_state, "complete");
    assert_eq!(
        row.release_evidence_kind.as_deref(),
        Some("operator_override")
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(row.release_evidence_json.as_deref().unwrap())
            .unwrap(),
        json!({"schema":"operator_override.v1","authority":"local_desktop_human","resolution":"interrupt_and_release","reason_provided":true,"at":9})
    );
    assert!(
        block(store.workspace_claim("root".into()))
            .unwrap()
            .is_none()
    );
    drop(store);

    let reopened = open(dir.path());
    assert_eq!(
        block(reopened.recover_claims(10)).unwrap(),
        Vec::<ClaimRecovery>::new()
    );
    assert_eq!(status(&reopened).status, "interrupted");
    assert!(
        block(reopened.workspace_claim("root".into()))
            .unwrap()
            .is_none()
    );
}

#[test]
fn manual_resolution_rolls_back_execution_and_claim_when_release_delete_fails() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    manual_resolution_fixture(&store);
    let before = execution_snapshot(&store);
    store
        .connection
        .lock()
        .unwrap()
        .execute_batch("CREATE TRIGGER manual_resolution_fault BEFORE DELETE ON workspace_claims BEGIN SELECT RAISE(ABORT,'fault'); END;")
        .unwrap();

    assert!(block(store.manual_resolve_and_release("e".into(), false, 9)).is_err());
    assert_eq!(execution_snapshot(&store), before);
    assert_eq!(
        block(store.workspace_claim("root".into()))
            .unwrap()
            .unwrap()
            .execution_id,
        "e"
    );
}

#[test]
fn manual_resolution_rejects_every_non_safe_execution_state() {
    for status in [
        "dispatch_pending",
        "running",
        "finalizing",
        "reconciling",
        "completed",
        "failed",
        "cancelled",
        "interrupted",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let store = open(dir.path());
        manual_resolution_fixture(&store);
        store
            .connection
            .lock()
            .unwrap()
            .execute("UPDATE executions SET status=?1 WHERE id='e'", [status])
            .unwrap();
        let before = execution_snapshot(&store);
        assert_eq!(
            block(store.manual_resolve_and_release("e".into(), false, 9)).unwrap_err(),
            "MANUAL_RESOLUTION_NOT_ALLOWED",
            "{status}"
        );
        assert_eq!(execution_snapshot(&store), before, "{status}");
    }
}

#[test]
fn manual_resolution_rejects_provider_identity_and_claim_inconsistencies() {
    for mutation in [
        "UPDATE executions SET provider_terminal_status='failed' WHERE id='e'",
        "UPDATE executions SET thread_id='thread' WHERE id='e'",
        "UPDATE executions SET turn_id='turn' WHERE id='e'",
        "UPDATE executions SET runtime_instance_id='runtime' WHERE id='e'",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let store = open(dir.path());
        manual_resolution_fixture(&store);
        if mutation.contains("runtime_instance_id") {
            store.connection.lock().unwrap().execute("INSERT INTO runtime_instances (id,owner_host_instance_id,state,created_at,updated_at) VALUES ('runtime','host','running',1,1)", []).unwrap();
        }
        store
            .connection
            .lock()
            .unwrap()
            .execute(mutation, [])
            .unwrap();
        assert_eq!(
            block(store.manual_resolve_and_release("e".into(), false, 9)).unwrap_err(),
            "MANUAL_RESOLUTION_NOT_ALLOWED"
        );
    }

    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    manual_resolution_fixture(&store);
    store.connection.lock().unwrap().execute("INSERT INTO runtime_instances (id,owner_host_instance_id,state,created_at,updated_at) VALUES ('runtime','host','running',1,1)", []).unwrap();
    store.connection.lock().unwrap().execute("UPDATE executions SET provider_terminal_evidence_runtime_instance_id='runtime',provider_terminal_evidence_at=1 WHERE id='e'", []).unwrap();
    assert_eq!(
        block(store.manual_resolve_and_release("e".into(), false, 9)).unwrap_err(),
        "MANUAL_RESOLUTION_NOT_ALLOWED"
    );

    for mutation in [
        "DELETE FROM workspace_claims WHERE execution_id='e'",
        "PRAGMA foreign_keys=OFF; UPDATE workspace_claims SET execution_id='missing'; PRAGMA foreign_keys=ON;",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let store = open(dir.path());
        manual_resolution_fixture(&store);
        store
            .connection
            .lock()
            .unwrap()
            .execute_batch(mutation)
            .unwrap();
        assert_eq!(
            block(store.manual_resolve_and_release("e".into(), false, 9)).unwrap_err(),
            "WORKSPACE_CLAIM_INCONSISTENT"
        );
    }
}
