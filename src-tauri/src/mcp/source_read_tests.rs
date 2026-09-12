use super::*;
use axum::{Json, Router, extract::State, response::IntoResponse, routing::post};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

#[derive(Clone)]
struct Fake {
    root: PathBuf,
    requests: Arc<Mutex<Vec<Value>>>,
    change: Arc<AtomicBool>,
    fail: Arc<AtomicBool>,
}

async fn rpc(State(fake): State<Fake>, Json(request): Json<Value>) -> axum::response::Response {
    if request.get("id").is_none() {
        return axum::http::StatusCode::ACCEPTED.into_response();
    }
    let result = match request["method"].as_str().unwrap() {
        "initialize" => json!({"protocolVersion":request["params"]["protocolVersion"],
            "capabilities":{"tools":{}},"serverInfo":{"name":"source-fixture","version":"1"}}),
        "tools/list" => {
            let mut tools: Vec<_> = super::super::registry::SOURCES
                .iter()
                .map(|(_, name, fields, _)| {
                    let mut properties = serde_json::Map::new();
                    for field in *fields {
                        properties.insert((*field).into(), json!({}));
                    }
                    properties.insert("max_answer_chars".into(), json!({}));
                    json!({"name":name,"inputSchema":{"type":"object","properties":properties}})
                })
                .collect();
            tools.push(json!({"name":"activate_project","inputSchema":{"type":"object","properties":{"project":{}}}}));
            tools.push(json!({"name":"get_current_config","inputSchema":{"type":"object"}}));
            json!({"tools":tools})
        }
        "tools/call" => {
            assert_eq!(request["params"]["name"], "read_file");
            let args = &request["params"]["arguments"];
            fake.requests.lock().unwrap().push(args.clone());
            if fake.fail.load(Ordering::SeqCst) {
                json!({"content":[{"type":"text","text":"fixture failure"}],"isError":true})
            } else {
                let path = fake.root.join(args["relative_path"].as_str().unwrap());
                let raw = std::fs::read_to_string(&path).unwrap();
                // Deliberately not the raw bytes: a downstream text view with CRLF normalized.
                let start = args["start_line"].as_u64().unwrap_or(0) as usize;
                let end = args["end_line"].as_u64().map(|n| n as usize);
                let text = raw
                    .lines()
                    .enumerate()
                    .filter(|(i, _)| *i >= start && end.is_none_or(|end| *i <= end))
                    .map(|(_, line)| line)
                    .collect::<Vec<_>>()
                    .join("\n");
                if fake.change.swap(false, Ordering::SeqCst) {
                    let changed = raw.replace("abc", "xyz");
                    std::fs::write(path, changed).unwrap();
                }
                json!({"content":[{"type":"text","text":text}],"isError":false})
            }
        }
        method => panic!("unexpected method {method}"),
    };
    Json(json!({"jsonrpc":"2.0","id":request["id"],"result":result})).into_response()
}

async fn fixture(root: PathBuf) -> (Workspace, serena::Client, Fake, tokio::task::JoinHandle<()>) {
    let workspace = Workspace {
        id: "W".into(),
        name: "Fixture".into(),
        root: root.canonicalize().unwrap(),
    };
    let fake = Fake {
        root: workspace.root.clone(),
        requests: Arc::new(Mutex::new(Vec::new())),
        change: Arc::new(AtomicBool::new(false)),
        fail: Arc::new(AtomicBool::new(false)),
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let app = Router::new()
        .route("/mcp", post(rpc))
        .with_state(fake.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let client = serena::Client::connect(port).await.unwrap();
    (workspace, client, fake, server)
}

#[tokio::test]
async fn source_read_file_returns_full_raw_sha_and_relative_path_across_ranges_and_budgets() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("nested")).unwrap();
    let bytes = b"\xef\xbb\xbfabc\r\nsecond\r\n";
    std::fs::write(dir.path().join("nested/file.txt"), bytes).unwrap();
    let (workspace, client, fake, server) = fixture(dir.path().into()).await;
    let mut hashes = Vec::new();
    for (start, end, budget, expected) in [
        (None, None, 32768, "\u{feff}abc\nsecond"),
        (Some(0), Some(0), 6, "\u{feff}abc"),
        (Some(1), Some(1), 6, "second"),
    ] {
        let mut args = json!({"relative_path":"nested/./file.txt","max_answer_chars":budget});
        if let Some(start) = start {
            args["start_line"] = json!(start);
        }
        if let Some(end) = end {
            args["end_line"] = json!(end);
        }
        let output = read(
            &workspace,
            &client,
            args.clone(),
            budget,
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(fake.requests.lock().unwrap().last(), Some(&args));
        assert_eq!(output["text"], expected);
        assert_eq!(output["truncated"], false);
        assert!(output.get("content").is_none());
        assert!(output.get("hint").is_none());
        assert_eq!(
            output["workspace"],
            serde_json::to_value(&workspace).unwrap()
        );
        assert_eq!(output["path"], "nested/file.txt");
        assert!(!std::path::Path::new(output["path"].as_str().unwrap()).is_absolute());
        hashes.push(output["sha256"].as_str().unwrap().to_owned());
    }
    assert!(
        hashes
            .iter()
            .all(|hash| hash == "98c468325cef7f63ade1c10cab22b29983bf78710ce6f50b9fdea26322c8e19e")
    );
    std::fs::write(dir.path().join("nested/file.txt"), b"changed\r\n").unwrap();
    let output = read(
        &workspace,
        &client,
        json!({"relative_path":"nested/file.txt"}),
        32768,
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_ne!(output["sha256"], hashes[0]);
    server.abort();
}

#[tokio::test]
async fn source_read_file_hashes_multiple_chunks_and_non_utf8_raw_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let bytes: Vec<_> = (0..1025).flat_map(|_| 0..=255u8).collect();
    std::fs::write(dir.path().join("binary"), &bytes).unwrap();
    let version = capture(
        dir.path().canonicalize().unwrap(),
        "binary".into(),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(
        version.sha256,
        "85e1298a87a2077b5de87c6ea60e77be9ceba06f955c51b73676ee5a71f1187f"
    );
    let bytes = [b"abc\r\n".as_slice(), &vec![b'x'; 150000], b"\r\n"].concat();
    std::fs::write(dir.path().join("large.txt"), bytes).unwrap();
    let (workspace, client, _, server) = fixture(dir.path().into()).await;
    let output = read(
        &workspace,
        &client,
        json!({"relative_path":"large.txt","start_line":0,"end_line":0,"max_answer_chars":3}),
        3,
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(output["text"], "abc");
    assert_eq!(
        output["sha256"],
        "f65d76e858e048add6846a32d413defd224678a3ed59661247ddf70e643822ad"
    );
    server.abort();
}

#[tokio::test]
async fn source_read_file_rejects_changed_version_and_preserves_existing_errors() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("file.txt"), b"abc\r\n").unwrap();
    let (workspace, client, fake, server) = fixture(dir.path().into()).await;
    for path in ["../outside", "missing.txt", "C:/outside"] {
        let error = read(
            &workspace,
            &client,
            json!({"relative_path":path}),
            10,
            CancellationToken::new(),
        )
        .await
        .unwrap_err();
        assert!(error.starts_with("INVALID_PATH"), "{error}");
    }
    assert!(fake.requests.lock().unwrap().is_empty());
    fake.change.store(true, Ordering::SeqCst);
    assert_eq!(
        read(
            &workspace,
            &client,
            json!({"relative_path":"file.txt"}),
            10,
            CancellationToken::new()
        )
        .await
        .unwrap_err(),
        "SOURCE_READ_CHANGED"
    );
    assert!(
        read(
            &workspace,
            &client,
            json!({"relative_path":"file.txt"}),
            1,
            CancellationToken::new()
        )
        .await
        .unwrap_err()
        .starts_with("OUTPUT_LIMIT_EXCEEDED")
    );
    fake.fail.store(true, Ordering::SeqCst);
    assert!(
        read(
            &workspace,
            &client,
            json!({"relative_path":"file.txt"}),
            10,
            CancellationToken::new()
        )
        .await
        .unwrap_err()
        .starts_with("BACKEND_ERROR")
    );
    let calls = fake.requests.lock().unwrap().len();
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert_eq!(
        read(
            &workspace,
            &client,
            json!({"relative_path":"file.txt"}),
            10,
            cancel
        )
        .await
        .unwrap_err(),
        "CANCELLED"
    );
    assert_eq!(fake.requests.lock().unwrap().len(), calls);
    server.abort();
}

#[cfg(windows)]
#[tokio::test]
async fn source_read_file_rejects_junction_escape_before_upstream_call() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    let outside = dir.path().join("outside");
    std::fs::create_dir(&root).unwrap();
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(outside.join("file.txt"), b"secret").unwrap();
    let output = std::process::Command::new("cmd.exe")
        .args(["/c", "mklink", "/J"])
        .arg(root.join("link"))
        .arg(&outside)
        .output()
        .unwrap();
    assert!(output.status.success());
    let (workspace, client, fake, server) = fixture(root).await;
    assert!(
        read(
            &workspace,
            &client,
            json!({"relative_path":"link/file.txt"}),
            10,
            CancellationToken::new()
        )
        .await
        .unwrap_err()
        .starts_with("INVALID_PATH")
    );
    assert!(fake.requests.lock().unwrap().is_empty());
    server.abort();
}
