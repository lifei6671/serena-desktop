use super::*;
use crate::{
    config::{self, AppPaths, BrokerConfig, ManagerConfig},
    mcp::Broker,
    serena::{ServerStatus, SupervisorState},
    workspace_registry::WorkspaceRegistry,
    workspace_resolver::WorkspaceLease,
};
use rmcp::{ServiceExt, model::CallToolRequestParams, transport::StreamableHttpClientTransport};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::Path,
    sync::{Arc, mpsc},
    time::Duration,
};

/// 创建只含请求 Lease 的 fixture，避免测试路径依赖任何全局选择或 Serena 状态。
fn lease(root: &Path, id: &str, generation: u64) -> WorkspaceLease {
    WorkspaceLease {
        workspace_id: id.into(),
        canonical_root: root.canonicalize().unwrap(),
        generation,
    }
}

/// 构造本 Tool 的最小输入，公共 `relative_path` 名称保持冻结契约。
fn arguments(relative_path: &str) -> SourceArgs {
    SourceArgs {
        relative_path: Some(relative_path.into()),
        ..Default::default()
    }
}

/// 通过公开本地读取入口调用，确保测试覆盖 cancellation 与输出 DTO。
async fn read_at(lease: &WorkspaceLease, arguments: SourceArgs) -> Result<Value, String> {
    read(lease, arguments, CancellationToken::new()).await
}

/// Consistency 失败不得返回已读正文；所有路径或版本漂移都投影为同一公开错误。
fn assert_source_read_changed(result: Result<ReadResult, String>) {
    match result {
        Err(error) => assert_eq!(error, "SOURCE_READ_CHANGED"),
        Ok(_) => panic!("changed source version must not produce a result"),
    }
}

/// 生成互不冲突的本地监听端口，使 MCP Gate 不依赖固定端口或已有 Serena 进程。
fn p2b_007_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// P2B-007：缺失 Serena 时，四个基础 Source 仍须经真实 MCP 并发地只使用各自请求 Lease。
#[tokio::test]
async fn p2b_007_source_gate_mcp_ab_lease_isolation_without_serena_or_legacy_lock() {
    let directory = tempfile::tempdir().unwrap();
    let paths = AppPaths {
        runtime_directory: directory.path().join("runtime"),
        config_file: directory.path().join("config.json"),
        log_directory: directory.path().join("logs"),
        app_log: directory.path().join("logs/app.log"),
        serena_log: directory.path().join("logs/serena.log"),
    };
    let config = ManagerConfig {
        // 显式不存在的可执行文件固定 Serena unavailable 前提，不允许测试意外启动本机 Serena。
        serena_path: Some(directory.path().join("missing-serena.exe")),
        port: p2b_007_port(),
        broker: BrokerConfig {
            enabled: false,
            port: p2b_007_port(),
            allow_lan: false,
        },
        dashboard_enabled: false,
        auto_start_server: false,
        ..Default::default()
    };
    config::save(&paths.config_file, &config).unwrap();
    let broker = Arc::new(Broker::new(Arc::new(SupervisorState::new(paths).unwrap())));
    let root_a = directory.path().join("workspace-a");
    let root_b = directory.path().join("workspace-b");

    // 两个 Workspace 保留同名相对路径，同时用独有文件与正文使所有 Tool 都可识别越权读取。
    for (root, marker) in [(&root_a, "A"), (&root_b, "B")] {
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/shared.rs"), format!("SOURCE-{marker}\n")).unwrap();
        fs::write(root.join(format!("src/only-{marker}.rs")), marker).unwrap();
    }
    let registry = WorkspaceRegistry::new(&broker.supervisor);
    let workspace_a = registry
        .register(root_a, Some("Workspace A".into()))
        .unwrap();
    let workspace_b = registry
        .register(root_b, Some("Workspace B".into()))
        .unwrap();
    // Desktop 选择故意指向 B；任何 A 请求仍只能由其 workspaceId 解析 Lease。
    broker
        .supervisor
        .select_desktop_workspace(&workspace_b.id)
        .unwrap();
    assert_ne!(
        broker.supervisor.snapshot().server_status,
        ServerStatus::Running
    );

    broker.start().await.unwrap();
    let uri = format!("http://127.0.0.1:{}/mcp", broker.snapshot().await.port);
    let client_a = ().serve(StreamableHttpClientTransport::from_uri(uri.clone())).await.unwrap();
    let client_b = ().serve(StreamableHttpClientTransport::from_uri(uri)).await.unwrap();

    // 持有 legacy active write lock 时，四个本地 Source 仍须在短时间内结束，不能读取或长持有该锁。
    let legacy_active_lock = broker.workspace.write().await;
    for (name, arguments) in [
        ("source_read_file", json!({"relative_path":"src/shared.rs"})),
        ("source_list_dir", json!({"relative_path":"src"})),
        ("source_find_file", json!({"file_mask":"only-*.rs"})),
        (
            "source_search_pattern",
            json!({"substring_pattern":"^SOURCE-(A|B)$"}),
        ),
    ] {
        let mut a_arguments = arguments.as_object().unwrap().clone();
        a_arguments.insert("workspaceId".into(), json!(workspace_a.id));
        let mut b_arguments = arguments.as_object().unwrap().clone();
        b_arguments.insert("workspaceId".into(), json!(workspace_b.id));
        let (a, b) = tokio::time::timeout(Duration::from_secs(3), async {
            tokio::join!(
                client_a.call_tool(CallToolRequestParams::new(name).with_arguments(a_arguments),),
                client_b.call_tool(CallToolRequestParams::new(name).with_arguments(b_arguments),)
            )
        })
        .await
        .expect("basic Source request must not wait on legacy active lock");
        let a = a.unwrap();
        let b = b.unwrap();
        assert_ne!(a.is_error, Some(true), "{name}: {a:?}");
        assert_ne!(b.is_error, Some(true), "{name}: {b:?}");
        let a = a.structured_content.unwrap();
        let b = b.structured_content.unwrap();
        assert_eq!(
            a["workspace"],
            json!({"id":workspace_a.id,"generation":workspace_a.generation}),
            "{name} A provenance"
        );
        assert_eq!(
            b["workspace"],
            json!({"id":workspace_b.id,"generation":workspace_b.generation}),
            "{name} B provenance"
        );
        let a_text = a["text"].as_str().unwrap();
        let b_text = b["text"].as_str().unwrap();
        match name {
            "source_read_file" => {
                assert_eq!(a_text, "SOURCE-A");
                assert_eq!(b_text, "SOURCE-B");
            }
            "source_list_dir" | "source_find_file" => {
                assert!(a_text.contains("only-A.rs"), "{name}: {a_text}");
                assert!(!a_text.contains("only-B.rs"), "{name}: {a_text}");
                assert!(b_text.contains("only-B.rs"), "{name}: {b_text}");
                assert!(!b_text.contains("only-A.rs"), "{name}: {b_text}");
            }
            "source_search_pattern" => {
                assert!(a_text.contains("SOURCE-A"), "{a_text}");
                assert!(!a_text.contains("SOURCE-B"), "{a_text}");
                assert!(b_text.contains("SOURCE-B"), "{b_text}");
                assert!(!b_text.contains("SOURCE-A"), "{b_text}");
            }
            _ => unreachable!(),
        }
    }
    drop(legacy_active_lock);
    assert_ne!(
        broker.supervisor.snapshot().server_status,
        ServerStatus::Running
    );
    client_a.cancel().await.unwrap();
    client_b.cancel().await.unwrap();
    broker.stop().await.unwrap();
}

/// `max_bytes` 的 omission、边界和越界行为必须与冻结 public contract 一致。
#[tokio::test]
async fn source_read_file_enforces_the_frozen_max_bytes_limits() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join("text.txt"), "abcdef").unwrap();
    let lease = lease(directory.path(), "workspace-a", 12);

    for max_bytes in [None, Some(1), Some(32_768), Some(131_072)] {
        let mut args = arguments("text.txt");
        args.max_bytes = max_bytes;
        let output = read_at(&lease, args).await.unwrap();
        if max_bytes == Some(1) {
            assert_eq!(output["text"], "a");
            assert_eq!(output["truncated"], true);
        } else {
            assert_eq!(output["text"], "abcdef");
            assert_eq!(output["truncated"], false);
        }
    }
    for max_bytes in [Some(0), Some(131_073)] {
        let mut args = arguments("text.txt");
        args.max_bytes = max_bytes;
        assert_eq!(
            read_at(&lease, args).await,
            Err("INVALID_PARAMS: max_bytes 超出范围".into())
        );
    }
}

/// 多字节预算必须只返回完整 code point，并精确标记截断状态。
#[tokio::test]
async fn source_read_file_honors_utf8_budget_boundaries_and_truncation() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join("text.txt"), "A中B").unwrap();
    let lease = lease(directory.path(), "workspace-a", 12);

    for (budget, text, truncated) in [(3, "A", true), (4, "A中", true), (5, "A中B", false)] {
        let mut args = arguments("text.txt");
        args.max_bytes = Some(budget);
        let output = read_at(&lease, args).await.unwrap();
        assert_eq!(output["text"], text);
        assert_eq!(output["truncated"], truncated);
    }
}

/// 完整原始 bytes 的 SHA 必须跨 line range、正文预算与多 chunk 文件保持不变。
#[tokio::test]
async fn source_read_file_hashes_complete_raw_bytes_across_ranges_and_budgets() {
    let directory = tempfile::tempdir().unwrap();
    let raw = [
        b"first\r\nsecond\r\n".as_slice(),
        &vec![b'x'; 80_000],
        b"\r\n",
    ]
    .concat();
    fs::write(directory.path().join("large.txt"), &raw).unwrap();
    let expected_hash = Sha256::digest(&raw)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let lease = lease(directory.path(), "workspace-a", 12);

    let cases = [
        (None, Some(1), 32_768, "first\nsecond"),
        (Some(1), Some(1), 32_768, "second"),
        (Some(2), Some(2), 3, "xxx"),
    ];
    for (start_line, end_line, max_bytes, expected_text) in cases {
        let mut args = arguments("large.txt");
        args.start_line = start_line;
        args.end_line = end_line;
        args.max_bytes = Some(max_bytes);
        let output = read_at(&lease, args).await.unwrap();
        assert_eq!(output["text"], expected_text);
        assert_eq!(output["sha256"], expected_hash);
        assert_eq!(output["path"], "large.txt");
    }
}

/// 读取行号延续既有零基、包含两端、CRLF 规范化和空行连接语义。
#[tokio::test]
async fn source_read_file_preserves_start_and_end_line_semantics() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join("lines.txt"), "zero\r\none\n\ntwo\n").unwrap();
    let lease = lease(directory.path(), "workspace-a", 12);

    for (start_line, end_line, expected) in [
        (None, None, "zero\none\n\ntwo"),
        (Some(0), Some(0), "zero"),
        (Some(1), Some(2), "one\n"),
        (Some(3), Some(2), ""),
    ] {
        let mut args = arguments("lines.txt");
        args.start_line = start_line;
        args.end_line = end_line;
        let output = read_at(&lease, args).await.unwrap();
        assert_eq!(output["text"], expected);
        assert_eq!(output["truncated"], false);
    }
}

/// NUL 与无效 UTF-8 都必须拒绝，不能通过 lossy conversion 泄露伪文本。
#[tokio::test]
async fn source_read_file_rejects_binary_input_with_the_existing_backend_error_category() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join("nul.bin"), b"safe\0text").unwrap();
    fs::write(directory.path().join("invalid.bin"), [b'a', 0xff, b'b']).unwrap();
    let lease = lease(directory.path(), "workspace-a", 12);

    for path in ["nul.bin", "invalid.bin"] {
        assert_eq!(
            read_at(&lease, arguments(path)).await,
            Err("BACKEND_ERROR: source_read_file only supports UTF-8 text".into())
        );
    }
}

/// 路径、普通文件与 Workspace root 的拒绝必须来自唯一 WorkspacePathResolver。
#[tokio::test]
async fn source_read_file_rejects_invalid_paths_and_non_regular_files() {
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir(directory.path().join("directory")).unwrap();
    let lease = lease(directory.path(), "workspace-a", 12);

    for path in ["", ".", "../outside", "C:/outside", "\\\\server\\share"] {
        assert!(
            read_at(&lease, arguments(path))
                .await
                .unwrap_err()
                .starts_with("INVALID_PATH"),
            "{path}"
        );
    }
    assert_eq!(
        read_at(&lease, arguments("directory")).await,
        Err("INVALID_PATH: expected a regular file".into())
    );
}

/// 取消在 blocking 文件工作开始前必须立即返回，且不产生任何输出。
#[tokio::test]
async fn source_read_file_stops_when_cancelled_before_work() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join("text.txt"), "text").unwrap();
    let lease = lease(directory.path(), "workspace-a", 12);
    let cancel = CancellationToken::new();
    cancel.cancel();

    assert_eq!(
        read(&lease, arguments("text.txt"), cancel).await,
        Err("CANCELLED".into())
    );
}

/// 已开始的多 chunk 读取会在同一 Token 取消后停止，不会脱离请求无限继续。
#[test]
fn source_read_file_stops_when_cancelled_during_chunked_read() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(
        directory.path().join("large.txt"),
        vec![b'x'; 2 * READ_CHUNK_BYTES],
    )
    .unwrap();
    let lease = lease(directory.path(), "workspace-a", 12);
    let cancel = CancellationToken::new();
    let worker_cancel = cancel.clone();
    let (entered_sender, entered) = mpsc::channel();
    let (release_sender, release) = mpsc::channel();

    let worker = std::thread::spawn(move || {
        read_and_recapture_with_hooks(
            &lease,
            "large.txt",
            0,
            None,
            DEFAULT_MAX_BYTES,
            &worker_cancel,
            move |_| {
                entered_sender.send(()).unwrap();
                release.recv().unwrap();
            },
            |_| {},
        )
    });
    entered.recv_timeout(Duration::from_secs(1)).unwrap();
    cancel.cancel();
    release_sender.send(()).unwrap();

    match worker.join().unwrap() {
        Err(error) => assert_eq!(error, "CANCELLED"),
        Ok(_) => panic!("cancelled read must not produce a result"),
    }
}

/// 原子替换发生在旧 File handle 已读取第一块之后，post-capture 必须发现当前路径版本已不同。
#[test]
fn source_read_file_rejects_atomic_path_replacement_after_old_handle_starts_reading() {
    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("target.txt");
    let replacement = directory.path().join("replacement.txt");
    fs::write(&target, vec![b'a'; 2 * READ_CHUNK_BYTES]).unwrap();
    fs::write(&replacement, vec![b'b'; 2 * READ_CHUNK_BYTES]).unwrap();
    let lease = lease(directory.path(), "workspace-a", 12);
    let mut replaced = false;

    let result = read_and_recapture_with_hooks(
        &lease,
        "target.txt",
        0,
        None,
        DEFAULT_MAX_BYTES,
        &CancellationToken::new(),
        |_| {
            if !replaced {
                // 此时 `target.txt` 的旧 handle 已经完成一块读取；替换只会由 post-capture 发现。
                fs::rename(&replacement, &target).unwrap();
                replaced = true;
            }
        },
        |_| {},
    );

    assert!(replaced);
    assert_source_read_changed(result);
}

/// 就地改变内容或 metadata 后，即使路径名称不变也不能返回已读取的旧正文。
#[test]
fn source_read_file_rejects_in_place_content_or_metadata_change() {
    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("target.txt");
    fs::write(&target, vec![b'a'; 2 * READ_CHUNK_BYTES]).unwrap();
    let lease = lease(directory.path(), "workspace-a", 12);
    let mut changed = false;

    let result = read_and_recapture_with_hooks(
        &lease,
        "target.txt",
        0,
        None,
        DEFAULT_MAX_BYTES,
        &CancellationToken::new(),
        |_| {
            if !changed {
                fs::write(&target, vec![b'c'; READ_CHUNK_BYTES]).unwrap();
                changed = true;
            }
        },
        |_| {},
    );

    assert!(changed);
    assert_source_read_changed(result);
}

/// post-read recapture 的分块循环同样必须服从原请求 Token 的取消。
#[test]
fn source_read_file_stops_when_cancelled_during_post_read_recapture() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(
        directory.path().join("large.txt"),
        vec![b'x'; 2 * READ_CHUNK_BYTES],
    )
    .unwrap();
    let lease = lease(directory.path(), "workspace-a", 12);
    let cancel = CancellationToken::new();
    let worker_cancel = cancel.clone();
    let (entered_sender, entered) = mpsc::channel();
    let (release_sender, release) = mpsc::channel();

    let worker = std::thread::spawn(move || {
        read_and_recapture_with_hooks(
            &lease,
            "large.txt",
            0,
            None,
            DEFAULT_MAX_BYTES,
            &worker_cancel,
            |_| {},
            move |_| {
                entered_sender.send(()).unwrap();
                release.recv().unwrap();
            },
        )
    });
    entered.recv_timeout(Duration::from_secs(1)).unwrap();
    cancel.cancel();
    release_sender.send(()).unwrap();

    match worker.join().unwrap() {
        Err(error) => assert_eq!(error, "CANCELLED"),
        Ok(_) => panic!("cancelled post-read recapture must not produce a result"),
    }
}

/// 不同的捕获 Lease 必须独立决定根目录与 provenance，完全不涉及 UI 或 global active state。
#[tokio::test]
async fn source_read_file_keeps_captured_lease_provenance() {
    let directory = tempfile::tempdir().unwrap();
    let root_a = directory.path().join("a");
    let root_b = directory.path().join("b");
    fs::create_dir(&root_a).unwrap();
    fs::create_dir(&root_b).unwrap();
    fs::write(root_a.join("same.txt"), "from-a").unwrap();
    fs::write(root_b.join("same.txt"), "from-b").unwrap();
    let lease_a = lease(&root_a, "workspace-a", 12);
    let lease_b = lease(&root_b, "workspace-b", 31);

    let (a, b) = tokio::join!(
        read_at(&lease_a, arguments("same.txt")),
        read_at(&lease_b, arguments("same.txt"))
    );
    let a = a.unwrap();
    let b = b.unwrap();
    assert_eq!(a["workspace"], json!({"id":"workspace-a","generation":12}));
    assert_eq!(a["text"], "from-a");
    assert_eq!(b["workspace"], json!({"id":"workspace-b","generation":31}));
    assert_eq!(b["text"], "from-b");
}

/// Windows junction 必须由共享 resolver 在打开文件前 fail closed。
#[cfg(windows)]
#[tokio::test]
async fn source_read_file_rejects_junction_escape() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("root");
    let outside = directory.path().join("outside");
    fs::create_dir(&root).unwrap();
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("secret.txt"), "secret").unwrap();
    let status = std::process::Command::new("cmd.exe")
        .args(["/c", "mklink", "/J"])
        .arg(root.join("link"))
        .arg(&outside)
        .status()
        .unwrap();
    assert!(status.success());

    assert!(
        read_at(
            &lease(&root, "workspace-a", 12),
            arguments("link/secret.txt")
        )
        .await
        .unwrap_err()
        .starts_with("INVALID_PATH")
    );
}

/// 读取期间重定向到 Workspace 外时，post-read resolver 失败必须收敛为版本改变而非泄露路径错误。
#[cfg(windows)]
#[test]
fn source_read_file_maps_post_read_junction_escape_to_source_read_changed() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("root");
    let inside = root.join("inside");
    let outside = directory.path().join("outside");
    fs::create_dir(&root).unwrap();
    fs::create_dir(&inside).unwrap();
    fs::create_dir(&outside).unwrap();
    let target = inside.join("target.txt");
    fs::write(&target, vec![b'a'; 2 * READ_CHUNK_BYTES]).unwrap();
    fs::write(outside.join("target.txt"), "outside").unwrap();
    let lease = lease(&root, "workspace-a", 12);
    let mut redirected = false;

    let result = read_and_recapture_with_hooks(
        &lease,
        "inside/target.txt",
        0,
        None,
        DEFAULT_MAX_BYTES,
        &CancellationToken::new(),
        |_| {
            if !redirected {
                fs::remove_file(&target).unwrap();
                fs::remove_dir(&inside).unwrap();
                let status = std::process::Command::new("cmd.exe")
                    .args(["/c", "mklink", "/J"])
                    .arg(&inside)
                    .arg(&outside)
                    .status()
                    .unwrap();
                assert!(status.success());
                redirected = true;
            }
        },
        |_| {},
    );

    assert!(redirected);
    assert_source_read_changed(result);
}
