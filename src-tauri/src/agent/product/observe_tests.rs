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
        assert_eq!(response["data"]["progress"]["activityPhase"], Value::Null);
        assert_eq!(response["data"]["progress"]["toolCategory"], Value::Null);
        assert_eq!(response["data"]["progress"]["lastActivityAt"], Value::Null);
        assert_eq!(response["data"]["progress"]["activityAgeMs"], Value::Null);
        assert_eq!(response["data"]["progress"]["silenceLevel"], Value::Null);
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
    change!(view.progress.activity_phase, Some(ActivityPhase::Provider));
    change!(view.progress.tool_category, Some(ToolCategory::Test));
    change!(view.progress.last_activity_at, Some(5));
    for silence_level in [
        ActivitySilence::Fresh,
        ActivitySilence::Quiet,
        ActivitySilence::Prolonged,
    ] {
        view.progress.silence_level = Some(silence_level);
        assert_eq!(view.observation_revision(), revision);
    }
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
async fn activity_changes_wake_observe_age_does_not_change_revision_and_restart_restores_hint() {
    let (dir, store, service) = pending().await;
    let db = rusqlite::Connection::open(dir.path().join("agent-state.db")).unwrap();
    db.execute(
        "UPDATE executions SET thread_id='ROOT',turn_id='TURN',dispatch_state='dispatched' WHERE id='e'",
        [],
    )
    .unwrap();
    store
        .execution_activity(
            "e".into(),
            "ROOT".into(),
            "TURN".into(),
            ActivityPhase::Provider,
            None,
            now(),
        )
        .await
        .unwrap();
    let initial = service.observe("e".into(), false).await.unwrap();
    assert_eq!(
        initial.progress.activity_phase,
        Some(ActivityPhase::Provider)
    );
    assert_eq!(initial.progress.tool_category, None);
    assert_eq!(initial.progress.silence_level, Some(ActivitySilence::Fresh));
    let initial_revision = initial.revision.clone();
    let waiter = service.checked_operation(
        json!({"action":"observe","executionId":"e","knownRevision":initial_revision.clone(),"waitMs":2500}),
        None,
    );
    let activity = async {
        tokio::time::sleep(Duration::from_millis(80)).await;
        store
            .execution_activity(
                "e".into(),
                "ROOT".into(),
                "TURN".into(),
                ActivityPhase::Tool,
                Some(ToolCategory::Test),
                now(),
            )
            .await
            .unwrap();
    };
    let started = Instant::now();
    let (observed, ()) = tokio::join!(waiter, activity);
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(observed["data"]["unchanged"], false);
    assert_eq!(observed["data"]["progress"]["phase"], "running");
    assert_eq!(observed["data"]["progress"]["activityPhase"], "tool");
    assert_eq!(observed["data"]["progress"]["toolCategory"], "test");
    assert_eq!(observed["data"]["progress"]["silenceLevel"], "fresh");
    assert_ne!(observed["data"]["revision"], initial_revision);
    let revision = observed["data"]["revision"].as_str().unwrap().to_owned();
    let age = observed["data"]["progress"]["activityAgeMs"]
        .as_i64()
        .unwrap();
    tokio::time::sleep(Duration::from_millis(25)).await;
    let aged = service.observe("e".into(), false).await.unwrap();
    assert_eq!(aged.revision, revision);
    assert!(aged.progress.activity_age_ms.unwrap() >= age);
    assert_eq!(aged.progress.silence_level, Some(ActivitySilence::Fresh));
    let unchanged = service
        .checked_operation(
            json!({"action":"observe","executionId":"e","knownRevision":revision,"waitMs":0}),
            None,
        )
        .await;
    assert_eq!(unchanged["data"]["unchanged"], true);
    assert_eq!(unchanged["data"]["revision"], revision);
    assert!(
        unchanged["data"]["progress"]["activityAgeMs"]
            .as_i64()
            .unwrap()
            >= age
    );
    assert_eq!(unchanged["data"]["progress"]["silenceLevel"], "fresh");

    drop(service);
    drop(store);
    let reopened_store = StateStore::open(dir.path().into()).await.unwrap();
    let reopened = AgentProductService::new(reopened_store);
    let restored = reopened.observe("e".into(), false).await.unwrap();
    assert_eq!(restored.revision, revision);
    assert_eq!(restored.progress.activity_phase, Some(ActivityPhase::Tool));
    assert_eq!(restored.progress.tool_category, Some(ToolCategory::Test));
    assert!(restored.progress.last_activity_at.is_some());
    assert_eq!(
        restored.progress.silence_level,
        Some(ActivitySilence::Fresh)
    );
}

#[tokio::test]
async fn late_old_turn_activity_cannot_change_unbound_continuation_or_wake_observe() {
    let (dir, store, _) = pending().await;
    let db = rusqlite::Connection::open(dir.path().join("agent-state.db")).unwrap();
    db.execute(
        "INSERT INTO runtime_instances(id,owner_host_instance_id,state,created_at,updated_at) VALUES ('R1','fixture','terminated',1,1)",
        [],
    )
    .unwrap();
    let result = json!({
        "historyMode":"paginated",
        "executionId":"e",
        "threadId":"ROOT",
        "turnId":"TURN-1",
        "sourceRuntimeId":"R1"
    })
    .to_string();
    db.execute(
        "UPDATE executions SET status='completed',dispatch_state='dispatched',runtime_instance_id='R1',
         thread_id='ROOT',turn_id='TURN-1',provider_terminal_status='completed',
         release_evidence_state='complete',release_evidence_kind='same_runtime_cleanup',
         release_evidence_json='{}',result_completeness='complete',final_result_json=?1,completed_at=2
         WHERE id='e'",
        [&result],
    )
    .unwrap();
    db.execute("DELETE FROM workspace_claims WHERE execution_id='e'", [])
        .unwrap();
    store
        .product_create_continuation(
            "e2".into(),
            "e".into(),
            "continue-key".into(),
            "continue".into(),
            3,
        )
        .await
        .unwrap();
    db.execute(
        "UPDATE executions SET status='running',dispatch_state='dispatched' WHERE id='e2'",
        [],
    )
    .unwrap();
    let service = AgentProductService::new(store.clone());
    let before = service.observe("e2".into(), false).await.unwrap();
    let before_record = store.execution("e2".into()).await.unwrap().unwrap();
    assert_eq!(before.thread_id.as_deref(), Some("ROOT"));
    assert!(before.turn_id.is_none());
    let wait = service.checked_operation(
        json!({"action":"observe","executionId":"e2","knownRevision":before.revision.clone(),"waitMs":120}),
        None,
    );
    let old_activity = async {
        assert_eq!(
            store
                .execution_activity(
                    "e2".into(),
                    "ROOT".into(),
                    "TURN-1".into(),
                    ActivityPhase::Tool,
                    Some(ToolCategory::Test),
                    4,
                )
                .await
                .unwrap_err(),
            "EXECUTION_PROTOCOL_IDENTITY_MISMATCH"
        );
    };
    let (unchanged, ()) = tokio::join!(wait, old_activity);
    assert_eq!(unchanged["data"]["unchanged"], true);
    let after_old = store.execution("e2".into()).await.unwrap().unwrap();
    assert_eq!(after_old.revision, before_record.revision);
    assert!(after_old.last_activity_at.is_none());
    assert!(after_old.activity_phase.is_none());
    assert!(after_old.tool_category.is_none());

    db.execute("UPDATE executions SET turn_id='TURN-2' WHERE id='e2'", [])
        .unwrap();
    store
        .execution_activity(
            "e2".into(),
            "ROOT".into(),
            "TURN-2".into(),
            ActivityPhase::Tool,
            Some(ToolCategory::Test),
            now(),
        )
        .await
        .unwrap();
    let current = service.observe("e2".into(), false).await.unwrap();
    assert_eq!(current.turn_id.as_deref(), Some("TURN-2"));
    assert_eq!(current.progress.activity_phase, Some(ActivityPhase::Tool));
    assert_eq!(current.progress.tool_category, Some(ToolCategory::Test));
    assert_ne!(current.revision, before.revision);
}

#[tokio::test]
async fn activity_projection_never_exposes_command_output_or_local_paths() {
    let (dir, store, service) = pending().await;
    let db = rusqlite::Connection::open(dir.path().join("agent-state.db")).unwrap();
    db.execute(
        "UPDATE executions SET thread_id='ROOT',turn_id='TURN',dispatch_state='dispatched' WHERE id='e'",
        [],
    )
    .unwrap();
    let command = "python C:\\private\\script.py --token secret-token --password hunter2";
    let activity = match crate::agent::codex::protocol::notification(
        "item/started".into(),
        json!({"threadId":"ROOT","turnId":"TURN","item":{
            "type":"commandExecution","command":command,"commandActions":[]
        }}),
    )
    .unwrap()
    {
        crate::agent::codex::protocol::Notification::Activity(activity) => activity,
        other => panic!("expected activity, got {other:?}"),
    };
    store
        .execution_activity(
            "e".into(),
            activity.thread_id,
            activity.turn_id,
            activity.phase,
            activity.tool_category,
            activity.observed_at,
        )
        .await
        .unwrap();
    assert!(matches!(
        crate::agent::codex::protocol::notification(
            "item/commandExecution/outputDelta".into(),
            json!({"threadId":"ROOT","turnId":"TURN","itemId":"I","delta":"stdout secret-token hunter2 C:\\private"}),
        )
        .unwrap(),
        crate::agent::codex::protocol::Notification::Other { .. }
    ));
    for response in [
        service
            .checked_operation(
                json!({"action":"observe","executionId":"e","waitMs":0}),
                None,
            )
            .await,
        service
            .checked_operation(json!({"action":"list"}), None)
            .await,
    ] {
        let exposed = response.to_string();
        for secret in [command, "secret-token", "hunter2", "C:\\private", "stdout"] {
            assert!(
                !exposed.contains(secret),
                "Product exposed {secret}: {exposed}"
            );
        }
    }
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

#[tokio::test]
async fn diagnostic_revision_wakes_observe_and_list_agrees_across_lifecycle() {
    let (dir, store, service) = pending().await;
    let db = rusqlite::Connection::open(dir.path().join("agent-state.db")).unwrap();
    db.execute("UPDATE executions SET status='running',dispatch_state='dispatched' WHERE id='e'", []).unwrap();
    let initial = service.observe("e".into(), false).await.unwrap();
    let waiter = service.checked_operation(json!({"action":"observe","executionId":"e","knownRevision":initial.revision,"waitMs":2500}), None);
    let write = async {
        tokio::time::sleep(Duration::from_millis(50)).await;
        store.execution_diagnostic("e".into(), "CODEX_TURN_ERROR".into(),
            json!({"willRetry":true,"error":{"codexErrorInfo":"sandboxError","message":"Authorization: Bearer secret-token", "additionalDetails":"stderr credential password"}}).to_string(), 2).await.unwrap();
    };
    let (observed, ()) = tokio::join!(waiter, write);
    assert_eq!(observed["data"]["status"], "running");
    assert_eq!(observed["data"]["unchanged"], false);
    assert_ne!(observed["data"]["revision"], initial.revision);
    assert_eq!(observed["data"]["errorCode"], "CODEX_TURN_ERROR");
    assert_eq!(observed["data"]["errorMessage"], "sandboxError: Codex reported a turn diagnostic.");
    let stored = store.execution("e".into()).await.unwrap().unwrap();
    assert_eq!(stored.error_code.as_deref(), Some("CODEX_TURN_ERROR"));
    assert!(stored.error_message.unwrap().contains("secret-token"));
    for status in ["running", "reconciling", "unknown", "interrupted", "completed"] {
        db.execute("UPDATE executions SET status=?1,provider_terminal_status=?2 WHERE id='e'",
            rusqlite::params![status, (status == "completed").then_some("completed")]).unwrap();
        let observe = service.checked_operation(json!({"action":"observe","executionId":"e","waitMs":0}), None).await;
        let list = service.checked_operation(json!({"action":"list"}), None).await;
        assert_eq!(observe["data"]["status"], status);
        assert_eq!(observe["data"]["errorCode"], "CODEX_TURN_ERROR");
        assert_eq!(list["data"]["executions"][0]["errorCode"], observe["data"]["errorCode"], "{list}");
        assert_eq!(list["data"]["executions"][0]["errorMessage"], observe["data"]["errorMessage"]);
        if status == "completed" { assert_eq!(observe["data"]["providerTerminalStatus"], "completed"); }
    }
}

#[tokio::test]
async fn diagnostic_projection_never_exposes_raw_provider_payload() {
    let (_dir, store, service) = pending().await;
    for (code, raw) in [
        ("CODEX_PROVIDER_FAILURE", format!("CODEX_PROTOCOL_QUEUE_FULL: Authorization token credential {}", "secret".repeat(1000))),
        ("CODEX_TURN_ERROR", json!({"error":{"message":"secret","codexErrorInfo":"secret"}}).to_string()),
        ("CODEX_PERMISSION_DENIED", "command".into()),
        ("secret", "raw stdout stderr secret".into()),
    ] {
        store.execution_diagnostic("e".into(), code.into(), raw, 2).await.unwrap();
        let view = service.observe("e".into(), false).await.unwrap();
        let message = view.error_message.unwrap();
        assert!(message.len() <= 256);
        assert!(!message.contains("secret"));
        assert!(!view.error_code.unwrap().contains("secret"));
    }
}
