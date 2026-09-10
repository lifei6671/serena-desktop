use super::*;

async fn pending() -> (tempfile::TempDir, StateStore, AgentProductService) {
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::open(dir.path().into()).await.unwrap();
    store
        .product_create_fresh(
            "e".into(),
            "a".into(),
            "k".into(),
            "prompt".into(),
            "W".into(),
            w(dir.path(), "W"),
            1,
        )
        .await
        .unwrap();
    let service = AgentProductService::new(store.clone());
    (dir, store, service)
}

#[tokio::test]
async fn bounded_observe_timeout_and_changed_revision_do_not_mutate_execution_or_claim() {
    let (_dir, store, service) = pending().await;
    let before = store.execution("e".into()).await.unwrap().unwrap();
    let snapshot = service
        .checked_operation(
            json!({"action":"observe","executionId":"e","waitMs":0}),
            None,
        )
        .await;
    let revision = snapshot["data"]["revision"].as_str().unwrap();
    assert_eq!(snapshot["data"]["unchanged"], false);
    assert_eq!(snapshot["data"]["nextAction"]["action"], "resume_pending");
    assert_eq!(snapshot["data"]["progress"]["phase"], "pending");
    let start = Instant::now();
    let same = service
        .checked_operation(
            json!({"action":"observe","executionId":"e","knownRevision":revision,"waitMs":120}),
            None,
        )
        .await;
    assert!(start.elapsed() >= Duration::from_millis(120));
    assert!(start.elapsed() < Duration::from_secs(2));
    assert_eq!(same["data"]["unchanged"], true);
    assert_eq!(same["data"]["revision"], revision);
    assert!(same["data"].get("finalResult").is_none());
    assert_eq!(store.execution("e".into()).await.unwrap().unwrap(), before);
    assert!(
        store
            .workspace_claim(before.canonical_workspace_root)
            .await
            .unwrap()
            .is_some()
    );

    // A missing knownRevision also uses the bounded wait, while a differing token is immediate.
    let start = Instant::now();
    service
        .checked_operation(
            json!({"action":"observe","executionId":"e","waitMs":60}),
            None,
        )
        .await;
    assert!(start.elapsed() >= Duration::from_millis(60));
    let changed = tokio::time::timeout(
        Duration::from_secs(1),
        service.checked_operation(
            json!({"action":"observe","executionId":"e","knownRevision":"stale","waitMs":25000}),
            None,
        ),
    )
    .await
    .unwrap();
    assert_eq!(changed["data"]["unchanged"], false);
    // Omitted waitMs must not silently behave like an immediate snapshot.
    assert!(
        tokio::time::timeout(
            Duration::from_millis(60),
            service.checked_operation(
                json!({"action":"observe","executionId":"e","knownRevision":revision}),
                None
            )
        )
        .await
        .is_err()
    );

    let waiter = service.checked_operation(
        json!({"action":"observe","executionId":"e","knownRevision":revision,"waitMs":2500}),
        None,
    );
    let transition = async {
        tokio::time::sleep(Duration::from_millis(80)).await;
        store.request_cancel("e".into(), 2).await.unwrap();
    };
    let start = Instant::now();
    let (terminal, ()) = tokio::join!(waiter, transition);
    assert!(start.elapsed() < Duration::from_secs(2));
    assert_eq!(terminal["data"]["status"], "cancelled");
    assert_eq!(terminal["data"]["unchanged"], false);
    assert_eq!(terminal["data"]["progress"]["phase"], "terminal");
    assert_eq!(terminal["data"]["nextAction"], Value::Null);
}

#[tokio::test]
async fn progress_phase_follows_persisted_dispatch_and_execution_facts() {
    let (dir, store, service) = pending().await;
    let db = rusqlite::Connection::open(dir.path().join("agent-state.db")).unwrap();
    for (status, dispatch, phase) in [
        ("dispatch_pending", "not_dispatched", "pending"),
        ("dispatch_pending", "dispatching", "dispatching"),
        ("dispatch_pending", "dispatched", "running"),
        ("dispatch_pending", "uncertain", "reconciling"),
        ("running", "dispatched", "running"),
        ("cancel_requested", "dispatched", "running"),
        ("cancelling", "dispatched", "running"),
        ("finalizing", "dispatched", "finalizing"),
        ("reconciling", "uncertain", "reconciling"),
        ("unknown", "uncertain", "reconciling"),
        ("completed", "dispatched", "terminal"),
        ("failed", "not_dispatched", "terminal"),
        ("cancelled", "not_dispatched", "terminal"),
        ("interrupted", "dispatched", "terminal"),
    ] {
        db.execute(
            "UPDATE executions SET status=?1,dispatch_state=?2 WHERE id='e'",
            [status, dispatch],
        )
        .unwrap();
        let before = store.execution("e".into()).await.unwrap().unwrap();
        let response = service
            .checked_operation(
                json!({"action":"observe","executionId":"e","waitMs":0}),
                None,
            )
            .await;
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["data"]["progress"]["phase"], phase);
        if status == "dispatch_pending" && dispatch == "not_dispatched" {
            assert_eq!(response["control"]["providerInvoked"], false);
            assert_eq!(response["control"]["dispatchCertainty"], "not_dispatched");
            assert_eq!(response["data"]["attention"], "pending_explicit_resume");
        }
        assert_eq!(store.execution("e".into()).await.unwrap().unwrap(), before);
    }
}

#[tokio::test]
async fn observe_validation_stays_at_product_boundary() {
    let (_dir, _store, service) = pending().await;
    for request in [
        json!({"action":"observe","executionId":"e","waitMs":25001}),
        json!({"action":"observe","executionId":"e","waitMs":-1}),
        json!({"action":"observe","executionId":"e","waitMs":0.5}),
        json!({"action":"observe","executionId":"e","knownRevision":1}),
        json!({"action":"observe","executionId":"e","includeResult":"yes"}),
        json!({"action":"observe","executionId":"e","unexpected":true}),
        json!({"action":"list","includeResult":true}),
        json!({"action":"cancel","executionId":"e","waitMs":0}),
        json!({"action":"start","agentId":"a","requestKey":"k","prompt":"p","knownRevision":"x"}),
    ] {
        assert_eq!(
            service.checked_operation(request.clone(), None).await["error"]["code"],
            "AGENT_INVALID_ARGUMENT",
            "{request}"
        );
    }
    assert!(parse(json!({"action":"observe","executionId":"e","waitMs":25000})).is_ok());
    let missing = tokio::time::timeout(
        Duration::from_secs(1),
        service.checked_operation(json!({"action":"observe","executionId":"missing"}), None),
    )
    .await
    .unwrap();
    assert_eq!(missing["error"]["code"], "AGENT_EXECUTION_NOT_FOUND");
}

#[tokio::test]
async fn persisted_result_is_opt_in_repeatable_and_does_not_change_revision_or_storage() {
    let (dir, store, service) = pending().await;
    store.request_cancel("e".into(), 2).await.unwrap();
    let result = json!({"executionId":"e","finalResult":[{"type":"agentMessage","phase":"final_answer","text":"原文\n <literal>"}],"extra":{"unchanged":true}});
    let db = rusqlite::Connection::open(dir.path().join("agent-state.db")).unwrap();
    db.execute(
        "UPDATE executions SET final_result_json=?1 WHERE id='e'",
        [result.to_string()],
    )
    .unwrap();
    let before = store.execution("e".into()).await.unwrap().unwrap();
    let summary = service
        .checked_operation(json!({"action":"observe","executionId":"e"}), None)
        .await;
    assert!(summary["data"].get("finalResult").is_none());
    assert_eq!(summary["data"]["resultAvailable"], true);
    assert_eq!(
        summary["data"]["nextAction"],
        json!({"action":"review_result","includeResult":true})
    );
    let revision = &summary["data"]["revision"];
    assert_eq!(
        summary["control"]["nextAction"],
        json!({"action":"review_result","includeResult":true,"executionId":"e"})
    );
    let list = service
        .checked_operation(json!({"action":"list"}), None)
        .await;
    assert!(list["data"]["executions"][0].get("finalResult").is_none());
    assert!(list["data"]["executions"][0].get("unchanged").is_none());
    assert_eq!(&list["data"]["executions"][0]["revision"], revision);
    for _ in 0..2 {
        let full = tokio::time::timeout(Duration::from_secs(1), service.checked_operation(json!({"action":"observe","executionId":"e","knownRevision":revision,"includeResult":true,"waitMs":25000}), None)).await.unwrap();
        assert_eq!(full["data"]["finalResult"], result);
        assert!(full["data"]["nextAction"].is_null());
        assert!(full["control"]["nextAction"].is_null());
        assert_eq!(&full["data"]["revision"], revision);
        assert_eq!(full["data"]["unchanged"], true);
    }
    assert_eq!(store.execution("e".into()).await.unwrap().unwrap(), before);
    assert!(
        store
            .workspace_claim(before.canonical_workspace_root)
            .await
            .unwrap()
            .is_none()
    );
    assert!(before.runtime_instance_id.is_none());
    assert!(before.turn_id.is_none());
    db.execute(
        "UPDATE executions SET final_result_json='invalid json' WHERE id='e'",
        [],
    )
    .unwrap();
    let failed = service
        .checked_operation(
            json!({"action":"observe","executionId":"e","includeResult":true,"waitMs":0}),
            None,
        )
        .await;
    assert_eq!(failed["ok"], false);
    assert_eq!(failed["error"]["code"], "AGENT_OPERATION_FAILED");
    assert!(failed.get("data").is_none());
    assert!(failed.get("finalResult").is_none());
}

#[tokio::test]
async fn revision_tracks_semantics_and_ignores_store_cas_clocks_and_result_projection() {
    let (dir, _store, service) = pending().await;
    let mut view = service.observe("e".into(), false).await.unwrap();
    let revision = view.revision.clone();
    let db = rusqlite::Connection::open(dir.path().join("agent-state.db")).unwrap();
    db.execute(
        "UPDATE executions SET revision=revision+1,updated_at=99 WHERE id='e'",
        [],
    )
    .unwrap();
    assert_eq!(
        service.observe("e".into(), false).await.unwrap().revision,
        revision
    );
    view.created_at += 1;
    view.updated_at += 1;
    view.completed_at = Some(8);
    view.final_result = Some(json!({"large":"result"}));
    view.unchanged = Some(true);
    assert_eq!(view.observation_revision(), revision);
    macro_rules! change {
        ($field:expr, $value:expr) => {{
            let mut old = $value;
            std::mem::swap(&mut $field, &mut old);
            assert_ne!(view.observation_revision(), revision);
            $field = old;
        }};
    }
    change!(view.thread_id, Some("THREAD".into()));
    change!(view.turn_id, Some("TURN".into()));
    change!(view.status, "running".into());
    change!(view.dispatch_state, "dispatched".into());
    change!(view.provider_terminal_status, Some("completed".into()));
    change!(view.result_completeness, "complete".into());
    change!(view.result_available, true);
    change!(view.interrupt_requested, true);
    change!(view.interrupt_acknowledged, true);
    change!(view.interrupt_timed_out, true);
    change!(view.attention, "none".into());
    change!(view.available_actions.can_cancel, false);
    change!(view.available_actions.can_continue, true);
    change!(view.available_actions.can_resume_pending, false);
    change!(view.progress.phase, ProgressPhase::Running);
    assert_eq!(view.observation_revision(), revision);
    db.execute("UPDATE executions SET status='unknown' WHERE id='e'", [])
        .unwrap();
    let unknown = service
        .checked_operation(
            json!({"action":"observe","executionId":"e","waitMs":0}),
            None,
        )
        .await;
    assert_eq!(unknown["data"]["nextAction"]["action"], "manual_resolution");
    assert_eq!(unknown["data"]["progress"]["phase"], "reconciling");
    assert_eq!(
        unknown["data"]["availableActions"],
        json!({"canCancel":false,"canContinue":false,"canResumePending":false})
    );
}

#[tokio::test]
async fn dropping_observer_does_not_stop_owned_worker_or_create_another_turn() {
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::open(dir.path().into()).await.unwrap();
    let (service, release, fake) = fake_service(
        store.clone(),
        dir.path().join("agent-state.db"),
        "R-observe",
        "T-observe",
        false,
        "paginated",
    )
    .await;
    let service = Arc::new(service);
    let receipt = service
        .checked_operation(start("a", "k"), w(dir.path(), "W"))
        .await;
    assert_eq!(receipt["ok"], true, "{receipt}");
    assert!(receipt["data"].get("finalResult").is_none());
    assert!(receipt["data"].get("unchanged").is_none());
    assert!(receipt["data"]["revision"].is_string());
    let id = receipt["data"]["executionId"].as_str().unwrap().to_string();
    let observer = {
        let service = service.clone();
        let id = id.clone();
        tokio::spawn(async move {
            service
                .checked_operation(
                    json!({"action":"observe","executionId":id,"waitMs":25000}),
                    None,
                )
                .await
        })
    };
    tokio::time::sleep(Duration::from_millis(60)).await;
    observer.abort();
    assert!(observer.await.unwrap_err().is_cancelled());
    assert!(
        store
            .workspace_claim(dir.path().to_string_lossy().into())
            .await
            .unwrap()
            .is_some()
    );
    release.send(()).unwrap();
    let finished = final_row(&service, &id).await;
    assert_eq!(finished.status, "completed");
    assert_eq!(finished.result_completeness, "complete");
    assert!(!finished.interrupt_requested);
    assert!(
        store
            .workspace_claim(finished.canonical_workspace_root.clone())
            .await
            .unwrap()
            .is_none()
    );
    let before = store.execution(id.clone()).await.unwrap().unwrap();
    for _ in 0..2 {
        let result = service.checked_operation(json!({"action":"observe","executionId":id,"knownRevision":finished.revision,"includeResult":true}), None).await;
        assert_eq!(
            result["data"]["finalResult"],
            finished.final_result.clone().unwrap()
        );
    }
    assert_eq!(store.execution(id).await.unwrap().unwrap(), before);
    drop(service);
    let methods = fake.await.unwrap();
    assert_eq!(methods.iter().filter(|m| *m == "thread/start").count(), 1);
    assert_eq!(methods.iter().filter(|m| *m == "turn/start").count(), 1);
    assert!(
        methods
            .iter()
            .any(|m| m == "thread/backgroundTerminals/list")
    );
    assert!(!methods.iter().any(|m| m == "turn/interrupt"));
}
