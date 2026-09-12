use super::*;

#[test]
fn quarantine_is_a_stable_product_error() {
    let response = failure("AGENT_RUNTIME_QUARANTINED: fixture".into(), None);
    assert_eq!(response["error"]["code"], "AGENT_RUNTIME_QUARANTINED");
    no_dispatch(&response["control"], false);
    assert_eq!(response["control"]["nextAction"]["action"], "manual_resolution");
}

#[tokio::test]
async fn quarantine_pending_receipts_preserve_identity_without_replay() {
    let (dir, store, service) = fixture().await;
    service.manager.runtime_pool.retain_failure(
        &store,
        dir.path().to_str().unwrap(),
        "old-runtime",
        crate::agent::codex::runtime::RuntimeFailure {
            code: "CODEX_RUNTIME_TERMINATION_TIMEOUT",
            message: "fixture quarantine".into(),
            runtime: None,
        },
    );
    let rejected = service.error_response(
        parse(json!({"action":"start","workspaceId":"W","agentId":"a","requestKey":"k","prompt":"hello"})).unwrap(),
        w(dir.path(), "W"),
        ProductError::new("AGENT_RUNTIME_QUARANTINED".into(), None),
    ).await;
    no_dispatch(&rejected["control"], false);
    assert_eq!(rejected["control"]["nextAction"]["action"], "manual_resolution");
    assert_eq!(count(dir.path(), "executions"), 0);

    let accepted = service.checked_operation(start("a", "k"), w(dir.path(), "W")).await;
    assert_eq!(accepted["error"]["code"], "AGENT_RUNTIME_QUARANTINED");
    no_dispatch(&accepted["control"], true);
    let id = accepted["error"]["executionId"].as_str().unwrap();
    assert_eq!(accepted["control"]["nextAction"], json!({"action":"manual_resolution","executionId":id}));
    let before = store.execution(id.into()).await.unwrap().unwrap();
    assert_eq!(before.status, "dispatch_pending");
    assert_eq!(before.dispatch_state, "not_dispatched");
    assert!(before.runtime_instance_id.is_none());
    for request in [
        json!({"action":"observe","executionId":id,"waitMs":0}),
        json!({"action":"list"}),
        start("a", "k"),
    ] {
        let response = service.checked_operation(request, w(dir.path(), "W")).await;
        assert_eq!(response["ok"], true, "{response}");
        let view = if response["data"]["executions"].is_array() {
            &response["data"]["executions"][0]
        } else {
            &response["data"]
        };
        assert_eq!(view["executionId"], id);
        assert_eq!(view["attention"], "manual_resolution_required");
        assert_eq!(view["availableActions"]["canResumePending"], false);
        assert_eq!(view["availableActions"]["canCancel"], true);
        assert_eq!(view["nextAction"]["action"], "manual_resolution");
        if !response["data"]["executions"].is_array() {
            assert_eq!(response["control"]["nextAction"], json!({"action":"manual_resolution","executionId":id}));
            no_dispatch(&response["control"], true);
        }
    }
    let resumed = service.checked_operation(json!({"action":"resume_pending","executionId":id}), None).await;
    assert_eq!(resumed["error"]["code"], "AGENT_RUNTIME_QUARANTINED");
    assert_eq!(resumed["error"]["executionId"], id);
    no_dispatch(&resumed["control"], true);
    assert_eq!(resumed["control"]["nextAction"], json!({"action":"manual_resolution","executionId":id}));
    assert_eq!(store.execution(id.into()).await.unwrap().unwrap(), before);
    assert_eq!(count(dir.path(), "executions"), 1);
    assert_eq!(count(dir.path(), "runtime_instances"), 0);
    assert_eq!(count(dir.path(), "execution_runtime_attempts"), 0);
    let cancelled = service.checked_operation(json!({"action":"cancel","executionId":id}), None).await;
    assert_eq!(cancelled["data"]["status"], "cancelled");
    assert_eq!(count(dir.path(), "workspace_claims"), 0);
}

#[tokio::test]
async fn control_attention_distinguishes_clean_pending_unknown_and_active_work() {
    let (dir, store, service) = fixture().await;
    store.product_create_fresh("e".into(), "a".into(), "k".into(), "hello".into(), "W".into(), w(dir.path(), "W"), 1).await.unwrap();
    let db = rusqlite::Connection::open(dir.path().join("agent-state.db")).unwrap();
    for (status, dispatch, attention, next, resume) in [
        ("dispatch_pending", "not_dispatched", "pending_explicit_resume", "resume_pending", true),
        ("dispatch_pending", "dispatching", "none", "observe", false),
        ("unknown", "uncertain", "manual_resolution_required", "manual_resolution", false),
        ("running", "dispatched", "none", "observe", false),
        ("finalizing", "dispatched", "none", "observe", false),
    ] {
        db.execute("UPDATE executions SET status=?1,dispatch_state=?2 WHERE id='e'", [status, dispatch]).unwrap();
        let response = service.checked_operation(json!({"action":"observe","executionId":"e","waitMs":0}), None).await;
        assert_eq!(response["data"]["attention"], attention);
        assert_eq!(response["data"]["availableActions"]["canResumePending"], resume);
        assert_eq!(response["data"]["nextAction"]["action"], next);
        assert_eq!(response["control"]["nextAction"]["action"], next);
    }
}

async fn fixture() -> (tempfile::TempDir, StateStore, AgentProductService) {
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::open(dir.path().into()).await.unwrap();
    let service = AgentProductService::new(store.clone());
    (dir, store, service)
}
fn no_dispatch(control: &Value, accepted: bool) {
    assert_eq!(control["requestAccepted"], accepted);
    assert_eq!(control["providerInvoked"], false);
    assert_eq!(control["dispatchCertainty"], "not_dispatched");
}
fn count(dir: &std::path::Path, table: &str) -> i64 {
    rusqlite::Connection::open(dir.join("agent-state.db"))
        .unwrap()
        .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

#[tokio::test]
async fn control_unavailable_storage_never_invents_non_acceptance_or_non_dispatch() {
    let (dir, store, service) = fixture().await;
    store
        .product_create_fresh(
            "e".into(),
            "a".into(),
            "k".into(),
            "hello".into(),
            "W".into(),
            w(dir.path(), "W"),
            1,
        )
        .await
        .unwrap();
    let db = rusqlite::Connection::open(dir.path().join("agent-state.db")).unwrap();
    db.execute(
        "ALTER TABLE executions RENAME TO unavailable_executions",
        [],
    )
    .unwrap();
    let mut request = start("a", "k");
    request["workspaceId"] = json!("W");
    let response = service
        .error_response(
            parse(request.clone()).unwrap(),
            None,
            ProductError::new("storage unavailable".into(), None),
        )
        .await;
    assert_output_contract(&response);
    assert_eq!(response["ok"], false);
    assert!(response["control"].is_null());
    let confirmed = service
        .error_response(
            parse(request).unwrap(),
            None,
            ProductError::accepted("storage unavailable".into(), "e".into()),
        )
        .await;
    assert_output_contract(&confirmed);
    assert_eq!(confirmed["control"]["requestAccepted"], true);
    assert!(confirmed["control"]["providerInvoked"].is_null());
    assert_eq!(confirmed["control"]["dispatchCertainty"], "uncertain");
    assert_eq!(confirmed["error"]["executionId"], "e");
    assert!(confirmed["error"].get("acceptedExecutionId").is_none());
}

#[tokio::test]
async fn control_invalid_and_missing_workspace_create_nothing() {
    let (dir, store, service) = fixture().await;
    let invalid = service
        .checked_operation(json!({"action":"start"}), w(dir.path(), "W"))
        .await;
    assert_eq!(invalid["error"]["code"], "AGENT_INVALID_ARGUMENT");
    no_dispatch(&invalid["control"], false);
    assert_eq!(
        invalid["control"]["nextAction"],
        json!({"action":"correct_input"})
    );
    let missing = service.checked_operation(start("a", "k"), None).await;
    no_dispatch(&missing["control"], false);
    assert_eq!(missing["error"]["code"], "AGENT_NO_ACTIVE_WORKSPACE");
    assert_eq!(
        missing["control"]["nextAction"]["action"],
        "activate_workspace"
    );
    assert!(
        store
            .product_read(None, None, None, 20)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(count(dir.path(), "runtime_instances"), 0);
    assert_eq!(count(dir.path(), "workspace_claims"), 0);
}

#[tokio::test]
async fn control_conflicts_direct_to_exact_owner_without_accepting_rejected_request() {
    let (dir, store, service) = fixture().await;
    store
        .product_create_fresh(
            "owner".into(),
            "a".into(),
            "k".into(),
            "hello".into(),
            "W".into(),
            w(dir.path(), "W"),
            1,
        )
        .await
        .unwrap();
    let before = store.execution("owner".into()).await.unwrap().unwrap();
    for (request, code, next) in [
        (start("b", "k"), "WORKSPACE_CLAIM_CONFLICT", "observe"),
        (start("a", "new"), "AGENT_LINEAGE_CONFLICT", "observe"),
        (
            json!({"action":"start","agentId":"a","requestKey":"k","prompt":"different"}),
            "AGENT_REQUEST_KEY_CONFLICT",
            "correct_input",
        ),
    ] {
        let response = service.checked_operation(request, w(dir.path(), "W")).await;
        assert_eq!(response["error"]["code"], code);
        no_dispatch(&response["control"], false);
        assert_eq!(response["control"]["nextAction"]["action"], next);
        if next == "observe" {
            assert_eq!(response["control"]["nextAction"]["executionId"], "owner");
        }
    }
    assert_eq!(count(dir.path(), "executions"), 1);
    assert_eq!(count(dir.path(), "runtime_instances"), 0);
    assert_eq!(
        store.execution("owner".into()).await.unwrap().unwrap(),
        before
    );
}

#[tokio::test]
async fn control_durable_backend_failure_and_same_hash_retry_preserve_original_identity() {
    let (dir, store, mut service) = fixture().await;
    service.manager.backend_error = Some("BACKEND_UNAVAILABLE: test backend".into());
    let failed = service
        .checked_operation(start("a", "k"), w(dir.path(), "W"))
        .await;
    assert_eq!(failed["error"]["code"], "BACKEND_UNAVAILABLE");
    no_dispatch(&failed["control"], true);
    let id = failed["error"]["executionId"].as_str().unwrap();
    assert_eq!(
        store.execution(id.into()).await.unwrap().unwrap().status,
        "dispatch_pending"
    );
    let retried = service.checked_operation(start("a", "k"), None).await;
    assert_eq!(retried["ok"], true);
    assert_eq!(retried["data"]["executionId"], id);
    no_dispatch(&retried["control"], true);
    assert_eq!(count(dir.path(), "executions"), 1);
    assert_eq!(count(dir.path(), "runtime_instances"), 0);
    assert_eq!(count(dir.path(), "workspace_claims"), 1);
}

#[tokio::test]
async fn control_uncertain_resume_rejection_uses_persisted_facts_not_runtime_or_status() {
    let (dir, store, mut service) = fixture().await;
    store
        .product_create_fresh(
            "e".into(),
            "a".into(),
            "k".into(),
            "hello".into(),
            "W".into(),
            w(dir.path(), "W"),
            1,
        )
        .await
        .unwrap();
    service.manager.backend_error = Some("BACKEND_UNAVAILABLE: no launch".into());
    let db = rusqlite::Connection::open(dir.path().join("agent-state.db")).unwrap();
    for dispatch in ["dispatching", "uncertain", "dispatched", "not_dispatched"] {
        db.execute(
            "UPDATE executions SET status='unknown',dispatch_state=?1 WHERE id='e'",
            [dispatch],
        )
        .unwrap();
        let before = store.execution("e".into()).await.unwrap().unwrap();
        for action in ["observe", "resume_pending", "cancel"] {
            let args = if action == "observe" {
                json!({"action":action,"executionId":"e","waitMs":0})
            } else {
                json!({"action":action,"executionId":"e"})
            };
            let response = service.checked_operation(args, None).await;
            assert_eq!(response["control"]["requestAccepted"], true, "{response}");
            assert_eq!(
                response["control"]["providerInvoked"],
                match dispatch {
                    "dispatched" => json!(true),
                    "not_dispatched" => json!(false),
                    _ => Value::Null,
                }
            );
            assert_eq!(
                response["control"]["dispatchCertainty"],
                match dispatch {
                    "dispatched" => "dispatched",
                    "not_dispatched" => "not_dispatched",
                    _ => "uncertain",
                }
            );
            assert_eq!(
                response["control"]["nextAction"],
                json!({"action":"manual_resolution","executionId":"e"})
            );
        }
        for (args, code) in [
            (
                json!({"action":"continue","executionId":"e","requestKey":"new","prompt":"hello"}),
                "AGENT_CONTINUE_NOT_ALLOWED",
            ),
            (start("a", "new"), "AGENT_LINEAGE_CONFLICT"),
        ] {
            let response = service.checked_operation(args, None).await;
            assert_eq!(response["error"]["code"], code);
            no_dispatch(&response["control"], false);
            assert_eq!(
                response["control"]["nextAction"],
                json!({"action":"manual_resolution","executionId":"e"})
            );
        }
        assert_eq!(store.execution("e".into()).await.unwrap().unwrap(), before);
    }
    assert_eq!(count(dir.path(), "runtime_instances"), 0);
    assert_eq!(count(dir.path(), "workspace_claims"), 1);
}

#[tokio::test]
async fn control_cancel_before_dispatch_is_terminal_but_never_provider_invoked() {
    let (dir, store, service) = fixture().await;
    store
        .product_create_fresh(
            "e".into(),
            "a".into(),
            "k".into(),
            "hello".into(),
            "W".into(),
            w(dir.path(), "W"),
            1,
        )
        .await
        .unwrap();
    let response = service
        .checked_operation(json!({"action":"cancel","executionId":"e"}), None)
        .await;
    assert_eq!(response["data"]["status"], "cancelled");
    no_dispatch(&response["control"], true);
    assert!(response["control"]["nextAction"].is_null());
    let invalid_resume = service
        .checked_operation(json!({"action":"resume_pending","executionId":"e"}), None)
        .await;
    assert_eq!(invalid_resume["error"]["code"], "AGENT_RESUME_NOT_ALLOWED");
    no_dispatch(&invalid_resume["control"], true);
    assert_eq!(count(dir.path(), "runtime_instances"), 0);
}

#[tokio::test]
async fn control_handoff_failure_after_durable_create_keeps_accepted_identity() {
    let (dir, store, mut service) = fixture().await;
    let hook = Arc::new((tokio::sync::Notify::new(), tokio::sync::Notify::new()));
    service.manager.test_handoff = Some(hook.clone());
    let workspace = w(dir.path(), "W");
    let request =
        tokio::spawn(async move { service.checked_operation(start("a", "k"), workspace).await });
    hook.0.notified().await;
    let row = store
        .product_read(None, None, None, 1)
        .await
        .unwrap()
        .pop()
        .unwrap()
        .execution;
    store.request_cancel(row.id.clone(), 2).await.unwrap();
    hook.1.notify_one();
    let response = request.await.unwrap();
    assert_eq!(response["ok"], false);
    assert_eq!(response["error"]["executionId"], row.id);
    no_dispatch(&response["control"], true);
    assert_eq!(count(dir.path(), "runtime_instances"), 0);
    assert_eq!(count(dir.path(), "executions"), 1);
}

#[tokio::test]
async fn control_idempotent_running_retry_and_terminal_receipt_do_not_dispatch_twice() {
    let (dir, store, _) = fixture().await;
    let (service, release, fake) = fake_service(
        store.clone(),
        dir.path().join("agent-state.db"),
        "RC",
        "TC",
        false,
        "paginated",
    )
    .await;
    let first = service
        .checked_operation(start("a", "k"), w(dir.path(), "W"))
        .await;
    assert_eq!(first["ok"], true, "{first}");
    assert_eq!(first["control"]["requestAccepted"], true);
    let id = first["data"]["executionId"].as_str().unwrap();
    for _ in 0..2 {
        let retry = service.checked_operation(start("a", "k"), None).await;
        assert_eq!(retry["data"]["executionId"], id);
        assert_eq!(retry["control"]["requestAccepted"], true);
    }
    release.send(()).unwrap();
    let final_view = final_row(&service, id).await;
    let response = service
        .checked_operation(
            json!({"action":"observe","executionId":id,"includeResult":true}),
            None,
        )
        .await;
    assert_eq!(response["control"]["providerInvoked"], true);
    assert_eq!(response["control"]["dispatchCertainty"], "dispatched");
    assert!(response["control"]["nextAction"].is_null());
    assert!(response["data"]["nextAction"].is_null());
    assert_eq!(
        response["data"]["finalResult"],
        final_view.final_result.unwrap()
    );
    assert_eq!(
        response["data"]["nextAction"]["action"],
        response["control"]["nextAction"]["action"]
    );
    assert_eq!(response["data"]["revision"], final_view.revision);
    let mut row = store.execution(id.into()).await.unwrap().unwrap();
    row.dispatch_state = "uncertain".into();
    let evidence = super::super::control::ControlReceipt::accepted(&row, None);
    assert_eq!(evidence.provider_invoked, Some(true));
    row.provider_terminal_evidence_at = None;
    assert_eq!(
        super::super::control::ControlReceipt::accepted(&row, None).provider_invoked,
        None
    );
    assert_eq!(count(dir.path(), "executions"), 1);
    drop(service);
    let methods = fake.await.unwrap();
    assert_eq!(methods.iter().filter(|m| *m == "turn/start").count(), 1);
}

#[tokio::test]
async fn expected_workspace_mismatch_has_no_durable_side_effects() {
    let (dir, _store, service) = fixture().await;
    let args =
        json!({"action":"start","agentId":"a","requestKey":"k","prompt":"hello","workspaceId":"A"});
    let response = service.checked_operation(args, w(dir.path(), "B")).await;
    assert_eq!(response["error"]["code"], "AGENT_WORKSPACE_CHANGED");
    no_dispatch(&response["control"], false);
    assert_eq!(
        response["control"]["nextAction"]["action"],
        "activate_workspace"
    );
    for table in ["executions", "runtime_instances", "workspace_claims"] {
        assert_eq!(count(dir.path(), table), 0);
    }
}

#[tokio::test]
async fn backend_failure_pending_can_resume_first_dispatch_or_cancel() {
    let (dir, store, mut service) = fixture().await;
    service.manager.backend_error = Some("BACKEND_UNAVAILABLE: fixture".into());
    let response = service
        .checked_operation(start("a", "k"), w(dir.path(), "W"))
        .await;
    let id = response["error"]["executionId"]
        .as_str()
        .unwrap()
        .to_owned();
    let pending = service
        .checked_operation(
            json!({"action":"observe","executionId":id,"waitMs":0}),
            None,
        )
        .await;
    assert_eq!(pending["data"]["status"], "dispatch_pending");
    assert_eq!(pending["data"]["attention"], "pending_explicit_resume");
    assert_eq!(
        pending["data"]["availableActions"]["canResumePending"],
        true
    );
    assert_eq!(pending["data"]["availableActions"]["canCancel"], true);
    assert_eq!(count(dir.path(), "workspace_claims"), 1);
    let (fixed, release, fake) = fake_service(
        store.clone(),
        dir.path().join("agent-state.db"),
        "FIXED",
        "T",
        false,
        "paginated",
    )
    .await;
    let resumed = fixed
        .checked_operation(json!({"action":"resume_pending","executionId":id}), None)
        .await;
    assert_eq!(resumed["ok"], true, "{resumed}");
    release.send(()).unwrap();
    assert_eq!(final_row(&fixed, &id).await.status, "completed");
    drop(fixed);
    assert_eq!(
        fake.await
            .unwrap()
            .iter()
            .filter(|m| *m == "turn/start")
            .count(),
        1
    );
    let response = service
        .checked_operation(start("b", "k"), w(dir.path(), "W"))
        .await;
    let cancelled = service
        .checked_operation(
            json!({"action":"cancel","executionId":response["error"]["executionId"]}),
            None,
        )
        .await;
    assert_eq!(cancelled["data"]["status"], "cancelled");
    assert_eq!(count(dir.path(), "workspace_claims"), 0);
}

#[tokio::test]
async fn binary_resolution_failure_pending_can_resume_first_dispatch() {
    let (dir, store, mut service) = fixture().await;
    service.manager = crate::agent::task_manager::AgentTaskManager::new(
        store.clone(), dir.path().join("missing-codex.exe"),
    );
    let response = service
        .checked_operation(start("a", "k"), w(dir.path(), "W"))
        .await;
    let id = response["data"]["executionId"]
        .as_str()
        .unwrap()
        .to_owned();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while store.product_worker_owned(&id) { tokio::task::yield_now().await; }
    }).await.unwrap();
    assert_eq!(count(dir.path(), "runtime_instances"), 0);
    let pending = service
        .checked_operation(
            json!({"action":"observe","executionId":id,"waitMs":0}),
            None,
        )
        .await;
    assert_eq!(pending["data"]["status"], "dispatch_pending");
    assert_eq!(pending["data"]["attention"], "pending_explicit_resume");
    assert_eq!(
        pending["data"]["availableActions"]["canResumePending"],
        true
    );
    assert_eq!(pending["data"]["availableActions"]["canCancel"], true);
    assert_eq!(count(dir.path(), "workspace_claims"), 1);
    let (fixed, release, fake) = fake_service(
        store.clone(),
        dir.path().join("agent-state.db"),
        "FIXED",
        "T",
        false,
        "paginated",
    )
    .await;
    let resumed = fixed
        .checked_operation(json!({"action":"resume_pending","executionId":id}), None)
        .await;
    assert_eq!(resumed["ok"], true, "{resumed}");
    release.send(()).unwrap();
    assert_eq!(final_row(&fixed, &id).await.status, "completed");
    drop(fixed);
    assert_eq!(
        fake.await
            .unwrap()
            .iter()
            .filter(|m| *m == "turn/start")
            .count(),
        1
    );
}

#[tokio::test]
async fn unbound_persisted_runtime_attempt_stays_fail_closed() {
    let (dir, store, service) = fixture().await;
    store
        .product_create_fresh(
            "e".into(),
            "a".into(),
            "k".into(),
            "hello".into(),
            "W".into(),
            w(dir.path(), "W"),
            1,
        )
        .await
        .unwrap();
    let db = rusqlite::Connection::open(dir.path().join("agent-state.db")).unwrap();
    db.execute("INSERT INTO runtime_instances(id,owner_host_instance_id,state,created_at,updated_at) VALUES ('runtime-e','fixture','running',1,1)",[]).unwrap();
    let before = store.execution("e".into()).await.unwrap().unwrap();
    let pending = service.observe("e".into(), false).await.unwrap();
    assert!(!pending.available_actions.can_resume_pending);
    let rejected = service.checked_operation(
        json!({"action":"resume_pending","executionId":"e"}), None,
    ).await;
    assert_eq!(rejected["ok"], false, "{rejected}");
    assert_eq!(store.execution("e".into()).await.unwrap().unwrap(), before);
    assert_eq!(count(dir.path(), "runtime_instances"), 1);
    let provider = crate::agent::codex::provider::CodexProvider { runtime_pool: Default::default(),
        store: store.clone(),
        executable: "unused".into(),
        owner: "fixture".into(),
    };
    provider.failed("e").await.unwrap();
    let response = service
        .checked_operation(
            json!({"action":"observe","executionId":"e","waitMs":0}),
            None,
        )
        .await;
    assert_eq!(response["data"]["status"], "reconciling");
    assert_eq!(
        response["data"]["availableActions"]["canResumePending"],
        false
    );
    assert_eq!(count(dir.path(), "workspace_claims"), 1);
}

#[tokio::test]
async fn start_workspace_id_is_required_and_retry_cannot_rebind() {
    let (dir, _store, mut service) = fixture().await;
    let missing = service.operation(start("a", "k"), w(dir.path(), "A")).await;
    assert_eq!(missing["error"]["code"], "AGENT_INVALID_ARGUMENT");
    service.manager.backend_error = Some("BACKEND_UNAVAILABLE: fixture".into());
    let args =
        json!({"action":"start","agentId":"a","requestKey":"k","prompt":"hello","workspaceId":"A"});
    let accepted = service
        .checked_operation(args.clone(), w(dir.path(), "A"))
        .await;
    let id = accepted["error"]["executionId"].clone();
    let retry = service
        .checked_operation(args.clone(), w(dir.path(), "B"))
        .await;
    assert_eq!(retry["data"]["executionId"], id);
    let mut changed = args;
    changed["workspaceId"] = json!("B");
    let rejected = service.checked_operation(changed, w(dir.path(), "B")).await;
    assert_eq!(rejected["error"]["code"], "AGENT_REQUEST_KEY_CONFLICT");
    no_dispatch(&rejected["control"], false);
    assert_eq!(count(dir.path(), "executions"), 1);
}
