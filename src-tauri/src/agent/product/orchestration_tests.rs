use super::*;
use crate::mcp::orchestration_tests::{active, fixture};
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn public_transport_preserves_context_idempotency_and_continuation_pipeline() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let store = StateStore::open(root.into()).await.unwrap();
    store
        .create_work_run(
            "work".into(),
            "W".into(),
            root.to_string_lossy().into(),
            "title".into(),
            None,
            1,
        )
        .await
        .unwrap();
    let (s, release, fake) = fake_service(
        store.clone(),
        root.join("agent-state.db"),
        "PUBLIC1",
        "T1",
        false,
        "paginated",
    )
    .await;
    let broker = fixture(root);
    assert!(broker.product.set(Arc::new(s)).is_ok());
    let upstream = active(&broker, root).await;
    std::fs::write(root.join("source.txt"), b"abc").unwrap();
    let context = json!({"summary":"Host reference","files":[{"path":"source.txt","sha256":"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"}]});
    let request = json!({"action":"start","workRunId":"work","requestKey":"key","prompt":"change task","context":context});
    let mut stale = request.clone();
    stale["context"]["files"][0]["sha256"] = json!("0".repeat(64));
    let failure = broker
        .call_tool("agent_execute", stale, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(failure["error"]["code"], "CONTEXT_STALE");
    assert_eq!(
        failure["control"],
        json!({"requestAccepted":false,"providerInvoked":false,"dispatchCertainty":"not_dispatched","nextAction":{"action":"correct_input"}})
    );
    assert!(
        store
            .product_read(None, None, None, 100)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        store
            .workspace_claim(root.to_string_lossy().into())
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .work_execution_links("work".into())
            .await
            .unwrap()
            .is_empty()
    );
    for invalid in [
        json!({"files":[{"path":"../escape","sha256":"bad"}]}),
        json!({"summary":" "}),
    ] {
        let mut request = request.clone();
        request["context"] = invalid;
        assert_eq!(
            broker
                .call_tool("agent_execute", request, CancellationToken::new())
                .await
                .unwrap()["error"]["code"],
            "WORK_INVALID_ARGUMENT"
        );
    }
    let first = broker
        .call_tool("agent_execute", request.clone(), CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(first["ok"], true);
    let id = first["data"]["executionId"].as_str().unwrap().to_string();
    assert!(
        first["data"]["prompt"]
            .as_str()
            .unwrap()
            .starts_with("Host verified context:")
    );
    assert!(
        store
            .work_execution_link(id.clone())
            .await
            .unwrap()
            .unwrap()
            .delegation_context_json
            .is_some()
    );
    std::fs::write(root.join("source.txt"), b"changed").unwrap();
    assert_eq!(
        broker
            .call_tool("agent_execute", request.clone(), CancellationToken::new())
            .await
            .unwrap()["data"]["executionId"],
        id
    );
    let mut conflict = request.clone();
    conflict["prompt"] = json!("other");
    assert_eq!(
        broker
            .call_tool("agent_execute", conflict, CancellationToken::new())
            .await
            .unwrap()["error"]["code"],
        "EXECUTION_REQUEST_KEY_CONFLICT"
    );
    release.send(()).unwrap();
    let parent = final_row(broker.product.get().unwrap(), &id).await;
    let parent_row = store.execution(id.clone()).await.unwrap();
    let result = broker
        .call_tool(
            "agent_query",
            json!({"action":"get","executionId":id,"includeResult":true}),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(result["data"]["finalResult"].is_object());
    assert_eq!(
        broker
            .call_tool(
                "agent_query",
                json!({"action":"list","workRunId":"work"}),
                CancellationToken::new()
            )
            .await
            .unwrap()["data"]["executions"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    drop(broker);
    upstream.abort();
    assert_eq!(
        fake.await
            .unwrap()
            .iter()
            .filter(|m| *m == "turn/start")
            .count(),
        1
    );
    let (s, release, fake) = fake_service(
        store.clone(),
        root.join("agent-state.db"),
        "PUBLIC2",
        "T2",
        true,
        "paginated",
    )
    .await;
    let broker = fixture(root);
    assert!(broker.product.set(Arc::new(s)).is_ok());
    let next=broker.call_tool("agent_execute",json!({"action":"continue","workRunId":"work","parentExecutionId":id,"requestKey":"next","prompt":"continue task"}),CancellationToken::new()).await.unwrap();
    assert_eq!(next["ok"], true);
    let next_id = next["data"]["executionId"].as_str().unwrap();
    assert_ne!(next_id, id);
    assert_eq!(next["data"]["threadId"], json!(parent.thread_id));
    assert_eq!(store.execution(id).await.unwrap(), parent_row);
    release.send(()).unwrap();
    final_row(broker.product.get().unwrap(), next_id).await;
    drop(broker);
    let methods = fake.await.unwrap();
    assert_eq!(methods.iter().filter(|m| *m == "turn/start").count(), 1);
    assert_eq!(methods.iter().filter(|m| *m == "thread/resume").count(), 1);
}

#[tokio::test]
async fn accepted_transport_errors_project_original_control_instead_of_rejecting() {
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::open(dir.path().into()).await.unwrap();
    store
        .product_create_fresh(
            "E".into(),
            "A".into(),
            "k".into(),
            "p".into(),
            "W".into(),
            w(dir.path(), "W"),
            1,
        )
        .await
        .unwrap();
    let service = AgentProductService::new(store.clone());
    let db = rusqlite::Connection::open(dir.path().join("agent-state.db")).unwrap();
    for state in ["not_dispatched", "dispatching", "uncertain", "dispatched"] {
        db.execute(
            "UPDATE executions SET dispatch_state=?1 WHERE id='E'",
            [state],
        )
        .unwrap();
        let expected =
            serde_json::to_value(service.observe("E".into(), false).await.unwrap().control)
                .unwrap();
        let value = service
            .adapter_error_response(ProductError::accepted(
                "SQL /private/path".into(),
                "E".into(),
            ))
            .await;
        assert_eq!(value["control"], expected);
        assert_eq!(value["error"]["executionId"], "E");
        assert!(!value.to_string().contains("/private/path"));
    }
    let value = service
        .adapter_error_response(ProductError::accepted(
            "SQL /secret".into(),
            "missing".into(),
        ))
        .await;
    assert_eq!(value["control"]["requestAccepted"], true);
    assert_eq!(value["control"]["providerInvoked"], Value::Null);
    assert_eq!(value["control"]["dispatchCertainty"], "uncertain");
}

#[tokio::test]
async fn public_finish_and_work_cancel_leave_execution_and_claim_to_agent_cancel() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let store = StateStore::open(root.into()).await.unwrap();
    store
        .create_work_run(
            "work".into(),
            "W".into(),
            root.to_string_lossy().into(),
            "title".into(),
            None,
            1,
        )
        .await
        .unwrap();
    store
        .product_create_fresh_with_work(
            "E".into(),
            "work".into(),
            "k".into(),
            "p".into(),
            "W".into(),
            w(root, "W"),
            Some(
                crate::agent::store::transactions::product::WorkExecutionContext {
                    work_run_id: "work".into(),
                    parent_execution_id: None,
                    delegation_context_json: None,
                },
            ),
            1,
        )
        .await
        .unwrap();
    let (s, _release, fake) = fake_service(
        store.clone(),
        root.join("agent-state.db"),
        "UNUSED",
        "T",
        false,
        "paginated",
    )
    .await;
    let broker = fixture(root);
    assert!(broker.product.set(Arc::new(s)).is_ok());
    let db = rusqlite::Connection::open(root.join("agent-state.db")).unwrap();
    let claim = store
        .workspace_claim(root.to_string_lossy().into())
        .await
        .unwrap()
        .unwrap();
    for status in ["running", "unknown"] {
        db.execute("UPDATE executions SET status=?1 WHERE id='E'", [status])
            .unwrap();
        let execution = store.execution("E".into()).await.unwrap();
        let work = store.work_run("work".into()).await.unwrap();
        for outcome in ["completed", "failed"] {
            let result = broker
                .call_tool(
                    "work_update",
                    json!({"action":"finish","workRunId":"work","outcome":outcome}),
                    CancellationToken::new(),
                )
                .await
                .unwrap();
            assert_eq!(result["error"]["code"], "WORK_HAS_ACTIVE_EXECUTIONS");
            assert_eq!(store.execution("E".into()).await.unwrap(), execution);
            assert_eq!(store.work_run("work".into()).await.unwrap(), work);
            assert_eq!(
                store
                    .workspace_claim(root.to_string_lossy().into())
                    .await
                    .unwrap()
                    .unwrap()
                    .execution_id,
                claim.execution_id
            );
        }
    }
    db.execute("UPDATE executions SET status='running' WHERE id='E'", [])
        .unwrap();
    let before = store.execution("E".into()).await.unwrap();
    let result = broker
        .call_tool(
            "work_update",
            json!({"action":"cancel","workRunId":"work"}),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(result["data"]["workRun"]["status"], "cancelled");
    assert_eq!(store.execution("E".into()).await.unwrap(), before);
    assert_eq!(
        store
            .workspace_claim(root.to_string_lossy().into())
            .await
            .unwrap()
            .unwrap()
            .execution_id,
        claim.execution_id
    );
    // Restore the undispatched fixture so the existing cancellation path can settle it.
    db.execute(
        "UPDATE executions SET status='dispatch_pending' WHERE id='E'",
        [],
    )
    .unwrap();
    let result = broker
        .call_tool(
            "agent_execute",
            json!({"action":"cancel","workRunId":"work","executionId":"E"}),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(result["data"]["status"], "cancelled");
    assert!(
        store
            .workspace_claim(root.to_string_lossy().into())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        store
            .work_execution_links("work".into())
            .await
            .unwrap()
            .len(),
        1
    );
    drop(broker);
    assert!(fake.await.unwrap().is_empty());
}

#[tokio::test]
async fn public_resume_pending_http_reuses_guard_and_returns_success_is_error_false() {
    use rmcp::{
        ServiceExt, model::CallToolRequestParams, transport::StreamableHttpClientTransport,
    };
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let store = StateStore::open(root.into()).await.unwrap();
    store
        .create_work_run(
            "work".into(),
            "W".into(),
            root.to_string_lossy().into(),
            "title".into(),
            None,
            1,
        )
        .await
        .unwrap();
    store
        .product_create_fresh_with_work(
            "E".into(),
            "work".into(),
            "k".into(),
            "p".into(),
            "W".into(),
            w(root, "W"),
            Some(
                crate::agent::store::transactions::product::WorkExecutionContext {
                    work_run_id: "work".into(),
                    parent_execution_id: None,
                    delegation_context_json: None,
                },
            ),
            1,
        )
        .await
        .unwrap();
    let (s, release, fake) = fake_service(
        store.clone(),
        root.join("agent-state.db"),
        "PUBLIC_RESUME",
        "T",
        false,
        "paginated",
    )
    .await;
    let broker = fixture(root);
    assert!(broker.product.set(Arc::new(s)).is_ok());
    broker.start().await.unwrap();
    let client = ()
        .serve(StreamableHttpClientTransport::from_uri(format!(
            "http://127.0.0.1:{}/mcp",
            broker.config().broker.port
        )))
        .await
        .unwrap();
    let request = || {
        CallToolRequestParams::new("agent_execute").with_arguments(
            json!({"action":"resume_pending","workRunId":"work","executionId":"E"})
                .as_object()
                .unwrap()
                .clone(),
        )
    };
    let db = rusqlite::Connection::open(root.join("agent-state.db")).unwrap();
    for state in ["uncertain", "dispatched"] {
        db.execute(
            "UPDATE executions SET dispatch_state=?1 WHERE id='E'",
            [state],
        )
        .unwrap();
        let before = store.execution("E".into()).await.unwrap();
        let rejected = client.call_tool(request()).await.unwrap();
        assert_eq!(rejected.is_error, Some(true));
        assert_eq!(
            rejected.structured_content.unwrap()["error"]["code"],
            "AGENT_RESUME_NOT_ALLOWED"
        );
        assert_eq!(store.execution("E".into()).await.unwrap(), before);
    }
    db.execute(
        "UPDATE executions SET dispatch_state='not_dispatched' WHERE id='E'",
        [],
    )
    .unwrap();
    let accepted = client.call_tool(request()).await.unwrap();
    assert_eq!(accepted.is_error, Some(false));
    assert_eq!(
        accepted.structured_content.unwrap()["data"]["executionId"],
        "E"
    );
    release.send(()).unwrap();
    final_row(broker.product.get().unwrap(), "E").await;
    assert_eq!(
        store
            .product_read(None, None, None, 100)
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        store
            .work_execution_links("work".into())
            .await
            .unwrap()
            .len(),
        1
    );
    client.cancel().await.unwrap();
    broker.stop().await.unwrap();
    drop(broker);
    let methods = fake.await.unwrap();
    assert_eq!(methods.iter().filter(|m| *m == "turn/start").count(), 1);
}

// Local public-transport E2E with fake upstreams, not real Codex or crash evidence.
#[tokio::test]
async fn public_vertical_work_source_start_continue_acceptance_e2e() {
    use crate::mcp::{Broker, orchestration_tests::active_with_read_file};
    async fn call(broker: &Broker, tool: &str, args: Value) -> Value {
        broker
            .call_tool(tool, args, CancellationToken::new())
            .await
            .unwrap()
    }
    async fn completed(broker: &Broker, id: &str) -> Value {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let response = call(
                    broker,
                    "agent_query",
                    json!({"action":"observe","executionId":id,"waitMs":500,"includeResult":false}),
                )
                .await;
                assert_eq!(response["ok"], true, "{response}");
                if response["data"]["status"] == "completed" {
                    return response["data"].clone();
                }
            }
        })
        .await
        .expect("public observe did not reach completed")
    }
    fn work_view_is_public(view: &Value) {
        let fields = [
            "workRunId",
            "workspaceId",
            "title",
            "goal",
            "status",
            "revision",
            "acceptance",
            "createdAt",
            "updatedAt",
            "completedAt",
        ];
        let object = view.as_object().unwrap();
        assert_eq!(object.len(), fields.len());
        for field in fields {
            assert!(object.contains_key(field), "missing {field}");
        }
        for private in [
            "canonical_workspace_root",
            "canonicalWorkspaceRoot",
            "acceptance_json",
            "acceptanceJson",
        ] {
            assert!(view.get(private).is_none());
        }
    }

    fn assert_one_provider_turn(methods: &[String], thread_method: &str) {
        let expected = [
            ("initialize", 1),
            ("initialized", 1),
            (thread_method, 1),
            ("turn/start", 1),
            ("thread/read", 1),
            ("thread/turns/list", 1),
            ("thread/items/list", 1),
            ("thread/backgroundTerminals/clean", 1),
            ("thread/backgroundTerminals/list", 1),
        ];
        assert_eq!(
            methods.len(),
            expected.len(),
            "unexpected Provider calls: {methods:?}"
        );
        for (method, count) in expected {
            assert_eq!(
                methods.iter().filter(|m| *m == method).count(),
                count,
                "{method}: {methods:?}"
            );
        }
    }

    let dir = tempfile::tempdir().unwrap();
    let canonical_root = dir.path().canonicalize().unwrap();
    let root = canonical_root.as_path();
    // Known raw bytes include BOM/CRLF, while the fake returns normalized line text.
    let raw = b"\xef\xbb\xbfabc\r\nsecond\r\n";
    std::fs::write(root.join("context.txt"), raw).unwrap();
    let store = StateStore::open(root.into()).await.unwrap();
    let (s, release, fake) = fake_service(
        store.clone(),
        root.join("agent-state.db"),
        "VERTICAL1",
        "TURN1",
        false,
        "paginated",
    )
    .await;
    let broker = fixture(root);
    assert!(broker.product.set(Arc::new(s)).is_ok());
    let (upstream_process, upstream) = active_with_read_file(&broker, root).await;

    let begun = call(
        &broker,
        "work_update",
        json!({"action":"begin","workspaceId":"W","title":"Vertical E2E"}),
    )
    .await;
    assert_eq!(begun["ok"], true, "{begun}");
    let initial_work = begun["data"]["workRun"].clone();
    work_view_is_public(&initial_work);
    let work_id = initial_work["workRunId"].as_str().unwrap().to_owned();
    assert_eq!(initial_work["status"], "active");
    assert_eq!(
        call(
            &broker,
            "work_query",
            json!({"action":"get","workRunId":work_id})
        )
        .await["data"]["workRun"],
        initial_work
    );

    let read_args =
        json!({"relative_path":"context.txt","start_line":1,"end_line":1,"max_bytes":6});
    let source = call(&broker, "source_read_file", read_args.clone()).await;
    assert_eq!(source["path"], "context.txt");
    assert_eq!(source["text"], "second");
    assert_eq!(source["truncated"], false);
    let sha = source["sha256"].as_str().unwrap();
    assert_eq!(sha.len(), 64);
    assert!(
        sha.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    );
    // An independent known-byte assertion; the context below uses only Source output.
    assert_eq!(
        sha,
        "98c468325cef7f63ade1c10cab22b29983bf78710ce6f50b9fdea26322c8e19e"
    );
    let full_source = call(
        &broker,
        "source_read_file",
        json!({"relative_path":"context.txt"}),
    )
    .await;
    assert_eq!(full_source["sha256"], source["sha256"]);
    assert_ne!(full_source["text"], source["text"]);
    let context = json!({"summary":"Host selected a versioned source reference","files":[{"path":source["path"],"sha256":source["sha256"]}]});
    let start = json!({"action":"start","workRunId":work_id,"requestKey":"vertical-start","prompt":"Run the agreed test task.","context":context});
    let accepted = call(&broker, "agent_execute", start.clone()).await;
    assert_eq!(accepted["ok"], true, "{accepted}");
    let e1 = accepted["data"]["executionId"].as_str().unwrap().to_owned();
    let link1 = store
        .work_execution_link(e1.clone())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(link1.work_run_id, work_id);
    assert_eq!(link1.parent_execution_id, None);
    assert_eq!(
        serde_json::from_str::<Value>(link1.delegation_context_json.as_deref().unwrap()).unwrap(),
        context
    );
    assert!(store.execution(e1.clone()).await.unwrap().is_some());
    assert_eq!(
        call(&broker, "agent_execute", start.clone()).await["data"]["executionId"],
        e1
    );

    let early_finish = json!({"action":"finish","workRunId":work_id,"outcome":"completed","acceptance":{"summary":"Host review","executionIds":[e1]}});
    let blocked = call(&broker, "work_update", early_finish.clone()).await;
    assert_eq!(blocked["ok"], false);
    assert_eq!(blocked["error"]["code"], "WORK_HAS_ACTIVE_EXECUTIONS");
    assert_eq!(
        call(
            &broker,
            "work_query",
            json!({"action":"get","workRunId":work_id})
        )
        .await["data"]["workRun"],
        initial_work
    );
    let observed = tokio::time::timeout(
        Duration::from_secs(2),
        call(
            &broker,
            "agent_query",
            json!({"action":"observe","executionId":e1,"waitMs":0,"includeResult":false}),
        ),
    )
    .await
    .unwrap();
    assert_eq!(observed["ok"], true);
    assert_ne!(observed["data"]["status"], "completed");
    assert_eq!(observed["control"]["requestAccepted"], true);
    release.send(()).unwrap();
    let terminal1 = completed(&broker, &e1).await;
    assert!(terminal1["finalResult"].is_null());
    assert_eq!(terminal1["resultAvailable"], true);
    let parent_row = store.execution(e1.clone()).await.unwrap();
    let result1 = call(
        &broker,
        "agent_query",
        json!({"action":"get","executionId":e1,"includeResult":true}),
    )
    .await;
    assert_eq!(result1["ok"], true);
    assert!(result1["data"]["finalResult"].is_object());
    for include in [false, true] {
        let read = call(
            &broker,
            "agent_query",
            json!({"action":"get","executionId":e1,"includeResult":include}),
        )
        .await;
        assert_eq!(
            read["data"]["finalResult"],
            if include {
                result1["data"]["finalResult"].clone()
            } else {
                Value::Null
            }
        );
    }
    assert_eq!(
        call(&broker, "agent_execute", start).await["data"]["executionId"],
        e1
    );
    assert_eq!(store.execution(e1.clone()).await.unwrap(), parent_row);
    assert!(
        store
            .workspace_claim(root.to_string_lossy().into())
            .await
            .unwrap()
            .is_none()
    );
    drop(broker);
    upstream.abort();
    drop(upstream_process);
    let methods1 = fake.await.unwrap();
    assert_one_provider_turn(&methods1, "thread/start");
    assert_eq!(methods1.iter().filter(|m| *m == "turn/start").count(), 1);
    assert_eq!(methods1.iter().filter(|m| *m == "thread/start").count(), 1);
    assert!(!methods1.iter().any(|m| m == "thread/resume"));

    // Service replacement with the same durable store, not a host-crash simulation.
    let (s, release, fake) = fake_service(
        store.clone(),
        root.join("agent-state.db"),
        "VERTICAL2",
        "TURN2",
        true,
        "paginated",
    )
    .await;
    let broker = fixture(root);
    assert!(broker.product.set(Arc::new(s)).is_ok());
    let (upstream_process, upstream) = active_with_read_file(&broker, root).await;
    let continued = call(&broker, "agent_execute", json!({"action":"continue","workRunId":work_id,"parentExecutionId":e1,"requestKey":"vertical-continue","prompt":"Run the follow-up test task."})).await;
    assert_eq!(continued["ok"], true, "{continued}");
    let e2 = continued["data"]["executionId"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_ne!(e2, e1);
    assert_eq!(continued["data"]["threadId"], terminal1["threadId"]);
    let link2 = store
        .work_execution_link(e2.clone())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(link2.work_run_id, work_id);
    assert_eq!(link2.parent_execution_id.as_deref(), Some(e1.as_str()));
    assert_eq!(store.execution(e1.clone()).await.unwrap(), parent_row);
    let blocked = call(&broker, "work_update", early_finish).await;
    assert_eq!(blocked["error"]["code"], "WORK_HAS_ACTIVE_EXECUTIONS");
    assert_eq!(
        call(
            &broker,
            "work_query",
            json!({"action":"get","workRunId":work_id})
        )
        .await["data"]["workRun"],
        initial_work
    );
    release.send(()).unwrap();
    let terminal2 = completed(&broker, &e2).await;
    assert!(terminal2["finalResult"].is_null());
    let child_row = store.execution(e2.clone()).await.unwrap();
    let result2 = call(
        &broker,
        "agent_query",
        json!({"action":"get","executionId":e2,"includeResult":true}),
    )
    .await;
    assert_eq!(result2["ok"], true);
    assert!(result2["data"]["finalResult"].is_object());
    assert_eq!(
        call(&broker, "source_read_file", read_args).await["sha256"],
        source["sha256"]
    );
    let listed = call(
        &broker,
        "agent_query",
        json!({"action":"list","workRunId":work_id}),
    )
    .await;
    assert_eq!(listed["data"]["executions"].as_array().unwrap().len(), 2);

    let finished = call(&broker, "work_update", json!({"action":"finish","workRunId":work_id,"outcome":"completed","acceptance":{"summary":"Host reviewed both execution results and source reference","executionIds":[e1,e2]}})).await;
    assert_eq!(finished["ok"], true, "{finished}");
    let work = &finished["data"]["workRun"];
    work_view_is_public(work);
    assert_eq!(work["status"], "completed");
    assert_eq!(
        work["revision"].as_i64().unwrap(),
        initial_work["revision"].as_i64().unwrap() + 1
    );
    assert_eq!(work["acceptance"]["decision"], "accepted");
    assert_eq!(work["acceptance"]["executionIds"], json!([e1, e2]));
    assert!(!work["acceptance"]["summary"].as_str().unwrap().is_empty());
    assert!(work["completedAt"].is_i64());
    assert_eq!(work["acceptance"]["acceptedAt"], work["completedAt"]);
    assert_eq!(work["updatedAt"], work["completedAt"]);
    assert_eq!(
        &call(
            &broker,
            "work_query",
            json!({"action":"get","workRunId":work_id})
        )
        .await["data"]["workRun"],
        work
    );
    let rejected = call(&broker, "agent_execute", json!({"action":"start","workRunId":work_id,"requestKey":"after-finish","prompt":"Must not dispatch."})).await;
    assert_eq!(rejected["ok"], false);
    assert_eq!(rejected["error"]["code"], "WORK_NOT_ACTIVE");
    assert_eq!(rejected["control"]["requestAccepted"], false);
    assert_eq!(rejected["control"]["providerInvoked"], false);
    assert_eq!(store.execution(e1.clone()).await.unwrap(), parent_row);
    assert_eq!(store.execution(e2.clone()).await.unwrap(), child_row);
    assert!(
        store
            .workspace_claim(root.to_string_lossy().into())
            .await
            .unwrap()
            .is_none()
    );
    let links = store.work_execution_links(work_id.clone()).await.unwrap();
    assert_eq!(links.len(), 2);
    let db = rusqlite::Connection::open(root.join("agent-state.db")).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM executions", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        2
    );
    for key in ["vertical-start", "vertical-continue"] {
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM executions WHERE agent_id=?1 AND request_key=?2",
                [&work_id, key],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
    }
    assert_eq!(std::fs::read(root.join("context.txt")).unwrap(), raw);
    drop(broker);
    upstream.abort();
    drop(upstream_process);
    let methods2 = fake.await.unwrap();
    assert_one_provider_turn(&methods2, "thread/resume");
    assert_eq!(methods2.iter().filter(|m| *m == "turn/start").count(), 1);
    assert_eq!(methods2.iter().filter(|m| *m == "thread/resume").count(), 1);
    assert!(!methods2.iter().any(|m| m == "thread/start"));
}
