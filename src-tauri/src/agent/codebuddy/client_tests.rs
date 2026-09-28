//! 仅内存 fake peer，不启动 CodeBuddy；Host wire 只提供脱敏 fixture 来源。
use super::super::protocol::StderrTail;
use super::*;
use agent_client_protocol::UntypedMessage;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, ReadHalf, WriteHalf};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

/// CB7-002 Host 裁决：typed SDK 与极窄扩展 snapshot 共同完整保留真实目录。
/// 仅 fake peer 回放 session/new response，不发送 prompt 或启动 Provider。
#[tokio::test]
async fn cb7_002_sdk_v1_preserves_host_catalog_with_extensions() {
    use agent_client_protocol::schema::v1::NewSessionRequest;

    let wire = include_str!(
        "../../../../.trellis/tasks/09-26-cb5-003-fresh-session-prompt-activity-contract/evidence/fresh-session.jsonl"
    );
    let host = wire
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .find(|row| row["message"]["result"]["models"].is_object())
        .expect("frozen CB5-003 success response contains a models catalog");
    let result = host["message"]["result"].clone();
    assert!(
        !result["models"]["availableModels"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let (client, mut peer) = pair(Limits::default()).await;
    let requests = client.requests.clone();
    let pending = tokio::spawn(async move {
        requests
            .request(NewSessionRequest::new(r"C:\fixture-workspace"))
            .await
    });
    let request = peer.next().await;
    assert_eq!(request["method"], "session/new");
    assert_eq!(
        request["params"],
        json!({"cwd":r"C:\fixture-workspace", "mcpServers":[]})
    );
    peer.respond(&request, result.clone()).await;
    let typed = pending.await.unwrap().unwrap();
    let captured = serde_json::to_value(typed).unwrap();
    assert_eq!(captured["sessionId"], result["sessionId"]);
    assert_eq!(captured["modes"], result["modes"]);
    assert_eq!(captured["configOptions"], result["configOptions"]);
    // models 仅来自 exact request/session 关联的白名单扩展，不能从 typed config 推导。
    assert!(captured.get("models").is_none());
    assert!(captured.get("_meta").is_none());
    let extensions = client
        .requests
        .shared
        .take_session_new_extensions(captured["sessionId"].as_str().unwrap())
        .unwrap();
    assert_eq!(extensions.models.as_ref(), Some(&result["models"]));
    client.shutdown().await;
}

/// 内存双工 peer，测试始终读取真实 SDK 输出的 id。
struct Peer {
    reader: BufReader<ReadHalf<DuplexStream>>,
    writer: WriteHalf<DuplexStream>,
}

/// 每个请求仍由 SDK 生成关联 id，假 peer 只回显它收到的 id。
fn new_session(
    requests: Requests,
) -> JoinHandle<Result<agent_client_protocol::schema::v1::NewSessionResponse, Failure>> {
    tokio::spawn(async move {
        requests
            .request(agent_client_protocol::schema::v1::NewSessionRequest::new(
                r"C:\fixture",
            ))
            .await
    })
}

#[tokio::test]
/// 精确匹配、单槽上限、缺失身份、shutdown 和 frame bound 均经真实 GuardedRead。
async fn session_new_extensions_fail_closed() {
    for case in [
        "wrong",
        "duplicate",
        "missing",
        "empty",
        "malformed",
        "shutdown",
        "bound",
    ] {
        let (client, mut peer) = pair(Limits {
            frame_bytes: 1024,
            ..Limits::default()
        })
        .await;
        let state = client.requests.shared.clone();
        let first = new_session(client.requests.clone());
        let request = peer.next().await;
        let result = match case {
            "missing" => json!({"models":{}}),
            "empty" => json!({"sessionId":"","models":{}}),
            "malformed" => json!({"sessionId":"s","models":[]}),
            "bound" => json!({"sessionId":"s","models":{"padding":"x".repeat(1500)}}),
            _ => json!({"sessionId":"s","models":{"currentModelId":"m"}}),
        };
        peer.respond(&request, result).await;
        let response = first.await.unwrap();
        match case {
            "missing" | "empty" | "malformed" | "bound" => {
                assert!(matches!(
                    response,
                    Err(Failure::Malformed | Failure::FrameLimit)
                ));
            }
            "wrong" => {
                response.unwrap();
                assert!(matches!(
                    state.take_session_new_extensions("other"),
                    Err(Failure::Malformed)
                ));
                assert_eq!(state.failure(), Some(Failure::Malformed));
            }
            "duplicate" => {
                response.unwrap();
                let second = new_session(client.requests.clone());
                let request = peer.next().await;
                peer.respond(&request, json!({"sessionId":"s2"})).await;
                assert!(matches!(second.await.unwrap(), Err(Failure::Malformed)));
            }
            "shutdown" => {
                response.unwrap();
                state.fail(Failure::Closed);
                assert!(matches!(
                    state.take_session_new_extensions("s"),
                    Err(Failure::Closed)
                ));
            }
            _ => unreachable!(),
        }
        client.shutdown().await;
    }
}

#[tokio::test]
/// 其他 method、未知 id 和重复 response 不能抢占 session/new 扩展槽。
async fn session_new_extensions_use_exact_sdk_id_and_method() {
    let (client, mut peer) = pair(Limits::default()).await;
    let unrelated = call(client.requests.clone(), "fixture/other");
    let other_request = peer.next().await;
    let pending = new_session(client.requests.clone());
    let request = peer.next().await;
    peer.respond(
        &other_request,
        json!({"sessionId":"other","models":{"wrong":true}}),
    )
    .await;
    unrelated.await.unwrap().unwrap();
    peer.send(json!({"jsonrpc":"2.0","id":"unknown","result":{"sessionId":"wrong","models":{}}}))
        .await;
    peer.respond(
        &request,
        json!({"sessionId":"exact","models":{"currentModelId":"m"}}),
    )
    .await;
    pending.await.unwrap().unwrap();
    let capture = client
        .requests
        .shared
        .take_session_new_extensions("exact")
        .unwrap();
    assert_eq!(capture.models, Some(json!({"currentModelId":"m"})));
    peer.respond(&request, json!({"sessionId":"wrong","models":{}}))
        .await;
    let barrier = call(client.requests.clone(), "fixture/barrier");
    let request = peer.next().await;
    peer.respond(&request, json!({})).await;
    barrier.await.unwrap().unwrap();
    assert_eq!(
        client.requests.shared.diagnostics().unmatched_response_id,
        2
    );
    assert!(matches!(
        client.requests.shared.take_session_new_extensions("wrong"),
        Err(Failure::Malformed)
    ));
    client.shutdown().await;
}
impl Peer {
    /// 等待 SDK request/response，测试不会把任意下一帧当成完成结果。
    async fn next(&mut self) -> Value {
        let mut line = String::new();
        tokio::time::timeout(Duration::from_secs(2), self.reader.read_line(&mut line))
            .await
            .unwrap()
            .unwrap();
        serde_json::from_str(&line).unwrap()
    }
    /// 序列化假 wire；不依赖 SDK 内部 pending 实现。
    async fn send(&mut self, value: Value) {
        self.writer
            .write_all(format!("{value}\n").as_bytes())
            .await
            .unwrap();
    }
    /// 用收到的 exact id 回传结果。
    async fn respond(&mut self, request: &Value, result: Value) {
        self.send(json!({"jsonrpc":"2.0","id":request["id"],"result":result}))
            .await;
    }
}

/// 仅建立受限 ByteStreams，不运行任何系统命令或 provider。
async fn pair(limits: Limits) -> (ManagedClient, Peer) {
    let (client, peer) = tokio::io::duplex(4096);
    let (read, write) = tokio::io::split(client);
    let (reader, writer) = tokio::io::split(peer);
    (
        ManagedClient::connect(write.compat_write(), read.compat(), limits)
            .await
            .unwrap(),
        Peer {
            reader: BufReader::new(reader),
            writer,
        },
    )
}

/// 以 fake 请求测试 SDK 多 pending，生产不会调用 session/new。
fn call(requests: Requests, method: &'static str) -> JoinHandle<Result<Value, Failure>> {
    tokio::spawn(async move {
        requests
            .request(UntypedMessage::new(method, json!({})).unwrap())
            .await
    })
}

/// 有界等待后台失败，不以 sleep 猜测 SDK dispatch 完成。
async fn failure(shared: &Shared) -> Failure {
    let mut stopped = shared.stop.subscribe();
    tokio::time::timeout(
        Duration::from_secs(2),
        stopped.wait_for(|value| value.is_some()),
    )
    .await
    .unwrap()
    .unwrap()
    .unwrap()
}

#[tokio::test]
/// 官方 SDK v1 initialize 消费 CB5-002 脱敏结果，非标准扩展不参与 gate。
async fn initialize_sanitized_fixture_and_missing_capability_keep_health_and_capabilities() {
    use crate::agent::{
        codebuddy::{
            discovery::DiscoveryResult, provider::register_codebuddy_provider_with_discovery,
        },
        provider::{
            ProviderId,
            registry::{ProviderHealth, ProviderRegistry},
        },
    };
    for raw in [
        serde_json::from_str::<Value>(
            include_str!("../../../tests/fixtures/codebuddy_initialize.json")
                .trim_start_matches('\u{feff}'),
        )
        .unwrap(),
        json!({"protocolVersion":1}),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let mut registry = ProviderRegistry::new();
        register_codebuddy_provider_with_discovery(
            &mut registry,
            crate::agent::store::StateStore::open(directory.path().into())
                .await
                .unwrap(),
            "test-host".into(),
            Ok(DiscoveryResult::direct_for_test("C:/fixture/codebuddy.exe")),
        )
        .unwrap();
        let id = ProviderId::new("codebuddy".into()).unwrap();
        let before = registry.capabilities(&id).unwrap();
        let (client, mut peer) = pair(Limits::default()).await;
        let requests = client.requests.clone();
        let initialize = tokio::spawn(async move { requests.initialize().await });
        let request = peer.next().await;
        assert_eq!(request["method"], "initialize");
        assert_eq!(request["params"]["protocolVersion"], 1);
        assert!(
            request["params"]["clientCapabilities"]
                .get("elicitation")
                .is_none()
        );
        peer.respond(&request, raw).await;
        assert_eq!(
            initialize.await.unwrap().unwrap().response.protocol_version,
            ProtocolVersion::V1
        );
        assert_eq!(registry.health(&id).unwrap(), ProviderHealth::Available);
        assert_eq!(registry.capabilities(&id).unwrap(), before);
        assert!(!before.can_continue && !before.token_usage);
        assert_eq!(before.can_cancel, cfg!(windows));
        assert_eq!(before.can_recover, cfg!(windows));
        assert_eq!(before.can_execute, cfg!(windows));
        assert_eq!(before.activity, cfg!(windows));
        client.shutdown().await;
    }
}

#[tokio::test]
/// raw 版本门禁独立于 schema 能否反序列化未来版本。
async fn mismatch_is_only_global_health_classification() {
    let (client, mut peer) = pair(Limits::default()).await;
    let requests = client.requests.clone();
    let initialize = tokio::spawn(async move { requests.initialize().await });
    let request = peer.next().await;
    peer.respond(&request, json!({"protocolVersion":999})).await;
    assert!(matches!(
        initialize.await.unwrap(),
        Err(Failure::Incompatible)
    ));
    assert_eq!(Failure::Incompatible.code(), "CODEBUDDY_ACP_INCOMPATIBLE");
    assert_eq!(
        Failure::Incompatible.health_change(),
        Some(crate::agent::provider::registry::ProviderHealth::Unavailable)
    );
    for error in [
        Failure::Eof,
        Failure::Timeout,
        Failure::Io,
        Failure::Launch,
        Failure::Remote,
        Failure::Malformed,
        Failure::InvalidJson,
    ] {
        assert_eq!(error.health_change(), None);
    }
    client.shutdown().await;
}

#[tokio::test]
/// initialize 前/期间 EOF 都是本地失败，没有永久 waiter。
async fn eof_before_and_during_initialize() {
    for during in [false, true] {
        let (client, mut peer) = pair(Limits::default()).await;
        if !during {
            drop(peer);
            assert_eq!(failure(&client.requests.shared).await, Failure::Eof);
            assert!(matches!(
                client.requests.initialize().await,
                Err(Failure::Eof)
            ));
            client.shutdown().await;
            continue;
        }
        let requests = client.requests.clone();
        let initialize = tokio::spawn(async move { requests.initialize().await });
        if during {
            assert_eq!(peer.next().await["method"], "initialize");
        }
        drop(peer);
        assert!(matches!(initialize.await.unwrap(), Err(Failure::Eof)));
        client.shutdown().await;
    }
}

#[tokio::test]
/// 版本字段缺失或非数字属于格式错误，不是 deterministic incompatibility。
async fn malformed_initialize_version_keeps_health_local() {
    for result in [json!({}), json!({"protocolVersion":"2"})] {
        let (client, mut peer) = pair(Limits::default()).await;
        let requests = client.requests.clone();
        let initialize = tokio::spawn(async move { requests.initialize().await });
        let request = peer.next().await;
        peer.respond(&request, result).await;
        assert!(matches!(initialize.await.unwrap(), Err(Failure::Malformed)));
        assert_eq!(Failure::Malformed.health_change(), None);
        client.shutdown().await;
    }
}

#[tokio::test]
/// 临时物理 I/O 错误不被字符串内容误判为 incompatible。
async fn temporary_io_error_is_local() {
    struct BrokenRead;
    impl AsyncRead for BrokenRead {
        /// 注入固定 I/O 失败，不读取真实机器资源。
        fn poll_read(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            _bytes: &mut [u8],
        ) -> Poll<io::Result<usize>> {
            Poll::Ready(Err(io::Error::other("incompatible token=private")))
        }
    }
    let result = ManagedClient::connect(
        futures::io::Cursor::new(Vec::new()),
        BrokenRead,
        Limits::default(),
    )
    .await;
    match result {
        Ok(client) => {
            assert!(matches!(
                client.requests.initialize().await,
                Err(Failure::Io)
            ));
            client.shutdown().await;
        }
        Err(error) => assert_eq!(error, Failure::Io),
    }
    assert_eq!(Failure::Io.health_change(), None);
    assert!(!Failure::Io.code().contains("private"));
}

#[tokio::test]
/// 写侧先观察 peer 关闭也必须稳定为 EOF；其他错误不改类，后续失败不得覆盖首错。
async fn write_and_flush_closed_pipe_keep_first_failure_and_health_local() {
    use futures::io::AsyncWriteExt;
    struct ErrorWriter {
        kind: io::ErrorKind,
        on_flush: bool,
    }
    impl AsyncWrite for ErrorWriter {
        /// 分别注入真实写入和延迟 flush 错误，不触及外部进程。
        fn poll_write(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            bytes: &[u8],
        ) -> Poll<io::Result<usize>> {
            Poll::Ready(if self.on_flush {
                Ok(bytes.len())
            } else {
                Err(io::Error::from(self.kind))
            })
        }
        /// Tokio file 可延迟到 flush 才暴露写入错误，因此单独覆盖此边界。
        fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(if self.on_flush {
                Err(io::Error::from(self.kind))
            } else {
                Ok(())
            })
        }
        /// 测试关闭无需额外资源。
        fn poll_close(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }
    for (kind, expected) in [
        (io::ErrorKind::BrokenPipe, Failure::Eof),
        (io::ErrorKind::Other, Failure::Io),
        (io::ErrorKind::TimedOut, Failure::Io),
    ] {
        for on_flush in [false, true] {
            let shared = Shared::new(Limits::default());
            let (sender, mut receiver) = oneshot::channel();
            let observer = Arc::new(std::sync::Mutex::new(Some(PromptFlush {
                session: "s".into(),
                conversation: "c".into(),
                sender,
            })));
            let mut writer = GuardedWrite::with_prompt_flush(
                ErrorWriter { kind, on_flush },
                shared.clone(),
                observer,
            );
            writer
                .write_all(
                    b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"session/prompt\",\"params\":{\"sessionId\":\"s\",\"_meta\":{\"codebuddy.ai/conversationRequestId\":\"c\"}}}\n",
                )
                .await
                .unwrap();
            assert!(writer.flush().await.is_err());
            assert!(receiver.try_recv().is_err());
            assert_eq!(shared.failure(), Some(expected));
            assert_eq!(shared.fail(Failure::Incompatible), expected);
            assert_eq!(expected.health_change(), None);
            // Cancel 的 observer 同样不能把 enqueue 或完整 write 误认成 inner.flush 成功。
            let shared = Shared::new(Limits::default());
            let (sender, mut receiver) = oneshot::channel();
            let slot = Arc::new(Mutex::new(CancelSlot {
                used: true,
                permit: Some(("s".into(), sender)),
                flushed_at: None,
            }));
            let mut writer = GuardedWrite::with_cancel(
                ErrorWriter { kind, on_flush },
                shared.clone(),
                Arc::new(Mutex::new(None)),
                slot.clone(),
            );
            writer.write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"session/cancel\",\"params\":{\"sessionId\":\"s\"}}\n").await.unwrap();
            assert!(writer.flush().await.is_err());
            assert!(receiver.try_recv().is_err());
            assert!(slot.lock().unwrap().flushed_at.is_none());
            assert!(slot.lock().unwrap().permit.is_none());
            assert_eq!(shared.failure(), Some(expected));
        }
    }
}

#[tokio::test]
/// 输入窗口在 SDK ack 前停止读取下一帧，直接约束其无界 transport channel。
async fn byte_stream_dispatch_window_backpressures() {
    use futures::io::AsyncReadExt;
    let state = Shared::new(Limits::default());
    let wire = b"{\"jsonrpc\":\"2.0\",\"method\":\"session/update\",\"params\":{\"sessionId\":\"s\"}}\n{\"jsonrpc\":\"2.0\",\"method\":\"session/update\",\"params\":{\"sessionId\":\"s\"}}\n";
    let mut reader = GuardedRead::new(futures::io::Cursor::new(wire), state.clone());
    let mut bytes = [0; 256];
    let size = reader.read(&mut bytes).await.unwrap();
    assert_eq!(
        bytes[..size].iter().filter(|byte| **byte == b'\n').count(),
        1
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(20), reader.read(&mut bytes))
            .await
            .is_err()
    );
    state.acknowledge();
    assert_eq!(reader.read(&mut bytes).await.unwrap(), size);
}

#[tokio::test]
/// SDK 提前处理的 control notification 不能卡住输入窗口，也不能收到 response。
async fn unsupported_control_notifications_do_not_block_response_or_eof() {
    let (client, mut peer) = pair(Limits::default()).await;
    let pending = call(client.requests.clone(), "fixture/query");
    let request = peer.next().await;
    for method in ["$/cancel_request", "_proxy/successor", "elicitation/create"] {
        peer.send(json!({"jsonrpc":"2.0","method":method,"params":{}}))
            .await;
    }
    peer.respond(&request, json!({"ok":true})).await;
    assert_eq!(pending.await.unwrap().unwrap(), json!({"ok":true}));
    assert_eq!(client.requests.shared.diagnostics().ignored_notification, 3);
    drop(peer);
    assert_eq!(failure(&client.requests.shared).await, Failure::Eof);
    client.shutdown().await;
}

#[tokio::test]
/// matching-id error 的 schema 超界必须立即 Malformed，不得被 SDK 静默过滤后超时。
async fn sdk_schema_rejected_error_code_fails_before_dispatch() {
    let (client, mut peer) = pair(Limits::default()).await;
    let pending = call(client.requests.clone(), "fixture/query");
    let request = peer.next().await;
    peer.send(json!({"jsonrpc":"2.0","id":request["id"],"error":{"code":2147483648_i64,"message":"private"}})).await;
    assert_eq!(pending.await.unwrap(), Err(Failure::Malformed));
    client.shutdown().await;
}

#[tokio::test]
/// 超时关闭 transport 并同步失败其他 pending，不发送 cancel 产品协议。
async fn initialize_and_request_timeout_close_all_pending() {
    let (client, mut peer) = pair(Limits {
        request_timeout: Duration::from_millis(40),
        ..Limits::default()
    })
    .await;
    let requests = client.requests.clone();
    let initialize = tokio::spawn(async move { requests.initialize().await });
    peer.next().await;
    let second = call(client.requests.clone(), "fixture/query");
    peer.next().await;
    assert!(matches!(initialize.await.unwrap(), Err(Failure::Timeout)));
    assert_eq!(second.await.unwrap(), Err(Failure::Timeout));
    client.shutdown().await;
}

#[tokio::test]
/// JSON 语法和 envelope 歧义均在 SDK 之前稳定终止。
async fn invalid_ndjson_and_malformed_jsonrpc() {
    for (wire, expected) in [
        ("broken\n", Failure::InvalidJson),
        (
            "{\"jsonrpc\":\"1.0\",\"id\":1,\"result\":{}}\n",
            Failure::Malformed,
        ),
        (
            "{\"jsonrpc\":\"2.0\",\"id\":null,\"result\":{}}\n",
            Failure::Malformed,
        ),
        (
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{},\"error\":{}}\n",
            Failure::Malformed,
        ),
    ] {
        let (client, mut peer) = pair(Limits::default()).await;
        let pending = call(client.requests.clone(), "fixture/query");
        peer.next().await;
        peer.writer.write_all(wire.as_bytes()).await.unwrap();
        assert_eq!(pending.await.unwrap(), Err(expected));
        client.shutdown().await;
    }
}

#[tokio::test]
/// out-of-order、unknown、duplicate、插入 notification 都不能错配 pending。
async fn exact_id_out_of_order_unknown_duplicate_and_notification() {
    let (client, mut peer) = pair(Limits::default()).await;
    let one = call(client.requests.clone(), "fixture/one");
    let request_one = peer.next().await;
    let two = call(client.requests.clone(), "fixture/two");
    let request_two = peer.next().await;
    peer.send(json!({"jsonrpc":"2.0","id":"unknown","result":{"wrong":true}}))
        .await;
    peer.send(json!({"jsonrpc":"2.0","method":"fixture/notice","params":{}}))
        .await;
    peer.respond(&request_two, json!({"two":true})).await;
    assert_eq!(two.await.unwrap().unwrap(), json!({"two":true}));
    assert!(!one.is_finished());
    peer.respond(&request_two, json!({"duplicate":true})).await;
    peer.respond(&request_one, json!({"one":true})).await;
    assert_eq!(one.await.unwrap().unwrap(), json!({"one":true}));
    let diagnostics = client.requests.shared.diagnostics();
    assert_eq!(diagnostics.unmatched_response_id, 2);
    assert_eq!(diagnostics.ignored_notification, 1);
    client.shutdown().await;
}

#[tokio::test]
/// Host exact launcher 顺序：early config update 在 synthetic session/new response 之前。
async fn early_config_update_replays_in_order_and_isolates_wrong_session() {
    let (client, mut peer) = pair(Limits::default()).await;
    let pending = call(client.requests.clone(), "session/new");
    let request = peer.next().await;
    let mut update: Value = serde_json::from_str(
        include_str!("../../../tests/fixtures/codebuddy_early_update.jsonl")
            .trim_start_matches('\u{feff}'),
    )
    .unwrap();
    peer.send(update.clone()).await;
    update["params"]["update"]["sequence"] = json!(2);
    peer.send(update.clone()).await;
    update["params"]["sessionId"] = json!("wrong-session");
    peer.send(update).await;
    peer.respond(&request, json!({"sessionId":"fixture-session"}))
        .await;
    assert_eq!(
        pending.await.unwrap().unwrap()["sessionId"],
        "fixture-session"
    );
    let state = &client.requests.shared;
    assert!(state.take_session("fixture-session").unwrap().is_empty());
    state.register_route("fixture-session").unwrap();
    let frames = state.take_session("fixture-session").unwrap();
    assert_eq!(frames.len(), 2);
    assert_eq!(frames[0].method, "session/update");
    assert_eq!(
        frames[0].params["update"]["sessionUpdate"],
        "config_option_update"
    );
    assert_eq!(frames[1].params["update"]["sequence"], 2);
    state.register_route("wrong-session").unwrap();
    assert_eq!(state.take_session("wrong-session").unwrap().len(), 1);
    let state = state.clone();
    client.shutdown().await;
    assert_eq!(
        state.take_session("fixture-session").err(),
        Some(Failure::Closed)
    );
}

#[tokio::test]
/// count/bytes/frame 三种预算分别触发固定诊断，pending 明确失败。
async fn queue_count_bytes_and_single_frame_bounds() {
    for (limits, expected) in [
        (
            Limits {
                queue_count: 0,
                ..Limits::default()
            },
            Failure::QueueCount,
        ),
        (
            Limits {
                queue_bytes: 1,
                ..Limits::default()
            },
            Failure::QueueBytes,
        ),
        (
            Limits {
                frame_bytes: 256,
                ..Limits::default()
            },
            Failure::FrameLimit,
        ),
    ] {
        let (client, mut peer) = pair(limits).await;
        let pending = call(client.requests.clone(), "fixture/query");
        peer.next().await;
        peer.send(json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s","padding":"x".repeat(300)}})).await;
        assert_eq!(pending.await.unwrap(), Err(expected));
        client.shutdown().await;
    }
}

#[tokio::test]
/// 每帧都合法时，累积 count/bytes 仍会明确失败，不只验证单帧超限。
async fn accumulated_queue_limits_and_replay_reclaims_budget() {
    let params = json!({"sessionId":"s","update":{"sessionUpdate":"config_option_update","configOptions":[]}});
    let frame_bytes = serde_json::to_vec(&params).unwrap().len() + "session/update".len();
    for (limits, expected) in [
        (
            Limits {
                queue_count: 1,
                ..Limits::default()
            },
            Failure::QueueCount,
        ),
        (
            Limits {
                queue_bytes: frame_bytes * 2 - 1,
                ..Limits::default()
            },
            Failure::QueueBytes,
        ),
    ] {
        let (client, mut peer) = pair(limits).await;
        let pending = call(client.requests.clone(), "fixture/query");
        peer.next().await;
        for _ in 0..2 {
            peer.send(json!({"jsonrpc":"2.0","method":"session/update","params":params}))
                .await;
        }
        assert_eq!(pending.await.unwrap(), Err(expected));
        client.shutdown().await;
    }
    let (client, mut peer) = pair(Limits {
        queue_count: 1,
        queue_bytes: frame_bytes,
        ..Limits::default()
    })
    .await;
    client.requests.shared.register_route("s").unwrap();
    for _ in 0..3 {
        let barrier = call(client.requests.clone(), "fixture/barrier");
        let request = peer.next().await;
        peer.send(json!({"jsonrpc":"2.0","method":"session/update","params":params}))
            .await;
        peer.respond(&request, json!({})).await;
        barrier.await.unwrap().unwrap();
        assert_eq!(client.requests.shared.take_session("s").unwrap().len(), 1);
    }
    client.shutdown().await;
}

#[tokio::test]
/// 发送端 frame budget 在 SDK enqueue 前拒绝过大参数，正常请求仍可继续。
async fn outgoing_frame_limit_does_not_enqueue() {
    let (client, mut peer) = pair(Limits {
        frame_bytes: 512,
        ..Limits::default()
    })
    .await;
    let oversized = UntypedMessage::new("fixture/query", json!({"data":"x".repeat(1024)})).unwrap();
    assert_eq!(
        client.requests.request(oversized).await,
        Err(Failure::FrameLimit)
    );
    let pending = call(client.requests.clone(), "fixture/query");
    let request = peer.next().await;
    peer.respond(&request, json!({"ok":true})).await;
    assert_eq!(pending.await.unwrap().unwrap(), json!({"ok":true}));
    client.shutdown().await;
}

#[tokio::test]
/// TTL 在空闲连接也自动收敛，不需要下一帧到来才清理。
async fn idle_early_queue_ttl_and_route_count() {
    let (client, mut peer) = pair(Limits {
        queue_ttl: Duration::from_millis(20),
        routes: 1,
        ..Limits::default()
    })
    .await;
    client.requests.shared.register_route("first").unwrap();
    assert_eq!(
        client.requests.shared.register_route("second"),
        Err(Failure::RouteLimit)
    );
    peer.send(json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"early"}}))
        .await;
    assert_eq!(
        failure(&client.requests.shared).await,
        Failure::QueueExpired
    );
    client.shutdown().await;
}

#[tokio::test]
/// pending 上限在 SDK enqueue 前生效，拒绝请求不会误完成其他 waiter。
async fn pending_count_and_shutdown_fail_all_waiters() {
    let (client, mut peer) = pair(Limits {
        pending: 2,
        ..Limits::default()
    })
    .await;
    let one = call(client.requests.clone(), "fixture/one");
    peer.next().await;
    let two = call(client.requests.clone(), "fixture/two");
    peer.next().await;
    let three = call(client.requests.clone(), "fixture/three");
    assert_eq!(three.await.unwrap(), Err(Failure::PendingLimit));
    client.shutdown().await;
    assert_eq!(one.await.unwrap(), Err(Failure::Closed));
    assert_eq!(two.await.unwrap(), Err(Failure::Closed));
}

#[tokio::test]
/// request with id 返回 -32601，notification 无论是否未知都不回复。
async fn server_request_permission_and_notification_baseline() {
    let (client, mut peer) = pair(Limits::default()).await;
    peer.send(json!({"jsonrpc":"2.0","method":"unknown","id":42,"params":{}}))
        .await;
    let error = peer.next().await;
    assert_eq!(error["id"], 42);
    assert_eq!(error["error"]["code"], -32601);
    peer.send(json!({"jsonrpc":"2.0","method":"session/request_permission","id":"permission","params":{}})).await;
    let denied = peer.next().await;
    assert_eq!(denied["id"], "permission");
    assert_eq!(denied["result"]["outcome"]["outcome"], "cancelled");
    peer.send(json!({"jsonrpc":"2.0","method":"unknown","params":{}}))
        .await;
    let mut line = String::new();
    assert!(
        tokio::time::timeout(Duration::from_millis(30), peer.reader.read_line(&mut line))
            .await
            .is_err()
    );
    client.shutdown().await;
}

#[test]
/// stderr 持续滚动但存储始终有界，UTF-8 截断也不转换成公开文本。
fn bounded_stderr_tail() {
    let mut tail = StderrTail::new(5);
    tail.push(b"0123456789");
    assert_eq!(tail.bytes(), b"56789");
    tail.push(b"ab");
    assert_eq!(tail.bytes(), b"789ab");
    let mut zero = StderrTail::new(0);
    zero.push(b"private");
    assert!(zero.bytes().is_empty());
}

#[tokio::test]
/// 外部取消请求 future 也不能遗留 SDK pending。
async fn dropped_request_closes_transport() {
    let (client, mut peer) = pair(Limits::default()).await;
    let pending = call(client.requests.clone(), "fixture/query");
    peer.next().await;
    pending.abort();
    let _ = pending.await;
    assert_eq!(failure(&client.requests.shared).await, Failure::Closed);
    client.shutdown().await;
}

/// CB7-005：只在 exact SDK frame 全部写入并成功 flush 后发送相同 id。
#[tokio::test]
async fn prompt_flush_observation_is_exact_and_physical() {
    use futures::io::AsyncWriteExt;
    let shared = Shared::new(Limits::default());
    let (sender, mut receiver) = oneshot::channel();
    let observer = Arc::new(std::sync::Mutex::new(Some(PromptFlush {
        session: "s".into(),
        conversation: "c".into(),
        sender,
    })));
    let mut writer =
        GuardedWrite::with_prompt_flush(futures::io::Cursor::new(Vec::new()), shared, observer);
    let frame = b"{\"jsonrpc\":\"2.0\",\"id\":57,\"method\":\"session/prompt\",\"params\":{\"sessionId\":\"s\",\"_meta\":{\"codebuddy.ai/conversationRequestId\":\"c\"}}}\n";
    writer.write_all(frame).await.unwrap();
    assert!(matches!(
        receiver.try_recv(),
        Err(oneshot::error::TryRecvError::Empty)
    ));
    assert!(writer.inner.get_ref().is_empty());
    writer.flush().await.unwrap();
    assert_eq!(receiver.await.unwrap(), json!(57));
    assert_eq!(writer.inner.get_ref(), frame);
    // 相同 method 但外部身份不符不得写入、不得通知 flush。
    let shared = Shared::new(Limits::default());
    let (sender, mut receiver) = oneshot::channel();
    let observer = Arc::new(std::sync::Mutex::new(Some(PromptFlush {
        session: "wrong".into(),
        conversation: "c".into(),
        sender,
    })));
    let mut writer =
        GuardedWrite::with_prompt_flush(futures::io::Cursor::new(Vec::new()), shared, observer);
    assert!(writer.write_all(frame).await.is_err());
    assert!(writer.inner.get_ref().is_empty());
    assert!(receiver.try_recv().is_err());
}

#[tokio::test]
/// 正式 SDK 生成的 Prompt id 与 observer 完全一致，flush 不能完成 response waiter。
async fn sdk_prompt_flush_id_matches_wire_without_completing_waiter() {
    use agent_client_protocol::schema::v1::PromptRequest;
    let (client, mut peer) = pair(Limits::default()).await;
    let requests = client.requests.clone();
    let flushed = requests
        .observe_prompt_flush("s".into(), "c".into())
        .unwrap();
    assert!(
        requests
            .observe_prompt_flush("s".into(), "c".into())
            .is_err()
    );
    let task = tokio::spawn(async move {
        requests
            .request(
                PromptRequest::new("s", vec![]).meta(serde_json::Map::from_iter([(
                    "codebuddy.ai/conversationRequestId".into(),
                    json!("c"),
                )])),
            )
            .await
    });
    let wire = peer.next().await;
    assert_eq!(wire["method"], "session/prompt");
    assert_eq!(flushed.await.unwrap(), wire["id"]);
    assert!(!task.is_finished());
    peer.respond(&wire, json!({"stopReason":"end_turn"})).await;
    assert!(task.await.unwrap().is_ok());
    client.shutdown().await;
}

/// CB8：官方 SDK 只发 exact sessionId，一次 flush 后拒绝再次授权。
#[tokio::test]
async fn cancel_sdk_exact_wire_once() {
    let (client, mut peer) = pair(Limits::default()).await;
    let requests = client.requests.clone();
    let task = tokio::spawn(async move { requests.cancel_session("exact".into()).await });
    assert_eq!(
        peer.next().await,
        json!({"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":"exact"}})
    );
    task.await.unwrap().unwrap();
    assert_eq!(
        client.requests.cancel_session("exact".into()).await,
        Err(Failure::Closed)
    );
    let slot = client.requests.cancel.clone();
    client.shutdown().await;
    assert!(slot.lock().unwrap().permit.is_none());
}

/// CB8：permit 不能授权错误会话、其它 notification 或附加私有字段。
#[tokio::test]
async fn cancel_guard_permit_and_physical_flush() {
    use futures::io::AsyncWriteExt;
    for (method, params, permitted, succeeds) in [
        ("session/cancel", json!({"sessionId":"s"}), true, true),
        ("session/cancel", json!({"sessionId":"wrong"}), true, false),
        ("session/cancel", json!({"sessionId":"s"}), false, false),
        ("other", json!({"sessionId":"s"}), true, false),
        (
            "session/cancel",
            json!({"sessionId":"s","conversationId":"private"}),
            true,
            false,
        ),
    ] {
        let shared = Shared::new(Limits::default());
        let (sender, mut receiver) = oneshot::channel();
        let slot = Arc::new(Mutex::new(CancelSlot {
            used: permitted,
            permit: if permitted {
                Some(("s".into(), sender))
            } else {
                None
            },
            flushed_at: None,
        }));
        let mut writer = GuardedWrite::with_cancel(
            futures::io::Cursor::new(Vec::new()),
            shared,
            Arc::new(Mutex::new(None)),
            slot,
        );
        let frame = format!(
            "{}\n",
            json!({"jsonrpc":"2.0","method":method,"params":params})
        );
        assert_eq!(writer.write_all(frame.as_bytes()).await.is_ok(), succeeds);
        assert!(writer.inner.get_ref().is_empty());
        assert!(receiver.try_recv().is_err());
        if succeeds {
            writer.flush().await.unwrap();
            receiver.await.unwrap();
            assert_eq!(writer.inner.get_ref(), frame.as_bytes());
            assert!(writer.write_all(frame.as_bytes()).await.is_err());
        }
    }
}

/// CB8：阻塞真实写入有固定上限，失败后单槽 authority 被清理且不重试。
#[tokio::test]
async fn cancel_blocked_pipe_times_out_and_clears_permit() {
    let (outgoing, _unread) = tokio::io::duplex(1);
    let (_held, incoming) = tokio::io::duplex(1);
    let client = ManagedClient::connect(
        outgoing.compat_write(),
        incoming.compat(),
        Limits {
            request_timeout: Duration::from_millis(30),
            ..Limits::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(
        client.requests.cancel_session("s".into()).await,
        Err(Failure::Timeout)
    );
    assert!(client.requests.cancel.lock().unwrap().permit.is_none());
    assert!(client.requests.cancel_session("s".into()).await.is_err());
    client.shutdown().await;
}

/// CB8：response 已过 exact read guard、owner 尚未被调度时，已 enqueue cancel 也不能写 pipe。
#[tokio::test]
async fn cancel_guard_suppresses_wire_after_exact_prompt_response() {
    use futures::io::AsyncWriteExt;
    let shared = Shared::new(Limits::default());
    shared
        .outgoing(&json!({"jsonrpc":"2.0","id":7,"method":"session/prompt","params":{}}))
        .unwrap();
    let (sender, receiver) = oneshot::channel();
    let slot = Arc::new(Mutex::new(CancelSlot {
        used: true,
        permit: Some(("s".into(), sender)),
        flushed_at: None,
    }));
    let mut writer = GuardedWrite::with_cancel(
        futures::io::Cursor::new(Vec::new()),
        shared.clone(),
        Arc::new(Mutex::new(None)),
        slot,
    );
    writer.write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"session/cancel\",\"params\":{\"sessionId\":\"s\"}}\n").await.unwrap();
    assert!(
        shared
            .incoming(&json!({"jsonrpc":"2.0","id":7,"result":{"stopReason":"end_turn"}}))
            .unwrap()
    );
    writer.flush().await.unwrap();
    assert!(writer.inner.get_ref().is_empty());
    assert!(receiver.await.is_err());
    assert!(shared.failure().is_none());
}
