use super::*;
use crate::agent::{execution::CreateExecutionInput, task_manager::AgentTaskManager};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream};

fn run(future: impl std::future::Future<Output = ()>) {
    tokio::runtime::Runtime::new().unwrap().block_on(future)
}
fn input(root: &std::path::Path) -> CreateExecutionInput {
    serde_json::from_value(json!({"agent_id":"slice-agent","request_key":"slice-key","prompt":"Reply exactly VERTICAL_SLICE_OK. Do not use tools or modify files.","execution_profile":{},"workspace_id":"isolated","canonical_workspace_root":root.to_str().unwrap(),"mode":"read_only"})).unwrap()
}
async fn recv(s: &mut BufReader<DuplexStream>) -> Value {
    let mut line = String::new();
    s.read_line(&mut line).await.unwrap();
    serde_json::from_str(&line).unwrap()
}
async fn send(s: &mut BufReader<DuplexStream>, value: Value) {
    s.write_all(&super::super::protocol::encode(&value).unwrap())
        .await
        .unwrap();
}
async fn reply(s: &mut BufReader<DuplexStream>, req: &Value, value: Value) {
    send(s, json!({"id":req["id"],"result":value})).await;
}
fn turn(status: &str) -> Value {
    json!({"id":"TURN","status":status,"items":[],"itemsView":"summary"})
}

// Hold only turn/start flush until the fake server has persisted terminal evidence.
struct DelayedTurnFlush {
    inner: tokio::io::WriteHalf<DuplexStream>,
    release: Option<tokio::sync::oneshot::Receiver<()>>,
    turn_written: bool,
}
impl tokio::io::AsyncWrite for DelayedTurnFlush {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        if buf.windows(b"turn/start".len()).any(|v| v == b"turn/start") {
            self.turn_written = true;
        }
        std::pin::Pin::new(&mut self.inner).poll_write(cx, buf)
    }
    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        if self.turn_written && let Some(release) = &mut self.release {
            match std::future::Future::poll(std::pin::Pin::new(release), cx) {
                std::task::Poll::Pending => return std::task::Poll::Pending,
                std::task::Poll::Ready(result) => result.expect("fake flush release"),
            }
            self.release = None;
        }
        std::pin::Pin::new(&mut self.inner).poll_flush(cx)
    }
    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

async fn slice_case(case: &'static str) {
    let failed = case.starts_with("failed");
    let late_deadline = case.ends_with("late-deadline");
    let invalid_ack = matches!(case, "wrong-ack-late-deadline" | "error-ack-late-deadline" | "invalid-ack-late-deadline");
    let terminal_status = if failed { "failed" } else { "completed" };
    let temp = tempfile::tempdir().unwrap();
    let store = StateStore::open(temp.path().into()).await.unwrap();
    let manager = AgentTaskManager::new(store.clone(), "does-not-exist.exe".into());
    let mut request = input(temp.path());
    if case == "continue-old-turn" {
        request.mode = crate::agent::execution::ExecutionMode::WorkspaceWrite;
    }
    let created = if case == "continue-old-turn" {
        let source = store
            .product_create_fresh(
                "SOURCE".into(),
                request.agent_id.clone(),
                "source-key".into(),
                "source".into(),
                request.workspace_id.clone(),
                Some(crate::agent::store::transactions::product::WorkspaceSnapshot {
                    id: request.workspace_id.clone(),
                    root: request.canonical_workspace_root.clone(),
                }),
                1,
            )
            .await
            .unwrap();
        let db = rusqlite::Connection::open(temp.path().join("agent-state.db")).unwrap();
        db.execute(
            "INSERT INTO runtime_instances(id,owner_host_instance_id,state,created_at,updated_at) VALUES ('R0','fixture','terminated',1,1)",
            [],
        )
        .unwrap();
        let final_result = json!({
            "historyMode":"paginated","executionId":source.execution_id,
            "threadId":"THREAD","turnId":"PRIOR-TURN","sourceRuntimeId":"R0"
        })
        .to_string();
        db.execute(
            "UPDATE executions SET status='completed',dispatch_state='dispatched',runtime_instance_id='R0',
             thread_id='THREAD',turn_id='PRIOR-TURN',provider_terminal_status='completed',
             release_evidence_state='complete',release_evidence_kind='same_runtime_cleanup',
             release_evidence_json='{}',result_completeness='complete',final_result_json=?1,completed_at=2
             WHERE id='SOURCE'",
            [&final_result],
        )
        .unwrap();
        db.execute("DELETE FROM workspace_claims WHERE execution_id='SOURCE'", [])
            .unwrap();
        store
            .product_create_continuation(
                "CONTINUE".into(),
                "SOURCE".into(),
                request.request_key.clone(),
                request.prompt.clone(),
                3,
            )
            .await
            .unwrap()
    } else {
        manager.create(request.clone()).await.unwrap()
    };
    assert!(created.created);
    assert_eq!(created.execution.status, "dispatch_pending");
    assert_eq!(created.execution.dispatch_state, "not_dispatched");
    assert!(created.execution.runtime_instance_id.is_none());
    assert_eq!(
        store
            .workspace_claim(request.canonical_workspace_root.clone())
            .await
            .unwrap()
            .unwrap()
            .execution_id,
        created.execution_id
    );
    let connection = rusqlite::Connection::open(temp.path().join("agent-state.db")).unwrap();
    if case != "resume" {
        connection.execute("INSERT INTO runtime_instances(id,owner_host_instance_id,state,created_at,updated_at) VALUES ('R1','fixture','running',1,1)",[]).unwrap();
    }
    if matches!(case, "rollback" | "failed-rollback") {
        connection.execute_batch("CREATE TRIGGER test_rollback BEFORE DELETE ON workspace_claims BEGIN SELECT RAISE(ABORT,'injected final rollback'); END;").unwrap();
    }
    let (wire, server) = tokio::io::duplex(128 * 1024);
    let (read, write) = tokio::io::split(wire);
    let (flush_release, flush_wait) = tokio::sync::oneshot::channel();
    let client = std::sync::Arc::new(Client::transport(
        "R1".into(),
        read,
        DelayedTurnFlush {
            inner: write,
            release: (case == "failed-delayed-flush").then_some(flush_wait),
            turn_written: false,
        },
        tokio::io::empty(),
    ));
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let (continue_tx, continue_rx) = tokio::sync::oneshot::channel();
    let fake_store = store.clone();
    let id = created.execution_id.clone();
    let request2 = request.clone();
    let fake = tokio::spawn(async move {
        let mut s = BufReader::new(server);
        let req = recv(&mut s).await;
        assert_eq!(req["method"], "initialize");
        if case == "resume" {
            entered_tx.send(()).unwrap();
            continue_rx.await.unwrap();
        }
        reply(&mut s,&req,json!({"userAgent":"fake","codexHome":"isolated","platformFamily":"windows","platformOs":"windows"})).await;
        assert_eq!(recv(&mut s).await["method"], "initialized");
        let req = recv(&mut s).await;
        if case == "continue-old-turn" {
            assert_eq!(req["method"], "thread/resume");
            assert_eq!(req["params"], json!({"threadId":"THREAD","excludeTurns":true}));
        } else {
            assert_eq!(req["method"], "thread/start");
            assert_eq!(req["params"]["historyMode"], "paginated");
            assert_eq!(req["params"]["ephemeral"], false);
            assert_eq!(req["params"]["sandbox"], "read-only");
            assert_eq!(req["params"]["cwd"], request2.canonical_workspace_root);
            assert_eq!(req["params"]["approvalPolicy"], "never");
            assert_eq!(req["params"]["dynamicTools"][0]["type"], "namespace");
            assert_eq!(req["params"]["dynamicTools"][0]["name"], "codex_app");
            assert_eq!(req["params"]["dynamicTools"].as_array().unwrap().len(), 1);
            assert_eq!(req["params"]["dynamicTools"][0]["tools"].as_array().unwrap().len(), 1);
            assert_eq!(req["params"]["dynamicTools"][0]["tools"][0]["name"], "set_thread_title");
        }
        let row = fake_store.execution(id.clone()).await.unwrap().unwrap();
        assert_eq!(row.runtime_instance_id.as_deref(), Some("R1"));
        assert_eq!(row.dispatch_state, "dispatching");
        assert_eq!(row.status, "dispatch_pending");
        if case == "resume" {
            assert!(
                AgentTaskManager::new(fake_store.clone(), "unused".into())
                    .resume_pending_execution(&id)
                    .await
                    .is_err()
            );
        }
        reply(
            &mut s,
            &req,
            json!({"thread":{"id":"THREAD","name":"初始名称","turns":[],"historyMode":"paginated"}}),
        )
        .await;
        let req = recv(&mut s).await;
        assert_eq!(req["method"], "turn/start");
        assert_eq!(req["params"]["threadId"], "THREAD");
        assert_eq!(req["params"]["approvalPolicy"], "never");
        assert_eq!(
            req["params"]["sandboxPolicy"],
            if case == "continue-old-turn" {
                json!({
                    "type":"workspaceWrite",
                    "writableRoots":[request2.canonical_workspace_root],
                    "networkAccess":true,
                    "excludeTmpdirEnvVar":false,
                    "excludeSlashTmp":false
                })
            } else {
                json!({"type":"readOnly","networkAccess":true})
            }
        );
        assert!(req["params"].get("cwd").is_none());
        assert_eq!(fake_store.product_read(Some(id.clone()), None, None, 1).await.unwrap()[0].thread_name.as_deref(), Some("初始名称"));
        send(&mut s, json!({"method":"thread/name/updated","params":{"threadId":"THREAD","threadName":"更新名称"}})).await;
        if case == "continue-old-turn" {
            let service = crate::agent::product::AgentProductService::new(fake_store.clone());
            tokio::time::timeout(Duration::from_secs(5), async {
                while fake_store
                    .product_read(Some(id.clone()), None, None, 1)
                    .await
                    .unwrap()[0]
                    .thread_name
                    .as_deref()
                    != Some("更新名称")
                {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            let snapshot = service
                .operation(
                    json!({"action":"observe","executionId":id,"waitMs":0}),
                    None,
                )
                .await;
            let activity_fault = crate::agent::store::ObservabilityFault::Activity;
            let permission_fault =
                crate::agent::store::ObservabilityFault::PermissionDiagnostic;
            fake_store.inject_observability_failure(activity_fault);
            fake_store.inject_observability_failure(permission_fault);
            let observe = service.operation(
                json!({"action":"observe","executionId":id,
                    "knownRevision":snapshot["data"]["revision"],"waitMs":120}),
                None,
            );
            let old_turn = async {
                for method in ["item/started", "item/completed"] {
                    send(&mut s, json!({"method":method,"params":{
                        "threadId":"THREAD","turnId":"PRIOR-TURN","item":{
                            "type":"commandExecution","id":"old","command":"cargo test","commandActions":[]
                        }
                    }})).await;
                }
                for (request_id, method, params) in [
                    ("old-command", "item/commandExecution/requestApproval", json!({
                        "itemId":"I","startedAtMs":1,"threadId":"THREAD","turnId":"PRIOR-TURN"
                    })),
                    ("old-file", "item/fileChange/requestApproval", json!({
                        "itemId":"I","startedAtMs":1,"threadId":"THREAD","turnId":"PRIOR-TURN"
                    })),
                    ("old-permissions", "item/permissions/requestApproval", json!({
                        "cwd":"C:\\workspace","itemId":"I","permissions":[],"startedAtMs":1,
                        "threadId":"THREAD","turnId":"PRIOR-TURN"
                    })),
                ] {
                    send(&mut s, json!({"id":request_id,"method":method,"params":params})).await;
                    let response = recv(&mut s).await;
                    assert_eq!(response["id"], request_id);
                    assert!(response.get("result").is_some());
                }
            };
            let (unchanged, ()) = tokio::join!(observe, old_turn);
            assert_eq!(unchanged["data"]["unchanged"], true);
            let row = fake_store.execution(id.clone()).await.unwrap().unwrap();
            assert!(row.turn_id.is_none());
            assert!(row.last_activity_at.is_none());
            assert!(row.activity_phase.is_none());
            assert!(row.tool_category.is_none());
            assert!(row.error_code.is_none());
            assert!(fake_store.observability_failure_pending(activity_fault));
            assert!(fake_store.observability_failure_pending(permission_fault));
            fake_store.clear_observability_failure(activity_fault);
            fake_store.clear_observability_failure(permission_fault);

            send(&mut s, json!({"method":"turn/started","params":{
                "threadId":"THREAD","turn":turn("inProgress")
            }})).await;
            tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    let row = fake_store.execution(id.clone()).await.unwrap().unwrap();
                    if row.turn_id.as_deref() == Some("TURN") && row.status == "running" {
                        break;
                    }
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            send(&mut s, json!({"method":"item/started","params":{
                "threadId":"THREAD","turnId":"TURN","item":{
                    "type":"commandExecution","id":"current","command":"cargo test","commandActions":[]
                }
            }})).await;
            tokio::time::timeout(Duration::from_secs(5), async {
                while fake_store.execution(id.clone()).await.unwrap().unwrap().tool_category.as_deref()
                    != Some("test")
                {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            send(&mut s, json!({"method":"thread/name/updated","params":{
                "threadId":"THREAD","threadName":"更新名称"
            }})).await;
        }
        if case == "title" {
            send(&mut s, json!({"method":"turn/started","params":{"threadId":"THREAD","turn":turn("inProgress")}})).await;
            tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    let row = fake_store.execution(id.clone()).await.unwrap().unwrap();
                    if row.status == "running" && row.dispatch_state == "dispatched" { break; }
                    tokio::task::yield_now().await;
                }
            }).await.unwrap();
            let before = fake_store.execution(id.clone()).await.unwrap().unwrap();
            let valid = json!({"threadId":"THREAD","turnId":"TURN","callId":"title-call","namespace":"codex_app","tool":"set_thread_title","arguments":{"title":"  新标题  "}});
            for (n, params) in [
                json!(null), json!({}),
                { let mut p = valid.clone(); p["threadId"] = json!("CHILD"); p },
                { let mut p = valid.clone(); p["threadId"] = json!("UNOWNED"); p },
                { let mut p = valid.clone(); p["turnId"] = json!("STALE-TURN"); p },
                { let mut p = valid.clone(); p["tool"] = json!("future_tool"); p },
                { let mut p = valid.clone(); p["arguments"] = json!({"title":42}); p },
                { let mut p = valid.clone(); p["arguments"] = json!({"title":"  "}); p },
                { let mut p = valid.clone(); p["arguments"] = json!({"title":"a\nb"}); p },
                { let mut p = valid.clone(); p["arguments"] = json!({"title":"中".repeat(201)}); p },
                { let mut p = valid.clone(); p["arguments"] = json!({"title":"ok","threadId":"CHILD"}); p },
            ].into_iter().enumerate() {
                send(&mut s, json!({"id":format!("bad-{n}"),"method":"item/tool/call","params":params})).await;
                let rejected = recv(&mut s).await;
                assert_eq!(rejected["id"], format!("bad-{n}")); // No rename RPC.
                assert_eq!(rejected["result"]["success"], false);
                assert!(rejected.to_string().len() < 350);
            }
            assert_eq!(fake_store.execution(id.clone()).await.unwrap().unwrap(), before);
            // An ordinary rename RPC error is a tool failure, not a Provider failure.
            send(&mut s, json!({"id":"denied","method":"item/tool/call","params":valid})).await;
            let rename = recv(&mut s).await;
            assert_eq!(rename["method"], "thread/name/set");
            assert_eq!(rename["params"], json!({"threadId":"THREAD","name":"新标题"}));
            send(&mut s, json!({"id":rename["id"],"error":{"code":-32000,"message":"fixture refusal"}})).await;
            assert_eq!(recv(&mut s).await["result"]["success"], false);
            send(&mut s, json!({"id":"valid","method":"item/tool/call","params":valid})).await;
            let rename = recv(&mut s).await;
            assert_eq!(rename["method"], "thread/name/set");
            assert_eq!(rename["params"], json!({"threadId":"THREAD","name":"新标题"}));
            reply(&mut s, &rename, json!({})).await;
            send(&mut s, json!({"method":"thread/name/updated","params":{"threadId":"THREAD","threadName":"新标题"}})).await;
            let response = recv(&mut s).await;
            assert_eq!(response["id"], "valid");
            assert_eq!(response["result"]["success"], true);
            tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    if fake_store.product_read(Some(id.clone()), None, None, 1).await.unwrap()[0].thread_name.as_deref() == Some("新标题") { break; }
                    tokio::task::yield_now().await;
                }
            }).await.unwrap();
            assert_eq!(fake_store.execution(id.clone()).await.unwrap().unwrap(), before);
        }
        if case.starts_with("unowned-") {
            let bound = case == "unowned-bound";
            if bound {
                send(&mut s, json!({"method":"turn/started","params":{"threadId":"THREAD","turn":turn("inProgress")}})).await;
            }
            tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    let row = fake_store.execution(id.clone()).await.unwrap().unwrap();
                    let name = fake_store.product_read(Some(id.clone()), None, None, 1).await.unwrap()[0].thread_name.clone();
                    if row.dispatch_state == "dispatched" && name.as_deref() == Some("更新名称")
                        && (!bound || row.status == "running") { break; }
                    tokio::task::yield_now().await;
                }
            }).await.unwrap();
            let before = fake_store.execution(id.clone()).await.unwrap().unwrap();
            assert_eq!(before.turn_id.as_deref(), bound.then_some("TURN"));
            assert!(before.provider_terminal_status.is_none());
            assert!(before.provider_terminal_evidence_runtime_instance_id.is_none());
            assert_ne!(before.release_evidence_state, "complete");
            let service = crate::agent::product::AgentProductService::new(fake_store.clone());
            let snapshot = service.operation(json!({"action":"observe","executionId":id,"waitMs":0}), None).await;
            let observe = service.operation(json!({"action":"observe","executionId":id,
                "knownRevision":snapshot["data"]["revision"],"waitMs":120}), None);
            let traffic = async {
                let mut child_turn = turn("inProgress");
                child_turn["id"] = json!("CHILD-TURN");
                send(&mut s, json!({"method":"thread/started","params":{"thread":{"id":"CHILD","historyMode":"paginated","turns":[]}}})).await;
                send(&mut s, json!({"method":"turn/started","params":{"threadId":"CHILD","turn":child_turn}})).await;
                send(&mut s, json!({"method":"item/started","params":{"threadId":"CHILD","turnId":"CHILD-TURN","item":{"type":"commandExecution","id":"child-shell","command":"cargo test --token child-secret","commandActions":[]}}})).await;
                send(&mut s, json!({"method":"item/completed","params":{"threadId":"CHILD","turnId":"CHILD-TURN","item":{"type":"commandExecution","id":"child-shell","command":"cargo test --token child-secret","commandActions":[]}}})).await;
                if case != "unowned-unknown" {
                    // Discovery arrives AFTER the child's lifecycle has begun and
                    // must neither bind the Root Turn nor require known ancestry.
                    send(&mut s, json!({"method":"item/started","params":{"threadId":"THREAD","turnId":"TURN","item":{"type":"subAgentActivity","id":"review","kind":"started","agentThreadId":"CHILD","agentPath":"/root/review"}}})).await;
                    send(&mut s, json!({"method":"item/started","params":{"threadId":"UNKNOWN-PARENT","turnId":"OTHER","item":{"type":"subAgentActivity","id":"nested","kind":"started","agentThreadId":"GRANDCHILD","agentPath":"/root/nested"}}})).await;
                }
                send(&mut s, json!({"method":"error","params":{"threadId":"CHILD","turnId":"CHILD-TURN","willRetry":false,"error":{"message":"child diagnostic"}}})).await;
                send(&mut s, json!({"method":"thread/name/updated","params":{"threadId":"CHILD","threadName":"child name"}})).await;
                for status in ["completed", "failed", "interrupted"] {
                    child_turn["status"] = json!(status);
                    send(&mut s, json!({"method":"turn/completed","params":{"threadId":"CHILD","turn":child_turn}})).await;
                }
                // FIFO Root-name barrier proves all prior events were consumed.
                // Names do not change Execution revision or wake observe.
                send(&mut s, json!({"method":"thread/name/updated","params":{"threadId":"THREAD","threadName":"barrier"}})).await;
                tokio::time::timeout(Duration::from_secs(5), async {
                    while fake_store.product_read(Some(id.clone()), None, None, 1).await.unwrap()[0].thread_name.as_deref() != Some("barrier") {
                        tokio::task::yield_now().await;
                    }
                }).await.unwrap();
                assert_eq!(fake_store.execution(id.clone()).await.unwrap().unwrap(), before,
                    "unowned events must not change identity, diagnostic, revision, lifecycle or release evidence");
                assert!(fake_store.workspace_claim(request2.canonical_workspace_root.clone()).await.unwrap().is_some());
                send(&mut s, json!({"method":"thread/name/updated","params":{"threadId":"THREAD","threadName":"更新名称"}})).await;
            };
            let start = tokio::time::Instant::now();
            let (observed, ()) = tokio::join!(observe, traffic);
            assert_eq!(observed["data"]["unchanged"], true);
            assert!(start.elapsed() >= Duration::from_millis(120));
        }
        if matches!(case, "long" | "failed") {
            reply(&mut s, &req, json!({"turn":turn("inProgress")})).await;
            loop {
                let row = fake_store.execution(id.clone()).await.unwrap().unwrap();
                if row.turn_id.is_some() && row.dispatch_state == "dispatched" {
                    break;
                }
                tokio::task::yield_now().await;
            }
            if case == "long" {
                tokio::time::pause();
                tokio::time::advance(Duration::from_secs(121)).await;
                tokio::time::resume();
                tokio::time::sleep(Duration::from_millis(10)).await;
                let row = fake_store.execution(id.clone()).await.unwrap().unwrap();
                assert_ne!(row.status, "reconciling", "old turn timeout fired");
                assert!(row.provider_terminal_status.is_none());
            }
        }
        if matches!(
            case,
            "activity-observability-failure" | "permission-observability-failure"
        ) {
            send(&mut s, json!({"method":"turn/started","params":{"threadId":"THREAD","turn":turn("inProgress")}})).await;
            reply(&mut s, &req, json!({"turn":turn("inProgress")})).await;
            tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    let row = fake_store.execution(id.clone()).await.unwrap().unwrap();
                    if row.status == "running" && row.turn_id.as_deref() == Some("TURN") {
                        break;
                    }
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            let fault = if case == "activity-observability-failure" {
                crate::agent::store::ObservabilityFault::Activity
            } else {
                crate::agent::store::ObservabilityFault::PermissionDiagnostic
            };
            fake_store.inject_observability_failure(fault);
            if case == "activity-observability-failure" {
                send(&mut s, json!({"method":"item/started","params":{"threadId":"THREAD","turnId":"TURN","item":{"type":"commandExecution","id":"test","command":"cargo test","commandActions":[]}}})).await;
            } else {
                send(&mut s, json!({"id":"permission-fault","method":"item/commandExecution/requestApproval","params":{"itemId":"I","startedAtMs":1,"threadId":"THREAD","turnId":"TURN"}})).await;
                let denied = recv(&mut s).await;
                assert_eq!(denied["id"], "permission-fault");
                assert_eq!(denied["result"]["decision"], "cancel");
            }
            tokio::time::timeout(Duration::from_secs(5), async {
                while fake_store.observability_failure_pending(fault) {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            let row = fake_store.execution(id.clone()).await.unwrap().unwrap();
            assert_eq!(row.status, "running");
            assert!(row.provider_terminal_status.is_none());
            assert!(row.last_activity_at.is_none());
            assert!(row.error_code.is_none());
            assert!(fake_store
                .workspace_claim(request2.canonical_workspace_root.clone())
                .await
                .unwrap()
                .is_some());
        }
        if late_deadline {
            while fake_store.execution(id.clone()).await.unwrap().unwrap().dispatch_state != "dispatched" {
                tokio::task::yield_now().await;
            }
        }
        if matches!(case, "retry-error" | "failed-error") {
            send(&mut s, json!({"method":"error","params":{"threadId":"THREAD","turnId":"TURN",
                "willRetry":case == "retry-error","error":{"message":"fixture error","codexErrorInfo":"sandboxError"}}})).await;
            loop {
                let db = rusqlite::Connection::open(fake_store_path(&request2)).unwrap();
                let recorded: Option<String> = db.query_row("SELECT error_code FROM executions WHERE id=?1", [&id], |r|r.get(0)).unwrap();
                if recorded.as_deref() == Some("CODEX_TURN_ERROR") { break; }
                tokio::task::yield_now().await;
            }
            let row = fake_store.execution(id.clone()).await.unwrap().unwrap();
            assert!(row.provider_terminal_status.is_none());
            assert_ne!(row.status, "reconciling");
            if case == "retry-error" {
                // ACK prevents its independent timeout from obscuring retry semantics.
                reply(&mut s, &req, json!({"turn":turn("inProgress")})).await;
                tokio::time::sleep(Duration::from_millis(20)).await;
                tokio::time::pause();
                tokio::time::advance(Duration::from_secs(121)).await;
                tokio::time::resume();
                tokio::time::sleep(Duration::from_millis(10)).await;
                assert_ne!(fake_store.execution(id.clone()).await.unwrap().unwrap().status, "reconciling");
            }
        }
        if case == "coding-command-failure" {
            send(&mut s, json!({"method":"turn/started","params":{"threadId":"THREAD","turn":turn("inProgress")}})).await;
            tokio::time::timeout(Duration::from_secs(5), async {
                while fake_store.execution(id.clone()).await.unwrap().unwrap().status != "running" {
                    tokio::task::yield_now().await;
                }
            }).await.unwrap();
            send(&mut s, json!({"method":"item/started","params":{"threadId":"THREAD","turnId":"TURN","item":{"type":"commandExecution","id":"shell","command":"cargo test","commandActions":[]}}})).await;
            for n in 0..1024 {
                send(&mut s, json!({"method":"item/commandExecution/outputDelta","params":{"threadId":"THREAD","turnId":"TURN","itemId":"shell","delta":format!("line {n}\n")}})).await;
            }
            send(&mut s, json!({"method":"item/completed","params":{"threadId":"THREAD","turnId":"TURN","item":{"type":"commandExecution","id":"shell","command":"Get-Content missing-file","status":"failed","exitCode":1}}})).await;
            send(&mut s, json!({"method":"item/completed","params":{"threadId":"THREAD","turnId":"TURN","item":{"type":"commandExecution","id":"next","command":"git status","status":"completed","exitCode":0}}})).await;
            send(&mut s, json!({"id":"unexpected-approval","method":"item/commandExecution/requestApproval","params":{"itemId":"next","startedAtMs":1,"threadId":"THREAD","turnId":"TURN","command":"secret-token"}})).await;
            let denied = recv(&mut s).await;
            assert_eq!(denied["id"], "unexpected-approval");
            assert_eq!(denied["result"]["decision"], "cancel");
            tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    let row = fake_store.execution(id.clone()).await.unwrap().unwrap();
                    if row.error_code.as_deref() == Some("CODEX_PERMISSION_DENIED") {
                        assert_eq!(row.error_message.as_deref(), Some("command"));
                        assert_eq!(row.status, "running");
                        assert!(row.provider_terminal_status.is_none());
                        break;
                    }
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
        }
        if case == "coding-command-failure" {
            send(&mut s, json!({"method":"item/started","params":{"threadId":"THREAD","turnId":"TURN","item":{"type":"subAgentActivity","id":"review","kind":"started","agentThreadId":"CHILD","agentPath":"/root/review"}}})).await;
            send(&mut s, json!({"method":"turn/started","params":{"threadId":"CHILD","turn":turn("inProgress")}})).await;
            send(&mut s, json!({"method":"thread/name/updated","params":{"threadId":"CHILD","threadName":"child name"}})).await;
            send(&mut s, json!({"method":"error","params":{"threadId":"CHILD","turnId":"TURN","willRetry":false,"error":{"message":"child diagnostic"}}})).await;
            send(&mut s, json!({"method":"turn/completed","params":{"threadId":"CHILD","turn":turn("failed")}})).await;
        }
        let turn_request = req.clone();
        // Except the ACK-first cases, persist terminal before sending a late ACK.
        send(&mut s,json!({"method":"turn/completed","params":{"threadId":"THREAD","turn":turn(terminal_status)}})).await;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            let row = fake_store.execution(id.clone()).await.unwrap().unwrap();
            if row.status == "finalizing" {
                assert_eq!(fake_store.product_read(Some(id.clone()), None, None, 1).await.unwrap()[0].thread_name.as_deref(), Some(if case == "title" { "新标题" } else { "更新名称" }));
                assert_eq!(row.turn_id.as_deref(), Some("TURN"));
                if case == "failed-delayed-flush" {
                    assert_eq!(row.dispatch_state, "dispatching");
                    assert!(row.final_result_json.is_none());
                    assert!(fake_store.workspace_claim(request2.canonical_workspace_root.clone()).await.unwrap().is_some());
                }
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "terminal was not persisted before ACK"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        if case == "failed-delayed-flush" {
            flush_release.send(()).unwrap();
        }
        {
            if !late_deadline && !matches!(case, "long" | "failed" | "failed-no-ack" | "failed-delayed-flush" | "retry-error" | "activity-observability-failure" | "permission-observability-failure") {
                reply(&mut s, &req, json!({"turn":turn("inProgress")})).await;
            }
            let req = recv(&mut s).await;
            assert_eq!(req["method"], "thread/read");
            assert_eq!(req["params"]["includeTurns"], false);
            assert_eq!(
                fake_store
                    .execution(id.clone())
                    .await
                    .unwrap()
                    .unwrap()
                    .status,
                "finalizing"
            );
            if late_deadline {
                tokio::time::pause();
                tokio::time::advance(Duration::from_secs(8)).await;
                tokio::time::resume();
            }
            reply(&mut s,&req,json!({"thread":{"id":if case=="wrong-result" {"OTHER"}else{"THREAD"},"name":"最终名称","turns":[],"historyMode":"paginated"}})).await;
            if case != "wrong-result" {
                let req = recv(&mut s).await;
                assert_eq!(req["method"], "thread/turns/list");
                if late_deadline {
                    tokio::time::pause();
                    tokio::time::advance(Duration::from_secs(8)).await;
                    tokio::time::resume();
                    // Each recovery RPC is within its deadline; only the obsolete
                    // turn/start ACK is late, after exact terminal was persisted.
                    if invalid_ack {
                        let response = if case.starts_with("wrong-ack") {
                            json!({"id":turn_request["id"],"result":{"turn":{"id":"OTHER","status":"inProgress","items":[],"itemsView":"summary"}}})
                        } else if case.starts_with("invalid-ack") {
                            json!({"id":turn_request["id"],"result":{"turn":{"id":"TURN","status":"unknown"}}})
                        } else {
                            json!({"id":turn_request["id"],"error":{"code":-1,"message":"conflict"}})
                        };
                        send(&mut s, response).await;
                        let mut extra = String::new();
                        assert_eq!(s.read_line(&mut extra).await.unwrap(), 0, "unexpected replay: {extra}");
                        return;
                    }
                    reply(&mut s, &turn_request, json!({"turn":turn("inProgress")})).await;
                }
                reply(
                    &mut s,
                    &req,
                    json!({"data":[turn(terminal_status)],"nextCursor":null}),
                )
                .await;
                let req = recv(&mut s).await;
                assert_eq!(req["method"], "thread/items/list");
                assert_eq!(req["params"]["turnId"], "TURN");
                reply(&mut s,&req,json!({"data":[{"turnId":"TURN","item":{"type":"agentMessage","id":"persisted","phase":"final_answer","text":"VERTICAL_SLICE_OK"}}],"nextCursor":null})).await;
                let req = recv(&mut s).await;
                assert_eq!(req["method"], "thread/backgroundTerminals/clean");
                reply(&mut s, &req, json!({})).await;
                for round in 0..2 {
                    let req = recv(&mut s).await;
                    assert_eq!(req["method"], "thread/backgroundTerminals/list");
                    assert_eq!(req["params"]["threadId"], "THREAD");
                    let row = fake_store.execution(id.clone()).await.unwrap().unwrap();
                    assert_eq!(row.status, "finalizing");
                    assert_ne!(row.release_evidence_state, "complete");
                    assert!(
                        fake_store
                            .workspace_claim(request2.canonical_workspace_root.clone())
                            .await
                            .unwrap()
                            .is_some()
                    );
                    let duplicate = if case == "continue-old-turn" {
                        fake_store
                            .product_create_continuation(
                                "UNUSED".into(),
                                "SOURCE".into(),
                                request2.request_key.clone(),
                                request2.prompt.clone(),
                                now(),
                            )
                            .await
                            .unwrap()
                    } else {
                        AgentTaskManager::new(fake_store.clone(), "not-a-binary.exe".into())
                            .execute(request2.clone())
                            .await
                            .unwrap()
                    };
                    assert!(!duplicate.created);
                    assert_eq!(duplicate.execution_id, id);
                    reply(&mut s,&req,json!({"data":if round==0 {json!([{"id":"still-active"}])}else{json!([])},"nextCursor":null})).await;
                }
            }
        }
        let mut extra = String::new();
        assert_eq!(
            s.read_line(&mut extra).await.unwrap(),
            0,
            "extra dispatch/fallback: {extra}"
        );
    });
    let provider = CodexProvider { runtime_pool: Default::default(),
        store: store.clone(),
        executable: "unused".into(),
        owner: "fixture".into(),
    };
    let outcome = if case == "resume" {
        for _ in 0..3 {
            assert!(matches!(
                &manager.recover_startup().await.unwrap()[0],
                crate::agent::task_manager::recovery::RecoveryOutcome::PendingExplicitResume { .. }
            ));
        }
        let duplicate = manager.execute(request.clone()).await.unwrap();
        assert!(!duplicate.created);
        assert_eq!(duplicate.execution, created.execution);
        assert_eq!(
            connection
                .query_row("SELECT count(*) FROM runtime_instances", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
        let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(3));
        let mut handles = Vec::new();
        for connection_store in [
            store.clone(),
            StateStore::open(temp.path().into()).await.unwrap(),
        ] {
            let mut resume_manager = AgentTaskManager::new(connection_store, "unused".into());
            resume_manager.test_client = Some((client.clone(), temp.path().join("agent-state.db")));
            let barrier = barrier.clone();
            let id = created.execution_id.clone();
            handles.push(tokio::spawn(async move {
                barrier.wait().await;
                resume_manager.resume_pending_execution(&id).await
            }));
        }
        let mut one = handles.remove(0);
        let mut two = handles.remove(0);
        barrier.wait().await;
        tokio::time::timeout(Duration::from_secs(5), entered_rx)
            .await
            .unwrap()
            .unwrap();
        // Winner is held before initialize reply, still not_dispatched/runtime NULL.
        // The other call must reject NOW, not only after the winner has dispatched.
        let (rejected, first) = tokio::time::timeout(Duration::from_secs(5), async {
            tokio::select! {r=&mut one=>(r.unwrap(),true),r=&mut two=>(r.unwrap(),false)}
        })
        .await
        .unwrap();
        assert!(
            matches!(rejected,Err(ExecutionFailure::State(ref e)) if e=="PENDING_RESUME_REJECTED")
        );
        assert_eq!(
            store
                .execution(created.execution_id.clone())
                .await
                .unwrap()
                .unwrap(),
            created.execution
        );
        assert_eq!(
            connection
                .query_row("SELECT count(*) FROM runtime_instances", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        continue_tx.send(()).unwrap();
        let completed = if first {
            two.await.unwrap()
        } else {
            one.await.unwrap()
        };
        completed.map_err(|e| format!("{e:?}"))
    } else {
        client.initialize().await.unwrap();
        tokio::time::timeout(
            Duration::from_secs(if matches!(case, "long" | "retry-error") { 150 } else if late_deadline { 60 } else { 15 }),
            provider.run_client(&created.execution_id, &client),
        )
        .await
        .unwrap()
    };
    let row = store
        .execution(created.execution_id.clone())
        .await
        .unwrap()
        .unwrap();
    if case.starts_with("unowned-") || (late_deadline && !invalid_ack) || matches!(case, "title" | "coding-command-failure" | "continue-old-turn" | "activity-observability-failure" | "permission-observability-failure" | "success" | "resume" | "long" | "failed" | "failed-late-ack" | "failed-no-ack" | "failed-delayed-flush" | "retry-error" | "failed-error") {
        if failed {
            assert_eq!(outcome.as_ref().unwrap_err(), "PROVIDER_TERMINAL_failed");
        } else {
            assert!(outcome.is_ok(), "{outcome:?}");
        }
        assert_eq!(row.status, terminal_status);
        assert_eq!(row.result_completeness, "complete");
        assert_eq!(store.product_read(Some(created.execution_id.clone()), None, None, 1).await.unwrap()[0].thread_name.as_deref(), Some("最终名称"));
        assert_eq!(row.release_evidence_state, "complete");
        assert!(
            store
                .workspace_claim(request.canonical_workspace_root.clone())
                .await
                .unwrap()
                .is_none()
        );
    } else {
        assert!(outcome.is_err());
        assert_eq!(row.status, "finalizing");
        assert!(row.final_result_json.is_none());
        assert_ne!(row.release_evidence_state, "complete");
        assert!(
            store
                .workspace_claim(request.canonical_workspace_root.clone())
                .await
                .unwrap()
                .is_some()
        );
    }
    if matches!(case, "rollback" | "failed-rollback") {
        assert_eq!(store.product_read(Some(created.execution_id.clone()), None, None, 1).await.unwrap()[0].thread_name.as_deref(), Some("更新名称"));
        assert!(outcome.unwrap_err().contains("injected final rollback"));
    }
    assert_eq!(row.provider_terminal_status.as_deref(), Some(terminal_status));
    assert_eq!(row.thread_id.as_deref(), Some("THREAD"));
    assert_eq!(row.turn_id.as_deref(), Some("TURN"));
    assert_eq!(row.runtime_instance_id.as_deref(), Some("R1"));
    let duplicate = if case == "continue-old-turn" {
        store
            .product_create_continuation(
                "UNUSED-FINAL".into(),
                "SOURCE".into(),
                request.request_key.clone(),
                request.prompt.clone(),
                now(),
            )
            .await
            .unwrap()
    } else {
        manager.execute(request).await.unwrap()
    };
    assert!(!duplicate.created);
    assert_eq!(duplicate.execution_id, created.execution_id);
    if case == "resume" {
        assert!(
            manager
                .resume_pending_execution(&created.execution_id)
                .await
                .is_err()
        );
        assert_eq!(
            connection
                .query_row("SELECT count(*) FROM runtime_instances", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }
    drop(client);
    fake.await.unwrap();
}

#[test]
fn title_calls_preserve_root_execution_and_product_metadata() {
    run(slice_case("title"));
}
#[test]
fn terminal_before_ack_and_idempotent_success() {
    run(slice_case("success"));
}
#[test]
fn explicit_resume_concurrent_callers_share_one_first_dispatch_and_complete() {
    run(slice_case("resume"));
}
#[test]
fn final_transaction_rollback_retains_claim_and_result_unknown() {
    run(slice_case("rollback"));
}
#[test]
fn result_identity_mismatch_cannot_complete() {
    run(slice_case("wrong-result"));
}
#[test]
fn provider_failure_finalizes_before_returning_error() {
    run(slice_case("failed"));
}

#[test]
fn failed_terminal_before_late_ack_finalizes() {
    run(slice_case("failed-late-ack"));
}
#[test]
fn failed_terminal_without_ack_finalizes() {
    run(slice_case("failed-no-ack"));
}
#[test]
fn failed_terminal_before_flush_without_ack_finalizes() {
    run(slice_case("failed-delayed-flush"));
}
#[test]
fn failed_final_transaction_rollback_retains_claim_and_result() {
    run(slice_case("failed-rollback"));
}

#[test]
#[ignore = "Explicit isolated codex-cli 0.153.4 vertical slice; run alone"]
fn real_fixed_binary_vertical_slice() {
    real_slice(false);
}

#[test]
#[ignore = "Explicit isolated codex-cli 0.153.4 pending Resume; run alone"]
fn real_fixed_binary_explicit_resume() {
    real_slice(true);
}

fn real_slice(explicit_resume: bool) {
    let temp = tempfile::tempdir().unwrap();
    let workspace = temp.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let git = std::process::Command::new("git")
        .arg("init")
        .arg(&workspace)
        .output()
        .unwrap();
    assert!(git.status.success());
    let home = temp.path().join("codex-home");
    std::fs::create_dir(&home).unwrap();
    std::fs::copy(
        PathBuf::from(std::env::var_os("USERPROFILE").unwrap()).join(".codex/auth.json"),
        home.join("auth.json"),
    )
    .unwrap();
    let evidence = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(if explicit_resume {
            "../docs/tasks/evidence/TASK-008/explicit-resume-2026-09-09"
        } else {
            "../docs/tasks/evidence/TASK-006/implementation-2026-09-09"
        })
        .join(format!("run-{}-{}", std::process::id(), now()));
    std::fs::create_dir_all(&evidence).unwrap();
    struct Environment(Vec<(&'static str, Option<std::ffi::OsString>)>);
    impl Drop for Environment {
        fn drop(&mut self) {
            for (key, value) in &self.0 {
                unsafe {
                    match value {
                        Some(value) => std::env::set_var(key, value),
                        None => std::env::remove_var(key),
                    }
                }
            }
        }
    }
    let _env = Environment(vec![
        ("CODEX_HOME", std::env::var_os("CODEX_HOME")),
        (
            "SERENA_CONTRACT_RAW_DIR",
            std::env::var_os("SERENA_CONTRACT_RAW_DIR"),
        ),
    ]);
    // This ignored test is invoked alone in its own process, never in the ordinary suite.
    unsafe {
        std::env::set_var("CODEX_HOME", home);
        std::env::set_var("SERENA_CONTRACT_RAW_DIR", &evidence);
    }
    run(async {
        let store = StateStore::open(temp.path().join("store")).await.unwrap();
        let audit = rusqlite::Connection::open(temp.path().join("store/agent-state.db")).unwrap();
        // Test-only observation of committed transitions; no production schema change.
        audit.execute_batch("CREATE TABLE slice_trace(seq INTEGER PRIMARY KEY,kind TEXT,status TEXT,dispatch TEXT,runtime TEXT,thread TEXT,turn TEXT,owns_claim INTEGER,runtime_running INTEGER);
            CREATE TRIGGER slice_created AFTER INSERT ON executions BEGIN INSERT INTO slice_trace(kind,status,dispatch,runtime,thread,turn) VALUES ('created',new.status,new.dispatch_state,new.runtime_instance_id,new.thread_id,new.turn_id); END;
            CREATE TRIGGER slice_claim AFTER INSERT ON workspace_claims BEGIN INSERT INTO slice_trace(kind,owns_claim) VALUES ('claim_acquired',1); END;
            CREATE TRIGGER slice_transition AFTER UPDATE OF status,dispatch_state,runtime_instance_id,thread_id,turn_id ON executions BEGIN
            INSERT INTO slice_trace(kind,status,dispatch,runtime,thread,turn,owns_claim,runtime_running) VALUES ('transition',new.status,new.dispatch_state,new.runtime_instance_id,new.thread_id,new.turn_id,EXISTS(SELECT 1 FROM workspace_claims WHERE execution_id=new.id),EXISTS(SELECT 1 FROM runtime_instances WHERE id=new.runtime_instance_id AND state='running')); END;
            CREATE TRIGGER slice_released AFTER DELETE ON workspace_claims BEGIN INSERT INTO slice_trace(kind,status,owns_claim) SELECT 'claim_released',status,0 FROM executions WHERE id=old.execution_id; END;").unwrap();
        let exe = PathBuf::from(
            r"C:\Users\lifei\AppData\Roaming\npm\node_modules\@openai\codex\node_modules\@openai\codex-win32-x64\vendor\x86_64-pc-windows-msvc\bin\codex.exe",
        );
        let manager = AgentTaskManager::new(store.clone(), exe);
        let request = input(&workspace);
        let result = if explicit_resume {
            let mut created = manager.create(request.clone()).await.unwrap();
            for _ in 0..3 {
                assert!(matches!(
                    &manager.recover_startup().await.unwrap()[0],
                    crate::agent::task_manager::recovery::RecoveryOutcome::PendingExplicitResume { .. }
                ));
            }
            let duplicate = manager.execute(request.clone()).await.unwrap();
            assert!(!duplicate.created);
            assert_eq!(duplicate.execution, created.execution);
            manager
                .resume_pending_execution(&created.execution_id)
                .await
                .map(|execution| {
                    created.execution = execution;
                    created
                })
        } else {
            manager.execute(request.clone()).await
        };
        std::fs::write(evidence.join("outcome.txt"), format!("{result:#?}")).unwrap();
        let result = result.unwrap();
        let row = &result.execution;
        assert!(result.created);
        assert_eq!(row.status, "completed");
        assert_eq!(row.dispatch_state, "dispatched");
        assert_eq!(
            row.runtime_instance_id.as_ref().unwrap(),
            &format!("runtime-{}", row.id)
        );
        assert_eq!(
            row.provider_terminal_evidence_runtime_instance_id,
            row.runtime_instance_id
        );
        assert_eq!(row.provider_terminal_status.as_deref(), Some("completed"));
        assert_eq!(row.background_cleanup_state, "empty");
        assert_eq!(row.result_completeness, "complete");
        assert_eq!(row.release_evidence_state, "complete");
        assert!(
            store
                .workspace_claim(request.canonical_workspace_root.clone())
                .await
                .unwrap()
                .is_none()
        );
        let final_result: Value =
            serde_json::from_str(row.final_result_json.as_ref().unwrap()).unwrap();
        assert_eq!(final_result["executionId"], row.id);
        assert_eq!(final_result["threadId"].as_str(), row.thread_id.as_deref());
        assert_eq!(final_result["turnId"].as_str(), row.turn_id.as_deref());
        assert_eq!(final_result["historyMode"], "paginated");
        assert_eq!(
            final_result["sourceRuntimeId"].as_str(),
            row.runtime_instance_id.as_deref()
        );
        assert_eq!(final_result["terminalTurn"]["status"], "completed");
        assert_eq!(final_result["resultCompleteness"], "complete");
        assert!(
            final_result["finalResult"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v["phase"] == "final_answer"
                    && v["text"].as_str().unwrap().contains("VERTICAL_SLICE_OK"))
        );
        let duplicate = manager.execute(request).await.unwrap();
        assert!(!duplicate.created);
        assert_eq!(duplicate.execution, row.clone());
        let runtime = store
            .runtime(row.runtime_instance_id.clone().unwrap())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(runtime.termination_evidence_state, "complete");
        let db = rusqlite::Connection::open(temp.path().join("store/agent-state.db")).unwrap();
        let counts: (i64, i64) = db
            .query_row(
                "SELECT (SELECT count(*) FROM executions),(SELECT count(*) FROM runtime_instances)",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(counts, (1, 1));
        if explicit_resume {
            assert!(manager.resume_pending_execution(&row.id).await.is_err());
            let raw =
                std::fs::read_to_string(evidence.join(format!("{}.stdin.raw.jsonl", runtime.id)))
                    .unwrap();
            let requests: Vec<Value> = raw
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
            for method in ["thread/start", "turn/start"] {
                assert_eq!(requests.iter().filter(|v| v["method"] == method).count(), 1);
            }
            std::fs::write(evidence.join("resume-identity.json"),serde_json::to_vec_pretty(&json!({"execution":row.id,"runtime":row.runtime_instance_id,"thread":row.thread_id,"turn":row.turn_id,"status":row.status,"resultCompleteness":row.result_completeness,"claim":"absent","runtimeCount":1,"threadStartCount":1,"turnStartCount":1,"jobConvergence":runtime.termination_evidence_state})).unwrap()).unwrap();
        }
        let cleanup_runtime: String = db
            .query_row(
                "SELECT background_cleanup_runtime_instance_id FROM executions",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(Some(&cleanup_runtime), row.runtime_instance_id.as_ref());
        let trace = audit.prepare("SELECT kind,status,dispatch,runtime,thread,turn,owns_claim,runtime_running FROM slice_trace ORDER BY seq").unwrap()
            .query_map([], |r| Ok(json!({"kind":r.get::<_,String>(0)?,"status":r.get::<_,Option<String>>(1)?,"dispatch":r.get::<_,Option<String>>(2)?,"runtime":r.get::<_,Option<String>>(3)?,"thread":r.get::<_,Option<String>>(4)?,"turn":r.get::<_,Option<String>>(5)?,"ownsClaim":r.get::<_,Option<i64>>(6)?,"runtimeRunning":r.get::<_,Option<i64>>(7)?}))).unwrap()
            .collect::<rusqlite::Result<Vec<_>>>().unwrap();
        assert_eq!(trace[0]["kind"], "created");
        assert_eq!(trace[0]["status"], "dispatch_pending");
        assert_eq!(trace[0]["dispatch"], "not_dispatched");
        assert!(trace[0]["runtime"].is_null());
        assert_eq!(trace[1]["kind"], "claim_acquired");
        let binding = trace
            .iter()
            .find(|v| v["dispatch"] == "dispatching")
            .unwrap();
        assert_eq!(binding["ownsClaim"], 1);
        assert_eq!(binding["runtimeRunning"], 1);
        assert!(
            trace
                .iter()
                .filter(|v| !v["runtime"].is_null())
                .all(|v| v["runtime"].as_str() == row.runtime_instance_id.as_deref())
        );
        assert_eq!(trace.last().unwrap()["kind"], "claim_released");
        assert_eq!(trace.last().unwrap()["status"], "completed");
        std::fs::write(
            evidence.join("db-trace.json"),
            serde_json::to_vec_pretty(&trace).unwrap(),
        )
        .unwrap();
        std::fs::write(evidence.join("final-state.txt"),format!("Execution={row:#?}\nRuntime={runtime:#?}\nClaim=None\ncounts={counts:?}\ncleanup_runtime={cleanup_runtime}\nDuplicate dispatch=false")).unwrap();
        assert!(
            std::process::Command::new("git")
                .arg("-C")
                .arg(&workspace)
                .args(["status", "--porcelain"])
                .output()
                .unwrap()
                .stdout
                .is_empty()
        );
    });
}

#[test]
fn coordinator_rejects_cleanup_from_another_execution() {
    run(async {
        let temp = tempfile::tempdir().unwrap();
        let store = StateStore::open(temp.path().into()).await.unwrap();
        let db = rusqlite::Connection::open(temp.path().join("agent-state.db")).unwrap();
        db.execute("INSERT INTO runtime_instances(id,owner_host_instance_id,state,created_at,updated_at) VALUES ('R1','fixture','running',1,1)", []).unwrap();
        for id in ["E1", "E2"] {
            let mut request = input(&temp.path().join(id));
            request.agent_id = id.into();
            store
                .create_execution(
                    id.into(),
                    crate::agent::execution::canonicalize_request(request).unwrap(),
                    now(),
                )
                .await
                .unwrap();
            store
                .transition_execution(
                    id.into(),
                    0,
                    Transition::Dispatch {
                        to: DispatchState::Dispatching,
                        runtime_id: Some("R1".into()),
                    },
                    now(),
                )
                .await
                .unwrap();
            store
                .bind_protocol_identity(
                    id.into(),
                    1,
                    "R1".into(),
                    id.into(),
                    Some("TURN".into()),
                    now(),
                )
                .await
                .unwrap();
            store
                .transition_execution(
                    id.into(),
                    2,
                    Transition::ProviderTerminal {
                        runtime_id: "R1".into(),
                        status: Status::Completed,
                    },
                    now(),
                )
                .await
                .unwrap();
        }
        let (wire, server) = tokio::io::duplex(128 * 1024);
        let (read, write) = tokio::io::split(wire);
        let client = Client::transport("R1".into(), read, write, tokio::io::empty());
        let fake = tokio::spawn(async move {
            let mut s = BufReader::new(server);
            let req = recv(&mut s).await;
            reply(&mut s,&req,json!({"userAgent":"fake","codexHome":"isolated","platformFamily":"windows","platformOs":"windows"})).await;
            assert_eq!(recv(&mut s).await["method"], "initialized");
            for (method, thread, result) in [
                (
                    "thread/read",
                    "E1",
                    json!({"thread":{"id":"E1","historyMode":"paginated","turns":[]}}),
                ),
                (
                    "thread/turns/list",
                    "E1",
                    json!({"data":[turn("completed")],"nextCursor":null}),
                ),
                (
                    "thread/items/list",
                    "E1",
                    json!({"data":[{"turnId":"TURN","item":{"type":"agentMessage","id":"i","phase":"final_answer","text":"ok"}}],"nextCursor":null}),
                ),
                ("thread/backgroundTerminals/clean", "E2", json!({})),
                (
                    "thread/backgroundTerminals/list",
                    "E2",
                    json!({"data":[],"nextCursor":null}),
                ),
            ] {
                let req = recv(&mut s).await;
                assert_eq!(req["method"], method);
                assert_eq!(req["params"]["threadId"], thread);
                reply(&mut s, &req, result).await;
            }
            let mut extra = String::new();
            assert_eq!(s.read_line(&mut extra).await.unwrap(), 0);
        });
        client.initialize().await.unwrap();
        let result = client
            .recover_result(
                RecoveryScope::same_runtime_for_execution(&store, "E1", "R1")
                    .await
                    .unwrap(),
            )
            .await
            .unwrap();
        let empty = client
            .cleanup(CleanupScope::for_execution(&store, "E2").await.unwrap())
            .await
            .unwrap();
        let before = store.execution("E1".into()).await.unwrap();
        assert_eq!(
            WorkspaceExecutionCoordinator {
                store: store.clone()
            }
            .finish("E1", result, empty)
            .await
            .unwrap_err(),
            "EXECUTION_RESULT_IDENTITY_MISMATCH"
        );
        assert_eq!(store.execution("E1".into()).await.unwrap(), before);
        for id in ["E1", "E2"] {
            assert!(
                store
                    .workspace_claim(temp.path().join(id).to_str().unwrap().into())
                    .await
                    .unwrap()
                    .is_some()
            );
        }
        drop(client);
        fake.await.unwrap();
    });
}

#[test]
fn long_turn_survives_old_120_second_boundary() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(slice_case("long"));
}

#[test]
fn completed_and_failed_terminal_ack_after_rpc_deadline() {
    for case in ["late-deadline", "failed-late-deadline"] {
        tokio::runtime::Builder::new_current_thread()
            .enable_all().build().unwrap().block_on(slice_case(case));
    }
}

#[test]
fn late_terminal_ack_conflicting_identity_or_error_retains_claim() {
    for case in ["wrong-ack-late-deadline", "error-ack-late-deadline", "invalid-ack-late-deadline"] {
        tokio::runtime::Builder::new_current_thread()
            .enable_all().build().unwrap().block_on(slice_case(case));
    }
}

fn fake_store_path(request: &CreateExecutionInput) -> PathBuf {
    PathBuf::from(&request.canonical_workspace_root).join("agent-state.db")
}
#[test]
fn retry_error_can_run_past_old_deadline_and_complete() {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(slice_case("retry-error"));
}
#[test]
fn nonretry_error_waits_for_authoritative_failed_terminal() { run(slice_case("failed-error")); }

// Full live worker path, including an independently owned ManagedClient monitor.
// Its evidence gate stands in for the Job query; real Job queries have runtime tests.
async fn live_failure_case(case: &'static str, evidence: bool, bind_turn: bool) {
    let temp = tempfile::tempdir().unwrap();
    let store = StateStore::open(temp.path().into()).await.unwrap();
    let manager = AgentTaskManager::new(store.clone(), "unused".into());
    let request = input(temp.path());
    let id = manager.create(request.clone()).await.unwrap().execution_id;
    let database = temp.path().join("agent-state.db");
    let db = rusqlite::Connection::open(&database).unwrap();
    db.execute("INSERT INTO runtime_instances(id,owner_host_instance_id,state,created_at,updated_at) VALUES ('R1','fixture','running',1,1)",[]).unwrap();
    let (wire, server) = tokio::io::duplex(32 * 1024);
    let (read, write) = tokio::io::split(wire);
    let client = Client::transport("R1".into(), read, write, tokio::io::empty());
    let (release_tx, release_rx) = tokio::sync::oneshot::channel();
    let (ending_tx, ending_rx) = tokio::sync::oneshot::channel();
    let mut failure = client.failure();
    let monitor = tokio::spawn(async move {
        while failure.borrow().is_none() { failure.changed().await.unwrap(); }
        ending_tx.send(()).unwrap();
        release_rx.await.unwrap();
        if evidence {
            let db = rusqlite::Connection::open(database).unwrap();
            db.execute("UPDATE runtime_instances SET state='terminated',termination_evidence_state='complete',termination_evidence_type='job_active_processes_zero',termination_evidence_at=10 WHERE id='R1'",[]).unwrap();
            Ok(())
        } else {
            Err(super::super::runtime::RuntimeFailure { code:"CODEX_JOB_QUERY_FAILED", message:"fixture query failure".into(), runtime:None })
        }
    });
    let fake = tokio::spawn(async move {
        let mut s = BufReader::new(server);
        let req = recv(&mut s).await;
        assert_eq!(req["method"],"initialize");
        reply(&mut s,&req,json!({"userAgent":"fake","codexHome":"isolated","platformFamily":"windows","platformOs":"windows"})).await;
        assert_eq!(recv(&mut s).await["method"], "initialized");
        let req = recv(&mut s).await;
        assert_eq!(req["method"], "thread/start");
        if !bind_turn {
            reply(&mut s,&req,json!({"thread":{"id":"THREAD","turns":[],"historyMode":"legacy"}})).await;
            // Fresh thread_start rejects legacy before starting a turn.
        } else {
            reply(&mut s,&req,json!({"thread":{"id":"THREAD","turns":[],"historyMode":"paginated"}})).await;
            let req = recv(&mut s).await;
            assert_eq!(req["method"],"turn/start");
            reply(&mut s,&req,json!({"turn":turn("inProgress")})).await;
            send(&mut s,json!({"method":"turn/started","params":{"threadId":"THREAD","turn":turn("inProgress")}})).await;
            tokio::time::sleep(Duration::from_millis(30)).await;
            match case {
                "eof" => return,
                "jsonl" => { s.write_all(b"{broken json}\n").await.unwrap(); s.flush().await.unwrap(); }
                "malformed-error" => send(&mut s,json!({"method":"error","params":{"threadId":"THREAD"}})).await,
                "wrong-start" | "wrong-completed" => {
                    let mut conflicting = turn(if case == "wrong-start" { "inProgress" } else { "completed" });
                    conflicting["id"] = json!("OTHER");
                    send(&mut s,json!({"method":if case == "wrong-start" {"turn/started"} else {"turn/completed"},"params":{"threadId":"THREAD","turn":conflicting}})).await;
                }
                "wrong-turn" | "nonretry" => {
                    send(&mut s,json!({"method":"error","params":{"threadId":"THREAD",
                        "turnId":if case=="wrong-turn" {"OTHER"} else {"TURN"},"willRetry":false,"error":{"message":"fixture final error","codexErrorInfo":"sandboxError"}}})).await;
                }
                _ => unreachable!(),
            }
        }
        let mut extra = String::new();
        if case == "nonretry" {
            // Unrelated progress must not extend or starve the error deadline.
            let mut progress = tokio::time::interval(Duration::from_millis(1));
            loop {
                tokio::select! {
                    read = s.read_line(&mut extra) => { assert_eq!(read.unwrap(),0,"unexpected replay: {extra}"); break; }
                    _ = progress.tick() => {
                        let bytes = super::super::protocol::encode(&json!({"method":"fixture/progress","params":{}})).unwrap();
                        if s.write_all(&bytes).await.is_err() { break; }
                    }
                }
            }
        } else {
            assert_eq!(s.read_line(&mut extra).await.unwrap(),0,"no replay or new turn: {extra}");
        }
    });
    client.initialize().await.unwrap();
    let managed = managed::ManagedClient::test_owned(client, monitor);
    let provider = CodexProvider { runtime_pool: Default::default(), store:store.clone(),executable:"C:/missing-result-provider.exe".into(),owner:"fixture".into() };
    let worker_id = id.clone();
    let worker = tokio::spawn(async move { provider.run_managed(&worker_id, managed, &mut None).await });
    tokio::time::timeout(Duration::from_secs(20), ending_rx).await.unwrap().unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while store.execution(id.clone()).await.unwrap().unwrap().status != "reconciling" {
            tokio::task::yield_now().await;
        }
    }).await.unwrap();
    assert!(!worker.is_finished(),"worker must await Runtime convergence");
    let row = store.execution(id.clone()).await.unwrap().unwrap();
    assert_eq!(row.status,"reconciling");
    assert!(store.workspace_claim(request.canonical_workspace_root.clone()).await.unwrap().is_some());
    assert!(manager.resume_pending_execution(&id).await.is_err());
    assert_eq!(db.query_row("SELECT count(*) FROM runtime_instances",[],|r|r.get::<_,i64>(0)).unwrap(),1);
    release_tx.send(()).unwrap();
    assert!(tokio::time::timeout(Duration::from_secs(5),worker).await.unwrap().unwrap().is_err());
    fake.await.unwrap();
    let row = store.execution(id.clone()).await.unwrap().unwrap();
    assert_eq!(row.status,if evidence && !bind_turn {"interrupted"} else {"unknown"});
    assert!(row.provider_terminal_status.is_none());
    if case.starts_with("wrong-") {
        assert_eq!(row.thread_id.as_deref(), Some("THREAD"));
        assert_eq!(row.turn_id.as_deref(), Some("TURN"));
        assert!(row.provider_terminal_evidence_runtime_instance_id.is_none());
        assert_eq!(row.error_message.as_deref(), Some("EXECUTION_PROTOCOL_IDENTITY_MISMATCH"));
    }
    assert_eq!(row.result_completeness,"unknown");
    let complete: Option<i64> = db.query_row("SELECT completed_at FROM executions WHERE id=?1",[&id],|r|r.get(0)).unwrap();
    if evidence && !bind_turn {
        assert!(complete.is_some());
        assert!(store.workspace_claim(request.canonical_workspace_root.clone()).await.unwrap().is_none());
        let mut next = request.clone(); next.request_key = "next".into();
        assert!(manager.create(next).await.unwrap().created);
    } else {
        assert!(store.workspace_claim(request.canonical_workspace_root.clone()).await.unwrap().is_some());
    }
    assert!(manager.resume_pending_execution(&id).await.is_err());
    if case == "nonretry" {
        let diagnostic: String = db.query_row("SELECT error_message FROM executions WHERE id=?1",[&id],|r|r.get(0)).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&diagnostic).unwrap()["willRetry"],false);
    }
}
#[test]
fn live_transport_and_protocol_failures_wait_for_shutdown_and_converge() {
    run(async {
        for case in ["eof","jsonl","malformed-error","wrong-turn","wrong-start","wrong-completed"] {
            live_failure_case(case,true,true).await;
        }
    });
}
#[test]
fn live_nonretry_error_without_terminal_is_bounded() { run(live_failure_case("nonretry",true,true)); }
#[test]
fn live_failure_without_termination_evidence_retains_claim_unknown() { run(live_failure_case("eof",false,true)); }
#[test]
fn live_failure_without_turn_releases_only_after_termination() { run(live_failure_case("early",true,false)); }

#[test]
fn shutdown_failure_survives_unknown_persistence_failure() {
    run(async {
        let temp = tempfile::tempdir().unwrap();
        let store = StateStore::open(temp.path().into()).await.unwrap();
        let manager = AgentTaskManager::new(store.clone(), "unused".into());
        let id = manager.create(input(temp.path())).await.unwrap().execution_id;
        let db = rusqlite::Connection::open(temp.path().join("agent-state.db")).unwrap();
        db.execute("UPDATE executions SET status='reconciling',dispatch_state='uncertain' WHERE id=?1",[&id]).unwrap();
        db.execute_batch("CREATE TRIGGER reject_unknown BEFORE UPDATE OF status ON executions WHEN NEW.status='unknown' BEGIN SELECT RAISE(ABORT,'fixture persistence failure'); END;").unwrap();
        let provider = CodexProvider { runtime_pool: Default::default(),store,executable:"unused".into(),owner:"fixture".into()};
        let result = provider.finish_after_shutdown(&id,Err("provider error".into()),
            Err(super::super::runtime::RuntimeFailure {code:"CODEX_JOB_QUERY_FAILED",message:"original termination error".into(),runtime:None})).await;
        match result.unwrap_err() {
            ExecutionFailure::Runtime(failure) => {
                assert_eq!(failure.code,"CODEX_JOB_QUERY_FAILED");
                assert!(failure.message.contains("original termination error"));
                assert!(failure.message.contains("fixture persistence failure"));
            }
            other => panic!("termination failure was lost: {other:?}"),
        }
    });
}

#[test]
fn coding_turn_recovers_command_exit_one_and_streaming_burst() {
    run(slice_case("coding-command-failure"));
}

#[test]
fn activity_persistence_failure_does_not_fail_provider_execution() {
    run(slice_case("activity-observability-failure"));
}

#[test]
fn permission_diagnostic_persistence_failure_does_not_fail_provider_execution() {
    run(slice_case("permission-observability-failure"));
}

#[test]
fn continued_thread_rejects_late_old_turn_activity_and_permission_hints() {
    run(slice_case("continue-old-turn"));
}

#[test]
fn child_lifecycle_before_discovery_does_not_bind_root_turn() { run(slice_case("unowned-child-first")); }

#[test]
fn completely_unknown_thread_is_isolated_without_discovery() { run(slice_case("unowned-unknown")); }

#[test]
fn child_events_cannot_pollute_bound_root_or_wake_observe() { run(slice_case("unowned-bound")); }

#[test]
#[ignore = "Explicit real coding/Skill regression in a temporary workspace; run alone"]
fn real_fixed_coding_skill_command_failure() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = temp.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    assert!(std::process::Command::new("git").arg("init").arg(&workspace).output().unwrap().status.success());
    std::fs::write(workspace.join("add.py"), "def add(a, b):\n    return a - b\n").unwrap();
    assert!(!workspace.join("AGENTS.md").exists());
    run(async {
        let store = StateStore::open(temp.path().join("store")).await.unwrap();
        let exe = PathBuf::from(std::env::var_os("USERPROFILE").unwrap()).join(r"AppData\Roaming\npm\node_modules\@openai\codex\node_modules\@openai\codex-win32-x64\vendor\x86_64-pc-windows-msvc\bin\codex.exe");
        let manager = AgentTaskManager::new(store.clone(), exe);
        let mut request = input(&workspace);
        request.mode = crate::agent::execution::ExecutionMode::WorkspaceWrite;
        request.prompt = "Use code-delivery-review for this small coding task. First run a PowerShell command that prints 2048 numbered lines and then explicitly exits with exitCode=1 as an intentional recoverable command failure. Continue coding after that failure: fix add.py so add returns the sum, verify add(2,3)==5 using Python. Use a child/subagent for the independent read-only code-delivery-review of this fix, and wait for its result before finishing. No commit or push. Keep the skill enabled. The workspace has no AGENTS.md; that is valid. Finish with CODING_REGRESSION_OK.".into();
        let id = manager.product_submit(crate::agent::product::Action::Start {
            workspace_id: request.workspace_id.clone(), agent_id: request.agent_id,
            request_key: request.request_key, prompt: request.prompt,
        }, Some(crate::agent::store::transactions::product::WorkspaceSnapshot {
            id: request.workspace_id, root: request.canonical_workspace_root,
        })).await.unwrap();
        let row = tokio::time::timeout(Duration::from_secs(360), async {
            loop {
                let row = store.execution(id.clone()).await.unwrap().unwrap();
                if matches!(row.status.as_str(), "completed" | "failed" | "interrupted" | "unknown" | "cancelled") { break row; }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }).await.expect("real coding test deadline");
        println!("real coding status={} diagnostic={:?}", row.status, row.error_code);
        assert_eq!(row.status, "completed");
        assert_eq!(row.provider_terminal_status.as_deref(), Some("completed"));
        assert_eq!(row.result_completeness, "complete");
        assert_eq!(row.release_evidence_state, "complete");
        assert!(store.workspace_claim(workspace.to_str().unwrap().into()).await.unwrap().is_none());
        assert!(row.final_result_json.as_deref().unwrap().contains("CODING_REGRESSION_OK"));
        let output = std::process::Command::new("python").current_dir(&workspace)
            .args(["-B", "-c", "from add import add; assert add(2,3)==5"]).output().unwrap();
        assert!(output.status.success());
        assert!(!workspace.join("AGENTS.md").exists());
        println!("coding execution={} thread={} turn={}", row.id, row.thread_id.unwrap(), row.turn_id.unwrap());
    });
}

#[test]
#[ignore = "Explicit fixed-binary title smoke in a temporary workspace; run alone"]
fn real_fixed_root_title_smoke() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = temp.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    assert!(std::process::Command::new("git").arg("init").arg(&workspace).output().unwrap().status.success());
    run(async {
        let store = StateStore::open(temp.path().join("store")).await.unwrap();
        let exe = PathBuf::from(std::env::var_os("USERPROFILE").unwrap()).join(r"AppData\Roaming\npm\node_modules\@openai\codex\node_modules\@openai\codex-win32-x64\vendor\x86_64-pc-windows-msvc\bin\codex.exe");
        let manager = AgentTaskManager::new(store.clone(), exe);
        let request = input(&workspace);
        let id = manager.product_submit(crate::agent::product::Action::Start {
            workspace_id: request.workspace_id.clone(), agent_id: request.agent_id,
            request_key: request.request_key,
            prompt: "Use codex_app.set_thread_title to set this Root conversation title to exactly 'Root title smoke'. This exact title is my explicit request for this isolated regression. Do not edit files or delegate. Return ROOT_TITLE_SMOKE_OK only after the tool confirms success.".into(),
        }, Some(crate::agent::store::transactions::product::WorkspaceSnapshot {
            id: request.workspace_id, root: request.canonical_workspace_root,
        })).await.unwrap();
        let row = tokio::time::timeout(Duration::from_secs(180), async {
            loop {
                let row = store.execution(id.clone()).await.unwrap().unwrap();
                if matches!(row.status.as_str(), "completed" | "failed" | "interrupted" | "unknown" | "cancelled") { break row; }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }).await.expect("real title smoke deadline");
        println!("title smoke status={} diagnostic={:?}", row.status, row.error_code);
        assert_eq!(row.status, "completed");
        assert_eq!(row.release_evidence_state, "complete");
        assert!(row.final_result_json.as_deref().unwrap().contains("ROOT_TITLE_SMOKE_OK"));
        assert_eq!(store.product_read(Some(id), None, None, 1).await.unwrap()[0].thread_name.as_deref(), Some("Root title smoke"));
        println!("title smoke thread={} turn={}", row.thread_id.unwrap(), row.turn_id.unwrap());
    });
}
