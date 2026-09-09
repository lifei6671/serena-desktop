use super::*;
use tokio::io::{AsyncBufReadExt, BufReader, DuplexStream};
fn run(f: impl std::future::Future<Output = ()>) {
    tokio::runtime::Runtime::new().unwrap().block_on(f)
}
fn pair() -> (Client, DuplexStream) {
    let (client, server) = tokio::io::duplex(128 * 1024);
    let (read, write) = tokio::io::split(client);
    (
        Client::transport("R1".into(), read, write, tokio::io::empty()),
        server,
    )
}
async fn recv(s: &mut BufReader<DuplexStream>) -> Value {
    let mut line = String::new();
    s.read_line(&mut line).await.unwrap();
    serde_json::from_str(&line).unwrap()
}
async fn reply(s: &mut BufReader<DuplexStream>, request: &Value, result: Value) {
    s.write_all(&encode(&json!({"id":request["id"],"result":result})).unwrap())
        .await
        .unwrap();
}
async fn handshake(s: &mut BufReader<DuplexStream>) {
    let req = recv(s).await;
    assert_eq!(req["method"], "initialize");
    assert_eq!(req["params"]["capabilities"]["experimentalApi"], true);
    reply(s,&req,json!({"userAgent":"fake","codexHome":"home","platformFamily":"windows","platformOs":"windows"})).await;
    assert_eq!(recv(s).await, json!({"method":"initialized"}));
}
fn scope() -> CleanupScope {
    CleanupScope::fixture("R1", "T1", "E1")
}

#[test]
fn compatibility_identity_requires_all_three_fields() {
    let original = CompatibilityIdentity {
        version: VERSION.into(),
        binary_sha256: BINARY_SHA256.into(),
        protocol_schema_sha256: SCHEMA_SHA256.into(),
    };
    assert!(original.check().is_ok());
    for field in 0..3 {
        let mut id = original.clone();
        match field {
            0 => id.version.push('x'),
            1 => id.binary_sha256.push('x'),
            _ => id.protocol_schema_sha256.push('x'),
        };
        assert_eq!(
            id.check().unwrap_err().code,
            "CODEX_APP_SERVER_INCOMPATIBLE"
        );
    }
}
#[test]
fn cursor_presence_is_not_option_normalization() {
    assert_eq!(
        TerminalPage::parse(json!({"data":[]})).unwrap().next_cursor,
        NextCursor::Missing
    );
    assert_eq!(
        TerminalPage::parse(json!({"data":[],"nextCursor":null}))
            .unwrap()
            .next_cursor,
        NextCursor::Null
    );
    for v in [
        json!({"nextCursor":null}),
        json!({"data":[],"nextCursor":3}),
        json!({"data":[],"nextCursor":""}),
    ] {
        assert!(TerminalPage::parse(v).is_err());
    }
}
#[test]
fn framing_split_coalesced_crlf_and_invalid_inputs() {
    run(async {
        let (mut write, mut read) = tokio::io::duplex(4096);
        let sender = tokio::spawn(async move {
            for part in [
                b"{\"id\":".as_slice(),
                b"1,\"result\":{}}\r\n{\"id\":2,\"result\":{}}\n",
            ] {
                write.write_all(part).await.unwrap();
            }
        });
        assert!(matches!(
            decode(&read_frame(&mut read).await.unwrap()).unwrap(),
            Message::Response { id: 1, .. }
        ));
        assert!(matches!(
            decode(&read_frame(&mut read).await.unwrap()).unwrap(),
            Message::Response { id: 2, .. }
        ));
        sender.await.unwrap();
        assert_eq!(
            read_frame(&mut read).await.unwrap_err().code,
            "CODEX_STDIO_EOF"
        );
        for bad in [
            b"\xff\n".as_slice(),
            b"{\n",
            b"[]\n",
            b"{\"id\":1}\n",
            b"{\"id\":1,\"result\":{},\"error\":{}}\n",
            b"{\"method\":null}\n",
        ] {
            assert_eq!(
                decode(bad).unwrap_err().code,
                "CODEX_PROTOCOL_INVALID_MESSAGE"
            );
        }
        assert!(
            read_frame(&mut b"{}".as_slice())
                .await
                .unwrap_err()
                .message
                .contains("partial")
        );
        let huge = vec![b'x'; MAX_MESSAGE + 1];
        assert_eq!(
            read_frame(&mut huge.as_slice()).await.unwrap_err().code,
            "CODEX_PROTOCOL_MESSAGE_TOO_LARGE"
        );
        assert_eq!(
            encode(&json!({"x":"x".repeat(MAX_MESSAGE)}))
                .unwrap_err()
                .code,
            "CODEX_PROTOCOL_MESSAGE_TOO_LARGE"
        );
    });
}
#[test]
fn initialization_sequence_and_response_correlation() {
    run(async {
        let (client, server) = pair();
        assert!(!client.is_ready());
        assert!(client.thread_read("T").await.is_err());
        let fake = tokio::spawn(async move {
            let mut s = BufReader::new(server);
            handshake(&mut s).await;
            let a = recv(&mut s).await;
            let b = recv(&mut s).await;
            reply(
                &mut s,
                &b,
                json!({"thread":{"id":"B","turns":[],"historyMode":"paginated"}}),
            )
            .await;
            reply(
                &mut s,
                &a,
                json!({"thread":{"id":"A","turns":[],"historyMode":"paginated"}}),
            )
            .await;
            tokio::time::sleep(Duration::from_millis(50)).await;
        });
        client.initialize().await.unwrap();
        assert!(client.is_ready());
        let (a, b) = tokio::join!(client.thread_read("A"), client.thread_read("B"));
        assert_eq!(a.unwrap().id, "A");
        assert_eq!(b.unwrap().id, "B");
        client.cancel();
        fake.await.unwrap();
    });
}
#[test]
fn init_rejection_and_missing_capability_method_fail_closed() {
    run(async {
        for code in [-32601, -32000] {
            let (client, server) = pair();
            let fake = tokio::spawn(async move {
                let mut s = BufReader::new(server);
                let req = recv(&mut s).await;
                s.write_all(
                    &encode(&json!({"id":req["id"],"error":{"code":code,"message":"rejected"}}))
                        .unwrap(),
                )
                .await
                .unwrap();
                tokio::time::sleep(Duration::from_millis(20)).await;
            });
            let error = client.initialize().await.unwrap_err();
            assert_eq!(
                error.code,
                if code == -32601 {
                    "CODEX_APP_SERVER_INCOMPATIBLE"
                } else {
                    "CODEX_APP_SERVER_INIT_FAILED"
                }
            );
            assert!(!client.is_ready());
            fake.await.unwrap();
        }
    });
}
#[test]
fn unknown_duplicate_and_late_responses_are_not_replayed() {
    run(async {
        for id in [1, 999] {
            let (client, server) = pair();
            let (start, started) = oneshot::channel();
            let fake = tokio::spawn(async move {
                let mut s = BufReader::new(server);
                handshake(&mut s).await;
                started.await.unwrap();
                s.write_all(&encode(&json!({"id":id,"result":{}})).unwrap())
                    .await
                    .unwrap();
                tokio::time::sleep(Duration::from_millis(30)).await;
            });
            client.initialize().await.unwrap();
            start.send(()).unwrap();
            let mut failure = client.failure();
            failure.wait_for(|v| v.is_some()).await.unwrap();
            assert_eq!(
                failure.borrow().as_ref().unwrap().code,
                "CODEX_PROTOCOL_INVALID_MESSAGE"
            );
            fake.await.unwrap();
        }
        let (client, server) = pair();
        let fake = tokio::spawn(async move {
            let mut s = BufReader::new(server);
            handshake(&mut s).await;
            let req = recv(&mut s).await;
            tokio::time::sleep(Duration::from_millis(40)).await;
            let _ = s
                .write_all(&encode(&json!({"id":req["id"],"result":{}})).unwrap())
                .await;
        });
        client.initialize().await.unwrap();
        let error = client
            .rpc(
                "turn/start",
                json!({}),
                Some("E1".into()),
                Instant::now() + Duration::from_millis(10),
                false,
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, "CODEX_RPC_TIMEOUT");
        assert!(client.shared.pending.lock().unwrap().is_empty());
        assert!(!client.is_ready());
        assert!(client.turn_start("T1", "E2", "no replay").await.is_err());
        fake.await.unwrap();
    });
}
#[test]
fn server_request_responses_are_explicit_refusals() {
    run(async {
        let (client, server) = pair();
        let fake = tokio::spawn(async move {
            let mut s = BufReader::new(server);
            handshake(&mut s).await;
            for method in [
                "item/commandExecution/requestApproval",
                "item/fileChange/requestApproval",
                "item/permissions/requestApproval",
                "item/tool/requestUserInput",
                "mcpServer/elicitation/request",
                "item/tool/call",
                "account/chatgptAuthTokens/refresh",
                "attestation/generate",
                "currentTime/read",
                "applyPatchApproval",
                "execCommandApproval",
                "unknown",
            ] {
                s.write_all(
                    &encode(&json!({"id":"server-id","method":method,"params":{}})).unwrap(),
                )
                .await
                .unwrap();
                let response = recv(&mut s).await;
                assert_eq!(response["id"], "server-id");
                let expected = match method {
                    "item/commandExecution/requestApproval" | "item/fileChange/requestApproval" => {
                        Some(json!({"decision":"cancel"}))
                    }
                    "applyPatchApproval" | "execCommandApproval" => {
                        Some(json!({"decision":"abort"}))
                    }
                    "item/permissions/requestApproval" => {
                        Some(json!({"permissions":{},"scope":"turn"}))
                    }
                    "item/tool/requestUserInput" => Some(json!({"answers":{}})),
                    "mcpServer/elicitation/request" => {
                        Some(json!({"action":"cancel","content":null}))
                    }
                    "item/tool/call" => Some(json!({"contentItems":[],"success":false})),
                    _ => None,
                };
                if let Some(expected) = expected {
                    assert_eq!(response["result"], expected);
                    assert!(response.get("error").is_none());
                } else {
                    assert_eq!(
                        response["error"]["code"],
                        if method == "unknown" { -32601 } else { -32000 }
                    );
                    assert!(response.get("result").is_none());
                }
            }
        });
        client.initialize().await.unwrap();
        fake.await.unwrap();
    });
}
#[test]
fn cleanup_pagination_matrix() {
    run(async {
        let cases = vec![
            (
                vec![
                    json!({"data":[],"nextCursor":"cursor-1"}),
                    json!({"data":[],"nextCursor":null}),
                ],
                true,
            ),
            (
                vec![
                    json!({"data":[],"nextCursor":"cursor-1"}),
                    json!({"data":[{"processId":"1"}],"nextCursor":null}),
                ],
                false,
            ),
            (
                vec![
                    json!({"data":[{"processId":"1"}],"nextCursor":"cursor-1"}),
                    json!({"data":[],"nextCursor":null}),
                ],
                false,
            ),
            (
                vec![
                    json!({"data":[],"nextCursor":"loop"}),
                    json!({"data":[],"nextCursor":"loop"}),
                ],
                false,
            ),
            (vec![json!({"data":[]})], false),
            (vec![json!({"data":[{}]})], false),
            (vec![json!({"nextCursor":null})], false),
            (vec![json!({"data":[],"nextCursor":7})], false),
        ];
        for (pages, success) in cases {
            let (client, server) = pair();
            let fake = tokio::spawn(async move {
                let mut s = BufReader::new(server);
                handshake(&mut s).await;
                let req = recv(&mut s).await;
                assert_eq!(req["method"], "thread/backgroundTerminals/clean");
                reply(&mut s, &req, json!({})).await;
                let mut cursor = Value::Null;
                for page in pages {
                    let req = recv(&mut s).await;
                    assert_eq!(req["params"]["cursor"], cursor);
                    assert_eq!(req["params"]["limit"], 100);
                    cursor = page.get("nextCursor").cloned().unwrap_or(Value::Null);
                    reply(&mut s, &req, page).await;
                }
                tokio::time::sleep(Duration::from_millis(60)).await;
            });
            client.initialize().await.unwrap();
            let result = client
                .cleanup_until(scope(), Instant::now() + Duration::from_millis(30))
                .await;
            assert_eq!(result.is_ok(), success, "{result:?}");
            if let Ok(e) = result {
                assert_eq!(e.scope().execution_id(), "E1");
            }
            fake.await.unwrap();
        }
    });
}
#[test]
fn cleanup_rounds_restart_and_runtime_cannot_be_substituted() {
    run(async {
        let (client, server) = pair();
        let fake = tokio::spawn(async move {
            let mut s = BufReader::new(server);
            handshake(&mut s).await;
            let req = recv(&mut s).await;
            reply(&mut s, &req, json!({})).await;
            for nonempty in [true, false] {
                let req = recv(&mut s).await;
                assert!(req["params"]["cursor"].is_null());
                reply(
                    &mut s,
                    &req,
                    json!({"data":if nonempty{vec![json!({})]}else{vec![]},"nextCursor":null}),
                )
                .await;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        });
        client.initialize().await.unwrap();
        assert!(client.cleanup(scope()).await.is_ok());
        fake.await.unwrap();
        let (r2, _) = pair();
        let mut wrong = scope();
        wrong.runtime_id = "R2".into();
        assert_eq!(
            r2.cleanup(wrong).await.unwrap_err().code,
            "CODEX_APP_SERVER_INCOMPATIBLE"
        );
    });
}
#[test]
fn queue_and_pending_bounds_fail_closed() {
    run(async {
        let (client, _server) = pair();
        // Writer cannot drain into the unread duplex. No unbounded buffering.
        for _ in 0..=QUEUE_COUNT + 1 {
            if let Err(e) = client
                .shared
                .enqueue(json!({"method":"x","params":"x".repeat(4096)}))
            {
                assert_eq!(e.code, "CODEX_PROTOCOL_QUEUE_FULL");
                break;
            }
        }
        let permit = client
            .shared
            .writer_bytes
            .clone()
            .acquire_many_owned(QUEUE_BYTES as u32 - 1024 * 1024)
            .await;
        drop(permit);
        client.cancel();
        let (client, _server) = pair();
        let p = client
            .shared
            .writer_bytes
            .clone()
            .try_acquire_many_owned(QUEUE_BYTES as u32)
            .unwrap();
        assert_eq!(
            client.shared.enqueue(json!({})).unwrap_err().code,
            "CODEX_PROTOCOL_QUEUE_FULL"
        );
        drop(p);
        let (client, _server) = pair();
        client.shared.ready.store(true, Ordering::Release);
        for id in 0..QUEUE_COUNT {
            let (tx, _) = oneshot::channel();
            client.shared.pending.lock().unwrap().insert(
                id as u64,
                Pending {
                    method: "x".into(),
                    execution: None,
                    deadline: Instant::now() + RPC_TIMEOUT,
                    response: tx,
                },
            );
        }
        assert_eq!(
            client.thread_read("T").await.unwrap_err().code,
            "CODEX_PROTOCOL_QUEUE_FULL"
        );
        assert!(client.shared.pending.lock().unwrap().is_empty());
    });
}
#[test]
fn notification_queue_saturation_and_terminal_shape() {
    run(async {
        let (client, server) = pair();
        let (start, started) = oneshot::channel();
        let fake = tokio::spawn(async move {
            let mut s = BufReader::new(server);
            handshake(&mut s).await;
            started.await.unwrap();
            for _ in 0..=QUEUE_COUNT {
                let _ = s
                    .write_all(&encode(&json!({"method":"unknown","params":{}})).unwrap())
                    .await;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        });
        client.initialize().await.unwrap();
        start.send(()).unwrap();
        let mut f = client.failure();
        f.wait_for(|v| v.is_some()).await.unwrap();
        assert_eq!(
            f.borrow().as_ref().unwrap().code,
            "CODEX_PROTOCOL_QUEUE_FULL"
        );
        fake.await.unwrap();
        for status in ["completed", "interrupted", "failed"] {
            assert!(
                notification(
                    "turn/completed".into(),
                    json!({"threadId":"T","turn":{"id":"turn","status":status,"items":[]}})
                )
                .is_ok()
            );
        }
        for p in [
            json!({}),
            json!({"threadId":"T","turn":{"id":"x","items":[],"status":"inProgress"}}),
        ] {
            assert!(notification("turn/completed".into(), p).is_err());
        }
    });
}

fn turn_value(id: &str, status: &str) -> Value {
    json!({"id":id,"status":status,"items":[],"itemsView":"summary"})
}
fn metadata(id: &str, mode: &str) -> Value {
    json!({"thread":{"id":id,"historyMode":mode,"turns":[]}})
}
fn final_item() -> Value {
    json!({"type":"agentMessage","id":"persisted-item-2","text":"actual result","phase":"final_answer"})
}
async fn ready_script(script: Vec<(&'static str, Value)>) -> (Client, tokio::task::JoinHandle<()>) {
    let (client, server) = pair();
    let fake = tokio::spawn(async move {
        let mut s = BufReader::new(server);
        handshake(&mut s).await;
        for (method, response) in script {
            let req = recv(&mut s).await;
            assert_eq!(req["method"], method);
            if method == "thread/read" {
                assert_eq!(
                    req["params"]["includeTurns"],
                    response.get("testLegacy").is_some()
                );
            }
            if method == "thread/start" {
                assert_eq!(req["params"]["ephemeral"], false);
                assert_eq!(req["params"]["historyMode"], "paginated");
            }
            if method == "thread/items/list" {
                assert_eq!(req["params"]["turnId"], "target");
            }
            if response.get("error").is_some() {
                s.write_all(&encode(&json!({"id":req["id"],"error":response["error"]})).unwrap())
                    .await
                    .unwrap();
            } else {
                reply(&mut s, &req, response).await;
            }
        }
        // Keep the stream open until the test's Client is dropped. Any extra
        // request (e.g. fallback or latest-turn substitution) is a test failure.
        let mut line = String::new();
        let n = s.read_line(&mut line).await.unwrap_or(0);
        assert_eq!(n, 0, "Unexpected request {line}");
    });
    client.initialize().await.unwrap();
    (client, fake)
}
fn recovery_scope() -> recovery::RecoveryScope {
    recovery::RecoveryScope::same_runtime("R1", "T", "target", Some(TurnStatus::Completed))
}
#[test]
fn managed_thread_requires_explicit_paginated_response() {
    run(async {
        for mode in [
            json!("paginated"),
            json!("legacy"),
            Value::Null,
            json!("future"),
        ] {
            let response = if mode.is_null() {
                json!({"thread":{"id":"T","turns":[]}})
            } else {
                json!({"thread":{"id":"T","turns":[],"historyMode":mode}})
            };
            let (client, fake) = ready_script(vec![("thread/start", response)]).await;
            let result = client.thread_start("C:\\test").await;
            assert_eq!(result.is_ok(), mode == "paginated");
            if let Err(e) = result {
                assert_eq!(e.code, "CODEX_APP_SERVER_INCOMPATIBLE");
                assert!(!client.is_ready());
            }
            drop(client);
            fake.await.unwrap();
        }
    });
}
#[test]
fn exact_turn_first_or_later_page_and_items_full_pagination() {
    run(async {
        for later in [false, true] {
            for multi_items in [false, true] {
                let mut script = vec![("thread/read", metadata("T", "paginated"))];
                if later {
                    script.push(("thread/turns/list",json!({"data":[turn_value("other-completed","completed")],"nextCursor":"p2"})));
                }
                script.push((
                    "thread/turns/list",
                    json!({"data":[turn_value("target","completed")],"nextCursor":null}),
                ));
                if multi_items {
                    script.push(("thread/items/list", json!({"data":[],"nextCursor":"i2"})));
                }
                script.push((
                    "thread/items/list",
                    json!({"data":[{"turnId":"target","item":final_item()}],"nextCursor":null}),
                ));
                let (client, fake) = ready_script(script).await;
                let result = client.recover_result(recovery_scope()).await.unwrap();
                let value = serde_json::to_value(result).unwrap();
                assert_eq!(value["threadId"], "T");
                assert_eq!(value["turnId"], "target");
                assert_eq!(value["resultCompleteness"], "complete");
                assert_eq!(value["terminalTurn"]["items"][0], final_item());
                assert_eq!(value["finalResult"][0]["text"], "actual result");
                assert_eq!(value["sourceRuntimeId"], "R1");
                drop(client);
                fake.await.unwrap();
            }
        }
    });
}
#[test]
fn recovery_absent_nonterminal_conflict_wrong_thread_and_no_legacy_fallback() {
    run(async {
        let cases = vec![
            (
                vec![("thread/read", metadata("wrong", "paginated"))],
                "CODEX_APP_SERVER_INCOMPATIBLE",
            ),
            (
                vec![
                    ("thread/read", metadata("T", "paginated")),
                    (
                        "thread/turns/list",
                        json!({"data":[turn_value("other","completed")],"nextCursor":null}),
                    ),
                ],
                "CODEX_RESULT_TARGET_NOT_FOUND",
            ),
            (
                vec![
                    ("thread/read", metadata("T", "paginated")),
                    (
                        "thread/turns/list",
                        json!({"data":[turn_value("target","inProgress")],"nextCursor":null}),
                    ),
                ],
                "CODEX_RESULT_NOT_TERMINAL",
            ),
            (
                vec![
                    ("thread/read", metadata("T", "paginated")),
                    (
                        "thread/turns/list",
                        json!({"data":[turn_value("target","failed")],"nextCursor":null}),
                    ),
                ],
                "CODEX_RESULT_EVIDENCE_CONFLICT",
            ),
            (
                vec![
                    ("thread/read", metadata("T", "paginated")),
                    (
                        "thread/turns/list",
                        json!({"error":{"code":-32601,"message":"unsupported"}}),
                    ),
                ],
                "CODEX_APP_SERVER_INCOMPATIBLE",
            ),
        ];
        for (script, code) in cases {
            let (client, fake) = ready_script(script).await;
            let e = client.recover_result(recovery_scope()).await.unwrap_err();
            assert_eq!(e.code, code);
            assert!(!client.is_ready());
            drop(client);
            fake.await.unwrap();
        }
    });
}
#[test]
fn history_pages_reject_missing_malformed_repeated_and_wrong_turn() {
    run(async {
        for method in ["thread/turns/list", "thread/items/list"] {
            for bad in [
                json!({"data":[]}),
                json!({"data":[],"nextCursor":7}),
                json!({"data":[],"nextCursor":""}),
                json!({"nextCursor":null}),
                json!({"data":[],"nextCursor":"loop"}),
                json!({"data":[],"nextCursor":null,"threadId":"wrong"}),
            ] {
                let mut script = vec![("thread/read", metadata("T", "paginated"))];
                if method == "thread/items/list" {
                    script.push((
                        "thread/turns/list",
                        json!({"data":[turn_value("target","completed")],"nextCursor":null}),
                    ));
                }
                script.push((method, bad.clone()));
                if bad["nextCursor"] == "loop" {
                    script.push((method, bad));
                }
                let (client, fake) = ready_script(script).await;
                assert_eq!(
                    client
                        .recover_result(recovery_scope())
                        .await
                        .unwrap_err()
                        .code,
                    "CODEX_APP_SERVER_INCOMPATIBLE"
                );
                drop(client);
                fake.await.unwrap();
            }
        }
        let (client, fake) = ready_script(vec![
            ("thread/read", metadata("T", "paginated")),
            (
                "thread/turns/list",
                json!({"data":[turn_value("target","completed")],"nextCursor":null}),
            ),
            (
                "thread/items/list",
                json!({"data":[{"turnId":"wrong","item":final_item()}],"nextCursor":null}),
            ),
        ])
        .await;
        assert_eq!(
            client
                .recover_result(recovery_scope())
                .await
                .unwrap_err()
                .code,
            "CODEX_APP_SERVER_INCOMPATIBLE"
        );
        drop(client);
        fake.await.unwrap();
    });
}
#[test]
fn legacy_metadata_selects_reader_and_item_identity_is_not_a_gate() {
    run(async {
        for present in [true, false] {
            let turns = if present {
                vec![
                    json!({"id":"target","status":"completed","items":[final_item()],"itemsView":"full"}),
                ]
            } else {
                vec![turn_value("other", "completed")]
            };
            let(client,fake)=ready_script(vec![("thread/read",metadata("T","legacy")),("thread/read",json!({"testLegacy":true,"thread":{"id":"T","historyMode":"legacy","turns":turns}}))]).await;
            let result = client.recover_result(recovery_scope()).await;
            assert_eq!(result.is_ok(), present);
            if let Ok(result) = result {
                assert_eq!(result.final_result()[0]["id"], "persisted-item-2");
            }
            drop(client);
            fake.await.unwrap();
        }
    });
}
#[test]
fn cross_runtime_scope_requires_confirmed_job_evidence() {
    run(async {
        let tmp = tempfile::tempdir().unwrap();
        let store = crate::agent::store::StateStore::open(tmp.path().into())
            .await
            .unwrap();
        let result = recovery::RecoveryScope::after_termination(
            &store,
            "missing-R1",
            "R2",
            "T",
            "target",
            None,
        )
        .await;
        assert_eq!(result.unwrap_err().code, "CODEX_RUNTIME_NOT_FOUND");
        let (client, _server) = pair();
        let scope = recovery::RecoveryScope::same_runtime("R2", "T", "target", None);
        assert_eq!(
            client.recover_result(scope).await.unwrap_err().code,
            "CODEX_APP_SERVER_INCOMPATIBLE"
        );
    });
}

pub(super) fn record_stdin<W: tokio::io::AsyncWrite + Unpin + Send + 'static>(
    writer: W,
    id: &str,
) -> Box<dyn tokio::io::AsyncWrite + Unpin + Send> {
    use std::io::Write;
    struct Tee<W> {
        inner: W,
        file: std::fs::File,
    }
    impl<W: tokio::io::AsyncWrite + Unpin> tokio::io::AsyncWrite for Tee<W> {
        fn poll_write(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &[u8],
        ) -> std::task::Poll<std::io::Result<usize>> {
            let result = std::pin::Pin::new(&mut self.inner).poll_write(cx, buf);
            if let std::task::Poll::Ready(Ok(n)) = result {
                self.file.write_all(&buf[..n])?;
                self.file.flush()?;
            }
            result
        }
        fn poll_flush(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            std::pin::Pin::new(&mut self.inner).poll_flush(cx)
        }
        fn poll_shutdown(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            std::pin::Pin::new(&mut self.inner).poll_shutdown(cx)
        }
    }
    if let Some(dir) = std::env::var_os("SERENA_CONTRACT_RAW_DIR") {
        Box::new(Tee {
            inner: writer,
            file: std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(std::path::PathBuf::from(dir).join(format!("{id}.stdin.raw.jsonl")))
                .unwrap(),
        })
    } else {
        Box::new(writer)
    }
}

#[cfg(windows)]
pub(super) fn record_stdout<R: tokio::io::AsyncRead + Unpin + Send + 'static>(
    reader: R,
    id: &str,
) -> Box<dyn tokio::io::AsyncRead + Unpin + Send> {
    use std::io::Write;
    struct Tee<R> {
        inner: R,
        file: std::fs::File,
    }
    impl<R: tokio::io::AsyncRead + Unpin> tokio::io::AsyncRead for Tee<R> {
        fn poll_read(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &mut tokio::io::ReadBuf<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            let start = buf.filled().len();
            let result = std::pin::Pin::new(&mut self.inner).poll_read(cx, buf);
            if let std::task::Poll::Ready(Ok(())) = &result {
                self.file.write_all(&buf.filled()[start..])?;
                self.file.flush()?;
            }
            result
        }
    }
    if let Some(dir) = std::env::var_os("SERENA_CONTRACT_RAW_DIR") {
        let path = std::path::PathBuf::from(dir).join(format!("{id}.stdout.raw.jsonl"));
        Box::new(Tee {
            inner: reader,
            file: std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
                .unwrap(),
        })
    } else {
        Box::new(reader)
    }
}
#[cfg(windows)]
#[test]
#[ignore = "Explicit fixed-binary Contract run with isolated Home and existing authentication"]
fn real_fixed_binary_contract() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("codex-home");
    std::fs::create_dir(&home).unwrap();
    std::fs::copy(
        std::path::PathBuf::from(std::env::var_os("USERPROFILE").unwrap()).join(".codex/auth.json"),
        home.join("auth.json"),
    )
    .unwrap();
    let evidence = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../docs/tasks/evidence/TASK-005/codex-0.153.4/final-client");
    std::fs::create_dir_all(&evidence).unwrap();
    let run_dir = evidence.join(format!(
        "run-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));
    std::fs::create_dir(&run_dir).unwrap();
    struct Environment(Vec<(&'static str, Option<std::ffi::OsString>)>);
    impl Drop for Environment {
        fn drop(&mut self) {
            for (key, value) in &self.0 {
                unsafe {
                    match value {
                        Some(v) => std::env::set_var(key, v),
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
    // Windows environment mutation is supported; this test is run explicitly alone.
    unsafe {
        std::env::set_var("CODEX_HOME", &home);
        std::env::set_var("SERENA_CONTRACT_RAW_DIR", &run_dir);
    }
    run(async {
        let exe = std::path::PathBuf::from(
            r"C:\Users\lifei\AppData\Roaming\npm\node_modules\@openai\codex\node_modules\@openai\codex-win32-x64\vendor\x86_64-pc-windows-msvc\bin\codex.exe",
        );
        let store = crate::agent::store::StateStore::open(temp.path().join("store"))
            .await
            .unwrap();
        let r1id = format!("final-R1-{}", std::process::id());
        let r2id = format!("final-R2-{}", std::process::id());
        let mut log = vec![format!(
            "version={VERSION}; binary={BINARY_SHA256}; schema={SCHEMA_SHA256}; source={SOURCE_COMMIT}"
        )];
        let mut r1 = managed::connect(
            store.clone(),
            "contract-host".into(),
            r1id.clone(),
            exe.clone(),
            temp.path().into(),
        )
        .await
        .unwrap();
        log.push(format!(
            "verified executable path={}",
            r1.compatibility.executable.display()
        ));
        log.push(
            "R1 fresh whitelist/schema export + initialize/initialized experimentalApi PASS".into(),
        );
        let observed=async {
            let thread=r1.client.thread_start(temp.path().to_str().unwrap()).await?;assert_eq!(thread.history_mode,HistoryMode::Paginated);
            assert_eq!(r1.client.thread_read(&thread.id).await?.id,thread.id);

            log.push("explicit paginated managed Thread; metadata PASS".into());
            let turn=r1.client.turn_start(&thread.id,"E-protocol-only","Reply exactly CONTRACT_RESULT_OK. Do not use tools or change files.").await?;
            let terminal=real_terminal(&mut r1.client,&thread.id,&turn.id,&mut log).await?;
            assert_eq!(terminal.status,TurnStatus::Completed);
            persisted_execution(&temp.path().join("store"),"E-protocol-only",Some(&r1id),Some(&thread.id),Some(&turn.id),None,None);
            let scope=recovery::RecoveryScope::same_runtime_for_execution(&store,"E-protocol-only",&r1id).await?;
            let result=r1.client.recover_result(scope).await?;
            let actual:Vec<_>=terminal.items.iter().filter(|i|i["type"]=="agentMessage").cloned().collect();
            assert!(!actual.is_empty());assert_eq!(result.final_result(),actual);
            log.push(format!("R1 turns/items recovery PASS; thread={}; turn={}; final={:?}",thread.id,turn.id,result.final_result()));
            assert_eq!(r1.client.thread_resume(&thread.id).await?.id,thread.id);
            log.push("post-Turn resume(excludeTurns=true) PASS".into());
            r1.client.clean(&thread.id).await?;
            let page=r1.client.list(&thread.id,None).await?;assert!(page.data.is_empty());assert_eq!(page.next_cursor,NextCursor::Null);
            r1.client.cleanup(CleanupScope::for_execution(&store,"E-protocol-only").await?).await?;
            log.push("clean accepted + independent full list/cleanup empty PASS".into());
            let interrupt=r1.client.turn_start(&thread.id,"E-interrupt-protocol-only","Write a short greeting. Do not use tools.").await?;
            let started_deadline=Instant::now()+Duration::from_secs(30);
            loop {
                let event=timeout_at(started_deadline,r1.client.next_event()).await.map_err(|_|ProtocolError::new("CODEX_RPC_TIMEOUT","Turn did not start"))??;
                if matches!(event.notification,Notification::TurnStarted{thread_id,turn} if thread_id==thread.id && turn.id==interrupt.id){break;}
            }
            r1.client.turn_interrupt(&thread.id,&interrupt.id).await?;
            let interrupted=real_terminal(&mut r1.client,&thread.id,&interrupt.id,&mut log).await?;
            log.push(format!("turn/interrupt ACK and terminal notification PASS; observed {:?}",interrupted.status));
            Ok::<_,ProtocolError>((thread.id,turn.id,serde_json::to_value(result).unwrap()))
        }.await;
        r1.client.cancel();
        let convergence = r1.shutdown().await;
        log.push(format!("R1 Job reconciliation={convergence:?}"));
        std::fs::write(run_dir.join("result.txt"), log.join("\n")).unwrap();
        convergence.unwrap();
        let (_thread, _turn, original) = observed.unwrap();
        let row = store.runtime(r1id.clone()).await.unwrap().unwrap();
        assert_eq!(row.termination_evidence_state, "complete");
        assert_eq!(row.state, "terminated");
        log.push(format!(
            "R1 durable Runtime identity / Job evidence: {row:?}"
        ));
        let r2 = managed::connect(
            store.clone(),
            "contract-host-2".into(),
            r2id.clone(),
            exe.clone(),
            temp.path().into(),
        )
        .await
        .unwrap();
        let scope = recovery::RecoveryScope::after_termination_for_execution(
            &store,
            "E-protocol-only",
            &r2id,
        )
        .await
        .unwrap();
        let original_execution = store.execution("E-protocol-only".into()).await.unwrap();
        let recovered = r2.client.recover_result(scope).await;
        assert_eq!(
            store.execution("E-protocol-only".into()).await.unwrap(),
            original_execution
        );
        r2.client.cancel();
        r2.shutdown().await.unwrap();
        let r2row = store.runtime(r2id.clone()).await.unwrap().unwrap();
        log.push(format!(
            "R2 durable Runtime identity / Job evidence: {r2row:?}"
        ));
        let result = serde_json::to_value(recovered.unwrap()).unwrap();
        assert_eq!(result["threadId"], original["threadId"]);
        assert_eq!(result["turnId"], original["turnId"]);
        assert_eq!(
            result["terminalTurn"]["status"],
            original["terminalTurn"]["status"]
        );
        assert_eq!(result["finalResult"], original["finalResult"]);
        assert_eq!(result["sourceRuntimeId"], r1id);
        assert_eq!(result["recoveredByRuntimeId"], r2id);
        log.push(
            "independent R2 exact-target result recovery PASS; no Provider Evidence written".into(),
        );
        for dropped in [true, false] {
            let id = format!("monitor-{}-{dropped}", std::process::id());
            let managed = managed::connect(
                store.clone(),
                "monitor-test".into(),
                id.clone(),
                exe.clone(),
                temp.path().into(),
            )
            .await
            .unwrap();
            if dropped {
                drop(managed);
            } else {
                assert!(
                    managed
                        .client
                        .rpc(
                            "deliberately/unsupported",
                            json!({}),
                            None,
                            Instant::now() + RPC_TIMEOUT,
                            false
                        )
                        .await
                        .is_err()
                );
                managed.wait_for_reconciliation().await.unwrap();
            }
            let deadline = Instant::now() + INIT_TIMEOUT;
            loop {
                let row = store.runtime(id.clone()).await.unwrap().unwrap();
                if row.termination_evidence_state == "complete" {
                    assert_eq!(
                        row.termination_evidence_type.as_deref(),
                        Some("job_active_processes_zero")
                    );
                    break;
                }
                assert!(Instant::now() < deadline);
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            log.push(format!(
                "private monitor: drop={dropped}, durable Job convergence PASS"
            ));
        }
        log.push("Real Server Requests / failed-terminal variant / malformed server stdout = UNAVAILABLE; exact schema + Fake Server coverage; historical CASE P legacy evidence referenced, not rerun".into());
        log.push(
            "Background MULTI_PAGE_WIRE = UNAVAILABLE; Fake pagination matrix is separate evidence"
                .into(),
        );
        std::fs::write(run_dir.join("result.txt"), log.join("\n")).unwrap();
    });
}
#[cfg(windows)]
async fn real_terminal(
    client: &mut Client,
    thread: &str,
    turn: &str,
    log: &mut Vec<String>,
) -> Result<Turn> {
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        let event = timeout_at(deadline, client.next_event())
            .await
            .map_err(|_| {
                ProtocolError::new(
                    "CODEX_RPC_TIMEOUT",
                    "Real terminal notification unavailable",
                )
            })??;
        match event.notification {
            Notification::ThreadStarted(_) => log.push("thread/started observed".into()),
            Notification::TurnStarted { .. } => log.push("turn/started observed".into()),
            Notification::TurnCompleted { thread_id, turn: t }
                if thread_id == thread && t.id == turn =>
            {
                log.push(format!("turn/completed observed {:?}", t.status));
                return Ok(t);
            }
            _ => {}
        }
    }
}

#[test]
fn recovery_and_cleanup_page_failures_never_produce_complete_evidence() {
    run(async {
        for history in [true, false] {
            for failure in ["rpc", "timeout", "oversize", "disconnect"] {
                let (client, server) = pair();
                let fake = tokio::spawn(async move {
                    let mut s = BufReader::new(server);
                    handshake(&mut s).await;
                    let req = recv(&mut s).await;
                    reply(
                        &mut s,
                        &req,
                        if history {
                            metadata("T", "paginated")
                        } else {
                            json!({})
                        },
                    )
                    .await;
                    let req = recv(&mut s).await;
                    reply(&mut s, &req, json!({"data":[],"nextCursor":"p2"})).await;
                    let req = recv(&mut s).await;
                    assert_eq!(req["params"]["cursor"], "p2");
                    match failure {
                "rpc"=>s.write_all(&encode(&json!({"id":req["id"],"error":{"code":-32000,"message":"page failed"}})).unwrap()).await.unwrap(),
                "timeout"=>tokio::time::sleep(Duration::from_millis(100)).await,
                "oversize"=>{let _=s.write_all(&vec![b'x';MAX_MESSAGE+1]).await;},
                _=>return,
            }
                    let mut rest = String::new();
                    let _ = s.read_line(&mut rest).await;
                });
                client.initialize().await.unwrap();
                let deadline = Instant::now()
                    + if failure == "timeout" {
                        Duration::from_millis(50)
                    } else {
                        Duration::from_secs(10)
                    };
                let error = if history {
                    client
                        .recover_until(recovery_scope(), deadline)
                        .await
                        .unwrap_err()
                } else {
                    client.cleanup_until(scope(), deadline).await.unwrap_err()
                };
                assert_eq!(
                    error.code,
                    match failure {
                        "rpc" => "CODEX_RPC_FAILED",
                        "timeout" => "CODEX_RPC_TIMEOUT",
                        "oversize" => "CODEX_PROTOCOL_MESSAGE_TOO_LARGE",
                        _ => "CODEX_STDIO_EOF",
                    }
                );
                assert!(!client.is_ready());
                assert!(client.shared.pending.lock().unwrap().is_empty());
                // R2 cannot append its pages to the failed R1 scan.
                let (read, _write) = tokio::io::duplex(1024);
                let r2 =
                    Client::transport("R2".into(), read, tokio::io::sink(), tokio::io::empty());
                let r2error = if history {
                    r2.recover_result(recovery_scope()).await.unwrap_err()
                } else {
                    r2.cleanup(scope()).await.unwrap_err()
                };
                assert_eq!(r2error.code, "CODEX_APP_SERVER_INCOMPATIBLE");
                drop(r2);
                drop(client);
                fake.await.unwrap();
            }
        }
    });
}

#[test]
fn items_pagination_transport_failures_discard_accumulated_results() {
    run(async {
        for failure in ["rpc", "timeout", "oversize", "disconnect"] {
            assert_items_page_failure(failure).await;
        }
    });
}

#[test]
fn items_pagination_obeys_remaining_total_recovery_deadline() {
    run(assert_items_page_failure("deadline"));
}

async fn assert_items_page_failure(failure: &'static str) {
    let temp = tempfile::tempdir().unwrap();
    let store = crate::agent::store::StateStore::open(temp.path().into())
        .await
        .unwrap();
    persisted_execution(
        temp.path(),
        "E-items",
        Some("R1"),
        Some("T"),
        Some("target"),
        Some("completed"),
        Some("R1"),
    );
    let scope = recovery::RecoveryScope::same_runtime_for_execution(&store, "E-items", "R1")
        .await
        .unwrap();
    let before = store.execution("E-items".into()).await.unwrap();
    let (client, server) = pair();
    let (second_page, observed_second_page) = oneshot::channel();
    let fake = tokio::spawn(async move {
        let mut s = BufReader::new(server);
        handshake(&mut s).await;
        let req = recv(&mut s).await;
        assert_eq!(req["method"], "thread/read");
        assert_eq!(req["params"], json!({"threadId":"T","includeTurns":false}));
        reply(&mut s, &req, metadata("T", "paginated")).await;
        let req = recv(&mut s).await;
        assert_eq!(req["method"], "thread/turns/list");
        assert_eq!(req["params"]["threadId"], "T");
        assert_eq!(req["params"]["cursor"], Value::Null);
        reply(
            &mut s,
            &req,
            json!({"data":[turn_value("target","completed")],"nextCursor":null}),
        )
        .await;
        let req = recv(&mut s).await;
        assert_eq!(req["method"], "thread/items/list");
        assert_eq!(req["params"]["threadId"], "T");
        assert_eq!(req["params"]["turnId"], "target");
        assert_eq!(req["params"]["cursor"], Value::Null);
        if failure == "deadline" {
            // Spend half of the 8s recovery budget before delivering page one.
            // Page two must inherit the remaining ~4s, not a fresh 8s or 15s RPC budget.
            tokio::time::sleep(Duration::from_secs(4)).await;
        }
        reply(
            &mut s,
            &req,
            json!({"data":[{"turnId":"target","item":final_item()}],"nextCursor":"i2"}),
        )
        .await;
        let req = recv(&mut s).await;
        assert_eq!(req["method"], "thread/items/list");
        assert_eq!(req["params"]["threadId"], "T");
        assert_eq!(req["params"]["turnId"], "target");
        assert_eq!(req["params"]["cursor"], "i2");
        second_page.send(Instant::now()).unwrap();
        match failure {
            "rpc" => s
                .write_all(
                    &encode(&json!({"id":req["id"],"error":{"code":-32000,"message":"items page failed"}}))
                        .unwrap(),
                )
                .await
                .unwrap(),
            "oversize" => {
                // Raw bytes exceed framing limit; no invalid JSON parse is needed.
                let _ = s.write_all(&vec![b'x'; MAX_MESSAGE + 1]).await;
            }
            "disconnect" => {
                // Close server stdout only, retaining stdin to detect any replay/fallback.
                s.get_mut().shutdown().await.unwrap();
            }
            "timeout" | "deadline" => {}
            _ => unreachable!(),
        }
        // No response for timeout cases: a 25s server delay exceeds both the
        // ordinary 15s RPC deadline and the remaining total deadline. EOF must
        // arrive first. Keep observing stdin, including after stdout disconnect.
        let mut extra = String::new();
        let n = timeout_at(
            Instant::now() + Duration::from_secs(25),
            s.read_line(&mut extra),
        )
        .await
        .expect("Client did not close failed items transport")
        .unwrap();
        assert_eq!(
            n, 0,
            "Unexpected replay/fallback/latest/legacy request: {extra}"
        );
    });
    client.initialize().await.unwrap();
    let failure_signal = client.failure();
    let started = Instant::now();
    let budget = Duration::from_secs(8);
    let watchdog = if failure == "deadline" {
        // 2s scheduling margin on Windows; still below a reset 8s page budget
        // after the first 4s delay (~12s), and far below a fresh 15s RPC timer.
        budget + Duration::from_secs(2)
    } else {
        RPC_TIMEOUT + Duration::from_secs(5)
    };
    let result = timeout_at(started + watchdog, async {
        if failure == "deadline" {
            client.recover_until(scope.clone(), started + budget).await
        } else {
            client.recover_result(scope.clone()).await
        }
    })
    .await
    .expect("Recovery exceeded its deadline plus scheduling margin");
    let elapsed = started.elapsed();
    // Err is the only returned value: no RecoveredResult (complete or otherwise)
    // can escape with the first page's already accumulated final_answer item.
    let error = result.expect_err("Partial items must never become a complete RecoveredResult");
    let expected = match failure {
        "rpc" => "CODEX_RPC_FAILED",
        "timeout" | "deadline" => "CODEX_RPC_TIMEOUT",
        "oversize" => "CODEX_PROTOCOL_MESSAGE_TOO_LARGE",
        "disconnect" => "CODEX_STDIO_EOF",
        _ => unreachable!(),
    };
    assert_eq!(error.code, expected);
    let second_at = observed_second_page.await.unwrap();
    if failure == "deadline" {
        assert!(second_at.duration_since(started) >= Duration::from_secs(4));
        assert!(
            second_at < started + budget,
            "Second page must start inside total budget"
        );
        assert!(elapsed >= budget);
    } else if failure == "timeout" {
        assert!(elapsed >= RPC_TIMEOUT);
    }
    eprintln!(
        "items failure={failure}; second_page_after={:?}; elapsed={elapsed:?}; watchdog={watchdog:?}; error={expected}",
        second_at.duration_since(started)
    );
    assert!(!client.is_ready());
    assert_eq!(failure_signal.borrow().as_ref().unwrap().code, expected);
    assert!(client.shared.cancel.is_cancelled());
    assert!(client.shared.pending.lock().unwrap().is_empty());
    assert_eq!(
        client.recover_result(scope.clone()).await.unwrap_err().code,
        expected
    );
    assert_eq!(store.execution("E-items".into()).await.unwrap(), before);
    timeout_at(Instant::now() + Duration::from_secs(5), fake)
        .await
        .unwrap()
        .unwrap();

    // A healthy, initialized R2 still cannot continue the failed R1 scope.
    // Observe its actual writer: no fresh scan, empty page or fallback may be requested.
    let (r2_io, r2_server) = tokio::io::duplex(128 * 1024);
    let (r2_read, r2_write) = tokio::io::split(r2_io);
    let r2 = Client::transport("R2".into(), r2_read, r2_write, tokio::io::empty());
    let r2_fake = tokio::spawn(async move {
        let mut s = BufReader::new(r2_server);
        handshake(&mut s).await;
        let mut extra = String::new();
        let n = timeout_at(
            Instant::now() + Duration::from_secs(5),
            s.read_line(&mut extra),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(n, 0, "R2 must not replace/continue R1 scan: {extra}");
    });
    r2.initialize().await.unwrap();
    assert!(r2.is_ready());
    assert_eq!(
        r2.recover_result(scope).await.unwrap_err().code,
        "CODEX_APP_SERVER_INCOMPATIBLE"
    );
    assert!(r2.shared.pending.lock().unwrap().is_empty());
    assert_eq!(
        recovery::RecoveryScope::after_termination_for_execution(&store, "E-items", "R2")
            .await
            .unwrap_err()
            .code,
        "CODEX_RESULT_RECOVERY_UNSAFE"
    );
    r2_fake.await.unwrap();
}

#[test]
fn total_deadline_and_metadata_contract_are_explicit() {
    run(async {
        let (client, _server) = pair();
        assert_eq!(
            client
                .recover_until(recovery_scope(), Instant::now())
                .await
                .unwrap_err()
                .code,
            "CODEX_RESULT_RECOVERY_TIMEOUT"
        );
        for meta in [
            json!({"thread":{"id":"T","turns":[]}}),
            metadata("T", "future"),
        ] {
            let (client, fake) = ready_script(vec![("thread/read", meta)]).await;
            assert_eq!(
                client
                    .recover_result(recovery_scope())
                    .await
                    .unwrap_err()
                    .code,
                "CODEX_APP_SERVER_INCOMPATIBLE"
            );
            drop(client);
            fake.await.unwrap();
        }
        // Even finding the target cannot excuse a missing page boundary.
        let (client, fake) = ready_script(vec![
            ("thread/read", metadata("T", "paginated")),
            (
                "thread/turns/list",
                json!({"data":[turn_value("target","completed")]}),
            ),
        ])
        .await;
        assert_eq!(
            client
                .recover_result(recovery_scope())
                .await
                .unwrap_err()
                .code,
            "CODEX_APP_SERVER_INCOMPATIBLE"
        );
        drop(client);
        fake.await.unwrap();
        let temp = tempfile::tempdir().unwrap();
        let store = crate::agent::store::StateStore::open(temp.path().into())
            .await
            .unwrap();
        store
            .prepare_runtime("R1", "host", "job", 1, "test.exe", 1)
            .unwrap();
        assert_eq!(
            recovery::RecoveryScope::after_termination(&store, "R1", "R2", "T", "target", None)
                .await
                .unwrap_err()
                .code,
            "CODEX_RESULT_RECOVERY_UNSAFE"
        );
    });
}

#[test]
fn abandoned_request_cleans_pending_and_reconciles_without_replay() {
    run(async {
        let (client, server) = pair();
        let fake = tokio::spawn(async move {
            let mut s = BufReader::new(server);
            handshake(&mut s).await;
            let req = recv(&mut s).await;
            assert_eq!(req["method"], "thread/read");
            let mut line = String::new();
            assert_eq!(s.read_line(&mut line).await.unwrap(), 0);
        });
        client.initialize().await.unwrap();
        assert!(
            timeout_at(
                Instant::now() + Duration::from_millis(20),
                client.thread_read("T")
            )
            .await
            .is_err()
        );
        assert_eq!(
            client.shared.check().unwrap_err().code,
            "CODEX_REQUEST_CANCELLED"
        );
        assert!(client.shared.pending.lock().unwrap().is_empty());
        assert!(!client.is_ready());
        drop(client);
        fake.await.unwrap();
    });
}

#[test]
fn malformed_final_phase_is_rejected_in_both_history_modes() {
    run(async {
        for mode in ["paginated", "legacy"] {
            for phase in [json!(42), json!("future")] {
                let mut item = final_item();
                item["phase"] = phase;
                let mut script = vec![("thread/read", metadata("T", mode))];
                if mode == "paginated" {
                    script.push((
                        "thread/turns/list",
                        json!({"data":[turn_value("target","completed")],"nextCursor":null}),
                    ));
                    script.push((
                        "thread/items/list",
                        json!({"data":[{"turnId":"target","item":item}],"nextCursor":null}),
                    ));
                } else {
                    script.push(("thread/read",json!({"testLegacy":true,"thread":{"id":"T","historyMode":"legacy","turns":[{"id":"target","status":"completed","items":[item]}]}})));
                }
                let (client, fake) = ready_script(script).await;
                assert_eq!(
                    client
                        .recover_result(recovery_scope())
                        .await
                        .unwrap_err()
                        .code,
                    "CODEX_APP_SERVER_INCOMPATIBLE"
                );
                drop(client);
                fake.await.unwrap();
            }
        }
    });
}

#[cfg(windows)]
#[test]
fn cancelled_creation_handoff_persists_job_termination() {
    use crate::agent::codex::{runtime::Runtime, windows_launcher::LaunchRequest};
    use std::os::windows::process::CommandExt;
    let temp = tempfile::tempdir().unwrap();
    let exe = temp.path().join("handoff-child.exe");
    let compiled = std::process::Command::new("rustc")
        .args(["--edition=2024", "--crate-name", "runtime_child"])
        .arg(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/runtime_child.rs"),
        )
        .arg("-o")
        .arg(&exe)
        .creation_flags(0x08000000)
        .output()
        .unwrap();
    assert!(compiled.status.success());
    run(async {
        let store = crate::agent::store::StateStore::open(temp.path().join("store"))
            .await
            .unwrap();
        for after_delivery in [false, true] {
            let id = format!("cancelled-handoff-{}-{after_delivery}", std::process::id());
            let request = LaunchRequest {
                executable: exe.clone(),
                current_dir: temp.path().into(),
                args: vec![temp.path().as_os_str().into(), "leaf".into()],
                runtime_instance_id: id.clone(),
            };
            let (start, started) = oneshot::channel();
            let worker_store = store.clone();
            let receiver = managed::handoff_creation(async move {
                started.await.unwrap();
                Runtime::create(
                    worker_store,
                    "test-host".into(),
                    request,
                    Duration::from_secs(5),
                )
                .await
            });
            // The caller goes away before the asynchronous creation returns a Child.
            if after_delivery {
                start.send(()).unwrap();
                let envelope = receiver.await.unwrap();
                drop(envelope);
            } else {
                drop(receiver);
                start.send(()).unwrap();
            }
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                if let Some(row) = store.runtime(id.clone()).await.unwrap()
                    && row.termination_evidence_state == "complete"
                {
                    assert_eq!(row.state, "terminated");
                    assert_eq!(
                        row.termination_evidence_type.as_deref(),
                        Some("job_active_processes_zero")
                    );
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "Created Runtime was left without confirmed termination evidence"
                );
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
    });
}

// Test-only DB fixtures; no SQL entry is exported to production providers.
fn persisted_execution(
    path: &std::path::Path,
    id: &str,
    runtime: Option<&str>,
    thread: Option<&str>,
    turn: Option<&str>,
    terminal: Option<&str>,
    owner: Option<&str>,
) {
    let c = rusqlite::Connection::open(path.join("agent-state.db")).unwrap();
    c.pragma_update(None, "foreign_keys", true).unwrap();
    for runtime in runtime.into_iter().chain(owner) {
        c.execute("INSERT INTO runtime_instances(id,owner_host_instance_id,state,created_at,updated_at) VALUES (?1,'fixture','unknown',1,1) ON CONFLICT(id) DO NOTHING",[runtime]).unwrap();
    }
    c.execute("INSERT INTO executions(id,agent_id,request_key,request_hash,prompt,execution_profile_json,workspace_id,canonical_workspace_root,provider,mode,status,runtime_instance_id,thread_id,turn_id,provider_terminal_status,provider_terminal_evidence_runtime_instance_id,created_at,updated_at) VALUES (?1,?1,'k','hash','prompt','{}','w',?1,'codex','read_only','finalizing',?2,?3,?4,?5,?6,1,1)",rusqlite::params![id,runtime,thread,turn,terminal,owner]).unwrap();
}
#[test]
fn cleanup_is_bound_to_persisted_execution_and_revision() {
    run(async {
        let temp = tempfile::tempdir().unwrap();
        let store = crate::agent::store::StateStore::open(temp.path().into())
            .await
            .unwrap();
        persisted_execution(
            temp.path(),
            "E1",
            Some("R1"),
            Some("T1"),
            Some("TURN1"),
            None,
            None,
        );
        persisted_execution(
            temp.path(),
            "E2",
            Some("R1"),
            Some("T2"),
            Some("TURN2"),
            None,
            None,
        );
        persisted_execution(
            temp.path(),
            "no-runtime",
            None,
            Some("T1"),
            None,
            None,
            None,
        );
        persisted_execution(temp.path(), "no-thread", Some("R1"), None, None, None, None);
        for id in ["missing", "no-runtime", "no-thread"] {
            assert!(CleanupScope::for_execution(&store, id).await.is_err());
        }
        for id in ["E1", "E2"] {
            let binding = CleanupScope::for_execution(&store, id).await.unwrap();
            let expected = binding.thread_id().to_owned();
            let (client, server) = pair();
            let fake = tokio::spawn(async move {
                let mut s = BufReader::new(server);
                handshake(&mut s).await;
                for method in [
                    "thread/backgroundTerminals/clean",
                    "thread/backgroundTerminals/list",
                ] {
                    let req = recv(&mut s).await;
                    assert_eq!(req["method"], method);
                    assert_eq!(req["params"]["threadId"], expected);
                    reply(
                        &mut s,
                        &req,
                        if method.ends_with("clean") {
                            json!({})
                        } else {
                            json!({"data":[],"nextCursor":null})
                        },
                    )
                    .await;
                }
                let mut line = String::new();
                let _ = s.read_line(&mut line).await;
            });
            client.initialize().await.unwrap();
            let evidence = client.cleanup(binding).await.unwrap();
            assert_eq!(evidence.scope().execution_id(), id);
            assert_eq!(evidence.scope().revision(), Some(0));
            drop(client);
            fake.await.unwrap();
        }
        let binding = CleanupScope::for_execution(&store, "E1").await.unwrap();
        let (reader, _hold) = tokio::io::duplex(64);
        let r2 = Client::transport("R2".into(), reader, tokio::io::sink(), tokio::io::empty());
        assert_eq!(
            r2.cleanup(binding).await.unwrap_err().code,
            "CODEX_APP_SERVER_INCOMPATIBLE"
        );
        let binding = CleanupScope::for_execution(&store, "E1").await.unwrap();
        let c = rusqlite::Connection::open(temp.path().join("agent-state.db")).unwrap();
        c.execute(
            "UPDATE executions SET revision=revision+1 WHERE id='E1'",
            [],
        )
        .unwrap();
        let (client, _) = pair();
        assert_eq!(
            client.cleanup(binding).await.unwrap_err().code,
            "CODEX_EXECUTION_BINDING_STALE"
        );
        // A revision change during the scan is also rejected at evidence construction.
        let binding = CleanupScope::for_execution(&store, "E1").await.unwrap();
        let path = temp.path().to_owned();
        let (client, server) = pair();
        let fake = tokio::spawn(async move {
            let mut s = BufReader::new(server);
            handshake(&mut s).await;
            let req = recv(&mut s).await;
            reply(&mut s, &req, json!({})).await;
            let req = recv(&mut s).await;
            let c = rusqlite::Connection::open(path.join("agent-state.db")).unwrap();
            c.execute(
                "UPDATE executions SET revision=revision+1 WHERE id='E1'",
                [],
            )
            .unwrap();
            reply(&mut s, &req, json!({"data":[],"nextCursor":null})).await;
            let mut line = String::new();
            let _ = s.read_line(&mut line).await;
        });
        client.initialize().await.unwrap();
        assert_eq!(
            client.cleanup(binding).await.unwrap_err().code,
            "CODEX_EXECUTION_BINDING_STALE"
        );
        drop(client);
        fake.await.unwrap();
    });
}
#[test]
fn recovery_uses_persisted_identity_and_cannot_hide_provider_evidence() {
    run(async {
        let temp = tempfile::tempdir().unwrap();
        let store = crate::agent::store::StateStore::open(temp.path().into())
            .await
            .unwrap();
        persisted_execution(
            temp.path(),
            "E1",
            Some("R1"),
            Some("T1"),
            Some("TURN1"),
            Some("completed"),
            Some("R1"),
        );
        for (id, turn, status, owner) in [
            (
                "wrong-owner",
                Some("TURN1"),
                Some("completed"),
                Some("OTHER"),
            ),
            ("unmapped", Some("TURN1"), Some("cancelled"), Some("R1")),
            ("no-turn", None, None, None),
        ] {
            persisted_execution(temp.path(), id, Some("R1"), Some("T1"), turn, status, owner);
            assert!(
                recovery::RecoveryScope::same_runtime_for_execution(&store, id, "R1")
                    .await
                    .is_err()
            );
        }
        assert!(
            recovery::RecoveryScope::same_runtime_for_execution(&store, "E1", "OTHER")
                .await
                .is_err()
        );
        assert!(
            recovery::RecoveryScope::after_termination_for_execution(&store, "E1", "R2")
                .await
                .is_err()
        );
        for scenario in ["same", "wrong-thread", "wrong-turn", "conflict", "stale"] {
            let binding = recovery::RecoveryScope::same_runtime_for_execution(&store, "E1", "R1")
                .await
                .unwrap();
            let (client, server) = pair();
            let path = temp.path().to_owned();
            let fake = tokio::spawn(async move {
                let mut s = BufReader::new(server);
                handshake(&mut s).await;
                let req = recv(&mut s).await;
                assert_eq!(req["params"]["threadId"], "T1");
                assert_eq!(req["params"]["includeTurns"], false);
                reply(
                    &mut s,
                    &req,
                    metadata(
                        if scenario == "wrong-thread" {
                            "T2"
                        } else {
                            "T1"
                        },
                        "paginated",
                    ),
                )
                .await;
                if scenario != "wrong-thread" {
                    let req = recv(&mut s).await;
                    assert_eq!(req["params"]["threadId"], "T1");
                    reply(&mut s,&req,json!({"data":[turn_value(if scenario=="wrong-turn"{"TURN2"}else{"TURN1"},if scenario=="conflict"{"failed"}else{"completed"})],"nextCursor":null})).await;
                    if matches!(scenario, "same" | "stale") {
                        let req = recv(&mut s).await;
                        assert_eq!(req["params"]["turnId"], "TURN1");
                        if scenario == "stale" {
                            rusqlite::Connection::open(path.join("agent-state.db"))
                                .unwrap()
                                .execute(
                                    "UPDATE executions SET revision=revision+1 WHERE id='E1'",
                                    [],
                                )
                                .unwrap();
                        }
                        reply(&mut s,&req,json!({"data":[{"turnId":"TURN1","item":final_item()}],"nextCursor":null})).await;
                    }
                }
                let mut line = String::new();
                let _ = s.read_line(&mut line).await;
            });
            client.initialize().await.unwrap();
            let before = store.execution("E1".into()).await.unwrap();
            let result = client.recover_result(binding).await;
            match scenario {
                "same" => {
                    assert_eq!(result.unwrap().terminal_status(), TurnStatus::Completed);
                    assert_eq!(store.execution("E1".into()).await.unwrap(), before);
                }
                "conflict" => {
                    assert_eq!(result.unwrap_err().code, "CODEX_RESULT_EVIDENCE_CONFLICT")
                }
                "stale" => assert_eq!(result.unwrap_err().code, "CODEX_EXECUTION_BINDING_STALE"),
                _ => assert!(result.is_err()),
            };
            drop(client);
            fake.await.unwrap();
        }
    });
}
