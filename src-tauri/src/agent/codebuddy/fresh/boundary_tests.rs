//! Fresh 的 launch 前边界与 typed OCC；故障 fixture 永不启动进程。
use super::*;

#[tokio::test]
/// 损坏的 frozen workspace 在 reservation/process/session 前失败，不因路径投影而换 Workspace。
async fn workspace_rejections_precede_runtime_attempt_and_launch() {
    let source = tempfile::tempdir().unwrap();
    let base = build(source.path());
    for scenario in ["relative", "missing", "unc", "identity"] {
        let (dir, store, id, resolved) = fixture(&base, "unused", false).await;
        let canonical = crate::config::canonicalize_workspace_root(dir.path()).unwrap();
        std::fs::create_dir(dir.path().join("child")).unwrap();
        let invalid = match scenario {
            "relative" => PathBuf::from("relative-workspace"),
            "missing" => canonical.join("missing"),
            "unc" => PathBuf::from(r"\\?\UNC\unreachable-fixture\share\workspace"),
            // 用原始字符串保留 ..；Windows verbatim PathBuf::join 会提前消除它。
            "identity" => PathBuf::from(format!(r"{}\child\..", canonical.display())),
            _ => unreachable!(),
        };
        let db = rusqlite::Connection::open(dir.path().join("agent-state.db")).unwrap();
        // 仅故障 fixture 绕过 FK 构造损坏的 frozen authority；生产 Store 约束保持开启。
        db.pragma_update(None, "foreign_keys", false).unwrap();
        db.execute(
            "UPDATE executions SET canonical_workspace_root=?2 WHERE id=?1",
            rusqlite::params![id, invalid.to_str().unwrap()],
        )
        .unwrap();
        let sink = Sink::default();
        let result = prepare(
            store.clone(),
            "host".into(),
            id.clone(),
            &resolved,
            DesiredConfiguration::default(),
            Limits::default(),
        )
        .await;
        assert!(matches!(result, Err(Failure::Launch)), "{scenario}");
        assert!(!sink.0.load(Ordering::SeqCst));
        assert!(!store.has_runtime_attempt(id.clone()).await.unwrap());
        assert!(!store.codebuddy_state_exists(id.clone()).await.unwrap());
        assert!(
            store
                .execution(id)
                .await
                .unwrap()
                .unwrap()
                .runtime_instance_id
                .is_none()
        );
        assert!(wire(dir.path()).is_empty());
    }
}

#[tokio::test]
/// LaunchSpec 只允许原 direct ACP argv；配置参数不能被偷偷注入进程启动。
async fn injected_cli_configuration_is_rejected_before_launch() {
    let source = tempfile::tempdir().unwrap();
    let base = source.path().join("unused.exe");
    std::fs::write(&base, b"must never execute").unwrap();
    for option in ["--permission-mode", "--tools", "--settings"] {
        let (dir, store, id, mut resolved) = fixture(&base, "unused", false).await;
        resolved.args.extend([option.into(), "injected".into()]);
        assert!(matches!(
            prepare(
                store.clone(),
                "host".into(),
                id.clone(),
                &resolved,
                DesiredConfiguration::default(),
                Limits::default()
            )
            .await,
            Err(Failure::Launch)
        ));
        assert!(!store.has_runtime_attempt(id).await.unwrap());
        assert!(wire(dir.path()).is_empty());
    }
}

#[tokio::test]
/// reservation、generic revision/R1 与 private revision 必须精确匹配，失败不得改变原绑定。
async fn prepared_runtime_binding_and_private_identity_enforce_occ() {
    let source = tempfile::tempdir().unwrap();
    let base = source.path().join("unused.exe");
    std::fs::write(&base, b"must never execute").unwrap();
    let (_dir, store, id, resolved) = fixture(&base, "unused", false).await;
    let row = store.execution(id.clone()).await.unwrap().unwrap();
    let private_store = CodeBuddyStore(store.clone());
    let private = private_store
        .create(
            id.clone(),
            Ownership {
                execution_revision: row.revision,
                runtime_instance_id: None,
            },
        )
        .await
        .unwrap();
    assert!(
        private_store
            .create(
                id.clone(),
                Ownership {
                    execution_revision: row.revision,
                    runtime_instance_id: None,
                }
            )
            .await
            .is_err()
    );
    for runtime in ["reserved-r1", "unreserved-r2"] {
        store
            .prepare_codebuddy_runtime(
                runtime.into(),
                "host".into(),
                crate::agent::codebuddy::recovery::current_session().unwrap(),
                resolved.executable.to_str().unwrap().into(),
                now(),
            )
            .await
            .unwrap();
    }
    store
        .reserve_runtime_attempt(id.clone(), "reserved-r1".into(), now())
        .await
        .unwrap();
    assert!(
        store
            .reserve_runtime_attempt(id.clone(), "unreserved-r2".into(), now())
            .await
            .is_err()
    );
    assert_eq!(
        store
            .bind_codebuddy_prepared_runtime(
                id.clone(),
                row.revision + 1,
                "reserved-r1".into(),
                now()
            )
            .await
            .unwrap_err(),
        "CODEBUDDY_PREPARATION_BINDING_CONFLICT"
    );
    assert_eq!(
        store
            .bind_codebuddy_prepared_runtime(
                id.clone(),
                row.revision,
                "unreserved-r2".into(),
                now()
            )
            .await
            .unwrap_err(),
        "CODEBUDDY_RUNTIME_RESERVATION_REQUIRED"
    );
    assert!(
        store
            .execution(id.clone())
            .await
            .unwrap()
            .unwrap()
            .runtime_instance_id
            .is_none()
    );
    store
        .bind_codebuddy_prepared_runtime(id.clone(), row.revision, "reserved-r1".into(), now())
        .await
        .unwrap();
    let ownership = Ownership {
        execution_revision: row.revision + 1,
        runtime_instance_id: Some("reserved-r1".into()),
    };
    assert!(
        private_store
            .mutate(
                id.clone(),
                Ownership {
                    execution_revision: row.revision,
                    runtime_instance_id: Some("reserved-r1".into())
                },
                private.revision,
                Mutation::BindRuntime
            )
            .await
            .is_err()
    );
    assert!(
        private_store
            .mutate(
                id.clone(),
                Ownership {
                    execution_revision: row.revision + 1,
                    runtime_instance_id: Some("unreserved-r2".into())
                },
                private.revision,
                Mutation::BindRuntime
            )
            .await
            .is_err()
    );
    let bound = private_store
        .mutate(
            id.clone(),
            ownership.clone(),
            private.revision,
            Mutation::BindRuntime,
        )
        .await
        .unwrap();
    assert!(
        private_store
            .mutate(
                id.clone(),
                ownership,
                private.revision,
                Mutation::NegotiatedProtocol(1)
            )
            .await
            .is_err()
    );
    let reread = private_store.read(id.clone()).await.unwrap();
    assert_eq!(reread, bound);
    assert_eq!(
        reread.conversation_request_id,
        private.conversation_request_id
    );
    let generic = store.execution(id).await.unwrap().unwrap();
    assert_eq!(generic.runtime_instance_id, reread.runtime_instance_id);
    assert_eq!(generic.dispatch_state, "not_dispatched");
    assert!(generic.thread_id.is_none() && generic.turn_id.is_none());

    // 另一 provider 即使拥有 reservation 和同名 prepared runtime，也不能使用 CodeBuddy binding。
    let other_dir = tempfile::tempdir().unwrap();
    let other = StateStore::open(other_dir.path().into()).await.unwrap();
    let input: CreateExecutionInput =
        serde_json::from_value(json!({"agent_id":"a","request_key":"k",
        "prompt":"unused","execution_profile":{},"workspace_id":"w",
        "canonical_workspace_root":other_dir.path(),"mode":"read_only","provider":"codex"}))
        .unwrap();
    let other_row = other
        .create_execution("other".into(), canonicalize_request(input).unwrap(), now())
        .await
        .unwrap();
    other
        .reserve_runtime_attempt("other".into(), "other-r1".into(), now())
        .await
        .unwrap();
    other
        .prepare_codebuddy_runtime(
            "other-r1".into(),
            "host".into(),
            crate::agent::codebuddy::recovery::current_session().unwrap(),
            "unused.exe".into(),
            now(),
        )
        .await
        .unwrap();
    assert_eq!(
        other
            .bind_codebuddy_prepared_runtime(
                "other".into(),
                other_row.execution.revision,
                "other-r1".into(),
                now()
            )
            .await
            .unwrap_err(),
        "CODEBUDDY_PREPARATION_BINDING_CONFLICT"
    );
    assert!(
        other
            .execution("other".into())
            .await
            .unwrap()
            .unwrap()
            .runtime_instance_id
            .is_none()
    );
}
