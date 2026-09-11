use super::super::tests::{handshake, pair, recv, reply as rpc_reply};
use super::*;
use tokio::io::BufReader;

#[test]
fn pinned_title_spec_is_one_namespaced_function() {
    let specs = tools();
    let spec = &specs[0];
    assert_eq!(specs.as_array().unwrap().len(), 1);
    assert_eq!(spec.as_object().unwrap().len(), 4);
    assert_eq!(spec["type"], "namespace");
    assert_eq!(spec["name"], "codex_app");
    assert!(spec["description"].is_string());
    assert_eq!(spec["tools"].as_array().unwrap().len(), 1);
    let tool = &spec["tools"][0];
    assert_eq!(tool.as_object().unwrap().len(), 4);
    assert_eq!(tool["type"], "function");
    assert_eq!(tool["name"], "set_thread_title");
    assert!(tool["description"].is_string());
    assert_eq!(
        tool["inputSchema"],
        json!({"type":"object","properties":{"title":{"type":"string","minLength":1,"maxLength":200}},"required":["title"],"additionalProperties":false})
    );
}

#[test]
fn only_execution_clients_advertise_title_tool() {
    super::super::tests::run(async {
        for enabled in [false, true] {
            let (client, server) = pair();
            let temp = tempfile::tempdir().unwrap();
            if enabled {
                client
                    .enable_root_title(StateStore::open(temp.path().into()).await.unwrap(), "E1")
                    .unwrap();
            }
            let fake = tokio::spawn(async move {
                let mut s = BufReader::new(server);
                handshake(&mut s).await;
                let req = recv(&mut s).await;
                assert_eq!(req["method"], "thread/start");
                if enabled {
                    assert_eq!(req["params"]["dynamicTools"], tools());
                } else {
                    assert!(req["params"].get("dynamicTools").is_none());
                }
                rpc_reply(
                    &mut s,
                    &req,
                    json!({"thread":{"id":"T1","turns":[],"historyMode":"paginated"}}),
                )
                .await;
                s
            });
            client.initialize().await.unwrap();
            client
                .thread_start("fixture", crate::agent::execution::ExecutionMode::ReadOnly)
                .await
                .unwrap();
            let _server = fake.await.unwrap();
        }
    });
}

#[test]
fn title_timeout_and_late_ack_are_tool_local_and_bounded() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            tokio::time::pause();
            let (client, server) = pair();
            let mut s = BufReader::new(server);
            let operation = rename(&client.shared, "ROOT".into(), "Title");
            tokio::pin!(operation);
            let request = tokio::select! {
                result = &mut operation => panic!("early completion: {result:?}"),
                request = recv(&mut s) => request,
            };
            tokio::time::advance(RPC_TIMEOUT + Duration::from_secs(1)).await;
            assert!(operation.await.unwrap_err().contains("outcome unknown"));
            assert!(client.shared.check().is_ok());
            assert_eq!(client.shared.pending.lock().unwrap().len(), 1);
            assert!(
                rename(&client.shared, "ROOT".into(), "Again")
                    .await
                    .is_err()
            );
            rpc_reply(&mut s, &request, json!({})).await;
            tokio::task::yield_now().await;
            assert!(client.shared.pending.lock().unwrap().is_empty());
            assert!(client.shared.check().is_ok());
        });
}
